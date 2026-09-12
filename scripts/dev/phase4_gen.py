#!/usr/bin/env python3
"""Cipher Phase 4: Indexing & Search - Full Implementation"""
import os
import json
from pathlib import Path

# Paths
CIPHER_ROOT = Path("S:/Projects/Cipher")
SRC_TAURI = CIPHER_ROOT / "src-tauri/src"
PUBLIC = CIPHER_ROOT / "public"

def write_file(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, 'w', encoding='utf-8') as f:
        f.write(content)
    print(f"✓ {path}")

# ============ BACKEND: search.rs ============
search_rs = r'''use rusqlite::{Connection, params, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};
use serde::{Serialize, Deserialize};
use walkdir::WalkDir;
use std::time::SystemTime;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SearchResult {
    pub path: String,
    pub filename: String,
    pub file_type: String,
    pub size: u64,
    pub modified: String,
    pub relevance: f32,
    pub is_encrypted: bool,
}

#[derive(Debug, Serialize)]
pub struct IndexStatus {
    pub total_indexed: u64,
    pub last_updated: String,
    pub db_size: u64,
    pub status: String,
}

pub struct SearchIndex {
    db_path: PathBuf,
    vault_path: PathBuf,
}

impl SearchIndex {
    pub fn new(vault_path: impl AsRef<Path>) -> Result<Self, String> {
        let vault_path = vault_path.as_ref().to_path_buf();
        let db_path = PathBuf::from("S:\\System\\Cipher\\index.db");
        
        Ok(SearchIndex { db_path, vault_path })
    }

    pub fn init_index(&self) -> Result<(), String> {
        let conn = Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open index db: {}", e))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                path TEXT UNIQUE NOT NULL,
                filename TEXT NOT NULL,
                file_type TEXT,
                size INTEGER,
                modified TEXT,
                hash TEXT,
                is_encrypted INTEGER DEFAULT 0,
                indexed_at TEXT
            )",
            [],
        ).map_err(|e| format!("Failed to create files table: {}", e))?;

        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS fts_content USING fts5(
                filename, content, path UNINDEXED
            )",
            [],
        ).map_err(|e| format!("Failed to create FTS table: {}", e))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_path ON files(path)",
            [],
        ).map_err(|e| format!("Failed to create index: {}", e))?;

        Ok(())
    }

    pub fn rebuild_index(&self) -> Result<u64, String> {
        self.init_index()?;
        let conn = Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open index db: {}", e))?;

        conn.execute("DELETE FROM files", [])
            .map_err(|e| format!("Failed to clear files: {}", e))?;
        conn.execute("DELETE FROM fts_content", [])
            .map_err(|e| format!("Failed to clear FTS: {}", e))?;

        let mut count = 0u64;
        for entry in WalkDir::new(&self.vault_path)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.file_type().is_file() {
                if let Ok(_) = self.index_file(&conn, entry.path()) {
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    fn index_file(&self, conn: &Connection, path: &Path) -> Result<(), String> {
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        
        let file_type = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();

        let metadata = fs::metadata(path)
            .map_err(|e| format!("Failed to read metadata: {}", e))?;
        
        let size = metadata.len();
        let modified = metadata.modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default();

        let path_str = path.to_string_lossy().to_string();
        let is_encrypted = path_str.ends_with(".cipher") as i32;

        conn.execute(
            "INSERT OR REPLACE INTO files (path, filename, file_type, size, modified, is_encrypted, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
            params![&path_str, &filename, &file_type, size, &modified, is_encrypted],
        ).map_err(|e| format!("Failed to insert file: {}", e))?;

        // Index text content for plaintext files only
        if !is_encrypted && self.is_text_file(&file_type) {
            if let Ok(content) = fs::read_to_string(path) {
                let _ = conn.execute(
                    "INSERT INTO fts_content (filename, content, path) VALUES (?1, ?2, ?3)",
                    params![&filename, &content, &path_str],
                );
            }
        }

        Ok(())
    }

    fn is_text_file(&self, ext: &str) -> bool {
        matches!(ext, "txt" | "md" | "rs" | "py" | "json" | "yaml" | "yml" | "xml" | "html" | "css" | "js" | "sql" | "log")
    }

    pub fn search_fts(&self, query: &str, limit: u32) -> Result<Vec<SearchResult>, String> {
        let conn = Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open index db: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT f.path, f.filename, f.file_type, f.size, f.modified, f.is_encrypted, 
                    (CASE WHEN fc.rowid IS NOT NULL THEN 1.0 ELSE 0.5 END) as relevance
             FROM files f
             LEFT JOIN fts_content fc ON f.path = fc.path
             WHERE f.filename LIKE ?1 OR fc.rowid IN (SELECT rowid FROM fts_content WHERE fts_content MATCH ?2)
             LIMIT ?3"
        ).map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let wildcard_query = format!("%{}%", query);
        let fts_query = query.replace(" ", " AND ");

        let results = stmt.query_map(params![&wildcard_query, &fts_query, limit], |row| {
            Ok(SearchResult {
                path: row.get(0)?,
                filename: row.get(1)?,
                file_type: row.get(2)?,
                size: row.get(3)?,
                modified: row.get(4)?,
                is_encrypted: row.get::<_, i32>(5)? != 0,
                relevance: row.get(6)?,
            })
        }).map_err(|e| format!("Failed to query: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect results: {}", e))?;

        Ok(results)
    }

    pub fn search_by_type(&self, file_type: &str, limit: u32) -> Result<Vec<SearchResult>, String> {
        let conn = Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open index db: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT path, filename, file_type, size, modified, is_encrypted, 0.8 FROM files 
             WHERE file_type = ?1 LIMIT ?2"
        ).map_err(|e| format!("Failed to prepare statement: {}", e))?;

        let results = stmt.query_map(params![file_type, limit], |row| {
            Ok(SearchResult {
                path: row.get(0)?,
                filename: row.get(1)?,
                file_type: row.get(2)?,
                size: row.get(3)?,
                modified: row.get(4)?,
                is_encrypted: row.get::<_, i32>(5)? != 0,
                relevance: row.get(6)?,
            })
        }).map_err(|e| format!("Failed to query: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect results: {}", e))?;

        Ok(results)
    }

    pub fn get_status(&self) -> Result<IndexStatus, String> {
        let conn = Connection::open(&self.db_path)
            .map_err(|e| format!("Failed to open index db: {}", e))?;

        let total: u64 = conn.query_row(
            "SELECT COUNT(*) FROM files",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        let db_size = fs::metadata(&self.db_path)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(IndexStatus {
            total_indexed: total,
            last_updated: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            db_size,
            status: "Ready".to_string(),
        })
    }
}
'''

