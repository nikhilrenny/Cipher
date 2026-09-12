// Fresh start. No Node sidecar — vault/crypto/MCP now live natively in
// Rust. This is deliberately just enough to open a window, confirm WebGL2
// renders, and prove the vault round-trips (create agent -> write encrypted
// note -> read it back) before building UI polish or the MCP server on top.

mod bookmarks;
mod crypto;
mod explorer;
mod favorites;
mod hashing;
mod keybindings;
mod file_crypto;
mod operations;
mod quick_access;
mod restrictions;
mod thumbnail;
mod usage;
mod vault;
mod key_backup;
mod vault_git;
mod git_identity;
mod github_auth;
mod search;
use search::SearchIndex;

use std::path::{Path, PathBuf};
use tauri::command;

fn app_dir() -> PathBuf {
    if cfg!(debug_assertions) {
        // Dev-mode: cargo runs from src-tauri/, so ".." is the project root.
        // Keys/vault live there, matching where they lived in the previous
        // (Node-based) project — same reasoning, not carried over by habit.
        std::env::current_dir()
            .expect("failed to read current directory")
            .join("..")
    } else {
        // Release/installed builds: the working directory is wherever
        // Windows launches the .exe from (e.g. Program Files), which is
        // neither writable nor a sensible data location. Use the standard
        // per-user app-data directory instead.
        let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
        let dir = base.join("Cipher");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}

#[command]
fn vault_create_agent(agent_id: String, passphrase: String) -> Result<crypto::AgentPublicRecord, String> {
    vault::create_agent(&app_dir(), &agent_id, &passphrase)
}

#[command]
fn vault_write_note(agent_id: String, passphrase: String, title: String, content: String, tags: Vec<String>) -> Result<vault::NoteSummary, String> {
    vault::write_note(&app_dir(), &agent_id, &passphrase, &title, &content, tags)
}

#[command]
fn vault_read_note(agent_id: String, passphrase: String, id: String) -> Result<vault::DecryptedNote, String> {
    vault::read_note(&app_dir(), &agent_id, &passphrase, &id)
}

#[command]
fn vault_list_notes() -> Result<Vec<vault::NoteSummary>, String> {
    vault::list_notes(&app_dir())
}

#[command]
fn get_drives() -> Vec<explorer::DriveInfo> {
    explorer::get_drives()
}

#[command]
fn open_in_os_explorer(path: String) -> Result<(), String> {
    explorer::open_in_os_explorer(&path)
}

#[command]
fn get_common_folders() -> Vec<explorer::CommonFolder> {
    explorer::get_common_folders()
}

#[command]
fn list_directory(path: String) -> Result<Vec<explorer::FileEntry>, String> {
    explorer::list_directory(&path)
}

#[command]
fn get_thumbnail(path: String) -> Result<String, String> {
    thumbnail::get_or_create_thumbnail(&path)
}

#[command]
fn get_recent_files(limit: usize) -> Vec<usage::UsageEntry> {
    usage::get_recent(&app_dir(), limit)
}

#[command]
fn get_most_used_files(limit: usize) -> Vec<usage::UsageEntry> {
    usage::get_most_used(&app_dir(), limit)
}

#[command]
fn record_file_access(path: String, name: String) -> Result<(), String> {
    usage::record_access(&app_dir(), &path, &name)
}

#[command]
fn add_favorite(path: String, name: String) -> Result<(), String> {
    favorites::add_favorite(&app_dir(), &path, &name)
}

#[command]
fn remove_favorite(path: String) -> Result<(), String> {
    favorites::remove_favorite(&app_dir(), &path)
}

#[command]
fn list_favorites() -> Vec<favorites::FavoriteEntry> {
    favorites::list_favorites(&app_dir())
}

#[command]
fn add_bookmark(path: String, name: String) -> Result<(), String> {
    bookmarks::add_bookmark(&app_dir(), &path, &name)
}

#[command]
fn remove_bookmark(path: String) -> Result<(), String> {
    bookmarks::remove_bookmark(&app_dir(), &path)
}

#[command]
fn list_bookmarks() -> Vec<bookmarks::BookmarkEntry> {
    bookmarks::list_bookmarks(&app_dir())
}

#[command]
fn get_bookmarks_location() -> Option<String> {
    bookmarks::get_location(&app_dir())
}

#[command]
fn set_bookmarks_location(location: Option<String>) -> Result<(), String> {
    bookmarks::set_location(&app_dir(), location)
}

#[command]
fn compute_file_hashes(path: String) -> Result<hashing::FileHashes, String> {
    hashing::compute_file_hashes(&path)
}

#[command]
fn get_keybindings() -> Vec<keybindings::KeyAction> {
    keybindings::get_keybindings(&app_dir())
}

#[command]
fn set_keybinding(action: String, combo: String) -> Result<(), String> {
    keybindings::set_keybinding(&app_dir(), &action, &combo)
}

#[command]
fn reset_keybindings() -> Result<(), String> {
    keybindings::reset_keybindings(&app_dir())
}

// ===== File operations (right-click menu) =====

#[command]
fn cut_entry(paths: Vec<String>) -> Result<(), String> {
    operations::cut_entry(paths)
}

#[command]
fn copy_entry(paths: Vec<String>) -> Result<(), String> {
    operations::copy_entry(paths)
}

#[command]
fn paste_into(dest_dir: String) -> Result<Vec<String>, String> {
    let clip_before = operations::clipboard_state();
    let was_cut = clip_before.cut;
    let old_paths = clip_before.paths.clone();
    let results = operations::paste_into(dest_dir)?;
    if was_cut {
        // Cache entries follow a moved file to its new location instead of
        // going stale — paired by index since paste_into processes the
        // clipboard in order (a source that vanished just yields fewer
        // results, in which case the tail pairs are simply skipped).
        let dir = app_dir();
        for (old, new) in old_paths.iter().zip(results.iter()) {
            let new_name = Path::new(new)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let _ = usage::update_path(&dir, old, new, &new_name);
            let _ = favorites::update_path(&dir, old, new, &new_name);
            let _ = quick_access::update_path(&dir, old, new, &new_name);
        }
    }
    Ok(results)
}

#[command]
fn clipboard_state() -> operations::ClipboardInfo {
    operations::clipboard_state()
}

#[command]
fn delete_entry(path: String) -> Result<(), String> {
    operations::delete_entry(path.clone())?;
    // Best-effort cleanup: a deleted file shouldn't linger as a dead entry
    // in Recent/Most used/Favorites/Quick access on the home screen.
    let dir = app_dir();
    let _ = usage::remove_usage_under(&dir, &path);
    let _ = favorites::remove_favorites_under(&dir, &path);
    let _ = quick_access::remove_quick_access_under(&dir, &path);
    Ok(())
}

#[command]
fn rename_entry(path: String, new_name: String) -> Result<String, String> {
    let new_path = operations::rename_entry(path.clone(), new_name.clone())?;
    // Same reasoning as the paste_into cache follow-up: a renamed file
    // shouldn't leave a dead entry behind in Recent/Favorites/Quick access.
    let dir = app_dir();
    let _ = usage::update_path(&dir, &path, &new_path, &new_name);
    let _ = favorites::update_path(&dir, &path, &new_path, &new_name);
    let _ = quick_access::update_path(&dir, &path, &new_path, &new_name);
    Ok(new_path)
}

#[command]
fn create_folder(parent_path: String, name: String) -> Result<String, String> {
    operations::create_folder(parent_path, name)
}

#[command]
fn create_empty_file(parent_path: String, name: String) -> Result<String, String> {
    operations::create_empty_file(parent_path, name)
}

#[command]
fn entry_properties(path: String) -> Result<operations::EntryProperties, String> {
    operations::entry_properties(path)
}

#[command]
fn set_readonly(path: String, readonly: bool) -> Result<(), String> {
    operations::set_readonly(path, readonly)
}

#[command]
fn is_readonly(path: String) -> Result<bool, String> {
    operations::is_readonly(path)
}

#[command]
fn duplicate_entry(path: String) -> Result<String, String> {
    operations::duplicate_entry(path)
}

#[command]
fn batch_rename(paths: Vec<String>, pattern: String) -> Result<Vec<String>, String> {
    operations::batch_rename(paths, pattern)
}

// ===== Clear cache (Settings) =====
// Best-effort: reports the first error but still attempts every step, so a
// locked thumbnail file (say) doesn't stop the JSON caches from clearing.

#[command]
fn clear_all_caches() -> Result<(), String> {
    let dir = app_dir();
    let mut first_err = None;
    // Quick access is user-curated pins, not derived cache — clearing
    // cache should never wipe it (or drop it back to the OS-seeded
    // defaults). Only Recent/Most-used/Favorites/thumbnails are cleared.
    for res in [
        usage::clear(&dir),
        favorites::clear(&dir),
        thumbnail::clear_cache(),
    ] {
        if let Err(e) = res {
            if first_err.is_none() {
                first_err = Some(e);
            }
        }
    }
    match first_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

// ===== Quick access (left icon strip) =====

#[command]
fn list_quick_access() -> Vec<quick_access::QuickAccessEntry> {
    quick_access::list_quick_access(&app_dir())
}

#[command]
fn add_quick_access(path: String, name: String) -> Result<(), String> {
    quick_access::add_quick_access(&app_dir(), &path, &name)
}

#[command]
fn remove_quick_access(path: String) -> Result<(), String> {
    quick_access::remove_quick_access(&app_dir(), &path)
}

// ===== File Restrictions (Phase 1.3) =====

#[command]
fn set_folder_write_lock(folder_path: String, locked: bool) -> Result<(), String> {
    restrictions::set_folder_write_lock(&folder_path, locked)
}

#[command]
fn is_write_locked(path: String) -> Result<bool, String> {
    restrictions::is_write_locked(&path)
}

#[command]
fn get_write_lock_parent(path: String) -> Result<Option<String>, String> {
    restrictions::get_write_lock_parent(&path)
}

#[command]
fn load_blacklist() -> Result<restrictions::Blacklist, String> {
    restrictions::load_blacklist(&app_dir())
}

#[command]
fn add_to_blacklist(extension: String) -> Result<(), String> {
    restrictions::add_to_blacklist(&app_dir(), &extension)
}

#[command]
fn remove_from_blacklist(extension: String) -> Result<(), String> {
    restrictions::remove_from_blacklist(&app_dir(), &extension)
}

#[command]
fn is_lock_override(path: String) -> Result<bool, String> {
    Ok(restrictions::is_override(&app_dir(), &path))
}

#[command]
fn add_lock_override(path: String) -> Result<(), String> {
    restrictions::add_override(&app_dir(), &path)
}

#[command]
fn remove_lock_override(path: String) -> Result<(), String> {
    restrictions::remove_override(&app_dir(), &path)
}

// ===== Phase 2.5: Key Backup & Recovery =====

#[command]
fn generate_seed_phrase() -> Result<String, String> {
    Ok(key_backup::KeyBackup::generate_seed_phrase())
}

#[command]
fn validate_seed_phrase(phrase: String) -> Result<bool, String> {
    Ok(key_backup::KeyBackup::validate_seed_phrase(&phrase))
}

#[command]
fn create_backup(seed_phrase: String, passphrase: String) -> Result<String, String> {
    let backup = key_backup::KeyBackup::new(None);
    let result = backup.create_backup(&seed_phrase, &passphrase)?;
    Ok(result.backup_id)
}

#[command]
fn recover_from_backup(backup_file: String, passphrase: String) -> Result<String, String> {
    let backup = key_backup::KeyBackup::new(None);
    backup.recover_from_backup(&backup_file, &passphrase)
}

#[command]
fn list_backups() -> Result<Vec<String>, String> {
    let backup = key_backup::KeyBackup::new(None);
    let backups = backup.list_backups()?;
    Ok(backups.into_iter().map(|(f, _)| f).collect())
}

#[command]
fn delete_backup(backup_id: String) -> Result<(), String> {
    let backup = key_backup::KeyBackup::new(None);
    backup.delete_backup(&backup_id)
}

// Dev-convenience "start over" switch, not part of any user-facing recovery
// flow: wipes every agent keypair, every vault note, and every seed-phrase
// backup on this machine. Does NOT touch files/folders encrypted via the
// Encrypt right-click action — those use a one-off passphrase with no
// persisted key material, so there's nothing here to reset for them.
#[command]
fn reset_all_keys() -> Result<(), String> {
    let dir = app_dir();
    let _ = std::fs::remove_dir_all(dir.join("keys"));
    let _ = std::fs::remove_dir_all(dir.join("vault"));
    let backup_dir = dirs::home_dir().unwrap_or_default().join(".cipher").join("backups");
    let _ = std::fs::remove_dir_all(&backup_dir);
    Ok(())
}

// ===== Phase 2: File/folder encryption (right-click Encrypt/Decrypt) =====

#[command]
fn encrypt_path(path: String, passphrase: String) -> Result<String, String> {
    let p = Path::new(&path);
    if p.is_dir() {
        let count = file_crypto::encrypt_folder(p, &passphrase)?;
        Ok(format!("Encrypted {} file(s)", count))
    } else {
        let out = file_crypto::encrypt_file(p, &passphrase)?;
        let name = out.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        Ok(format!("Encrypted: {}", name))
    }
}

#[command]
fn decrypt_path(path: String, passphrase: String) -> Result<String, String> {
    let p = Path::new(&path);
    if p.is_dir() {
        let count = file_crypto::decrypt_folder(p, &passphrase)?;
        Ok(format!("Decrypted {} file(s)", count))
    } else {
        let out = file_crypto::decrypt_file(p, &passphrase)?;
        let name = out.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        Ok(format!("Decrypted: {}", name))
    }
}

// ===== Phase 3: Git Integration =====

#[command]
fn init_vault(vault_path: String) -> Result<String, String> {
    vault_git::VaultGit::init(&vault_path, true)?;
    Ok(format!("Vault initialized at {}", vault_path))
}

#[command]
fn auto_commit(vault_path: String, message: String) -> Result<String, String> {
    let vault = vault_git::VaultGit::open(&vault_path)?;
    let identity = git_identity::load(&app_dir());
    vault.auto_commit(&message, &identity.name, &identity.email)
}

#[command]
fn is_git_repo(path: String) -> Result<bool, String> {
    Ok(vault_git::VaultGit::is_repo(&path))
}

#[command]
fn git_has_changes(path: String) -> Result<bool, String> {
    let vault = vault_git::VaultGit::open(&path)?;
    vault.has_changes()
}

#[command]
fn get_current_branch(path: String) -> Result<String, String> {
    let vault = vault_git::VaultGit::open(&path)?;
    vault.current_branch()
}

#[command]
fn get_git_identity() -> Result<git_identity::GitIdentity, String> {
    Ok(git_identity::load(&app_dir()))
}

#[command]
fn set_git_identity(name: String, email: String, default_location: Option<String>) -> Result<(), String> {
    git_identity::save(&app_dir(), &git_identity::GitIdentity { name, email, default_location })
}

#[command]
fn get_commit_history(vault_path: String, max_count: usize) -> Result<Vec<vault_git::CommitInfo>, String> {
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.get_history(max_count)
}

#[command]
fn list_branches(vault_path: String) -> Result<Vec<String>, String> {
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.list_branches()
}

#[command]
fn create_branch(vault_path: String, branch_name: String) -> Result<(), String> {
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.create_branch(&branch_name)
}

#[command]
fn checkout_branch(vault_path: String, branch_name: String) -> Result<(), String> {
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.checkout_branch(&branch_name)
}

// ===== Phase 3: Remote, push, pull, conflicts =====

#[command]
fn get_github_token_status() -> Result<bool, String> {
    Ok(github_auth::has_token(&app_dir()))
}

#[command]
fn set_github_token(token: String) -> Result<(), String> {
    github_auth::save(&app_dir(), &github_auth::GitHubAuth { token })
}

#[command]
fn set_remote(vault_path: String, name: String, url: String) -> Result<(), String> {
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.add_remote(&name, &url)
}

#[command]
fn get_remote(vault_path: String, name: String) -> Result<Option<String>, String> {
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.get_remote_url(&name)
}

#[command]
fn git_push(vault_path: String, remote_name: String, branch_name: String) -> Result<(), String> {
    let auth = github_auth::load(&app_dir());
    if auth.token.trim().is_empty() {
        return Err("No GitHub token configured — add one in Settings → GitHub.".to_string());
    }
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.push(&remote_name, &branch_name, &auth.token)
}

#[command]
fn git_pull(vault_path: String, remote_name: String, branch_name: String) -> Result<vault_git::PullResult, String> {
    let auth = github_auth::load(&app_dir());
    if auth.token.trim().is_empty() {
        return Err("No GitHub token configured — add one in Settings → GitHub.".to_string());
    }
    let identity = git_identity::load(&app_dir());
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.pull(&remote_name, &branch_name, &auth.token, &identity.name, &identity.email)
}

#[command]
fn git_fetch(vault_path: String, remote_name: String, branch_name: String) -> Result<(usize, usize), String> {
    let auth = github_auth::load(&app_dir());
    if auth.token.trim().is_empty() {
        return Err("No GitHub token configured — add one in Settings → GitHub.".to_string());
    }
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.fetch(&remote_name, &branch_name, &auth.token)?;
    vault.ahead_behind(&remote_name, &branch_name)
}

#[command]
fn get_ahead_behind(vault_path: String, remote_name: String, branch_name: String) -> Result<(usize, usize), String> {
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.ahead_behind(&remote_name, &branch_name)
}

#[command]
fn resolve_conflict_file(vault_path: String, file_path: String, action: String) -> Result<(), String> {
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.resolve_conflict_file(&file_path, &action)
}

#[command]
fn finish_merge(vault_path: String) -> Result<String, String> {
    let identity = git_identity::load(&app_dir());
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.finalize_merge(&identity.name, &identity.email)
}

#[command]
fn abort_merge(vault_path: String) -> Result<(), String> {
    let vault = vault_git::VaultGit::open(&vault_path)?;
    vault.abort_merge()
}

// ===== Phase 4: Indexing & Search (Drives Only) =====

#[command]
fn init_search_index() -> Result<String, String> {
    SearchIndex::init_index()
}

#[command]
fn search_list_drives() -> Result<Vec<search::DriveInfo>, String> {
    SearchIndex::list_drives()
}

#[command]
fn get_drive_index_status() -> Result<search::IndexStatus, String> {
    SearchIndex::get_status()
}

#[command]
fn rebuild_drive_index() -> Result<u32, String> {
    SearchIndex::rebuild_index()
}

#[command]
fn index_drive(drive: String, app_handle: tauri::AppHandle) -> Result<(), String> {
    // Spawned rather than run inline: a #[command] body runs on Tauri's main
    // thread, which blocks the webview — so every `index-progress` event
    // queued during the walk only rendered once the whole thing finished,
    // making the panel look frozen at 0 files. Progress now arrives purely
    // through events; the command itself returns immediately.
    std::thread::spawn(move || {
        let _ = SearchIndex::index_drive(&app_handle, &drive);
    });
    Ok(())
}

#[command]
fn search_files(query: String, limit: u32, offset: u32, file_types: Option<Vec<String>>, encrypted_only: Option<bool>) -> Result<Vec<search::SearchResult>, String> {
    SearchIndex::search_files(&query, limit, offset, file_types.as_deref(), encrypted_only.unwrap_or(false))
}

#[command]
fn search_by_type(file_type: String, limit: u32, offset: u32) -> Result<Vec<search::SearchResult>, String> {
    SearchIndex::search_by_type(&file_type, limit, offset)
}

#[command]
fn watch_drive(drive: String, app_handle: tauri::AppHandle) -> Result<(), String> {
    SearchIndex::watch_drive(app_handle, drive)
}

#[command]
fn unwatch_drive(drive: String) -> Result<(), String> {
    SearchIndex::unwatch_drive(&drive)
}

#[command]
fn is_watching_drive(drive: String) -> Result<bool, String> {
    Ok(SearchIndex::is_watching(&drive))
}

fn main() {
    // Hybrid-graphics laptop (Intel Arc iGPU + NVIDIA discrete GPU). WebView2
    // defaults to the power-saving iGPU; this asks its internal Chromium
    // engine for the high-performance GPU instead. The blank page's on-screen
    // "GPU:" readout is what actually confirms whether this took effect.
    std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", "--force_high_performance_gpu");

    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            vault_create_agent,
            vault_write_note,
            vault_read_note,
            vault_list_notes,
            get_drives,
            open_in_os_explorer,
            get_common_folders,
            list_directory,
            get_thumbnail,
            get_recent_files,
            get_most_used_files,
            record_file_access,
            add_favorite,
            remove_favorite,
            list_favorites,
            add_bookmark,
            remove_bookmark,
            list_bookmarks,
            get_bookmarks_location,
            set_bookmarks_location,
            compute_file_hashes,
            get_keybindings,
            set_keybinding,
            reset_keybindings,
            cut_entry,
            copy_entry,
            paste_into,
            clipboard_state,
            delete_entry,
            rename_entry,
            create_folder,
            create_empty_file,
            entry_properties,
            set_readonly,
            is_readonly,
            duplicate_entry,
            batch_rename,
            clear_all_caches,
            list_quick_access,
            add_quick_access,
            remove_quick_access,
            set_folder_write_lock,
            is_write_locked,
            get_write_lock_parent,
            load_blacklist,
            add_to_blacklist,
            remove_from_blacklist,
            is_lock_override,
            add_lock_override,
            remove_lock_override,
            generate_seed_phrase,
            validate_seed_phrase,
            create_backup,
            recover_from_backup,
            list_backups,
            delete_backup,
            reset_all_keys,
            encrypt_path,
            decrypt_path,
            init_vault,
            auto_commit,
            is_git_repo,
            git_has_changes,
            get_current_branch,
            get_git_identity,
            set_git_identity,
            get_commit_history,
            list_branches,
            create_branch,
            checkout_branch,
            get_github_token_status,
            set_github_token,
            set_remote,
            get_remote,
            git_push,
            git_pull,
            git_fetch,
            get_ahead_behind,
            resolve_conflict_file,
            finish_merge,
            abort_merge,
            init_search_index,
            search_list_drives,
            get_drive_index_status,
            rebuild_drive_index,
            index_drive,
            search_files,
            search_by_type,
            watch_drive,
            unwatch_drive,
            is_watching_drive,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Cipher");
}
