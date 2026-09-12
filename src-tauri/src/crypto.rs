// Cryptography for the Cipher vault.
//
// - Per-agent identity: X25519 keypair via `crypto_box` — this crate has
//   received a security audit (Cure53, 2022, funded by Threema), unlike the
//   Node/tweetnacl version this replaces, which was never independently audited.
// - Passphrase -> key: Argon2id in raw key-derivation mode (`hash_password_into`,
//   NOT the PHC-string password-hashing mode — those solve different problems).
//   Upgrade over the original's scrypt.
// - Content encryption: XChaCha20Poly1305 rather than legacy NaCl secretbox —
//   the direct Rust port of secretbox (`crypto_secretbox`) explicitly has NOT
//   been audited; XChaCha20Poly1305 is the more modern, far more widely
//   deployed choice (used by e.g. age, WireGuard-adjacent tooling).
// - Envelope wrapping (per-recipient key wrap): `crypto_box`'s SalsaBox
//   (X25519 ECDH + XSalsa20Poly1305) — this is specifically the audited
//   primitive, used only for the "wrap this content key for one recipient" step.

use argon2::Argon2;
use base64::{engine::general_purpose::STANDARD, Engine};
use chacha20poly1305::{
    aead::{Aead as ChaChaAead, AeadCore as ChaChaAeadCore, KeyInit, OsRng as ChaChaOsRng},
    Key as ChaChaKey, XChaCha20Poly1305, XNonce,
};
use crypto_box::{
    aead::{Aead as BoxAead, AeadCore as BoxAeadCore, OsRng as BoxOsRng},
    PublicKey, SalsaBox, SecretKey,
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct WrappedKey {
    pub wrapped: String,
    pub nonce: String,
    pub sender_public_key: String,
}

#[derive(Serialize, Deserialize)]
pub struct AgentPublicRecord {
    pub agent_id: String,
    pub public_key: String,
}

#[derive(Serialize, Deserialize)]
struct AgentPrivateRecord {
    agent_id: String,
    salt: String,
    nonce: String,
    encrypted_secret: String,
}

fn b64e(data: &[u8]) -> String {
    STANDARD.encode(data)
}
fn b64d(s: &str) -> Result<Vec<u8>, String> {
    STANDARD.decode(s).map_err(|e| format!("bad base64: {}", e))
}
fn as_32(bytes: &[u8], what: &str) -> Result<[u8; 32], String> {
    <[u8; 32]>::try_from(bytes).map_err(|_| format!("{} must be 32 bytes", what))
}

// --- content encryption (XChaCha20Poly1305) ---

pub fn generate_content_key() -> Vec<u8> {
    XChaCha20Poly1305::generate_key(&mut ChaChaOsRng).to_vec()
}

pub fn encrypt_content(content_key: &[u8], plaintext: &str) -> Result<(String, String), String> {
    let cipher = XChaCha20Poly1305::new(ChaChaKey::from_slice(content_key));
    let nonce = XChaCha20Poly1305::generate_nonce(&mut ChaChaOsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|e| format!("encryption failed: {}", e))?;
    Ok((b64e(&ciphertext), b64e(&nonce)))
}

pub fn decrypt_content(content_key: &[u8], ciphertext_b64: &str, nonce_b64: &str) -> Result<String, String> {
    let cipher = XChaCha20Poly1305::new(ChaChaKey::from_slice(content_key));
    let ciphertext = b64d(ciphertext_b64)?;
    let nonce_bytes = b64d(nonce_b64)?;
    let nonce = XNonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_slice())
        .map_err(|_| "decryption failed — wrong key or tampered ciphertext".to_string())?;
    String::from_utf8(plaintext).map_err(|e| e.to_string())
}

// --- envelope wrapping (crypto_box / SalsaBox — audited) ---

