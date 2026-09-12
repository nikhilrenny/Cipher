// Drive/storage info for the home screen. Uses `sysinfo` rather than raw
// Windows APIs specifically because it's cross-platform — same code path
// will work unchanged on the eventual Linux port, matching the pattern set
// for the rest of the app.

use serde::Serialize;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;
use sysinfo::Disks;

#[derive(Serialize)]
pub struct DriveInfo {
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub is_removable: bool,
}

pub fn get_drives() -> Vec<DriveInfo> {
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .map(|d| DriveInfo {
            name: d.name().to_string_lossy().to_string(),
            mount_point: d.mount_point().to_string_lossy().to_string(),
            total_bytes: d.total_space(),
            free_bytes: d.available_space(),
            is_removable: d.is_removable(),
        })
        .collect()
}

#[derive(Serialize, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<u64>,
    pub readonly: bool,
    pub write_locked: bool,
    pub overridden: bool,
    pub has_encrypted: bool,
}

// Bounded scan for whether a folder contains any .cipher file, used to
// badge a folder row in the browser even though the encrypted file itself
// might be several levels down. Capped (entries visited + depth) rather
// than exhaustive — a full recursive walk on something like C:\Windows
// just to answer "does this contain any .cipher file" would be far too
// slow for a per-row UI hint; this is a best-effort signal, not a
// guarantee of absence.
const ENCRYPTED_SCAN_BUDGET: usize = 2000;
const ENCRYPTED_SCAN_MAX_DEPTH: usize = 6;

fn contains_encrypted(dir: &Path) -> bool {
    let mut stack: Vec<(std::path::PathBuf, usize)> = vec![(dir.to_path_buf(), 0)];
    let mut visited = 0usize;
    while let Some((d, depth)) = stack.pop() {
        if visited >= ENCRYPTED_SCAN_BUDGET || depth > ENCRYPTED_SCAN_MAX_DEPTH {
            continue;
        }
        let read_dir = match fs::read_dir(&d) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for item in read_dir {
            let item = match item { Ok(i) => i, Err(_) => continue };
            visited += 1;
            if visited > ENCRYPTED_SCAN_BUDGET {
                break;
            }
            let p = item.path();
            let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                stack.push((p, depth + 1));
            } else if p.extension().and_then(|e| e.to_str()) == Some("cipher") {
                return true;
            }
        }
    }
    false
}

// Real in-app folder listing — this is what clicking a drive or a Quick
// access folder actually calls now, instead of handing off to the OS's own
// Explorer. Unreadable entries (permissions, etc.) are skipped rather than
// failing the whole listing.
pub fn list_directory(path: &str) -> Result<Vec<FileEntry>, String> {
    let dir = Path::new(path);
    if !dir.is_dir() {
        return Err(format!("not a directory: {}", path));
    }
    // Checked once for the listed directory itself — if it (or an ancestor)
    // is write-locked, every entry inside inherits that protection, so
    // there's no need to re-walk ancestors per entry. An override on the
    // listed directory itself cancels that inheritance for everything in
    // it, same as unlocking would.
    let app_dir = crate::app_dir();
    let dir_locked = crate::restrictions::is_write_locked(path).unwrap_or(false)
        && !crate::restrictions::is_override(&app_dir, path);
    let read_dir = fs::read_dir(dir).map_err(|e| format!("failed to read {}: {}", path, e))?;
    let mut entries = Vec::new();
    for item in read_dir {
        let item = match item { Ok(i) => i, Err(_) => continue };
        let metadata = match item.metadata() { Ok(m) => m, Err(_) => continue };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64);
        let item_path = item.path();
        let item_path_str = item_path.to_string_lossy().to_string();
        // Locked if the containing directory is (inherited protection,
        // applies to files and folders alike) or this specific folder
        // carries its own marker (starts a new lock scope) — unless this
        // exact item has its own override, which wins over both.
        let raw_locked = dir_locked || (metadata.is_dir() && item_path.join(".cipher-lock").exists());
        let overridden = raw_locked && crate::restrictions::is_override(&app_dir, &item_path_str);
        let write_locked = raw_locked && !overridden;
        let has_encrypted = if metadata.is_dir() {
            contains_encrypted(&item_path)
        } else {
            item_path.extension().and_then(|e| e.to_str()) == Some("cipher")
        };
        entries.push(FileEntry {
            name: item.file_name().to_string_lossy().to_string(),
            path: item_path_str,
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            modified,
            readonly: metadata.permissions().readonly(),
            write_locked,
            overridden,
            has_encrypted,
        });
    }
    // directories first, then alphabetical (case-insensitive) — standard explorer convention
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(entries)
}

