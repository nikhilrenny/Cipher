// Phase 1: core file operations backing the in-app right-click menu.
//
// Deliberately dependency-light — the only new crate is `trash`, so delete
// goes to the Recycle Bin rather than being unrecoverable. Folder sizing and
// clipboard state are plain std, avoiding another build-fragility surface
// (the pdfium/ffmpeg lesson).

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

// ===== Clipboard state =====
// Mutex::new is const, so no lazy_static/once_cell needed.

#[derive(Clone)]
struct Clip {
    paths: Vec<String>,
    cut: bool,
}

static CLIPBOARD: Mutex<Option<Clip>> = Mutex::new(None);

#[derive(Serialize)]
pub struct ClipboardInfo {
    pub has_content: bool,
    pub count: usize,
    pub name: String,
    pub paths: Vec<String>,
    pub cut: bool,
}

pub fn clipboard_state() -> ClipboardInfo {
    let guard = CLIPBOARD.lock().unwrap();
    match guard.as_ref() {
        Some(c) => ClipboardInfo {
            has_content: true,
            count: c.paths.len(),
            name: c.paths.first().map(|p| file_name_of(p)).unwrap_or_default(),
            paths: c.paths.clone(),
            cut: c.cut,
        },
        None => ClipboardInfo {
            has_content: false,
            count: 0,
            name: String::new(),
            paths: Vec::new(),
            cut: false,
        },
    }
}

fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn stage(paths: Vec<String>, cut: bool) -> Result<(), String> {
    if paths.is_empty() {
        return Err("nothing selected".to_string());
    }
    for p in &paths {
        if !Path::new(p).exists() {
            return Err(format!("not found: {}", p));
        }
    }
    *CLIPBOARD.lock().unwrap() = Some(Clip { paths, cut });
    Ok(())
}

pub fn cut_entry(paths: Vec<String>) -> Result<(), String> {
    stage(paths, true)
}

pub fn copy_entry(paths: Vec<String>) -> Result<(), String> {
    stage(paths, false)
}

// ===== Paste =====

// Never silently clobber: if the name is taken, append " (2)", " (3)", ...
fn unique_target(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    let ext = path.extension().map(|e| e.to_string_lossy().to_string());

    for n in 2..10_000 {
        let candidate_name = match &ext {
            Some(e) => format!("{} ({}).{}", stem, n, e),
            None => format!("{} ({})", stem, n),
        };
        let candidate = dir.join(candidate_name);
        if !candidate.exists() {
            return candidate;
        }
    }
    dir.join(name)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), String> {
    fs::create_dir_all(dest).map_err(|e| format!("create dir failed: {}", e))?;
    let entries = fs::read_dir(src).map_err(|e| format!("read dir failed: {}", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read entry failed: {}", e))?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|e| format!("copy failed: {}", e))?;
        }
    }
    Ok(())
}

pub fn paste_into(dest_dir: String) -> Result<Vec<String>, String> {
    let app_dir = crate::app_dir();
    // Paste/Move-to/Copy-to all funnel through here, and none of them were
    // ever validated against locks or the extension blacklist — create_folder
    // and create_empty_file block direct writes into a locked folder, but a
    // paste bypassed that entirely. This closes the gap: check the
    // destination once up front, then each incoming name against the
    // blacklist as it's processed.
    crate::restrictions::validate_write_to(&dest_dir, &app_dir)?;

    let clip = {
        let guard = CLIPBOARD.lock().unwrap();
        guard.clone().ok_or("clipboard is empty")?
    };

    let dest = Path::new(&dest_dir);
    if !dest.is_dir() {
        return Err(format!("not a folder: {}", dest_dir));
    }

    // Cut is consumed up front, same semantics as the single-item version —
    // a retry after a partial failure starts clean instead of re-pasting
    // whatever's left in the clipboard.
    if clip.cut {
        *CLIPBOARD.lock().unwrap() = None;
    }

    let mut results = Vec::with_capacity(clip.paths.len());
    for path_str in &clip.paths {
        let source = PathBuf::from(path_str);
        if !source.exists() {
            continue; // vanished since staging — skip rather than fail the whole batch
        }

        // Refuse to paste a folder into itself or its own descendant.
        if source.is_dir() && dest.starts_with(&source) {
            return Err(format!("can't paste \"{}\" into itself", file_name_of(path_str)));
        }

        let name = file_name_of(path_str);
        crate::restrictions::validate_filename(&name, &app_dir)?;
        let target = unique_target(dest, &name);

        if clip.cut {
            // rename() fails across volumes; fall back to copy-then-remove.
            if fs::rename(&source, &target).is_err() {
                if source.is_dir() {
                    copy_dir_recursive(&source, &target)?;
                    fs::remove_dir_all(&source).map_err(|e| format!("cleanup failed: {}", e))?;
                } else {
                    fs::copy(&source, &target).map_err(|e| format!("move failed: {}", e))?;
                    fs::remove_file(&source).map_err(|e| format!("cleanup failed: {}", e))?;
                }
            }
        } else if source.is_dir() {
            copy_dir_recursive(&source, &target)?;
        } else {
            fs::copy(&source, &target).map_err(|e| format!("copy failed: {}", e))?;
        }

        results.push(target.to_string_lossy().to_string());
    }

    Ok(results)
}

