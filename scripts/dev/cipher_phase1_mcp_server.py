#!/usr/bin/env python3
"""
Cipher Phase 1 MCP Server: Agent Cryptography & Key Management

Exposes tools for:
- Agent registration (keypair generation)
- Public key lookup (safe to share)
- Encryption (for other agents' data)
- Decryption (only for your own data)
- Key management
"""

from mcp.server.fastmcp import FastMCP
import json
from cipher_phase1_crypto import AgentRegistry, AgentCrypto, EncryptedEnvelope

# Global registry (Phase 2: persistent storage)
registry = AgentRegistry()

# Initialize FastMCP server
mcp = FastMCP("cipher-crypto")


@mcp.tool()
def register_agent(agent_id: str) -> dict:
    """
    Register a new agent and generate an X25519 keypair.
    
    Returns:
    - agent_id: The agent's unique identifier
    - public_key: PEM-encoded public key (safe to share)
    - created_at: ISO timestamp
    
    The private key is securely stored server-side and never exposed.
    """
    try:
        keypair = registry.register_agent(agent_id)
        return {
            "status": "success",
            "agent_id": keypair.agent_id,
            "public_key": keypair.public_key_pem,
            "created_at": keypair.created_at,
            "message": f"Agent '{agent_id}' registered with X25519 keypair",
        }
    except ValueError as e:
        return {"status": "error", "message": str(e)}


@mcp.tool()
def get_public_key(agent_id: str) -> dict:
    """
    Retrieve the public key for an agent (safe to share).
    
    Args:
    - agent_id: The agent's identifier
    
    Returns:
    - public_key: PEM-encoded X25519 public key
    """
    public_key = registry.get_agent_public_key(agent_id)
    if not public_key:
        return {"status": "error", "message": f"Agent '{agent_id}' not found"}
    return {
        "status": "success",
        "agent_id": agent_id,
        "public_key": public_key,
    }


@mcp.tool()
def encrypt_data(agent_id: str, recipient_agent_id: str, plaintext: str) -> dict:
    """
    Encrypt data for another agent using their public key.
    
    Args:
    - agent_id: Your agent ID (for audit/context)
    - recipient_agent_id: Who this data is encrypted for
    - plaintext: The data to encrypt (JSON string recommended)
    
    Returns:
    - encrypted_envelope: Contains ephemeral_public_key, ciphertext, nonce
    - Only the recipient can decrypt this using their private key
    """
    recipient_public_key = registry.get_agent_public_key(recipient_agent_id)
    if not recipient_public_key:
        return {"status": "error", "message": f"Recipient agent '{recipient_agent_id}' not found"}

    try:
        envelope = AgentCrypto.encrypt_for_agent(recipient_public_key, plaintext)
        envelope.agent_id = recipient_agent_id
        return {
            "status": "success",
            "encrypted_for": recipient_agent_id,
            "envelope": {
                "agent_id": envelope.agent_id,
                "ephemeral_public_key": envelope.ephemeral_public_key,
                "ciphertext": envelope.ciphertext,
                "nonce": envelope.nonce,
            },
        }
    except Exception as e:
        return {"status": "error", "message": f"Encryption failed: {str(e)}"}


@mcp.tool()
def decrypt_envelope(agent_id: str, envelope_json: str) -> dict:
    """
    Decrypt an encrypted envelope using your private key.
    
    Args:
    - agent_id: Your agent ID (verifies you have the right key)
    - envelope_json: JSON string with ephemeral_public_key, ciphertext, nonce
    
    Returns:
    - plaintext: The decrypted data
    
    Only the agent the data was encrypted for can successfully decrypt.
    """
    private_key = registry.get_agent_private_key(agent_id)
    if not private_key:
        return {"status": "error", "message": f"Agent '{agent_id}' not found"}

    try:
        envelope_dict = json.loads(envelope_json)
        envelope = EncryptedEnvelope(
            agent_id=envelope_dict.get("agent_id", agent_id),
            ephemeral_public_key=envelope_dict["ephemeral_public_key"],
            ciphertext=envelope_dict["ciphertext"],
            nonce=envelope_dict["nonce"],
        )
        plaintext = AgentCrypto.decrypt_envelope(private_key, envelope)
        return {
            "status": "success",
            "plaintext": plaintext,
            "agent_id": agent_id,
        }
    except json.JSONDecodeError:
        return {"status": "error", "message": "Invalid envelope JSON"}
    except Exception as e:
        return {"status": "error", "message": f"Decryption failed: {str(e)}"}


@mcp.tool()
def list_agents() -> dict:
    """
    List all registered agent IDs (for discovery, not sensitive data).
    """
    agents = registry.list_agents()
    return {
        "status": "success",
        "agents": agents,
        "count": len(agents),
    }


@mcp.tool()
def get_agent_status(agent_id: str) -> dict:
    """
    Check if an agent exists and get basic metadata.
    """
    public_key = registry.get_agent_public_key(agent_id)
    if not public_key:
        return {"status": "not_found", "agent_id": agent_id}
    
    keypair = registry.agents.get(agent_id)
    return {
        "status": "active",
        "agent_id": agent_id,
        "created_at": keypair.created_at if keypair else None,
        "public_key_hash": hash(public_key) % (10**8),  # Short fingerprint, not sensitive
    }


if __name__ == "__main__":
    import asyncio
    
    # Run on stdio for local testing
    # In production: use Streamable HTTP or Cloud Transport
    print("Starting Cipher Phase 1 MCP Server (stdio transport)...")
    print("Available tools:")
    print("  - register_agent")
    print("  - get_public_key")
    print("  - encrypt_data")
    print("  - decrypt_envelope")
    print("  - list_agents")
    print("  - get_agent_status")
    print("\nWaiting for MCP client connections...\n")
    
    asyncio.run(mcp.run(debug=True))
