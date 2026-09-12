// Phase 2.5: Key Backup & Recovery
// Simplified BIP39-style seed phrase + AES-256-GCM backup encryption

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use pbkdf2::pbkdf2_hmac;
use rand::Rng;
use sha2::Sha256;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use chrono::Local;

// Simplified word list (production: load the real 2048-word BIP39 list from a file)
const WORDLIST: &[&str] = &[
    "abandon", "ability", "able", "about", "above", "absent", "absorb", "abstract",
    "abuse", "access", "accident", "account", "achieve", "acid", "acoustic", "acquire",
    "across", "act", "action", "actor", "actress", "actual", "adapt", "add",
    "addict", "address", "adjust", "admit", "adult", "advance", "advice", "aerobic",
    "affair", "afford", "afraid", "again", "age", "agent", "agree", "ahead",
    "aim", "air", "airport", "aisle", "alarm", "album", "alcohol", "alert",
    "alien", "all", "alley", "allow", "almost", "alone", "alpha", "already",
    "also", "alter", "always", "amateur", "amazing", "among", "amount", "amused",
    "analyst", "anchor", "ancient", "anger", "angle", "angry", "animal", "ankle",
    "announce", "annual", "another", "answer", "antenna", "antique", "anxiety", "any",
    "apart", "apology", "appear", "apple", "approve", "april", "arch", "arctic",
    "area", "arena", "argue", "arm", "armed", "armor", "army", "around",
    "arrange", "arrest", "arrive", "arrow", "art", "artefact", "artist", "artwork",
    "ask", "aspect", "assault", "asset", "assist", "assume", "asthma", "athlete",
    "atom", "attack", "attend", "attitude", "attract", "auction", "audit", "august",
    "aunt", "author", "auto", "autumn", "average", "avocado", "avoid", "awake",
    "aware", "away", "awesome", "awful", "awkward", "axis", "baby", "bachelor",
    "bacon", "badge", "bag", "balance", "balcony", "ball", "bamboo", "banana",
    "banner", "bar", "barely", "bargain", "barrel", "base", "basic", "basket",
    "battle", "beach", "bean", "beauty", "because", "become", "beef", "before",
    "begin", "behave", "behind", "believe", "below", "belt", "bench", "benefit",
    "best", "betray", "better", "between", "beyond", "bicycle", "bid", "bike",
    "bind", "biology", "bird", "birth", "bitter", "black", "blade", "blame",
    "blanket", "blast", "bleak", "bless", "blind", "blood", "blossom", "blouse",
    "blue", "blur", "blush", "board", "boat", "body", "boil", "bomb",
    "bone", "bonus", "book", "boost", "border", "boring", "borrow", "boss",
    "bottom", "bounce", "box", "boy", "bracket", "brain", "brand", "brass",
    "brave", "bread", "breeze", "brick", "bridge", "brief", "bright", "bring",
    "brisk", "broccoli", "broken", "bronze", "broom", "brother", "brown", "brush",
    "bubble", "buddy", "budget", "buffalo", "build", "bulb", "bulk", "bullet",
    "bundle", "bunker", "burden", "burger", "burst", "bus", "business", "busy",
];

#[derive(Debug, Serialize, Deserialize)]
pub struct BackupFile {
    pub backup_id: String,
    pub timestamp: String,
    pub version: u32,
    pub ciphertext: String,
    pub iv: String,
    pub salt: String,
    pub algorithm: String,
    pub kdf: String,
}

pub struct KeyBackup {
    backup_dir: PathBuf,
}

impl KeyBackup {
    pub fn new(backup_dir: Option<String>) -> Self {
        let dir = if let Some(d) = backup_dir {
            PathBuf::from(d)
        } else {
            let home = dirs::home_dir().unwrap_or_default();
            home.join(".cipher").join("backups")
        };
        let _ = fs::create_dir_all(&dir);
        KeyBackup { backup_dir: dir }
    }

    /// Generate a 24-word seed phrase (simplified — not full BIP39 checksum math)
    pub fn generate_seed_phrase() -> String {
        let mut rng = rand::thread_rng();
        let mut words = Vec::with_capacity(24);
        for _ in 0..24 {
            let idx = rng.gen_range(0..WORDLIST.len());
            words.push(WORDLIST[idx]);
        }
        words.join(" ")
    }

