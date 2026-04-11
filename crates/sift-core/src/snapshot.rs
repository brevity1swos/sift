//! Content-addressed snapshot blob store.
//!
//! Blobs are stored at `<session>/snapshots/<first-2>/<remaining-38>`.
//! Store is write-once: writing the same content twice is a no-op.

use anyhow::{bail, Context, Result};
use sha1::{Digest, Sha1};
use std::fs;
use std::path::{Path, PathBuf};

use crate::paths::Paths;

pub struct SnapshotStore<'a> {
    paths: &'a Paths,
    session_id: &'a str,
}

impl<'a> SnapshotStore<'a> {
    pub fn new(paths: &'a Paths, session_id: &'a str) -> Self {
        Self { paths, session_id }
    }

    /// Hash `content`, write it to the sharded blob path (if not already present),
    /// and return the sha1 hex string.
    pub fn put(&self, content: &[u8]) -> Result<String> {
        let hex = sha1_hex(content);
        let path = self.paths.snapshot_path(self.session_id, &hex)?;
        if path.exists() {
            return Ok(hex);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating snapshot dir {parent:?}"))?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, content).with_context(|| format!("writing tmp blob {tmp:?}"))?;
        fs::rename(&tmp, &path).with_context(|| format!("renaming to {path:?}"))?;
        Ok(hex)
    }

    /// Read a blob by hash; verifies the hash matches the content and quarantines on mismatch.
    pub fn get(&self, hex: &str) -> Result<Vec<u8>> {
        let path = self.paths.snapshot_path(self.session_id, hex)?;
        let content = fs::read(&path).with_context(|| format!("reading blob {path:?}"))?;
        let actual = sha1_hex(&content);
        if actual != hex {
            self.quarantine(&path, &actual)?;
            bail!("snapshot blob corruption: {path:?} (expected {hex}, got {actual})");
        }
        Ok(content)
    }

    /// Check if a blob exists without reading it. Returns false on any resolution error.
    pub fn has(&self, hex: &str) -> bool {
        self.paths
            .snapshot_path(self.session_id, hex)
            .map(|p| p.exists())
            .unwrap_or(false)
    }

    fn quarantine(&self, path: &Path, actual_hash: &str) -> Result<PathBuf> {
        let quarantine_dir = path
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join(".bad"))
            .context("snapshot blob has no parent")?;
        fs::create_dir_all(&quarantine_dir)?;
        let dest = quarantine_dir.join(actual_hash);
        fs::rename(path, &dest)?;
        Ok(dest)
    }
}

fn sha1_hex(content: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(content);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup(td: &TempDir) -> (Paths, String) {
        let paths = Paths::new(td.path());
        let session_id = "sess-test".to_string();
        fs::create_dir_all(paths.session_dir(&session_id).join("snapshots")).unwrap();
        (paths, session_id)
    }

    #[test]
    fn put_stores_content_at_sharded_path() {
        let td = TempDir::new().unwrap();
        let (paths, sid) = setup(&td);
        let store = SnapshotStore::new(&paths, &sid);
        let hex = store.put(b"hello").unwrap();
        assert_eq!(hex.len(), 40); // sha1 hex
        let blob_path = paths.snapshot_path(&sid, &hex).unwrap();
        assert!(blob_path.exists());
        assert_eq!(fs::read(&blob_path).unwrap(), b"hello");
    }

    #[test]
    fn put_is_idempotent() {
        let td = TempDir::new().unwrap();
        let (paths, sid) = setup(&td);
        let store = SnapshotStore::new(&paths, &sid);
        let h1 = store.put(b"world").unwrap();
        let h2 = store.put(b"world").unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn get_returns_content() {
        let td = TempDir::new().unwrap();
        let (paths, sid) = setup(&td);
        let store = SnapshotStore::new(&paths, &sid);
        let h = store.put(b"hello").unwrap();
        assert_eq!(store.get(&h).unwrap(), b"hello");
    }

    #[test]
    fn get_quarantines_corrupted_blob() {
        let td = TempDir::new().unwrap();
        let (paths, sid) = setup(&td);
        let store = SnapshotStore::new(&paths, &sid);
        let h = store.put(b"original").unwrap();
        // Tamper with the blob on disk.
        let p = paths.snapshot_path(&sid, &h).unwrap();
        fs::write(&p, b"tampered").unwrap();
        let err = store.get(&h).unwrap_err();
        assert!(err.to_string().contains("corruption"));
        // The bad blob should have moved to snapshots/.bad/
        let bad_dir = paths.session_dir(&sid).join("snapshots").join(".bad");
        assert!(bad_dir.exists());
        assert!(bad_dir.read_dir().unwrap().next().is_some());
    }
}