// ===== Delete / rename / create =====

pub fn delete_entry(path: String) -> Result<(), String> {
    let app_dir = crate::app_dir();
    if !Path::new(&path).exists() {
        return Err(format!("not found: {}", path));
    }
    // Validated against the item's own path (not just its parent) so a
    // per-item override actually applies here, not just at create-time.
    crate::restrictions::validate_write_to(&path, &app_dir)?;
    // Recycle Bin, not permanent — recoverable if this was a mistake.
    trash::delete(&path).map_err(|e| format!("delete failed: {}", e))
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("name can't be empty".to_string());
    }
    // Written as a char scan rather than an array pattern, which needs a
    // newer rustc than this project pins to.
    if name.chars().any(|c| "\\/:*?\"<>|".contains(c)) {
        return Err("name contains an invalid character".to_string());
    }
    Ok(())
}

pub fn rename_entry(path: String, new_name: String) -> Result<String, String> {
    validate_name(&new_name)?;
    let app_dir = crate::app_dir();
    crate::restrictions::validate_filename(&new_name, &app_dir)?;
    let source = Path::new(&path);
    if !source.exists() {
        return Err(format!("not found: {}", path));
    }
    // Validated against the item's own path, not its parent — see delete_entry.
    crate::restrictions::validate_write_to(&path, &app_dir)?;
    let parent = source.parent().ok_or("no parent directory")?;
    let target = parent.join(&new_name);
    if target.exists() {
        return Err(format!("\"{}\" already exists here", new_name));
    }
    fs::rename(source, &target).map_err(|e| format!("rename failed: {}", e))?;
    Ok(target.to_string_lossy().to_string())
}

pub fn create_folder(parent_path: String, name: String) -> Result<String, String> {
    validate_name(&name)?;
    let app_dir = crate::app_dir();
    crate::restrictions::validate_write_to(&parent_path, &app_dir)?;
    let target = Path::new(&parent_path).join(&name);
    if target.exists() {
        return Err(format!("\"{}\" already exists here", name));
    }
    fs::create_dir(&target).map_err(|e| format!("create folder failed: {}", e))?;
    Ok(target.to_string_lossy().to_string())
}

pub fn create_empty_file(parent_path: String, name: String) -> Result<String, String> {
    validate_name(&name)?;
    let app_dir = crate::app_dir();
    crate::restrictions::validate_write_to(&parent_path, &app_dir)?;
    crate::restrictions::validate_filename(&name, &app_dir)?;
    let target = Path::new(&parent_path).join(&name);
    if target.exists() {
        return Err(format!("\"{}\" already exists here", name));
    }
    fs::File::create(&target).map_err(|e| format!("create file failed: {}", e))?;
    Ok(target.to_string_lossy().to_string())
}

// ===== Properties =====

#[derive(Serialize)]
pub struct EntryProperties {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub item_count: Option<u64>,
    pub modified: Option<u64>,
    pub readonly: bool,
}

