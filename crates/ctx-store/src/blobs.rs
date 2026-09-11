//! Content-addressed blob storage on the filesystem.
//!
//! Addressing by content hash makes writes idempotent: capturing the same
//! 40 MB test log twice costs one copy, and a snapshot handle stays valid for
//! as long as the content exists.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Hex blake3 digest of `bytes`.
pub fn hash(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Filesystem location for a blob, sharded two levels to keep directories
/// small on repositories that capture a lot of output.
fn blob_path(root: &Path, hash: &str) -> PathBuf {
    debug_assert!(hash.len() >= 4, "blake3 hex digests are 64 chars");
    root.join(&hash[0..2]).join(&hash[2..4]).join(hash)
}

/// Write `bytes` and return its hash. Idempotent: an existing blob with the
/// same content is left untouched.
pub fn put(root: &Path, bytes: &[u8]) -> Result<String> {
    let h = hash(bytes);
    let path = blob_path(root, &h);
    if path.exists() {
        return Ok(h);
    }
    let parent = path
        .parent()
        .expect("blob paths always have a sharded parent");
    fs::create_dir_all(parent)
        .with_context(|| format!("creating blob directory {}", parent.display()))?;
    // Write to a temporary sibling then rename, so a crash mid-write cannot
    // leave a truncated blob visible under a hash that promises full content.
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).with_context(|| format!("writing blob {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("finalising blob {}", path.display()))?;
    Ok(h)
}

/// Read a blob back byte-exactly.
pub fn get(root: &Path, hash: &str) -> Result<Vec<u8>> {
    let path = blob_path(root, hash);
    fs::read(&path).with_context(|| format!("blob {hash} not found at {}", path.display()))
}

pub fn exists(root: &Path, hash: &str) -> bool {
    blob_path(root, hash).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_bytes_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let data = b"Tests: 428\nFailed: 7\n\x00\xff binary too";
        let h = put(dir.path(), data).unwrap();
        assert_eq!(get(dir.path(), &h).unwrap(), data);
    }

    #[test]
    fn put_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let h1 = put(dir.path(), b"same").unwrap();
        let h2 = put(dir.path(), b"same").unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn distinct_content_distinct_hash() {
        let dir = tempfile::tempdir().unwrap();
        assert_ne!(
            put(dir.path(), b"a").unwrap(),
            put(dir.path(), b"b").unwrap()
        );
    }

    #[test]
    fn missing_blob_is_an_error_not_empty_bytes() {
        let dir = tempfile::tempdir().unwrap();
        assert!(get(dir.path(), &"0".repeat(64)).is_err());
    }

    #[test]
    fn large_blob_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..4 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let h = put(dir.path(), &data).unwrap();
        assert_eq!(get(dir.path(), &h).unwrap(), data);
    }
}
