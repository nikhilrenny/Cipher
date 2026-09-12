// Phase 3: Git Integration & Auto-Sync
// Local repo lifecycle (init/open/commit/history/branches), remote push
// and pull with GitHub PAT auth over HTTPS, and real 3-way merge conflict
// detection + per-file resolution (keep local / use remote / keep both).

use git2::{Repository, Signature};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultConfig {
    pub vault_path: PathBuf,
    pub auto_commit: bool,
    pub auto_commit_interval_ms: u64,
}

pub struct VaultGit {
    pub repo: Repository,
    pub config: VaultConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitInfo {
    pub oid: String,
    pub message: String,
    pub author: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictFile {
    pub path: String,
    pub local_content: String,
    pub remote_content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullResult {
    /// "up_to_date" | "fast_forward" | "merged" | "conflicts"
    pub status: String,
    pub oid: Option<String>,
    pub conflicts: Vec<ConflictFile>,
}

impl VaultGit {
    /// Initialize vault as git repository (or open if it already exists)
    pub fn init(vault_path: &str, auto_commit: bool) -> Result<Self, String> {
        let path = Path::new(vault_path);

        let repo = match Repository::open(path) {
            Ok(r) => r,
            Err(_) => Repository::init(path).map_err(|e| format!("Failed to init repo: {}", e))?,
        };

        let config = VaultConfig {
            vault_path: path.to_path_buf(),
            auto_commit,
            auto_commit_interval_ms: 5000,
        };

        // Best-effort: persist config next to the vault, don't fail init on write errors
        let config_path = path.join(".vault-config.json");
        if let Ok(json) = serde_json::to_string_pretty(&config) {
            let _ = std::fs::write(&config_path, json);
        }

        Ok(VaultGit { repo, config })
    }

    /// Check whether a path is already a git repository.
    pub fn is_repo(path: &str) -> bool {
        Repository::open(path).is_ok()
    }

    /// Open an existing repository without ever creating one.
    pub fn open(path: &str) -> Result<Self, String> {
        let repo = Repository::open(path).map_err(|e| format!("Not a git repository: {}", e))?;
        let config = VaultConfig {
            vault_path: PathBuf::from(path),
            auto_commit: true,
            auto_commit_interval_ms: 5000,
        };
        Ok(VaultGit { repo, config })
    }

    /// True if there are any staged or unstaged changes to commit.
    pub fn has_changes(&self) -> Result<bool, String> {
        let statuses = self.repo.statuses(None).map_err(|e| format!("Status check failed: {}", e))?;
        Ok(!statuses.is_empty())
    }

    /// Current branch's short name, or "detached" if HEAD isn't a branch tip.
    pub fn current_branch(&self) -> Result<String, String> {
        let head = self.repo.head().map_err(|e| format!("Get HEAD failed: {}", e))?;
        if head.is_branch() {
            Ok(head.shorthand().unwrap_or("HEAD").to_string())
        } else {
            Ok("detached".to_string())
        }
    }

    /// Stage all changes and create a commit, signed with the given identity.
    pub fn auto_commit(&self, message: &str, author_name: &str, author_email: &str) -> Result<String, String> {
        let mut index = self.repo.index().map_err(|e| format!("Index failed: {}", e))?;

        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .map_err(|e| format!("Add failed: {}", e))?;
        index.write().map_err(|e| format!("Write index failed: {}", e))?;

        let tree_id = index.write_tree().map_err(|e| format!("Write tree failed: {}", e))?;
        let tree = self.repo.find_tree(tree_id).map_err(|e| format!("Find tree failed: {}", e))?;

        let parent_commit = match self.repo.head() {
            Ok(h) => {
                let oid = h.target().ok_or("No HEAD target")?;
                vec![self.repo.find_commit(oid).map_err(|e| format!("Find parent failed: {}", e))?]
            }
            Err(_) => vec![], // first commit
        };

        let sig = Signature::now(author_name, author_email)
            .map_err(|e| format!("Signature failed: {}", e))?;

        let oid = self
            .repo
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                message,
                &tree,
                &parent_commit.iter().collect::<Vec<_>>(),
            )
            .map_err(|e| format!("Commit failed: {}", e))?;

        Ok(oid.to_string())
    }

    /// Get commit history (most recent first)
    pub fn get_history(&self, max_count: usize) -> Result<Vec<CommitInfo>, String> {
        let mut commits = Vec::new();
        let mut revwalk = self.repo.revwalk().map_err(|e| format!("Revwalk failed: {}", e))?;
        revwalk.push_head().map_err(|e| format!("Push HEAD failed: {}", e))?;

        for (i, oid_result) in revwalk.enumerate() {
            if i >= max_count {
                break;
            }
            let oid = oid_result.map_err(|e| format!("Revwalk iteration failed: {}", e))?;
            let commit = self.repo.find_commit(oid).map_err(|e| format!("Find commit failed: {}", e))?;

            commits.push(CommitInfo {
                oid: oid.to_string(),
                message: commit.message().unwrap_or("").to_string(),
                author: commit.author().name().unwrap_or("unknown").to_string(),
                timestamp: commit.time().seconds().to_string(),
            });
        }

        Ok(commits)
    }

    /// List local branches only (remote-tracking branches like
    /// "origin/hello" live under refs/remotes/, not refs/heads/, so they're
    /// excluded here — checkout_branch only knows how to check out a local
    /// branch, and showing remotes in a click-to-checkout list is misleading).
    pub fn list_branches(&self) -> Result<Vec<String>, String> {
        let branches = self
            .repo
            .branches(Some(git2::BranchType::Local))
            .map_err(|e| format!("List branches failed: {}", e))?;

        let mut branch_names = Vec::new();
        for branch_result in branches {
            let (branch, _) = branch_result.map_err(|e| format!("Branch iteration failed: {}", e))?;
            if let Ok(Some(name)) = branch.name() {
                branch_names.push(name.to_string());
            }
        }

        Ok(branch_names)
    }

    /// Create a new branch pointing at HEAD
    pub fn create_branch(&self, branch_name: &str) -> Result<(), String> {
        let head = self.repo.head().map_err(|e| format!("Get HEAD failed: {}", e))?;
        let commit = head.peel_to_commit().map_err(|e| format!("Peel failed: {}", e))?;

        self.repo
            .branch(branch_name, &commit, false)
            .map_err(|e| format!("Create branch failed: {}", e))?;

        Ok(())
    }

    /// Switch to an existing branch
    pub fn checkout_branch(&self, branch_name: &str) -> Result<(), String> {
        let obj = self
            .repo
            .revparse_single(&format!("refs/heads/{}", branch_name))
            .map_err(|e| format!("Find ref failed: {}", e))?;

        self.repo.checkout_tree(&obj, None).map_err(|e| format!("Checkout failed: {}", e))?;
        self.repo
            .set_head(&format!("refs/heads/{}", branch_name))
            .map_err(|e| format!("Failed to set HEAD: {}", e))?;

        Ok(())
    }

    // ===== Remote =====

    /// Add (or replace) a remote by name.
    pub fn add_remote(&self, name: &str, url: &str) -> Result<(), String> {
        if self.repo.find_remote(name).is_ok() {
            self.repo
                .remote_delete(name)
                .map_err(|e| format!("Failed to remove existing remote: {}", e))?;
        }
        self.repo.remote(name, url).map_err(|e| format!("Failed to add remote: {}", e))?;
        Ok(())
    }

    /// Get a remote's URL, or None if it doesn't exist.
    pub fn get_remote_url(&self, name: &str) -> Result<Option<String>, String> {
        match self.repo.find_remote(name) {
            Ok(remote) => Ok(remote.url().ok().map(|s| s.to_string())),
            Err(_) => Ok(None),
        }
    }

    fn credential_callbacks(token: &str) -> git2::RemoteCallbacks<'static> {
        let mut callbacks = git2::RemoteCallbacks::new();
        let token_owned = token.to_string();
        callbacks.credentials(move |_url, username_from_url, _allowed| {
            // GitHub PAT over HTTPS: any non-empty username works, PAT is the password.
            git2::Cred::userpass_plaintext(username_from_url.unwrap_or("x-access-token"), &token_owned)
        });
        callbacks
    }

    /// Fetch from a remote (does not merge).
    pub fn fetch(&self, remote_name: &str, branch_name: &str, token: &str) -> Result<(), String> {
        let mut remote = self
            .repo
            .find_remote(remote_name)
            .map_err(|e| format!("Remote '{}' not found: {}", remote_name, e))?;

        let mut fetch_opts = git2::FetchOptions::new();
        fetch_opts.remote_callbacks(Self::credential_callbacks(token));

        remote
            .fetch(&[branch_name], Some(&mut fetch_opts), None)
            .map_err(|e| format!("Fetch failed: {}", e))?;

        Ok(())
    }

    /// How many commits local is ahead/behind the remote-tracking branch.
    /// (0, 0) if there's no remote-tracking ref yet (never fetched).
    pub fn ahead_behind(&self, remote_name: &str, branch_name: &str) -> Result<(usize, usize), String> {
        let local_oid = self
            .repo
            .refname_to_id(&format!("refs/heads/{}", branch_name))
            .map_err(|e| format!("Local branch not found: {}", e))?;

        let remote_ref = format!("refs/remotes/{}/{}", remote_name, branch_name);
        let remote_oid = match self.repo.refname_to_id(&remote_ref) {
            Ok(oid) => oid,
            Err(_) => return Ok((0, 0)),
        };

        self.repo
            .graph_ahead_behind(local_oid, remote_oid)
            .map_err(|e| format!("Ahead/behind check failed: {}", e))
    }

    /// Push the given branch to a remote.
    pub fn push(&self, remote_name: &str, branch_name: &str, token: &str) -> Result<(), String> {
        let mut remote = self
            .repo
            .find_remote(remote_name)
            .map_err(|e| format!("Remote '{}' not found: {}", remote_name, e))?;

        let mut push_opts = git2::PushOptions::new();
        push_opts.remote_callbacks(Self::credential_callbacks(token));

        let refspec = format!("refs/heads/{}:refs/heads/{}", branch_name, branch_name);
        remote
            .push(&[&refspec], Some(&mut push_opts))
            .map_err(|e| format!("Push failed: {}", e))?;

        Ok(())
    }

    /// Fetch + merge. Fast-forwards or clean 3-way merges complete
    /// immediately. Real content conflicts leave the repo mid-merge and
    /// return the conflicting files for resolution via
    /// `resolve_conflict_file` + `finalize_merge`.
    pub fn pull(
        &self,
        remote_name: &str,
        branch_name: &str,
        token: &str,
        author_name: &str,
        author_email: &str,
    ) -> Result<PullResult, String> {
        self.fetch(remote_name, branch_name, token)?;

        let remote_ref_name = format!("refs/remotes/{}/{}", remote_name, branch_name);
        let remote_oid = self
            .repo
            .refname_to_id(&remote_ref_name)
            .map_err(|e| format!("Remote branch not found after fetch: {}", e))?;
        let remote_commit = self
            .repo
            .find_annotated_commit(remote_oid)
            .map_err(|e| format!("Failed to load remote commit: {}", e))?;

        let (analysis, _) = self
            .repo
            .merge_analysis(&[&remote_commit])
            .map_err(|e| format!("Merge analysis failed: {}", e))?;

        if analysis.is_up_to_date() {
            return Ok(PullResult { status: "up_to_date".into(), oid: None, conflicts: vec![] });
        }

        if analysis.is_fast_forward() {
            let branch_ref_name = format!("refs/heads/{}", branch_name);
            let mut reference = self
                .repo
                .find_reference(&branch_ref_name)
                .map_err(|e| format!("Branch ref not found: {}", e))?;
            reference
                .set_target(remote_oid, "Fast-forward pull")
                .map_err(|e| format!("Failed to fast-forward: {}", e))?;
            self.repo.set_head(&branch_ref_name).map_err(|e| format!("Failed to set HEAD: {}", e))?;
            let mut co = git2::build::CheckoutBuilder::new();
            co.force();
            self.repo.checkout_head(Some(&mut co)).map_err(|e| format!("Checkout failed: {}", e))?;
            return Ok(PullResult { status: "fast_forward".into(), oid: Some(remote_oid.to_string()), conflicts: vec![] });
        }

        // Normal (non-fast-forward) merge.
        self.repo.merge(&[&remote_commit], None, None).map_err(|e| format!("Merge failed: {}", e))?;

        let index = self.repo.index().map_err(|e| format!("Index failed: {}", e))?;

        if index.has_conflicts() {
            let mut conflicts = Vec::new();
            let conflict_iter = index.conflicts().map_err(|e| format!("Failed to read conflicts: {}", e))?;
            for conflict in conflict_iter {
                let conflict = conflict.map_err(|e| format!("Conflict iteration error: {}", e))?;
                let path = conflict
                    .our
                    .as_ref()
                    .or(conflict.their.as_ref())
                    .map(|e| String::from_utf8_lossy(&e.path).to_string())
                    .unwrap_or_default();
                let local_content = conflict
                    .our
                    .as_ref()
                    .and_then(|e| self.repo.find_blob(e.id).ok())
                    .and_then(|b| String::from_utf8(b.content().to_vec()).ok())
                    .unwrap_or_default();
                let remote_content = conflict
                    .their
                    .as_ref()
                    .and_then(|e| self.repo.find_blob(e.id).ok())
                    .and_then(|b| String::from_utf8(b.content().to_vec()).ok())
                    .unwrap_or_default();
                conflicts.push(ConflictFile { path, local_content, remote_content });
            }
            return Ok(PullResult { status: "conflicts".into(), oid: None, conflicts });
        }

        // Clean merge, nothing for the user to resolve — finish it now.
        let oid = self.finalize_merge(author_name, author_email)?;
        Ok(PullResult { status: "merged".into(), oid: Some(oid), conflicts: vec![] })
    }

    /// Resolve one conflicted file: "keep_local" | "use_remote" | "keep_both".
    pub fn resolve_conflict_file(&self, path: &str, action: &str) -> Result<(), String> {
        let mut index = self.repo.index().map_err(|e| format!("Index failed: {}", e))?;
        let full_path = self.config.vault_path.join(path);

        let conflict = index
            .conflict_get(Path::new(path))
            .map_err(|e| format!("No conflict found for '{}': {}", path, e))?;

        match action {
            "keep_local" | "keep_both" => {
                if let Some(our) = &conflict.our {
                    let blob = self.repo.find_blob(our.id).map_err(|e| format!("Blob not found: {}", e))?;
                    std::fs::write(&full_path, blob.content()).map_err(|e| format!("Write failed: {}", e))?;
                }
                if action == "keep_both" {
                    if let Some(their) = &conflict.their {
                        let blob = self.repo.find_blob(their.id).map_err(|e| format!("Blob not found: {}", e))?;
                        let remote_rel_path = format!("{}.remote", path);
                        let remote_full_path = self.config.vault_path.join(&remote_rel_path);
                        std::fs::write(&remote_full_path, blob.content()).map_err(|e| format!("Write failed: {}", e))?;
                        index
                            .add_path(Path::new(&remote_rel_path))
                            .map_err(|e| format!("Stage remote copy failed: {}", e))?;
                    }
                }
                index.conflict_remove(Path::new(path)).map_err(|e| format!("Clear conflict failed: {}", e))?;
                index.add_path(Path::new(path)).map_err(|e| format!("Stage failed: {}", e))?;
            }
            "use_remote" => {
                if let Some(their) = &conflict.their {
                    let blob = self.repo.find_blob(their.id).map_err(|e| format!("Blob not found: {}", e))?;
                    std::fs::write(&full_path, blob.content()).map_err(|e| format!("Write failed: {}", e))?;
                    index.conflict_remove(Path::new(path)).map_err(|e| format!("Clear conflict failed: {}", e))?;
                    index.add_path(Path::new(path)).map_err(|e| format!("Stage failed: {}", e))?;
                } else {
                    // Remote deleted the file.
                    index.conflict_remove(Path::new(path)).map_err(|e| format!("Clear conflict failed: {}", e))?;
                    let _ = index.remove_path(Path::new(path));
                    let _ = std::fs::remove_file(&full_path);
                }
            }
            _ => return Err(format!("Unknown action: {}", action)),
        }

        index.write().map_err(|e| format!("Write index failed: {}", e))?;
        Ok(())
    }

    /// Complete an in-progress merge once all conflicts are resolved.
    pub fn finalize_merge(&self, author_name: &str, author_email: &str) -> Result<String, String> {
        let mut index = self.repo.index().map_err(|e| format!("Index failed: {}", e))?;
        if index.has_conflicts() {
            return Err("Unresolved conflicts remain".to_string());
        }

        let tree_id = index.write_tree().map_err(|e| format!("Write tree failed: {}", e))?;
        let tree = self.repo.find_tree(tree_id).map_err(|e| format!("Find tree failed: {}", e))?;

        let head_commit = self
            .repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| format!("Failed to get HEAD commit: {}", e))?;

        let merge_head_oid = self
            .repo
            .find_reference("MERGE_HEAD")
            .ok()
            .and_then(|r| r.target())
            .ok_or("MERGE_HEAD not found")?;
        let merge_commit = self
            .repo
            .find_commit(merge_head_oid)
            .map_err(|e| format!("Failed to find merge commit: {}", e))?;

        let sig = Signature::now(author_name, author_email).map_err(|e| format!("Signature failed: {}", e))?;

        let oid = self
            .repo
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                "Merge remote changes",
                &tree,
                &[&head_commit, &merge_commit],
            )
            .map_err(|e| format!("Commit failed: {}", e))?;

        self.repo.cleanup_state().map_err(|e| format!("Cleanup failed: {}", e))?;

        Ok(oid.to_string())
    }

    /// Abort an in-progress merge, discarding conflict resolution progress
    /// and restoring the working directory to pre-merge HEAD.
    pub fn abort_merge(&self) -> Result<(), String> {
        let head_commit = self
            .repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| format!("Failed to get HEAD commit: {}", e))?;

        let mut co = git2::build::CheckoutBuilder::new();
        co.force();
        self.repo
            .reset(head_commit.as_object(), git2::ResetType::Hard, Some(&mut co))
            .map_err(|e| format!("Reset failed: {}", e))?;

        self.repo.cleanup_state().map_err(|e| format!("Cleanup failed: {}", e))?;

        Ok(())
    }
}