    /// Validate a seed phrase: correct word count and all words recognized
    pub fn validate_seed_phrase(phrase: &str) -> bool {
        let words: Vec<&str> = phrase.split_whitespace().collect();
        if words.len() != 24 {
            return false;
        }
        words.iter().all(|w| WORDLIST.contains(w))
    }

    fn derive_key(passphrase: &str, salt: &[u8]) -> [u8; 32] {
        let mut key = [0u8; 32];
        pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, 480_000, &mut key);
        key
    }

    /// Encrypt a seed phrase with a passphrase and write it to a backup file
    pub fn create_backup(&self, seed_phrase: &str, passphrase: &str) -> Result<BackupFile, String> {
        if !Self::validate_seed_phrase(seed_phrase) {
            return Err("Invalid seed phrase".to_string());
        }

        let mut rng = rand::thread_rng();
        let salt: [u8; 16] = rng.gen();
        let iv_bytes: [u8; 12] = rng.gen();
        let nonce = Nonce::from_slice(&iv_bytes);

        let key = Self::derive_key(passphrase, &salt);
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Cipher init failed: {}", e))?;

        let ciphertext = cipher
            .encrypt(nonce, seed_phrase.as_bytes())
            .map_err(|e| format!("Encryption failed: {}", e))?;

        let backup_id = format!("{:x}", rng.gen::<u64>());

        let backup = BackupFile {
            backup_id: backup_id.clone(),
            timestamp: Local::now().to_rfc3339(),
            version: 1,
            ciphertext: hex::encode(&ciphertext),
            iv: hex::encode(&iv_bytes),
            salt: hex::encode(&salt),
            algorithm: "AES-256-GCM".to_string(),
            kdf: "PBKDF2-SHA256-480000".to_string(),
        };

        let backup_file = self.backup_dir.join(format!("vault-key-{}.backup", backup_id));
        let json = serde_json::to_string_pretty(&backup).map_err(|e| format!("Serialize failed: {}", e))?;
        fs::write(&backup_file, json).map_err(|e| format!("Write failed: {}", e))?;

        Ok(backup)
    }

    /// Decrypt a backup file with the given passphrase
    pub fn recover_from_backup(&self, backup_file: &str, passphrase: &str) -> Result<String, String> {
        let json = fs::read_to_string(backup_file).map_err(|e| format!("Read failed: {}", e))?;
        let backup: BackupFile = serde_json::from_str(&json).map_err(|e| format!("Parse failed: {}", e))?;

        let salt = hex::decode(&backup.salt).map_err(|e| format!("Salt decode failed: {}", e))?;
        let iv_bytes = hex::decode(&backup.iv).map_err(|e| format!("IV decode failed: {}", e))?;
        let ciphertext = hex::decode(&backup.ciphertext).map_err(|e| format!("Ciphertext decode failed: {}", e))?;

        let key = Self::derive_key(passphrase, &salt);
        let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("Cipher init failed: {}", e))?;
        let nonce = Nonce::from_slice(&iv_bytes[..12]);

        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|_| "Decryption failed — wrong passphrase or corrupted backup".to_string())?;

        let seed_phrase = String::from_utf8(plaintext).map_err(|e| format!("UTF-8 decode failed: {}", e))?;

        if !Self::validate_seed_phrase(&seed_phrase) {
            return Err("Recovered phrase failed validation".to_string());
        }

        Ok(seed_phrase)
    }

    /// List backup files as (full_path, filename) pairs
    pub fn list_backups(&self) -> Result<Vec<(String, String)>, String> {
        let mut backups = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.backup_dir) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_file() {
                        if let Some(filename) = entry.file_name().to_str() {
                            if filename.ends_with(".backup") {
                                backups.push((entry.path().to_string_lossy().to_string(), filename.to_string()));
                            }
                        }
                    }
                }
            }
        }
        Ok(backups)
    }

    /// Delete a backup file by ID
    pub fn delete_backup(&self, backup_id: &str) -> Result<(), String> {
        let path = self.backup_dir.join(format!("vault-key-{}.backup", backup_id));
        if path.exists() {
            fs::remove_file(&path).map_err(|e| format!("Delete failed: {}", e))?;
        }
        Ok(())
    }
}