pub fn wrap_key_for_agent(
    content_key: &[u8],
    recipient_public_key_b64: &str,
    sender_secret_key_b64: &str,
) -> Result<WrappedKey, String> {
    let recipient_pk = PublicKey::from(as_32(&b64d(recipient_public_key_b64)?, "public key")?);
    let sender_sk = SecretKey::from(as_32(&b64d(sender_secret_key_b64)?, "secret key")?);
    let sender_pk_bytes = sender_sk.public_key().as_bytes().to_vec();

    let sbox = SalsaBox::new(&recipient_pk, &sender_sk);
    let nonce = SalsaBox::generate_nonce(&mut BoxOsRng);
    let wrapped = sbox
        .encrypt(&nonce, content_key)
        .map_err(|e| format!("wrap failed: {}", e))?;

    Ok(WrappedKey {
        wrapped: b64e(&wrapped),
        nonce: b64e(&nonce),
        sender_public_key: b64e(&sender_pk_bytes),
    })
}

pub fn unwrap_key_for_agent(wrapped: &WrappedKey, recipient_secret_key_b64: &str) -> Result<Vec<u8>, String> {
    let sender_pk = PublicKey::from(as_32(&b64d(&wrapped.sender_public_key)?, "sender public key")?);
    let recipient_sk = SecretKey::from(as_32(&b64d(recipient_secret_key_b64)?, "recipient secret key")?);

    let sbox = SalsaBox::new(&sender_pk, &recipient_sk);
    let wrapped_bytes = b64d(&wrapped.wrapped)?;
    let nonce_bytes = b64d(&wrapped.nonce)?;
    let nonce = crypto_box::Nonce::from_slice(&nonce_bytes);

    sbox.decrypt(nonce, wrapped_bytes.as_slice())
        .map_err(|_| "unwrap failed — not an authorized recipient".to_string())
}

// --- agent identity (X25519 keypair, passphrase-protected at rest) ---

fn derive_key_from_passphrase(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut output = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut output)
        .map_err(|e| format!("key derivation failed: {}", e))?;
    Ok(output)
}

/// Returns (public record, private record as a JSON string to persist).
pub fn create_agent(agent_id: &str, passphrase: &str) -> Result<(AgentPublicRecord, String), String> {
    let secret_key = SecretKey::generate(&mut BoxOsRng);
    let public_key = secret_key.public_key();

    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let derived = derive_key_from_passphrase(passphrase, &salt)?;

    let cipher = XChaCha20Poly1305::new(ChaChaKey::from_slice(&derived));
    let nonce = XChaCha20Poly1305::generate_nonce(&mut ChaChaOsRng);
    let encrypted_secret = cipher
        .encrypt(&nonce, secret_key.to_bytes().as_slice())
        .map_err(|e| format!("failed to encrypt secret key: {}", e))?;

    let public_record = AgentPublicRecord {
        agent_id: agent_id.to_string(),
        public_key: b64e(public_key.as_bytes()),
    };
    let private_record = AgentPrivateRecord {
        agent_id: agent_id.to_string(),
        salt: b64e(&salt),
        nonce: b64e(&nonce),
        encrypted_secret: b64e(&encrypted_secret),
    };
    let private_json = serde_json::to_string_pretty(&private_record).map_err(|e| e.to_string())?;

    Ok((public_record, private_json))
}

/// Returns the base64-encoded raw secret key, for use in wrap/unwrap calls.
pub fn unlock_secret_key(private_json: &str, passphrase: &str) -> Result<String, String> {
    let record: AgentPrivateRecord =
        serde_json::from_str(private_json).map_err(|e| format!("corrupt key file: {}", e))?;
    let salt = b64d(&record.salt)?;
    let derived = derive_key_from_passphrase(passphrase, &salt)?;

    let cipher = XChaCha20Poly1305::new(ChaChaKey::from_slice(&derived));
    let nonce_bytes = b64d(&record.nonce)?;
    let nonce = XNonce::from_slice(&nonce_bytes);
    let encrypted_secret = b64d(&record.encrypted_secret)?;

    let secret_bytes = cipher
        .decrypt(nonce, encrypted_secret.as_slice())
        .map_err(|_| format!("wrong passphrase for agent \"{}\"", record.agent_id))?;

    Ok(b64e(&secret_bytes))
}
