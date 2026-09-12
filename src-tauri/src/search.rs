use serde::Serialize;
use tauri::{AppHandle, Emitter};
use std::path::{Path, PathBuf};
use std::time::Instant;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use rusqlite::{Connection, params, ToSql};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

#[derive(Debug, Serialize, Clone)]
pub struct IndexProgress {
    pub current: u64,
    pub drive: String,
    pub done: bool,
    pub current_path: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct DriveInfo {
    pub letter: String,
    pub total_space: u64,
    pub free_space: u64,
    pub used_space: u64,
}

#[derive(Debug, Serialize)]
pub struct IndexStatus {
    pub total_drives: u32,
    pub total_indexed: u64,
    pub last_updated: String,
    pub status: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct SearchResult {
    pub path: String,
    pub filename: String,
    pub file_type: String,
    pub size: u64,
    pub modified: String,
    pub drive: String,
    pub is_encrypted: bool,
}

pub struct SearchIndex;

/// Live watchers, keyed by drive letter (e.g. "C:\\"). Held here so they
/// aren't dropped (and stop watching) the moment a command returns; a
/// watcher must outlive the Tauri command that created it.
static WATCHERS: OnceLock<Mutex<HashMap<String, RecommendedWatcher>>> = OnceLock::new();

fn watchers() -> &'static Mutex<HashMap<String, RecommendedWatcher>> {
    WATCHERS.get_or_init(|| Mutex::new(HashMap::new()))
}

impl SearchIndex {
    /// Index DB lives under the OS per-user data dir, independent of any
    /// particular drive being indexed.
    fn db_path() -> PathBuf {
        let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        let dir = base.join("Cipher");
        let _ = std::fs::create_dir_all(&dir);
        dir.join("index.db")
    }

