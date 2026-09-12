// Phase 2: manual file/folder encryption (right-click Encrypt/Decrypt).
//
// Deliberately NOT the "transparent encryption on write" from the original
// 2.1 spec — that needs an app-wide write interceptor (FUSE-like on
// Windows, unresolved open question from earlier planning). This is 2.2's
// simpler, testable version: explicit Encrypt/Decrypt actions, passphrase
// per operation, no persisted key material — reuses the same Argon2 +
// XChaCha20Poly1305 primitives as the agent vault (crypto.rs), just over
// raw file bytes instead of note text, and with no envelope/agent-identity
// layer since this is single-user local encryption, not multi-recipient.

use argon2::Argon2;
use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng as ChaChaOsRng},
    Key, XChaCha20Poly1305, XNonce,
};
use rand_core::{OsRng, RngCore};
use std::fs;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"CPHR";
const FORMAT_VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const HEADER_LEN: usize = 4 + 1 + SALT_LEN + NONCE_LEN;

fn derive_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| format!("key derivation failed: {}", e))?;
    Ok(key)
}

/// Encrypts a single file in place: `name.ext` -> `name.ext.cipher`,
/// original removed. File layout: MAGIC | VERSION | SALT | NONCE | ciphertext.
pub fn encrypt_file(path: &Path, passphrase: &str) -> Result<PathBuf, String> {
    let plaintext = fs::read(path).map_err(|e| format!("read failed: {}", e))?;

    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let key = derive_key(passphrase, &salt)?;

    let cipher = XChaCha20Poly1305::new(Key::from_slice(&key));
    let nonce = XChaCha20Poly1305::generate_nonce(&mut ChaChaOsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_slice())
        .map_err(|e| format!("encryption failed: {}", e))?;

    let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);

    let out_path = PathBuf::from(format!("{}.cipher", path.to_string_lossy()));
    fs::write(&out_path, out).map_err(|e| format!("write failed: {}", e))?;
    fs::remove_file(path).map_err(|e| format!("failed to remove original: {}", e))?;
    Ok(out_path)
}

/// Reverses encrypt_file: `name.ext.cipher` -> `name.ext`, ciphertext removed.
pub fn decrypt_file(path: &Path, passphrase: &str) -> Result<PathBuf, String> {
    let data = fs::read(path).map_err(|e| format!("read failed: {}", e))?;
    if data.len() < HEADER_LEN || &data[0..4] != MAGIC {
        return Err("not a Cipher-encrypted file".to_string());
    }
    let salt = &data[5..5 + SALT_LEN];
    let nonce_bytes = &data[5 + SALT_LEN..HEADER_LEN];
    let ciphertext = &data[HEADER_LEN..];

    let key = derive_key(passphrase, salt)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&key));
    let nonce = XNonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "decryption failed — wrong passphrase or corrupted file".to_string())?;

    let path_str = path.to_string_lossy().to_string();
    let out_path_str = path_str
        .strip_suffix(".cipher")
        .ok_or_else(|| "not a .cipher file".to_string())?
        .to_string();
    let out_path = PathBuf::from(out_path_str);
    fs::write(&out_path, plaintext).map_err(|e| format!("write failed: {}", e))?;
    fs::remove_file(path).map_err(|e| format!("failed to remove ciphertext: {}", e))?;
    Ok(out_path)
}

/// Recursively encrypts every non-.cipher file under a folder, in place.
pub fn encrypt_folder(dir: &Path, passphrase: &str) -> Result<u32, String> {
    let mut count = 0u32;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = fs::read_dir(&d).map_err(|e| format!("read_dir failed: {}", e))?;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) != Some("cipher") {
                encrypt_file(&p, passphrase)?;
                count += 1;
            }
        }
    }
    Ok(count)
}

/// Recursively decrypts every .cipher file under a folder, in place.
pub fn decrypt_folder(dir: &Path, passphrase: &str) -> Result<u32, String> {
    let mut count = 0u32;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = fs::read_dir(&d).map_err(|e| format!("read_dir failed: {}", e))?;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("cipher") {
                decrypt_file(&p, passphrase)?;
                count += 1;
            }
        }
    }
    Ok(count)
}