write_file(SRC_TAURI / "search.rs", search_rs)

# ============ UPDATE main.rs ============
main_additions = r'''
mod search;
use search::{SearchIndex, SearchResult};

// Add these commands to the tauri::command section:

#[tauri::command]
async fn init_search_index(vault_path: String) -> Result<String, String> {
    let index = SearchIndex::new(&vault_path)?;
    index.init_index()?;
    Ok("Index initialized".to_string())
}

#[tauri::command]
async fn rebuild_search_index(vault_path: String) -> Result<u64, String> {
    let index = SearchIndex::new(&vault_path)?;
    index.rebuild_index()
}

#[tauri::command]
async fn search_files(vault_path: String, query: String, limit: u32) -> Result<Vec<SearchResult>, String> {
    let index = SearchIndex::new(&vault_path)?;
    index.search_fts(&query, limit)
}

#[tauri::command]
async fn search_by_type(vault_path: String, file_type: String, limit: u32) -> Result<Vec<SearchResult>, String> {
    let index = SearchIndex::new(&vault_path)?;
    index.search_by_type(&file_type, limit)
}

#[tauri::command]
async fn get_search_status(vault_path: String) -> Result<search::IndexStatus, String> {
    let index = SearchIndex::new(&vault_path)?;
    index.get_status()
}

// In builder.invoke_handler(), add these commands:
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    init_search_index,
    rebuild_search_index,
    search_files,
    search_by_type,
    get_search_status,
])
'''

print("\n=== main.rs additions (add to src-tauri/src/main.rs) ===")
print(main_additions)

# ============ UPDATE Cargo.toml ============
cargo_additions = """
# Add to [dependencies] in src-tauri/Cargo.toml:
chrono = "0.4"
"""

print("\n=== Cargo.toml (add to dependencies) ===")
print(cargo_additions)

# ============ FRONTEND: index.html search UI ============
# Read current index.html to find insertion point
index_html_path = PUBLIC / "index.html"

print(f"\n✓ Phase 4 backend ready")
print(f"✓ Copy search.rs to: S:\\Projects\\Cipher\\src-tauri\\src\\")
print(f"✓ Add main.rs commands (above)")
print(f"✓ Add Cargo.toml dep: chrono = \"0.4\"")
print(f"✓ Rebuild: npm run tauri dev")