    fn open_db() -> Result<Connection, String> {
        let conn = Connection::open(Self::db_path())
            .map_err(|e| format!("Failed to open index db: {}", e))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=OFF;
             PRAGMA temp_store=MEMORY;"
        ).map_err(|e| format!("Failed to set pragmas: {}", e))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                filename TEXT NOT NULL,
                file_type TEXT,
                size INTEGER,
                modified TEXT,
                drive TEXT,
                is_encrypted INTEGER DEFAULT 0,
                indexed_at TEXT
            )", [],
        ).map_err(|e| format!("Failed to create files table: {}", e))?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_filename ON files(filename COLLATE NOCASE)", [])
            .map_err(|e| format!("Failed to create filename index: {}", e))?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_file_type ON files(file_type)", [])
            .map_err(|e| format!("Failed to create file_type index: {}", e))?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_drive ON files(drive)", [])
            .map_err(|e| format!("Failed to create drive index: {}", e))?;
        Ok(conn)
    }

    pub fn init_index() -> Result<String, String> {
        Self::open_db()?;
        Ok("Index initialized".to_string())
    }

    pub fn list_drives() -> Result<Vec<DriveInfo>, String> {
        use sysinfo::Disks;

        let disks = Disks::new_with_refreshed_list();
        let mut drives = Vec::new();

        for disk in disks.list() {
            let mount = disk.mount_point().to_string_lossy().to_string();
            let letter = mount.trim_end_matches(['\\', '/']).to_string();
            let total = disk.total_space();
            let free = disk.available_space();

            drives.push(DriveInfo {
                letter,
                total_space: total,
                free_space: free,
                used_space: total.saturating_sub(free),
            });
        }

        Ok(drives)
    }

    pub fn rebuild_index() -> Result<u32, String> {
        let drives = Self::list_drives()?;
        Ok(drives.len() as u32)
    }

    /// Manual stack-based scan (std::fs::read_dir, not WalkDir), writing
    /// metadata (path/filename/type/size/modified) for every file into the
    /// SQLite index as it goes, and emitting `index-progress` events
    /// (throttled ~80ms) for live UI feedback. Content is NOT read or
    /// indexed — full-text search is deferred to a later version; this pass
    /// is metadata-only, which is what keeps it fast across a whole drive.
    pub fn index_drive(app: &AppHandle, drive: &str) -> Result<u64, String> {
        let conn = Self::open_db()?;
        conn.execute("DELETE FROM files WHERE drive = ?1", params![drive])
            .map_err(|e| format!("Failed to clear old entries: {}", e))?;
        conn.execute_batch("BEGIN")
            .map_err(|e| format!("Failed to begin transaction: {}", e))?;

        let mut count: u64 = 0;
        let mut stack: Vec<PathBuf> = vec![PathBuf::from(drive)];
        let mut last_emit = Instant::now();

        {
            let mut stmt = conn.prepare(
                "INSERT OR REPLACE INTO files
                 (path, filename, file_type, size, modified, drive, is_encrypted, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))"
            ).map_err(|e| format!("Failed to prepare insert: {}", e))?;

            while let Some(dir) = stack.pop() {
                let entries = match std::fs::read_dir(&dir) {
                    Ok(e) => e,
                    Err(_) => continue, // permission denied / inaccessible — skip
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    let file_type = match entry.file_type() {
                        Ok(ft) => ft,
                        Err(_) => continue,
                    };
                    if file_type.is_dir() {
                        stack.push(path);
                        continue;
                    }
                    if !file_type.is_file() {
                        continue;
                    }

                    let filename = path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let ext = path.extension()
                        .map(|e| e.to_string_lossy().to_lowercase())
                        .unwrap_or_default();
                    let path_str = path.to_string_lossy().to_string();
                    let is_encrypted = ext == "cipher";

                    let (size, modified) = match entry.metadata() {
                        Ok(meta) => {
                            let modified = meta.modified().ok()
                                .map(|t| {
                                    let dt: chrono::DateTime<chrono::Utc> = t.into();
                                    dt.to_rfc3339()
                                })
                                .unwrap_or_default();
                            (meta.len(), modified)
                        }
                        Err(_) => (0, String::new()),
                    };

                    let _ = stmt.execute(params![
                        &path_str, &filename, &ext, size, &modified, drive, is_encrypted as i32
                    ]);

                    count += 1;
                    if last_emit.elapsed().as_millis() >= 80 {
                        let _ = app.emit("index-progress", IndexProgress {
                            current: count,
                            drive: drive.to_string(),
                            done: false,
                            current_path: Some(path_str),
                        });
                        last_emit = Instant::now();
                    }
                }
            }
        }

        conn.execute_batch("COMMIT")
            .map_err(|e| format!("Failed to commit: {}", e))?;

        let _ = app.emit("index-progress", IndexProgress {
            current: count,
            drive: drive.to_string(),
            done: true,
            current_path: None,
        });
        Ok(count)
    }

    pub fn get_status() -> Result<IndexStatus, String> {
        let drives = Self::list_drives()?;
        let conn = Self::open_db()?;
        let total_indexed: u64 = conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .unwrap_or(0);
        Ok(IndexStatus {
            total_drives: drives.len() as u32,
            total_indexed,
            last_updated: chrono::Local::now().to_rfc3339(),
            status: "Ready".to_string(),
        })
    }

    /// Filename search with optional filters: a category's extension list
    /// (empty/None = all types) and an encrypted-only toggle. Built as
    /// dynamic SQL since rusqlite's `params!` macro can't express a
    /// variable-length `IN (...)` list.
    pub fn search_files(
        query: &str,
        limit: u32,
        offset: u32,
        file_types: Option<&[String]>,
        encrypted_only: bool,
    ) -> Result<Vec<SearchResult>, String> {
        let conn = Self::open_db()?;
        let pattern = format!("%{}%", query);

        let mut sql = String::from(
            "SELECT path, filename, file_type, size, modified, drive, is_encrypted
             FROM files WHERE filename LIKE ?1 COLLATE NOCASE"
        );
        let mut owned_params: Vec<Box<dyn ToSql>> = vec![Box::new(pattern)];

        if let Some(types) = file_types {
            if !types.is_empty() {
                let placeholders: Vec<String> = types.iter()
                    .enumerate()
                    .map(|(i, _)| format!("?{}", owned_params.len() + i + 1))
                    .collect();
                sql.push_str(&format!(" AND file_type IN ({})", placeholders.join(",")));
                for t in types {
                    owned_params.push(Box::new(t.clone()));
                }
            }
        }
        if encrypted_only {
            sql.push_str(" AND is_encrypted = 1");
        }

        sql.push_str(&format!(
            " ORDER BY filename LIMIT ?{} OFFSET ?{}",
            owned_params.len() + 1,
            owned_params.len() + 2
        ));
        owned_params.push(Box::new(limit));
        owned_params.push(Box::new(offset));

        let mut stmt = conn.prepare(&sql)
            .map_err(|e| format!("Failed to prepare search: {}", e))?;
        let param_refs: Vec<&dyn ToSql> = owned_params.iter().map(|b| b.as_ref()).collect();

        let results = stmt.query_map(param_refs.as_slice(), |row| {
            Ok(SearchResult {
                path: row.get(0)?,
                filename: row.get(1)?,
                file_type: row.get(2)?,
                size: row.get(3)?,
                modified: row.get(4)?,
                drive: row.get(5)?,
                is_encrypted: row.get::<_, i32>(6)? != 0,
            })
        }).map_err(|e| format!("Failed to query: {}", e))?
          .collect::<Result<Vec<_>, _>>()
          .map_err(|e| format!("Failed to collect: {}", e))?;

        Ok(results)
    }

    pub fn search_by_type(file_type: &str, limit: u32, offset: u32) -> Result<Vec<SearchResult>, String> {
        Self::search_files("", limit, offset, Some(&[file_type.to_string()]), false)
    }

    // ===== Live watching (incremental index updates) =====
    // Uses `notify` (ReadDirectoryChangesW on Windows) in recursive mode
    // rather than periodic full rescans. On any change under a watched
    // drive, the touched path is re-stat'd directly: present + a file ->
    // upsert; gone -> delete the row. This sidesteps needing precise
    // create/modify/rename enum matching, at the cost of a little extra
    // work per event — an acceptable trade for correctness here.

    fn upsert_file(conn: &Connection, path: &Path, drive: &str) {
        let filename = path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let ext = path.extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let path_str = path.to_string_lossy().to_string();
        let is_encrypted = ext == "cipher";
        let (size, modified) = match std::fs::metadata(path) {
            Ok(meta) => {
                let modified = meta.modified().ok()
                    .map(|t| {
                        let dt: chrono::DateTime<chrono::Utc> = t.into();
                        dt.to_rfc3339()
                    })
                    .unwrap_or_default();
                (meta.len(), modified)
            }
            Err(_) => (0, String::new()),
        };
        let _ = conn.execute(
            "INSERT OR REPLACE INTO files
             (path, filename, file_type, size, modified, drive, is_encrypted, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
            params![&path_str, &filename, &ext, size, &modified, drive, is_encrypted as i32],
        );
    }

    fn handle_fs_event(app: &AppHandle, drive: &str, event: Event) {
        let conn = match Self::open_db() {
            Ok(c) => c,
            Err(_) => return,
        };
        for path in &event.paths {
            if path.is_file() {
                Self::upsert_file(&conn, path, drive);
            } else if !path.exists() {
                let path_str = path.to_string_lossy().to_string();
                let _ = conn.execute("DELETE FROM files WHERE path = ?1", params![path_str]);
            }
        }
        let _ = app.emit("index-watch-update", drive.to_string());
    }

    pub fn watch_drive(app: AppHandle, drive: String) -> Result<(), String> {
        if watchers().lock().unwrap().contains_key(&drive) {
            return Ok(()); // already watching
        }
        let drive_for_thread = drive.clone();
        let app_for_thread = app.clone();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                Self::handle_fs_event(&app_for_thread, &drive_for_thread, event);
            }
        }).map_err(|e| format!("Failed to create watcher: {}", e))?;

        watcher.watch(Path::new(&drive), RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch drive: {}", e))?;

        watchers().lock().unwrap().insert(drive, watcher);
        Ok(())
    }

    pub fn unwatch_drive(drive: &str) -> Result<(), String> {
        watchers().lock().unwrap().remove(drive);
        Ok(())
    }

    pub fn is_watching(drive: &str) -> bool {
        watchers().lock().unwrap().contains_key(drive)
    }
}
