// Bookmarked folder "pages" — like favorites.rs, but the storage file can
// live somewhere other than app_dir (configurable in Settings), so bookmarks
// can be kept in e.g. a synced folder. The pointer to that location is a
// tiny config file that *does* always live in app_dir, since we need a
// fixed place to find it before we know where anything else is.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone)]
pub struct BookmarkEntry {
    pub path: String,
    pub name: String,
}

#[derive(Serialize, Deserialize, Default)]
struct LocationConfig {
    location: Option<String>,
}

fn location_config_file(app_dir: &Path) -> PathBuf {
    app_dir.join("bookmarks-location.json")
}

fn load_location_config(app_dir: &Path) -> LocationConfig {
    fs::read_to_string(location_config_file(app_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn get_location(app_dir: &Path) -> Option<String> {
    load_location_config(app_dir).location
}

pub fn set_location(app_dir: &Path, location: Option<String>) -> Result<(), String> {
    // Moving location: carry existing bookmarks over so nothing is lost.
    let entries = list_bookmarks(app_dir);
    let cfg = LocationConfig { location: location.clone() };
    let json = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    fs::write(location_config_file(app_dir), json).map_err(|e| e.to_string())?;
    save(app_dir, &entries)
}

// Directory the bookmarks.json file actually lives in: the configured
// location if set and creatable, otherwise app_dir.
fn resolve_dir(app_dir: &Path) -> PathBuf {
    match get_location(app_dir) {
        Some(loc) if !loc.trim().is_empty() => {
            let p = PathBuf::from(&loc);
            if fs::create_dir_all(&p).is_ok() { p } else { app_dir.to_path_buf() }
        }
        _ => app_dir.to_path_buf(),
    }
}

fn bookmarks_file(app_dir: &Path) -> PathBuf {
    resolve_dir(app_dir).join("bookmarks.json")
}

fn load(app_dir: &Path) -> Vec<BookmarkEntry> {
    fs::read_to_string(bookmarks_file(app_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(app_dir: &Path, entries: &[BookmarkEntry]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    fs::write(bookmarks_file(app_dir), json).map_err(|e| e.to_string())
}

pub fn add_bookmark(app_dir: &Path, path: &str, name: &str) -> Result<(), String> {
    let mut entries = load(app_dir);
    if !entries.iter().any(|e| e.path == path) {
        entries.push(BookmarkEntry { path: path.to_string(), name: name.to_string() });
        save(app_dir, &entries)?;
    }
    Ok(())
}

pub fn remove_bookmark(app_dir: &Path, path: &str) -> Result<(), String> {
    let mut entries = load(app_dir);
    entries.retain(|e| e.path != path);
    save(app_dir, &entries)
}

pub fn list_bookmarks(app_dir: &Path) -> Vec<BookmarkEntry> {
    load(app_dir)
}
