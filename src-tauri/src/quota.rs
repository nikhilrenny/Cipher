// Phase 1.2: per-folder storage quota tracking. Config lives as a plain
// JSON file inside the folder itself, so quota is scoped per-location
// rather than global — matching how favorites/quick_access are scoped to
// app_dir, not the OS.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_FILE: &str = ".cipher-config.json";
const DEFAULT_QUOTA_BYTES: u64 = 100 * 1024 * 1024 * 1024; // 100 GB

#[derive(Serialize, Deserialize, Clone)]
pub struct QuotaConfig {
    pub quota_bytes: u64, // 0 = unlimited
    pub warn_threshold_pct: u8,
    pub critical_threshold_pct: u8,
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self {
            quota_bytes: DEFAULT_QUOTA_BYTES,
            warn_threshold_pct: 70,
            critical_threshold_pct: 90,
        }
    }
}

#[derive(Serialize)]
pub struct QuotaStatus {
    pub quota_bytes: u64,
    pub used_bytes: u64,
    pub used_pct: f64,
    pub status: String, // "ok" | "warn" | "critical" | "unlimited"
}

fn config_path(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}

fn read_quota_config(dir: &Path) -> QuotaConfig {
    fs::read_to_string(config_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_quota_config(dir: &Path, config: &QuotaConfig) -> Result<(), String> {
    let json = serde_json::to_string_pretty(config).map_err(|e| e.to_string())?;
    fs::write(config_path(dir), json).map_err(|e| format!("failed to write quota config: {}", e))
}

// Unreadable subtrees are skipped rather than failing the whole scan,
// matching operations::dir_size_and_count's approach.
fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

pub fn get_quota_status(dir: &Path) -> QuotaStatus {
    let config = read_quota_config(dir);
    let used = dir_size(dir);

    if config.quota_bytes == 0 {
        return QuotaStatus {
            quota_bytes: 0,
            used_bytes: used,
            used_pct: 0.0,
            status: "unlimited".to_string(),
        };
    }

    let pct = (used as f64 / config.quota_bytes as f64) * 100.0;
    let status = if pct >= config.critical_threshold_pct as f64 {
        "critical"
    } else if pct >= config.warn_threshold_pct as f64 {
        "warn"
    } else {
        "ok"
    };

    QuotaStatus {
        quota_bytes: config.quota_bytes,
        used_bytes: used,
        used_pct: pct,
        status: status.to_string(),
    }
}

pub fn set_quota_config(dir: &Path, quota_bytes: u64, warn_pct: u8, crit_pct: u8) -> Result<(), String> {
    write_quota_config(
        dir,
        &QuotaConfig {
            quota_bytes,
            warn_threshold_pct: warn_pct,
            critical_threshold_pct: crit_pct,
        },
    )
}

pub fn check_write_allowed(dir: &Path, additional_bytes: u64) -> bool {
    let config = read_quota_config(dir);
    if config.quota_bytes == 0 {
        return true;
    }
    dir_size(dir) + additional_bytes <= config.quota_bytes
}
