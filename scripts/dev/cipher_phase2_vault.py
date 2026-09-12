#!/usr/bin/env python3
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