// Opens a file (or folder) with its OS-default application. Previously
// shelled out to `explorer.exe <path>` on Windows, but that only reliably
// works for *folders* — given a file path, Explorer's behavior is
// inconsistent and can silently fall back to a generic window (e.g.
// whatever "Open File Explorer to:" is set to) instead of actually
// opening the file. open::that() uses the real OS default-open mechanism
// (ShellExecute on Windows — the same path a double-click takes) and
// works correctly for both files and folders on every platform, which
// also lets this be one call instead of three per-OS branches.
pub fn open_in_os_explorer(path: &str) -> Result<(), String> {
    println!("[OPEN] click fired, raw path = {}", path);
    
    match std::fs::canonicalize(path) {
        Ok(canonical) => {
            println!("[OPEN] canonicalized to: {:?}", canonical);
            
            // Strip Windows UNC prefix \\?\ that canonicalize adds
            // ShellExecute doesn't handle it reliably
            let path_str = canonical.to_string_lossy().to_string();
            let clean_path = if path_str.starts_with("\\\\?\\") {
                path_str[4..].to_string()
            } else {
                path_str
            };
            println!("[OPEN] clean path (UNC stripped): {}", clean_path);
            
            match open::that(&clean_path) {
                Ok(_) => {
                    println!("[OPEN] open::that() succeeded");
                    Ok(())
                },
                Err(e) => {
                    println!("[OPEN] open::that() FAILED: {}", e);
                    Err(format!("failed to open: {}", e))
                }
            }
        },
        Err(e) => {
            println!("[OPEN] canonicalize FAILED: {}, trying as-is", e);
            match open::that(path) {
                Ok(_) => {
                    println!("[OPEN] open::that(as-is) succeeded");
                    Ok(())
                },
                Err(e) => {
                    println!("[OPEN] open::that(as-is) FAILED: {}", e);
                    Err(format!("failed to open: {}", e))
                }
            }
        }
    }
}

#[derive(Serialize)]
pub struct CommonFolder {
    pub name: String,
    pub path: String,
    pub icon: String,
}

// Real standard-folder resolution via the `dirs` crate (Known Folder API on
// Windows, XDG on Linux) — not hardcoded paths, so this doesn't break for
// users who've relocated their Pictures/Documents/etc. folders. Folders that
// don't resolve or don't exist are simply omitted, not shown as broken links.
pub fn get_common_folders() -> Vec<CommonFolder> {
    let candidates: Vec<(&str, Option<std::path::PathBuf>, &str)> = vec![
        ("Pictures", dirs::picture_dir(), "ti-photo"),
        ("Videos", dirs::video_dir(), "ti-video"),
        ("Documents", dirs::document_dir(), "ti-file-text"),
        ("Music", dirs::audio_dir(), "ti-music"),
        ("Downloads", dirs::download_dir(), "ti-download"),
    ];
    candidates
        .into_iter()
        .filter_map(|(name, path, icon)| {
            let p = path?;
            if !p.is_dir() {
                return None;
            }
            Some(CommonFolder { name: name.to_string(), path: p.to_string_lossy().to_string(), icon: icon.to_string() })
        })
        .collect()
}
