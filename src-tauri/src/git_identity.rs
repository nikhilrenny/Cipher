// Phase 3: Git author identity — a small global setting (name + email)
// used when creating commits, instead of the previous hardcoded values.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitIdentity {
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub default_location: Option<String>,
}

impl Default for GitIdentity {
    fn default() -> Self {
        GitIdentity {
            name: "Cipher Agent".to_string(),
            email: "cipher@vault.local".to_string(),
            default_location: None,
        }
    }
}

fn config_path(app_dir: &Path) -> PathBuf {
    app_dir.join("git-identity.json")
}

pub fn load(app_dir: &Path) -> GitIdentity {
    let path = config_path(app_dir);
    if let Ok(data) = fs::read_to_string(&path) {
        if let Ok(identity) = serde_json::from_str(&data) {
            return identity;
        }
    }
    GitIdentity::default()
}

pub fn save(app_dir: &Path, identity: &GitIdentity) -> Result<(), String> {
    let path = config_path(app_dir);
    let json = serde_json::to_string_pretty(identity).map_err(|e| format!("Serialize failed: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Write failed: {}", e))
}
