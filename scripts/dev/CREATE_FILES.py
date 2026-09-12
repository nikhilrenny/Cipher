#!/usr/bin/env python3
import os

os.makedirs("S:\\Projects\\Cipher\\phase1", exist_ok=True)

# File 1
with open("S:\\Projects\\Cipher\\phase1\\cipher_phase1_crypto.py", "w", encoding="utf-8") as f:
    f.write('''#!/usr/bin/env python3
import json, base64, os
from dataclasses import dataclass
from cryptography.hazmat.primitives.asymmetric import x25519
from cryptography.hazmat.primitives import serialization, hashes
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes
from cryptography.hazmat.backends import default_backend
from datetime import datetime, timezone

@dataclass
class AgentKeypair:
    agent_id: str
    private_key_pem: str
    public_key_pem: str
    created_at: str

@dataclass
class EncryptedEnvelope:
    agent_id: str
    ephemeral_public_key: str
    ciphertext: str
    nonce: str
    tag: str = ""

class AgentCrypto:
    @staticmethod
    def generate_keypair(agent_id: str) -> AgentKeypair:
        private_key = x25519.X25519PrivateKey.generate()
        public_key = private_key.public_key()
        private_pem = private_key.private_bytes(
            encoding=serialization.Encoding.PEM,
            format=serialization.PrivateFormat.PKCS8,
            encryption_algorithm=serialization.NoEncryption(),
        ).decode()
        public_pem = public_key.public_bytes(
            encoding=serialization.Encoding.PEM,
            format=serialization.PublicFormat.SubjectPublicKeyInfo,
        ).decode()
        return AgentKeypair(
            agent_id=agent_id,
            private_key_pem=private_pem,
            public_key_pem=public_pem,
            created_at=datetime.now(timezone.utc).isoformat(),
        )

    @staticmethod
    def _load_private_key(pem: str):
        return serialization.load_pem_private_key(pem.encode(), password=None, backend=default_backend())

    @staticmethod
    def _load_public_key(pem: str):
        return serialization.load_pem_public_key(pem.encode(), backend=default_backend())

    @staticmethod
    def encrypt_for_agent(agent_public_key_pem: str, plaintext: str) -> EncryptedEnvelope:
        agent_public_key = AgentCrypto._load_public_key(agent_public_key_pem)
        ephemeral_private = x25519.X25519PrivateKey.generate()
        ephemeral_public = ephemeral_private.public_key()
        shared_secret = ephemeral_private.exchange(agent_public_key)
        enc_key = hashes.Hash(hashes.SHA256(), backend=default_backend())
        enc_key.update(shared_secret)
        key = enc_key.finalize()[:32]
        nonce = os.urandom(12)
        cipher = Cipher(algorithms.AES(key), modes.GCM(nonce), backend=default_backend())
        encryptor = cipher.encryptor()
        ciphertext = encryptor.update(plaintext.encode()) + encryptor.finalize()
        tag = encryptor.tag
        ephemeral_public_bytes = ephemeral_public.public_bytes_raw()
        return EncryptedEnvelope(
            agent_id="<recipient>",
            ephemeral_public_key=base64.b64encode(ephemeral_public_bytes).decode(),
            ciphertext=base64.b64encode(ciphertext).decode(),
            nonce=base64.b64encode(nonce).decode(),
            tag=base64.b64encode(tag).decode(),
        )

    @staticmethod
    def decrypt_envelope(agent_private_key_pem: str, envelope: EncryptedEnvelope) -> str:
        agent_private_key = AgentCrypto._load_private_key(agent_private_key_pem)
        ephemeral_public_bytes = base64.b64decode(envelope.ephemeral_public_key)
        ephemeral_public = x25519.X25519PublicKey.from_public_bytes(ephemeral_public_bytes)
        shared_secret = agent_private_key.exchange(ephemeral_public)
        enc_key = hashes.Hash(hashes.SHA256(), backend=default_backend())
        enc_key.update(shared_secret)
        key = enc_key.finalize()[:32]
        nonce = base64.b64decode(envelope.nonce)
        ciphertext = base64.b64decode(envelope.ciphertext)
        tag = base64.b64decode(envelope.tag)
        cipher = Cipher(algorithms.AES(key), modes.GCM(nonce, tag), backend=default_backend())
        decryptor = cipher.decryptor()
        plaintext = decryptor.update(ciphertext) + decryptor.finalize()
        return plaintext.decode()

class AgentRegistry:
    def __init__(self):
        self.agents = {}

    def register_agent(self, agent_id: str) -> AgentKeypair:
        if agent_id in self.agents:
            raise ValueError(f"Agent {agent_id} already registered")
        keypair = AgentCrypto.generate_keypair(agent_id)
        self.agents[agent_id] = keypair
        return keypair

    def get_agent_public_key(self, agent_id: str):
        return self.agents.get(agent_id).public_key_pem if agent_id in self.agents else None

    def get_agent_private_key(self, agent_id: str):
        return self.agents.get(agent_id).private_key_pem if agent_id in self.agents else None

    def list_agents(self) -> list:
        return list(self.agents.keys())

if __name__ == "__main__":
    registry = AgentRegistry()
    alice = registry.register_agent("alice")
    bob = registry.register_agent("bob")
    print(f"Registered agents: {registry.list_agents()}")
    
    secret = "confidential agent memory: password=xyz123"
    envelope = AgentCrypto.encrypt_for_agent(alice.public_key_pem, secret)
    envelope.agent_id = "alice"
    print(f"Alice encrypted data for herself")
    
    decrypted = AgentCrypto.decrypt_envelope(alice.private_key_pem, envelope)
    print(f"Alice decrypted her data: {decrypted}")
    
    try:
        AgentCrypto.decrypt_envelope(bob.private_key_pem, envelope)
        print("BUG: Bob should not decrypt Alice's data!")
    except Exception as e:
        print(f"Bob cannot decrypt Alice's data")
        print(f"  Error: {type(e).__name__}")
    
    print("Phase 1 tests PASSED")
''')

