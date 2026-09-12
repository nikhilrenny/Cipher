// Recent + most-used tracking for the home screen. This is real, persisted
// infrastructure — not a mockup — but it will show empty until something
// actually calls record_access, which won't happen until Explorer's file
// browsing/opening is rebuilt (the next phase). An empty "no recent files
// yet" state is the honest, correct result of a fresh install, not a bug.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone)]
pub struct UsageEntry {
    pub path: String,
    pub name: String,
    pub last_accessed: u64,
    pub access_count: u32,
}

fn usage_file(app_dir: &Path) -> std::path::PathBuf {
    app_dir.join("usage.json")
}

fn load(app_dir: &Path) -> Vec<UsageEntry> {
    fs::read_to_string(usage_file(app_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(app_dir: &Path, entries: &[UsageEntry]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    fs::write(usage_file(app_dir), json).map_err(|e| e.to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn record_access(app_dir: &Path, path: &str, name: &str) -> Result<(), String> {
    let mut entries = load(app_dir);
    if let Some(existing) = entries.iter_mut().find(|e| e.path == path) {
        existing.last_accessed = now_ms();
        existing.access_count += 1;
    } else {
        entries.push(UsageEntry {
            path: path.to_string(),
            name: name.to_string(),
            last_accessed: now_ms(),
            access_count: 1,
        });
    }
    save(app_dir, &entries)
}

pub fn get_recent(app_dir: &Path, limit: usize) -> Vec<UsageEntry> {
    let mut entries = load(app_dir);
    entries.sort_by(|a, b| b.last_accessed.cmp(&a.last_accessed));
    entries.truncate(limit);
    entries
}

pub fn get_most_used(app_dir: &Path, limit: usize) -> Vec<UsageEntry> {
    let mut entries = load(app_dir);
    entries.sort_by(|a, b| b.access_count.cmp(&a.access_count));
    entries.truncate(limit);
    entries
}

// Called after a delete so a removed file/folder doesn't linger as a dead
// entry on the home screen. Matches the exact path plus anything nested
// under it, since deleting a folder should clear its children's history too.
pub fn remove_usage_under(app_dir: &Path, path: &str) -> Result<(), String> {
    let mut entries = load(app_dir);
    entries.retain(|e| !is_same_or_under(&e.path, path));
    save(app_dir, &entries)
}

fn is_same_or_under(candidate: &str, base: &str) -> bool {
    candidate == base
        || candidate.starts_with(&format!("{}\\", base))
        || candidate.starts_with(&format!("{}/", base))
}

// Called after a rename or move so a cached entry keeps pointing at the
// real file instead of going stale (which is what caused "open" to fail
// on a path that no longer exists).
pub fn update_path(app_dir: &Path, old_path: &str, new_path: &str, new_name: &str) -> Result<(), String> {
    let mut entries = load(app_dir);
    let mut changed = false;
    for e in entries.iter_mut() {
        if e.path == old_path {
            e.path = new_path.to_string();
            e.name = new_name.to_string();
            changed = true;
        }
    }
    if changed { save(app_dir, &entries) } else { Ok(()) }
}

// "Clear cache" in Settings — wipes recorded history entirely rather than
// filtering it, for testing / starting clean.
pub fn clear(app_dir: &Path) -> Result<(), String> {
    save(app_dir, &[])
}
