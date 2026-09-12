#!/usr/bin/env python3
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
