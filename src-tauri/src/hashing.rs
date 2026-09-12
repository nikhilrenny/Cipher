// File hashing — MD5 + SHA-256, streamed so large files don't get fully
// buffered into memory. Used by the right-click "View hash" / "Compare
// hashes" actions.

use md5::{Digest as Md5Digest, Md5};
use sha2::{Digest as Sha2Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct FileHashes {
    pub path: String,
    pub name: String,
    pub md5: String,
    pub sha256: String,
}

pub fn compute_file_hashes(path: &str) -> Result<FileHashes, String> {
    let p = Path::new(path);
    if p.is_dir() {
        return Err("Can't hash a folder".to_string());
    }
    let file = File::open(p).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(file);
    let mut md5_hasher = Md5::new();
    let mut sha_hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        md5_hasher.update(&buf[..n]);
        sha_hasher.update(&buf[..n]);
    }
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(FileHashes {
        path: path.to_string(),
        name,
        md5: hex::encode(md5_hasher.finalize()),
        sha256: hex::encode(sha_hasher.finalize()),
    })
}
