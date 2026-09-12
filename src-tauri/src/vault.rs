// Vault storage: notes on disk as ciphertext + metadata JSON. Anything
// opening these files directly (raw Explorer, another program) sees
// gibberish — only `read_note`, going through the right agent's unlocked
// key, ever produces plaintext. This mirrors the original Node vault's
// design, rebuilt natively.
//
// Scope for this pass: single agent, matching how the original project did
// "plain CRUD" + "encryption" as one combined step before multi-agent
// isolation. Zones (scratch/shared) and multi-recipient envelopes come next.

use crate::crypto;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone)]
pub struct NoteMeta {
    pub id: String,
    pub title: String,
    pub agent: String,
    pub created: u64,
    pub updated: u64,
    pub tags: Vec<String>,
    pub content_nonce: String,
    pub envelopes: Vec<crypto::WrappedKey>,
}

#[derive(Serialize, Deserialize)]
struct NoteFile {
    meta: NoteMeta,
    ciphertext: String,
}

#[derive(Serialize, Clone)]
pub struct NoteSummary {
    pub id: String,
    pub title: String,
    pub agent: String,
    pub created: u64,
    pub updated: u64,
    pub tags: Vec<String>,
}

#[derive(Serialize)]
pub struct DecryptedNote {
    pub id: String,
    pub title: String,
    pub agent: String,
    pub tags: Vec<String>,
    pub content: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn slug_id(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    let slug = if slug.is_empty() { "note".to_string() } else { slug };
    let suffix: u32 = {
        let mut b = [0u8; 4];
        rand_core::OsRng.fill_bytes(&mut b);
        u32::from_le_bytes(b)
    };
    format!("{}-{:08x}", slug, suffix)
}

use rand_core::RngCore;

fn note_path(vault_dir: &Path, id: &str) -> PathBuf {
    vault_dir.join(format!("{}.json", id))
}

fn keys_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("keys")
}
fn vault_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("vault")
}

fn public_key_path(app_dir: &Path, agent_id: &str) -> PathBuf {
    keys_dir(app_dir).join(format!("{}.pub.json", agent_id))
}
fn private_key_path(app_dir: &Path, agent_id: &str) -> PathBuf {
    keys_dir(app_dir).join(format!("{}.key.json", agent_id))
}

pub fn create_agent(app_dir: &Path, agent_id: &str, passphrase: &str) -> Result<crypto::AgentPublicRecord, String> {
    fs::create_dir_all(keys_dir(app_dir)).map_err(|e| e.to_string())?;
    let (public_record, private_json) = crypto::create_agent(agent_id, passphrase)?;

    let public_json = serde_json::to_string_pretty(&public_record).map_err(|e| e.to_string())?;
    fs::write(public_key_path(app_dir, agent_id), public_json).map_err(|e| e.to_string())?;
    fs::write(private_key_path(app_dir, agent_id), private_json).map_err(|e| e.to_string())?;

    Ok(public_record)
}

fn load_public_key(app_dir: &Path, agent_id: &str) -> Result<String, String> {
    let raw = fs::read_to_string(public_key_path(app_dir, agent_id))
        .map_err(|_| format!("no public key found for agent \"{}\" — has it been created?", agent_id))?;
    let record: crypto::AgentPublicRecord = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    Ok(record.public_key)
}

fn unlock_secret_key(app_dir: &Path, agent_id: &str, passphrase: &str) -> Result<String, String> {
    let raw = fs::read_to_string(private_key_path(app_dir, agent_id))
        .map_err(|_| format!("no private key found for agent \"{}\"", agent_id))?;
    crypto::unlock_secret_key(&raw, passphrase)
}

pub fn write_note(
    app_dir: &Path,
    agent_id: &str,
    passphrase: &str,
    title: &str,
    content: &str,
    tags: Vec<String>,
) -> Result<NoteSummary, String> {
    let vdir = vault_dir(app_dir);
    fs::create_dir_all(&vdir).map_err(|e| e.to_string())?;

    let own_public_key = load_public_key(app_dir, agent_id)?;
    let secret_key = unlock_secret_key(app_dir, agent_id, passphrase)?;

    let content_key = crypto::generate_content_key();
    let (ciphertext, content_nonce) = crypto::encrypt_content(&content_key, content)?;
    let envelope = crypto::wrap_key_for_agent(&content_key, &own_public_key, &secret_key)?;

    let id = slug_id(title);
    let now = now_ms();
    let meta = NoteMeta {
        id: id.clone(),
        title: title.to_string(),
        agent: agent_id.to_string(),
        created: now,
        updated: now,
        tags: tags.clone(),
        content_nonce,
        envelopes: vec![envelope],
    };
    let note_file = NoteFile { meta: meta.clone(), ciphertext };
    let json = serde_json::to_string_pretty(&note_file).map_err(|e| e.to_string())?;
    fs::write(note_path(&vdir, &id), json).map_err(|e| e.to_string())?;

    Ok(NoteSummary { id, title: title.to_string(), agent: agent_id.to_string(), created: now, updated: now, tags })
}

pub fn read_note(app_dir: &Path, agent_id: &str, passphrase: &str, id: &str) -> Result<DecryptedNote, String> {
    let vdir = vault_dir(app_dir);
    let raw = fs::read_to_string(note_path(&vdir, id)).map_err(|_| format!("note \"{}\" not found", id))?;
    let note_file: NoteFile = serde_json::from_str(&raw).map_err(|e| e.to_string())?;

    // Single-agent scope for this phase: exactly one envelope exists per
    // note (wrapped for its creating agent). Multi-recipient matching by
    // agent_id lands with the multi-agent pass — until then this is honestly
    // just "the one envelope", not a real access check. The actual security
    // boundary is still enforced correctly regardless: unwrap_key_for_agent
    // below fails closed if the wrong secret key is supplied.
    let envelope = note_file
        .meta
        .envelopes
        .first()
        .ok_or_else(|| format!("note \"{}\" has no envelopes", id))?;

    let secret_key = unlock_secret_key(app_dir, agent_id, passphrase)?;
    let content_key = crypto::unwrap_key_for_agent(envelope, &secret_key)?;
    let content = crypto::decrypt_content(&content_key, &note_file.ciphertext, &note_file.meta.content_nonce)?;

    Ok(DecryptedNote {
        id: note_file.meta.id,
        title: note_file.meta.title,
        agent: note_file.meta.agent,
        tags: note_file.meta.tags,
        content,
    })
}

pub fn list_notes(app_dir: &Path) -> Result<Vec<NoteSummary>, String> {
    let vdir = vault_dir(app_dir);
    if !vdir.is_dir() {
        return Ok(vec![]);
    }
    let mut notes = Vec::new();
    for entry in fs::read_dir(&vdir).map_err(|e| e.to_string())? {
        let entry = match entry { Ok(e) => e, Err(_) => continue };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let raw = match fs::read_to_string(&path) { Ok(r) => r, Err(_) => continue };
        let note_file: NoteFile = match serde_json::from_str(&raw) { Ok(n) => n, Err(_) => continue };
        notes.push(NoteSummary {
            id: note_file.meta.id,
            title: note_file.meta.title,
            agent: note_file.meta.agent,
            created: note_file.meta.created,
            updated: note_file.meta.updated,
            tags: note_file.meta.tags,
        });
    }
    notes.sort_by(|a, b| b.updated.cmp(&a.updated));
    Ok(notes)
}