# File 2
with open("S:\\Projects\\Cipher\\phase1\\cipher_phase2_vault.py", "w", encoding="utf-8") as f:
    f.write('''#!/usr/bin/env python3
import json, sqlite3
from dataclasses import dataclass
from typing import List
from datetime import datetime, timezone

@dataclass
class DocumentNode:
    doc_id: str
    title: str
    agent_id: str
    created_at: str
    modified_at: str
    content_hash: str
    is_private: bool
    tags: List[str] = None

    def __post_init__(self):
        if self.tags is None:
            self.tags = []

class VaultSchema:
    def __init__(self, db_path: str):
        self.db_path = db_path
        self.conn = sqlite3.connect(db_path)
        self.conn.row_factory = sqlite3.Row
        self._init_schema()

    def _init_schema(self):
        cursor = self.conn.cursor()
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS documents (
                doc_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                modified_at TEXT NOT NULL,
                content_hash TEXT,
                is_private INTEGER DEFAULT 0,
                tags TEXT
            )
        """)
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS agents (
                agent_id TEXT PRIMARY KEY,
                created_at TEXT
            )
        """)
        cursor.execute("""
            CREATE TABLE IF NOT EXISTS envelopes (
                envelope_id TEXT PRIMARY KEY,
                doc_id TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                ephemeral_public_key TEXT,
                nonce TEXT,
                tag TEXT
            )
        """)
        self.conn.commit()

    def register_agent(self, agent_id: str) -> None:
        cursor = self.conn.cursor()
        cursor.execute("INSERT OR IGNORE INTO agents (agent_id, created_at) VALUES (?, ?)",
                      (agent_id, datetime.now(timezone.utc).isoformat()))
        self.conn.commit()

    def index_document(self, doc: DocumentNode) -> None:
        cursor = self.conn.cursor()
        cursor.execute("""INSERT OR REPLACE INTO documents
            (doc_id, title, agent_id, created_at, modified_at, content_hash, is_private, tags)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
            (doc.doc_id, doc.title, doc.agent_id, doc.created_at, doc.modified_at,
             doc.content_hash, 1 if doc.is_private else 0, json.dumps(doc.tags or [])))
        self.conn.commit()

    def close(self):
        self.conn.close()

if __name__ == "__main__":
    import tempfile, os
    with tempfile.TemporaryDirectory() as tmpdir:
        db_path = os.path.join(tmpdir, "vault.db")
        vault = VaultSchema(db_path)
        vault.register_agent("alice")
        vault.register_agent("bob")
        doc1 = DocumentNode("doc-001", "Alice's Notes", "alice",
                           datetime.now(timezone.utc).isoformat(),
                           datetime.now(timezone.utc).isoformat(),
                           "abc123", True, ["research"])
        vault.index_document(doc1)
        print("Phase 2 vault tests PASSED")
        vault.close()
''')

# File 3
with open("S:\\Projects\\Cipher\\phase1\\cipher_phase2_search.py", "w", encoding="utf-8") as f:
    f.write('''#!/usr/bin/env python3
from dataclasses import dataclass
from typing import List

@dataclass
class SearchResult:
    doc_id: str
    title: str
    agent_id: str
    relevance_score: float
    excerpt: str
    match_type: str

class VaultSearch:
    def __init__(self):
        self.documents_cache = {}

    def index_document(self, doc_id: str, title: str, content: str,
                      agent_id: str, tags: List[str], is_private: bool) -> None:
        self.documents_cache[doc_id] = {
            "doc_id": doc_id, "title": title, "content": content,
            "agent_id": agent_id, "tags": tags, "is_private": is_private
        }

    def search(self, query: str, agent_id: str, limit: int = 20) -> List[SearchResult]:
        results = []
        query_lower = query.lower()
        for doc_id, doc in self.documents_cache.items():
            if doc["agent_id"] != agent_id and doc["is_private"]:
                continue
            score = 0.0
            if query_lower in doc["title"].lower():
                score += 10.0
            if any(query_lower in tag.lower() for tag in doc["tags"]):
                score += 5.0
            if query_lower in doc["content"].lower():
                score += 2.0
            if score > 0:
                results.append(SearchResult(
                    doc_id=doc_id, title=doc["title"], agent_id=doc["agent_id"],
                    relevance_score=score, excerpt=doc["content"][:200],
                    match_type="title" if query_lower in doc["title"].lower() else "content"
                ))
        return sorted(results, key=lambda r: r.relevance_score, reverse=True)[:limit]

if __name__ == "__main__":
    search = VaultSearch()
    search.index_document("doc-001", "Alice's Crypto", "X25519...", "alice", ["crypto"], True)
    search.index_document("doc-002", "Shared Design", "Storage...", "alice", ["design"], False)
    results = search.search("crypto", "alice")
    print(f"Phase 2 search tests PASSED: {len(results)} results")
''')

print("Created 3 files in S:\\Projects\\Cipher\\phase1\\")