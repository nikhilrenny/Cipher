// Phase 1.3: File Restrictions
// - Folder-level write locks (inherited by children)
// - Extension blacklist (configurable, prevents create/rename)
// - Write validation before file ops

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const LOCK_FILE: &str = ".cipher-lock";

pub fn set_folder_write_lock(folder_path: &str, locked: bool) -> Result<(), String> {
    let path = Path::new(folder_path);
    if !path.is_dir() {
        return Err(format!("not a directory: {}", folder_path));
    }

    let lock_marker = path.join(LOCK_FILE);
    if locked {
        fs::write(&lock_marker, "locked").map_err(|e| format!("failed to create lock: {}", e))?;
    } else {
        if lock_marker.exists() {
            fs::remove_file(&lock_marker).map_err(|e| format!("failed to remove lock: {}", e))?;
        }
    }
    Ok(())
}

pub fn is_write_locked(path: &str) -> Result<bool, String> {
    let mut current = PathBuf::from(path);

    if current.is_file() {
        current = current.parent().ok_or("no parent")?.to_path_buf();
    }

    loop {
        let lock_marker = current.join(LOCK_FILE);
        if lock_marker.exists() {
            return Ok(true);
        }

        if !current.pop() {
            break;
        }
    }

    Ok(false)
}

pub fn get_write_lock_parent(path: &str) -> Result<Option<String>, String> {
    let mut current = PathBuf::from(path);

    if current.is_file() {
        current = current.parent().ok_or("no parent")?.to_path_buf();
    }

    loop {
        let lock_marker = current.join(LOCK_FILE);
        if lock_marker.exists() {
            return Ok(Some(current.to_string_lossy().to_string()));
        }

        if !current.pop() {
            break;
        }
    }

    Ok(None)
}

#[derive(Serialize, Deserialize)]
pub struct Blacklist {
    pub extensions: HashSet<String>,
}

impl Default for Blacklist {
    fn default() -> Self {
        let mut ext = HashSet::new();
        ext.insert("exe".to_string());
        ext.insert("com".to_string());
        ext.insert("scr".to_string());
        ext.insert("vbs".to_string());
        ext.insert("bat".to_string());
        ext.insert("cmd".to_string());
        ext.insert("ps1".to_string());
        ext.insert("ps2".to_string());
        ext.insert("psc1".to_string());
        ext.insert("psc2".to_string());
        ext.insert("msh".to_string());
        ext.insert("msh1".to_string());
        ext.insert("msh2".to_string());
        ext.insert("mshxml".to_string());
        ext.insert("msh1xml".to_string());
        ext.insert("msh2xml".to_string());
        ext.insert("lnk".to_string());
        ext.insert("pif".to_string());
        Blacklist { extensions: ext }
    }
}

fn blacklist_file(app_dir: &Path) -> PathBuf {
    app_dir.join("blacklist.json")
}

pub fn load_blacklist(app_dir: &Path) -> Result<Blacklist, String> {
    let path = blacklist_file(app_dir);
    if !path.exists() {
        return Ok(Blacklist::default());
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("failed to read blacklist: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("invalid blacklist JSON: {}", e))
}

pub fn save_blacklist(app_dir: &Path, blacklist: &Blacklist) -> Result<(), String> {
    let path = blacklist_file(app_dir);
    let json = serde_json::to_string_pretty(blacklist).map_err(|e| e.to_string())?;
    fs::write(&path, &json).map_err(|e| format!("failed to save blacklist: {}", e))
}

pub fn is_extension_blacklisted(filename: &str, blacklist: &Blacklist) -> bool {
    Path::new(filename)
        .extension()
        .and_then(|ext| Some(ext.to_string_lossy().to_lowercase()))
        .map(|ext| blacklist.extensions.contains(ext.as_str()))
        .unwrap_or(false)
}

pub fn add_to_blacklist(app_dir: &Path, extension: &str) -> Result<(), String> {
    let mut blacklist = load_blacklist(app_dir)?;
    blacklist.extensions.insert(extension.to_lowercase());
    save_blacklist(app_dir, &blacklist)
}

pub fn remove_from_blacklist(app_dir: &Path, extension: &str) -> Result<(), String> {
    let mut blacklist = load_blacklist(app_dir)?;
    blacklist.extensions.remove(&extension.to_lowercase());
    save_blacklist(app_dir, &blacklist)
}

// ===== Per-path lock overrides =====
// Locking a folder protects everything inside it. Sometimes one specific
// child needs to be writable anyway without unlocking the whole tree — the
// override list is that exception, keyed by exact path.

fn overrides_file(app_dir: &Path) -> PathBuf {
    app_dir.join("lock-overrides.json")
}

fn load_overrides(app_dir: &Path) -> HashSet<String> {
    fs::read_to_string(overrides_file(app_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_overrides(app_dir: &Path, overrides: &HashSet<String>) -> Result<(), String> {
    let json = serde_json::to_string_pretty(overrides).map_err(|e| e.to_string())?;
    fs::write(overrides_file(app_dir), json).map_err(|e| e.to_string())
}

pub fn is_override(app_dir: &Path, path: &str) -> bool {
    let overrides = load_overrides(app_dir);
    if overrides.contains(path) {
        return true;
    }
    // A directory override cascades to everything inside it, mirroring how
    // a lock itself cascades — otherwise overriding a folder full of
    // existing items would mean adding every single one individually.
    overrides.iter().any(|entry| {
        path.starts_with(&format!("{}\\", entry)) || path.starts_with(&format!("{}/", entry))
    })
}

pub fn add_override(app_dir: &Path, path: &str) -> Result<(), String> {
    let mut overrides = load_overrides(app_dir);
    overrides.insert(path.to_string());
    save_overrides(app_dir, &overrides)
}

pub fn remove_override(app_dir: &Path, path: &str) -> Result<(), String> {
    let mut overrides = load_overrides(app_dir);
    overrides.remove(path);
    save_overrides(app_dir, &overrides)
}

pub fn validate_write_to(path: &str, app_dir: &Path) -> Result<(), String> {
    if is_write_locked(path)? {
        if is_override(app_dir, path) {
            return Ok(());
        }
        let locked_parent = get_write_lock_parent(path)?;
        return Err(format!(
            "write-protected: \"{}\" is inside locked folder \"{}\"",
            path,
            locked_parent.unwrap_or_else(|| "[unknown]".to_string())
        ));
    }

    Ok(())
}

pub fn validate_filename(filename: &str, app_dir: &Path) -> Result<(), String> {
    let blacklist = load_blacklist(app_dir)?;
    if is_extension_blacklisted(filename, &blacklist) {
        return Err(format!(
            "blocked: cannot create files with .{} extension",
            Path::new(filename)
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_else(|| "[unknown]".to_string())
        ));
    }
    Ok(())
}