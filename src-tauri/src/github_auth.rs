// Phase 3: GitHub Personal Access Token storage.
// Stored locally in plaintext (same trust model as git-identity.json) —
// this machine only, used solely to authenticate outgoing HTTPS push/pull
// requests to your configured remote.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitHubAuth {
    pub token: String,
}

fn config_path(app_dir: &Path) -> PathBuf {
    app_dir.join("github-auth.json")
}

pub fn load(app_dir: &Path) -> GitHubAuth {
    let path = config_path(app_dir);
    if let Ok(data) = fs::read_to_string(&path) {
        if let Ok(auth) = serde_json::from_str(&data) {
            return auth;
        }
    }
    GitHubAuth::default()
}

pub fn save(app_dir: &Path, auth: &GitHubAuth) -> Result<(), String> {
    let path = config_path(app_dir);
    let json = serde_json::to_string_pretty(auth).map_err(|e| format!("Serialize failed: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Write failed: {}", e))
}

pub fn has_token(app_dir: &Path) -> bool {
    !load(app_dir).token.trim().is_empty()
}
