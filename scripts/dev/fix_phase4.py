#!/usr/bin/env python3
"""Fix Phase 4 dependencies and type errors"""

# Fix 1: Cargo.toml - add dependencies
cargo_path = r"S:\Projects\Cipher\src-tauri\Cargo.toml"
with open(cargo_path, 'r') as f:
    content = f.read()

# Replace chrono line with 3 lines
old = 'chrono = "0.4"'
new = 'rusqlite = { version = "0.31", features = ["bundled", "chrono"] }\nwalkdir = "2.4"\nchrono = "0.4"'
content = content.replace(old, new)

with open(cargo_path, 'w') as f:
    f.write(content)
print("✓ Cargo.toml: added rusqlite + walkdir")

# Fix 2: search.rs - fix type mismatch
search_path = r"S:\Projects\Cipher\src-tauri\src\search.rs"
with open(search_path, 'r') as f:
    content = f.read()

# Fix the boolean check for i32
old = '        // Index text content for plaintext files only\n        if !is_encrypted && self.is_text_file(&file_type) {'
new = '        // Index text content for plaintext files only\n        if is_encrypted == 0 && self.is_text_file(&file_type) {'
content = content.replace(old, new)

with open(search_path, 'w') as f:
    f.write(content)
print("✓ search.rs: fixed is_encrypted type check")
print("\nReady: npm run tauri dev")