#!/usr/bin/env python3
import os
from pathlib import Path

root = Path(r"S:\Projects\Cipher\src-tauri\src")

required = [
    "search.rs",           # NEW (Phase 4)
    "main.rs",            # Updated with search commands
    "crypto.rs",
    "explorer.rs",
    "operations.rs",
    "vault.rs",
    "vault_git.rs",
    "git_identity.rs",
    "github_auth.rs",
    "key_backup.rs",
    "thumbnail.rs",
    "usage.rs",
    "favorites.rs",
    "quick_access.rs",
    "restrictions.rs",
]

print("=== Phase 4 File Check ===\n")
for f in required:
    path = root / f
    exists = "✓" if path.exists() else "✗ MISSING"
    print(f"{exists} {f}")

# Check if search.rs has the right content
search_file = root / "search.rs"
if search_file.exists():
    with open(search_file) as f:
        content = f.read()
    has_struct = "pub struct SearchIndex" in content
    has_fts = "fts5" in content
    has_rebuild = "fn rebuild_index" in content
    print(f"\n=== search.rs internals ===")
    print(f"✓ SearchIndex struct" if has_struct else "✗ SearchIndex missing")
    print(f"✓ FTS5 tables" if has_fts else "✗ FTS5 missing")
    print(f"✓ rebuild_index method" if has_rebuild else "✗ rebuild_index missing")

# Check Cargo.toml
cargo = Path(r"S:\Projects\Cipher\src-tauri\Cargo.toml")
if cargo.exists():
    with open(cargo) as f:
        content = f.read()
    has_rusqlite = "rusqlite" in content
    has_walkdir = "walkdir" in content
    print(f"\n=== Cargo.toml ===")
    print(f"✓ rusqlite" if has_rusqlite else "✗ rusqlite missing")
    print(f"✓ walkdir" if has_walkdir else "✗ walkdir missing")

# Check main.rs for search commands
main = Path(r"S:\Projects\Cipher\src-tauri\src\main.rs")
if main.exists():
    with open(main) as f:
        content = f.read()
    has_mod = "mod search" in content
    has_init = "init_search_index" in content
    has_rebuild = "rebuild_search_index" in content
    has_search = "search_files" in content
    print(f"\n=== main.rs search integration ===")
    print(f"✓ mod search" if has_mod else "✗ mod search missing")
    print(f"✓ init_search_index" if has_init else "✗ init_search_index missing")
    print(f"✓ rebuild_search_index" if has_rebuild else "✗ rebuild_search_index missing")
    print(f"✓ search_files" if has_search else "✗ search_files missing")