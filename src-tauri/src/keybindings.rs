// Remappable keyboard shortcuts. Defaults live in code (single source of
// truth for what a fresh install looks like); only user overrides are
// persisted, so a later change to a default doesn't get masked by a stale
// saved copy of the old default.

use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Serialize, Clone)]
pub struct KeyAction {
    pub id: String,
    pub label: String,
    pub combo: String,
}

fn defaults() -> Vec<(&'static str, &'static str, &'static str)> {
    // (id, label, default combo)
    vec![
        ("refresh", "Refresh", "F5"),
        ("select-all", "Select all", "Ctrl+A"),
        ("copy-path", "Copy as path", "Ctrl+Shift+C"),
        ("paste", "Paste", "Ctrl+V"),
        ("undo", "Undo", "Ctrl+Z"),
        ("redo", "Redo", "Ctrl+Shift+Z"),
        ("new-tab", "New tab", "Ctrl+T"),
        ("close-tab", "Close tab", "Ctrl+W"),
        ("enter-folder", "Open folder (arrow key)", "ArrowRight"),
        ("go-up-folder", "Up one folder (arrow key)", "ArrowLeft"),
        ("command-palette", "Command palette", "Ctrl+K"),
    ]
}

fn overrides_file(app_dir: &Path) -> std::path::PathBuf {
    app_dir.join("keybindings.json")
}

fn load_overrides(app_dir: &Path) -> HashMap<String, String> {
    fs::read_to_string(overrides_file(app_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_overrides(app_dir: &Path, overrides: &HashMap<String, String>) -> Result<(), String> {
    let json = serde_json::to_string_pretty(overrides).map_err(|e| e.to_string())?;
    fs::write(overrides_file(app_dir), json).map_err(|e| e.to_string())
}

pub fn get_keybindings(app_dir: &Path) -> Vec<KeyAction> {
    let overrides = load_overrides(app_dir);
    defaults()
        .into_iter()
        .map(|(id, label, default_combo)| {
            let combo = overrides.get(id).cloned().unwrap_or_else(|| default_combo.to_string());
            KeyAction { id: id.to_string(), label: label.to_string(), combo }
        })
        .collect()
}

pub fn set_keybinding(app_dir: &Path, action: &str, combo: &str) -> Result<(), String> {
    if !defaults().iter().any(|(id, _, _)| *id == action) {
        return Err(format!("Unknown action: {}", action));
    }
    let mut overrides = load_overrides(app_dir);
    overrides.insert(action.to_string(), combo.to_string());
    save_overrides(app_dir, &overrides)
}

pub fn reset_keybindings(app_dir: &Path) -> Result<(), String> {
    save_overrides(app_dir, &HashMap::new())
}
