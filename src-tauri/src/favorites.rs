// Favorites/pinned items. Same honesty rule as usage.rs: this is real,
// persisted infrastructure, but starts empty — there's no "add to favorites"
// action anywhere in the app yet since Explorer browsing doesn't exist. The
// commands are ready for when it does.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Clone)]
pub struct FavoriteEntry {
    pub path: String,
    pub name: String,
}

fn favorites_file(app_dir: &Path) -> std::path::PathBuf {
    app_dir.join("favorites.json")
}

fn load(app_dir: &Path) -> Vec<FavoriteEntry> {
    fs::read_to_string(favorites_file(app_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(app_dir: &Path, entries: &[FavoriteEntry]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    fs::write(favorites_file(app_dir), json).map_err(|e| e.to_string())
}

pub fn add_favorite(app_dir: &Path, path: &str, name: &str) -> Result<(), String> {
    let mut entries = load(app_dir);
    if !entries.iter().any(|e| e.path == path) {
        entries.push(FavoriteEntry { path: path.to_string(), name: name.to_string() });
        save(app_dir, &entries)?;
    }
    Ok(())
}

pub fn remove_favorite(app_dir: &Path, path: &str) -> Result<(), String> {
    let mut entries = load(app_dir);
    entries.retain(|e| e.path != path);
    save(app_dir, &entries)
}

pub fn list_favorites(app_dir: &Path) -> Vec<FavoriteEntry> {
    load(app_dir)
}

// Called after a delete — same exact-or-nested match as usage.rs's
// remove_usage_under, kept as a separate helper so the manual "remove from
// favorites" star-toggle path stays a strict exact match only.
pub fn remove_favorites_under(app_dir: &Path, path: &str) -> Result<(), String> {
    let mut entries = load(app_dir);
    entries.retain(|e| !(e.path == path || e.path.starts_with(&format!("{}\\", path)) || e.path.starts_with(&format!("{}/", path))));
    save(app_dir, &entries)
}

// Called after a rename or move so a favorited item keeps pointing at the
// real file.
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

// "Clear cache" in Settings.
pub fn clear(app_dir: &Path) -> Result<(), String> {
    save(app_dir, &[])
}
