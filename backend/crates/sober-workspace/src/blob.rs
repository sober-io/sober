//! Content-addressed blob storage.
//!
//! Blobs are stored as `{root}/{sha256_prefix}/{sha256}` where the prefix
//! is the first 2 hex characters. This prevents any single directory from
//! accumulating too many entries.

use std::path::PathBuf;

use sha2::{Digest, Sha256};
use tokio::fs;

use crate::WorkspaceError;

/// Content-addressed blob store backed by the local filesystem.
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    /// Create a new blob store rooted at the given directory.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Store data and return its content-addressed key (hex SHA-256).
    ///
    /// If the blob already exists (same hash), this is a no-op and returns
    /// the existing key.
    pub async fn store(&self, data: &[u8]) -> Result<String, WorkspaceError> {
        let key = hex_sha256(data);
        let path = self.blob_path(&key);

        if path.exists() {
            return Ok(key);
        }

        let parent = path.parent().expect("blob path always has a parent");
        fs::create_dir_all(parent)
            .await
            .map_err(WorkspaceError::Filesystem)?;
        fs::write(&path, data)
            .await
            .map_err(WorkspaceError::Filesystem)?;

        Ok(key)
    }

    /// Retrieve blob data by key.
    pub async fn retrieve(&self, key: &str) -> Result<Vec<u8>, WorkspaceError> {
        let path = self.blob_path(key);
        fs::read(&path).await.map_err(WorkspaceError::Filesystem)
    }

    /// Check if a blob exists.
    pub async fn exists(&self, key: &str) -> bool {
        self.blob_path(key).exists()
    }

    /// Delete a blob by key.
    pub async fn delete(&self, key: &str) -> Result<(), WorkspaceError> {
        let path = self.blob_path(key);
        if path.exists() {
            fs::remove_file(&path)
                .await
                .map_err(WorkspaceError::Filesystem)?;
        }
        Ok(())
    }

    /// Returns the filesystem path for a given content-addressed key.
    pub fn blob_path(&self, key: &str) -> PathBuf {
        let prefix = &key[..2];
        self.root.join(prefix).join(key)
    }
}

fn hex_sha256(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hex::encode(hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn store_and_retrieve_blob() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path().to_path_buf());

        let data = b"hello world";
        let key = store.store(data).await.unwrap();

        let retrieved = store.retrieve(&key).await.unwrap();
        assert_eq!(retrieved, data);
    }

    #[tokio::test]
    async fn store_deduplicates() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path().to_path_buf());

        let data = b"duplicate content";
        let key1 = store.store(data).await.unwrap();
        let key2 = store.store(data).await.unwrap();

        assert_eq!(key1, key2);
    }

    #[tokio::test]
    async fn retrieve_missing_blob_errors() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path().to_path_buf());

        let result = store.retrieve("aabbccdd").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn delete_blob() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path().to_path_buf());

        let key = store.store(b"to be deleted").await.unwrap();
        assert!(store.exists(&key).await);

        store.delete(&key).await.unwrap();
        assert!(!store.exists(&key).await);
    }

    #[tokio::test]
    async fn key_is_hex_sha256() {
        let data = b"test data";
        let key = hex_sha256(data);
        // SHA-256 produces 64 hex characters
        assert_eq!(key.len(), 64);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