// Recursive size for folders. Unreadable subtrees are skipped rather than
// failing the whole call, matching how list_directory handles them.
fn dir_size_and_count(path: &Path) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut count = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            count += 1;
            let child = entry.path();
            if child.is_dir() {
                let (b, c) = dir_size_and_count(&child);
                bytes += b;
                count += c;
            } else if let Ok(meta) = entry.metadata() {
                bytes += meta.len();
            }
        }
    }
    (bytes, count)
}

pub fn entry_properties(path: String) -> Result<EntryProperties, String> {
    let target = Path::new(&path);
    let meta = fs::metadata(target).map_err(|e| format!("failed to read {}: {}", path, e))?;

    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64);

    let is_dir = meta.is_dir();
    let (size, item_count) = if is_dir {
        let (b, c) = dir_size_and_count(target);
        (b, Some(c))
    } else {
        (meta.len(), None)
    };

    Ok(EntryProperties {
        name: file_name_of(&path),
        path: path.clone(),
        is_dir,
        size,
        item_count,
        modified,
        readonly: meta.permissions().readonly(),
    })
}

// ===== Read-only flag =====

pub fn set_readonly(path: String, readonly: bool) -> Result<(), String> {
    let target = Path::new(&path);
    let meta = fs::metadata(target).map_err(|e| format!("failed to read {}: {}", path, e))?;
    let mut perms = meta.permissions();
    perms.set_readonly(readonly);
    fs::set_permissions(target, perms).map_err(|e| format!("failed to set readonly: {}", e))
}

// Cheap readonly check — unlike entry_properties, doesn't walk directories,
// so it's safe to call once per home-screen tile without a perf hit.
pub fn is_readonly(path: String) -> Result<bool, String> {
    let meta = fs::metadata(&path).map_err(|e| format!("failed to read {}: {}", path, e))?;
    Ok(meta.permissions().readonly())
}

// ===== Duplicate =====
// Reuses unique_target (already handles " (2)", " (3)", ... collisions)
// and copy_dir_recursive from the paste path above.

pub fn duplicate_entry(path: String) -> Result<String, String> {
    let app_dir = crate::app_dir();
    let source = Path::new(&path);
    if !source.exists() {
        return Err(format!("not found: {}", path));
    }
    // Validated against the item's own path, not its parent — see delete_entry.
    crate::restrictions::validate_write_to(&path, &app_dir)?;
    let parent = source.parent().ok_or("no parent directory")?;
    let name = file_name_of(&path);
    let target = unique_target(parent, &name);

    if source.is_dir() {
        copy_dir_recursive(source, &target)?;
    } else {
        fs::copy(source, &target).map_err(|e| format!("duplicate failed: {}", e))?;
    }
    Ok(target.to_string_lossy().to_string())
}

// ===== Batch rename =====
// Pattern tokens: {n} = 1-based sequence, {n:03} = zero-padded to 3 digits,
// {name} = original file stem (extension is preserved automatically).

pub fn batch_rename(paths: Vec<String>, pattern: String) -> Result<Vec<String>, String> {
    let app_dir = crate::app_dir();
    let mut results = Vec::with_capacity(paths.len());

    for (idx, path) in paths.iter().enumerate() {
        let source = Path::new(path);
        if !source.exists() {
            return Err(format!("not found: {}", path));
        }
        let stem = source
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.clone());
        let ext = source.extension().map(|e| e.to_string_lossy().to_string());
        let parent = source.parent().ok_or("no parent directory")?;
        // Validated against the item's own path, not its parent — see delete_entry.
        crate::restrictions::validate_write_to(path, &app_dir)?;

        let seq = idx + 1;
        let mut new_stem = pattern
            .replace("{n:03}", &format!("{:03}", seq))
            .replace("{n}", &seq.to_string())
            .replace("{name}", &stem);
        validate_name(&new_stem)?;

        if let Some(e) = &ext {
            new_stem = format!("{}.{}", new_stem, e);
        }
        crate::restrictions::validate_filename(&new_stem, &app_dir)?;
        let target = parent.join(&new_stem);
        if target.exists() && target != source {
            return Err(format!("\"{}\" already exists here", new_stem));
        }
        fs::rename(source, &target).map_err(|e| format!("rename failed: {}", e))?;
        results.push(target.to_string_lossy().to_string());
    }

    Ok(results)
}
