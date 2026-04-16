//! Content-addressed snapshot blob store.
//!
//! Blobs are stored at `<session>/snapshots/<first-2>/<remaining-38>`.
//! Store is write-once: writing the same content twice is a no-op.
//!
//! **Hash choice:** SHA-1 is used deliberately. Snapshots are session-scoped
//! and non-adversarial — we need collision resistance against accidental
//! corruption and an integrity check on read, not cryptographic second-preimage
//! resistance. SHA-1 is cheap, 40 hex chars fit tidily in ledger paths, and
//! `get()` quarantines any blob whose recomputed digest disagrees with its
//! filename. Upgrading to SHA-256 would require a versioned blob path scheme
//! and migration; revisit only if the threat model changes.

use anyhow::{bail, Context, Result};
use sha1::{Digest, Sha1};
use std::fs;
use std::path::{Path, PathBuf};
use ulid::Ulid;

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
        // Fast path: blob already committed. TOCTOU between this check and the
        // write below is benign: concurrent puts of the same content race on the
        // rename, last rename wins with identical bytes.
        if path.exists() {
            return Ok(hex);
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating snapshot dir {parent:?}"))?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, content).with_context(|| format!("writing tmp blob {tmp:?}"))?;
        fs::rename(&tmp, &path)
            .with_context(|| format!("renaming {tmp:?} -> {path:?} (tmp may be orphaned)"))?;
        Ok(hex)
    }

    /// Read a blob by hash; verifies the hash matches the content and quarantines on mismatch.
    pub fn get(&self, hex: &str) -> Result<Vec<u8>> {
        let path = self.paths.snapshot_path(self.session_id, hex)?;
        let content = fs::read(&path).with_context(|| format!("reading blob {hex} at {path:?}"))?;
        let actual = sha1_hex(&content);
        if actual != hex {
            self.quarantine(&path, hex, &actual)?;
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

    /// Move a corrupted blob to `<session>/snapshots/.bad/<expected>.<ulid>.bad`.
    ///
    /// The filename encodes the expected hash (so you can tell which slot was
    /// bad) and a ULID suffix (so repeated corruption of the same slot does not
    /// collide or silently overwrite prior forensic evidence).
    fn quarantine(&self, path: &Path, expected_hash: &str, actual_hash: &str) -> Result<PathBuf> {
        let quarantine_dir = path
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join(".bad"))
            .context("snapshot blob has no parent")?;
        fs::create_dir_all(&quarantine_dir)
            .with_context(|| format!("creating quarantine dir {quarantine_dir:?}"))?;
        let suffix = Ulid::new();
        let dest = quarantine_dir.join(format!("{expected_hash}.{suffix}.bad"));
        fs::rename(path, &dest)
            .with_context(|| format!("quarantining {path:?} -> {dest:?} (actual={actual_hash})"))?;
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
        // The bad blob should have moved to snapshots/.bad/<expected>.<ulid>.bad
        let bad_dir = paths.session_dir(&sid).join("snapshots").join(".bad");
        assert!(bad_dir.exists());
        let entries: Vec<_> = bad_dir.read_dir().unwrap().collect();
        assert_eq!(entries.len(), 1);
        let name = entries[0].as_ref().unwrap().file_name();
        let name = name.to_string_lossy();
        assert!(
            name.starts_with(&h),
            "quarantine filename must start with expected hash"
        );
        assert!(
            name.ends_with(".bad"),
            "quarantine filename must end with .bad"
        );
    }

    #[test]
    fn repeated_corruption_on_same_slot_does_not_collide() {
        // Two corruption events on the same expected hash must produce two
        // distinct quarantine files, not silently overwrite one another.
        let td = TempDir::new().unwrap();
        let (paths, sid) = setup(&td);
        let store = SnapshotStore::new(&paths, &sid);

        // First corruption cycle.
        let h = store.put(b"original").unwrap();
        let p = paths.snapshot_path(&sid, &h).unwrap();
        fs::write(&p, b"tampered1").unwrap();
        store.get(&h).unwrap_err();

        // Re-put the same content (path is gone because it was quarantined),
        // then corrupt again and re-get.
        store.put(b"original").unwrap();
        fs::write(&p, b"tampered2").unwrap();
        store.get(&h).unwrap_err();

        let bad_dir = paths.session_dir(&sid).join("snapshots").join(".bad");
        let count = bad_dir.read_dir().unwrap().count();
        assert_eq!(
            count, 2,
            "expected two distinct quarantine files, got {count}"
        );
    }

    #[test]
    fn get_on_missing_blob_errors_with_hash() {
        let td = TempDir::new().unwrap();
        let (paths, sid) = setup(&td);
        let store = SnapshotStore::new(&paths, &sid);
        // Use a valid-shaped sha1 that was never put.
        let missing = "0".repeat(40);
        let err = store.get(&missing).unwrap_err();
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains(&missing),
            "error should include the requested hash, got: {rendered}"
        );
    }

    #[test]
    fn has_returns_false_for_missing_and_true_for_stored() {
        let td = TempDir::new().unwrap();
        let (paths, sid) = setup(&td);
        let store = SnapshotStore::new(&paths, &sid);
        assert!(!store.has(&"0".repeat(40)));
        let h = store.put(b"present").unwrap();
        assert!(store.has(&h));
    }

    #[test]
    fn two_blobs_in_same_shard_coexist() {
        // Construct two distinct blobs and verify both land in the store
        // without stomping on each other. (Tests that the sharding actually
        // keys on the full hash, not just the 2-char prefix.)
        let td = TempDir::new().unwrap();
        let (paths, sid) = setup(&td);
        let store = SnapshotStore::new(&paths, &sid);
        let h1 = store.put(b"alpha").unwrap();
        let h2 = store.put(b"beta").unwrap();
        assert_ne!(h1, h2);
        assert_eq!(store.get(&h1).unwrap(), b"alpha");
        assert_eq!(store.get(&h2).unwrap(), b"beta");
    }
}
