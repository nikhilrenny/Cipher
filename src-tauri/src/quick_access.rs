// Quick access — the left icon strip. Previously just rebuilt from OS
// common folders (Pictures/Videos/Documents/Music/Downloads) on every load,
// with nothing pinnable. Now a real, persisted, user-editable list: any
// folder can be pinned or unpinned from the right-click menu. Seeded once
// from the OS common folders on first run so existing behavior isn't lost;
// every load after that reads the persisted file as-is.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone)]
pub struct QuickAccessEntry {
    pub path: String,
    pub name: String,
    pub icon: String,
}

fn qa_file(app_dir: &Path) -> std::path::PathBuf {
    app_dir.join("quick-access.json")
}

fn load(app_dir: &Path) -> Vec<QuickAccessEntry> {
    fs::read_to_string(qa_file(app_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(app_dir: &Path, entries: &[QuickAccessEntry]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    fs::write(qa_file(app_dir), json).map_err(|e| e.to_string())
}

pub fn list_quick_access(app_dir: &Path) -> Vec<QuickAccessEntry> {
    let file = qa_file(app_dir);
    
    // Only seed if the file truly doesn't exist (never been initialized).
    // After it's created (even if empty), load from disk as-is.
    if !file.exists() {
        let seeded: Vec<QuickAccessEntry> = crate::explorer::get_common_folders()
            .into_iter()
            .map(|f| QuickAccessEntry { path: f.path, name: f.name, icon: f.icon })
            .collect();
        // On first initialization, seed must succeed or we'll re-seed every time.
        // Panic is intentional — if we can't write to app_dir, the whole app is broken.
        let json = serde_json::to_string_pretty(&seeded).unwrap();
        std::fs::write(&file, &json).expect("failed to write initial quick-access.json");
        return seeded;
    }
    
    // File exists (even if empty) — load whatever is actually there.
    load(app_dir)
}

pub fn add_quick_access(app_dir: &Path, path: &str, name: &str) -> Result<(), String> {
    let mut entries = list_quick_access(app_dir); // ensures seeded first
    if !entries.iter().any(|e| e.path == path) {
        entries.push(QuickAccessEntry {
            path: path.to_string(),
            name: name.to_string(),
            icon: "ti-folder".to_string(),
        });
        save(app_dir, &entries)?;
    }
    Ok(())
}

pub fn remove_quick_access(app_dir: &Path, path: &str) -> Result<(), String> {
    let mut entries = list_quick_access(app_dir);
    entries.retain(|e| e.path != path);
    save(app_dir, &entries)
}

// Called after a delete — same exact-or-nested match as usage.rs's
// remove_usage_under.
pub fn remove_quick_access_under(app_dir: &Path, path: &str) -> Result<(), String> {
    let mut entries = list_quick_access(app_dir);
    entries.retain(|e| !(e.path == path || e.path.starts_with(&format!("{}\\", path)) || e.path.starts_with(&format!("{}/", path))));
    save(app_dir, &entries)
}

// Called after a rename or move so a pinned shortcut keeps pointing at the
// real folder.
pub fn update_path(app_dir: &Path, old_path: &str, new_path: &str, new_name: &str) -> Result<(), String> {
    let mut entries = list_quick_access(app_dir);
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

// "Clear cache" in Settings — deletes the persisted file entirely (rather
// than emptying it), so the next call to list_quick_access() reseeds from
// OS common folders. Emptying it previously left defaults gone for good.
pub fn clear(app_dir: &Path) -> Result<(), String> {
    let file = qa_file(app_dir);
    if file.exists() {
        fs::remove_file(&file).map_err(|e| e.to_string())?;
    }
    Ok(())
}
