#!/usr/bin/env python3
"""
Phase 2.5: Key Backup & Recovery
24-word BIP39 seed phrase generation, encrypted backup files, recovery validation.
"""

import os
import json
import secrets
from datetime import datetime
from pathlib import Path
from cryptography.hazmat.primitives import hashes, hmac
from cryptography.hazmat.primitives.kdf.pbkdf2 import PBKDF2
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from cryptography.hazmat.backends import default_backend

# BIP39 English wordlist (simplified for brevity; use full list in production)
BIP39_WORDLIST = [
    "abandon", "ability", "able", "about", "above", "absent", "absorb", "abstract",
    "abuse", "access", "accident", "account", "achieve", "acid", "acoustic", "acquire",
    # ... (truncated for this example; production uses all 2048)
] * 256  # Repeat to reach 2048 words for demo

# Generate proper wordlist if needed
def _expand_bip39():
    """Expand to full 2048-word BIP39 list (production: load from file)."""
    global BIP39_WORDLIST
    if len(BIP39_WORDLIST) < 2048:
        # Placeholder: in production, load from bip39_words.json or similar
        BIP39_WORDLIST = [f"word{i:04d}" for i in range(2048)]

_expand_bip39()


class KeyBackup:
    """Generate, encrypt, and recover encryption keys."""

    def __init__(self, backup_dir: str = None):
        self.backup_dir = Path(backup_dir or os.path.expanduser("~/.cipher/backups"))
        self.backup_dir.mkdir(parents=True, exist_ok=True)

    def generate_seed_phrase(self) -> str:
        """
        Generate a 24-word BIP39 seed phrase.
        Returns space-separated words.
        """
        # Generate 32 bytes of entropy (256 bits = 24 words)
        entropy = secrets.token_bytes(32)
        
        # BIP39 checksum: SHA256 hash, take first 8 bits
        h = hashes.Hash(hashes.SHA256(), backend=default_backend())
        h.update(entropy)
        digest = h.finalize()
        checksum_bits = bin(digest[0])[2:].zfill(8)[:8]
        
        # Convert entropy + checksum to 11-bit indices
        bits = bin(int.from_bytes(entropy, 'big'))[2:].zfill(256) + checksum_bits
        indices = [int(bits[i:i+11], 2) for i in range(0, len(bits), 11)]
        
        # Map indices to words
        words = [BIP39_WORDLIST[idx] for idx in indices]
        return " ".join(words)

    def validate_seed_phrase(self, phrase: str) -> bool:
        """
        Validate a 24-word seed phrase (BIP39 checksum).
        Returns True if valid, False otherwise.
        """
        words = phrase.strip().split()
        
        # Must be exactly 24 words
        if len(words) != 24:
            return False
        
        # All words must be in wordlist
        if not all(w in BIP39_WORDLIST for w in words):
            return False
        
        # Reconstruct entropy and validate checksum
        indices = [BIP39_WORDLIST.index(w) for w in words]
        bits = "".join(f"{idx:011b}" for idx in indices)
        
        # Split into entropy (256 bits) and checksum (8 bits)
        entropy_bits = bits[:256]
        checksum_bits = bits[256:264]
        
        entropy = int(entropy_bits, 2).to_bytes(32, 'big')
        
        # Recalculate checksum
        h = hashes.Hash(hashes.SHA256(), backend=default_backend())
        h.update(entropy)
        digest = h.finalize()
        expected_checksum = bin(digest[0])[2:].zfill(8)[:8]
        
        return checksum_bits == expected_checksum

    def derive_key_from_passphrase(self, passphrase: str, salt: bytes = None) -> tuple:
        """
        Derive encryption key from passphrase using PBKDF2.
        Returns (key, salt) where salt is generated if not provided.
        """
        if salt is None:
            salt = secrets.token_bytes(16)
        
        kdf = PBKDF2(
            algorithm=hashes.SHA256(),
            length=32,
            salt=salt,
            iterations=480000,  # OWASP recommendation
            backend=default_backend()
        )
        key = kdf.derive(passphrase.encode())
        return key, salt

    def create_backup(self, seed_phrase: str, passphrase: str) -> dict:
        """
        Encrypt a seed phrase and create a backup file.
        Returns metadata dict with backup_id, timestamp, hash.
        """
        if not self.validate_seed_phrase(seed_phrase):
            raise ValueError("Invalid seed phrase")
        
        # Derive encryption key from passphrase
        key, salt = self.derive_key_from_passphrase(passphrase)
        
        # Encrypt seed phrase with AES-256-GCM
        iv = secrets.token_bytes(12)
        cipher = Cipher(
            algorithms.AES(key),
            modes.GCM(iv),
            backend=default_backend()
        )
        encryptor = cipher.encryptor()
        ciphertext = encryptor.update(seed_phrase.encode()) + encryptor.finalize()
        tag = encryptor.tag
        
        # Create backup object
        backup_id = secrets.token_hex(8)
        backup_data = {
            "backup_id": backup_id,
            "timestamp": datetime.utcnow().isoformat(),
            "version": 1,
            "ciphertext": ciphertext.hex(),
            "iv": iv.hex(),
            "tag": tag.hex(),
            "salt": salt.hex(),
            "algorithm": "AES-256-GCM",
            "kdf": "PBKDF2-SHA256-480000"
        }
        
        # Save to disk
        backup_file = self.backup_dir / f"vault-key-{backup_id}.backup"
        with open(backup_file, "w") as f:
            json.dump(backup_data, f, indent=2)
        
        return backup_data

    def recover_from_backup(self, backup_file: str, passphrase: str) -> str:
        """
        Recover seed phrase from encrypted backup file.
        Returns the original seed phrase.
        """
        with open(backup_file, "r") as f:
            backup_data = json.load(f)
        
        # Derive key with stored salt
        salt = bytes.fromhex(backup_data["salt"])
        key, _ = self.derive_key_from_passphrase(passphrase, salt)
        
        # Decrypt
        iv = bytes.fromhex(backup_data["iv"])
        tag = bytes.fromhex(backup_data["tag"])
        ciphertext = bytes.fromhex(backup_data["ciphertext"])
        
        cipher = Cipher(
            algorithms.AES(key),
            modes.GCM(iv, tag),
            backend=default_backend()
        )
        decryptor = cipher.decryptor()
        seed_phrase = (decryptor.update(ciphertext) + decryptor.finalize()).decode()
        
        # Validate recovered phrase
        if not self.validate_seed_phrase(seed_phrase):
            raise ValueError("Recovered phrase failed validation — backup may be corrupted")
        
        return seed_phrase

    def list_backups(self) -> list:
        """List all backup files with metadata."""
        backups = []
        for backup_file in sorted(self.backup_dir.glob("vault-key-*.backup")):
            with open(backup_file, "r") as f:
                data = json.load(f)
                backups.append({
                    "file": str(backup_file),
                    "backup_id": data["backup_id"],
                    "timestamp": data["timestamp"]
                })
        return backups

    def delete_backup(self, backup_id: str):
        """Securely delete a backup file."""
        backup_file = self.backup_dir / f"vault-key-{backup_id}.backup"
        if backup_file.exists():
            # Overwrite 3 times before deletion (DOD 5220.22-M)
            with open(backup_file, "r+b") as f:
                size = f.seek(0, 2)
                f.seek(0)
                for _ in range(3):
                    f.write(secrets.token_bytes(size))
                    f.seek(0)
            backup_file.unlink()


# Test
if __name__ == "__main__":
    backup = KeyBackup("/tmp/cipher-backups")
    
    # Generate seed phrase
    seed = backup.generate_seed_phrase()
    print(f"✓ Generated seed: {seed[:50]}...")
    print(f"✓ Valid: {backup.validate_seed_phrase(seed)}")
    
    # Create backup
    backup_meta = backup.create_backup(seed, "my-vault-password")
    print(f"✓ Backup created: {backup_meta['backup_id']}")
    
    # List backups
    backups = backup.list_backups()
    print(f"✓ Backups: {len(backups)}")
    
    # Recover
    recovered = backup.recover_from_backup(backups[0]["file"], "my-vault-password")
    print(f"✓ Recovered matches: {recovered == seed}")
