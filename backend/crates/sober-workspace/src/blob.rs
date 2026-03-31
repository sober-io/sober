//! Content-addressed blob storage.
//!
//! Blobs are stored as `{root}/{sha256_prefix}/{sha256}` where the prefix
//! is the first 2 hex characters. This prevents any single directory from
//! accumulating too many entries.

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use metrics::{counter, histogram};
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
        let start = Instant::now();
        let key = hex_sha256(data);
        let path = self.blob_path(&key);

        if path.exists() {
            counter!("sober_workspace_blob_operations_total", "operation" => "store", "status" => "dedup")
                .increment(1);
            histogram!("sober_workspace_blob_duration_seconds", "operation" => "store")
                .record(start.elapsed().as_secs_f64());
            return Ok(key);
        }

        let parent = path.parent().expect("blob path always has a parent");
        fs::create_dir_all(parent)
            .await
            .map_err(|e| {
                counter!("sober_workspace_blob_operations_total", "operation" => "store", "status" => "error")
                    .increment(1);
                histogram!("sober_workspace_blob_duration_seconds", "operation" => "store")
                    .record(start.elapsed().as_secs_f64());
                WorkspaceError::Filesystem(e)
            })?;
        fs::write(&path, data)
            .await
            .map_err(|e| {
                counter!("sober_workspace_blob_operations_total", "operation" => "store", "status" => "error")
                    .increment(1);
                histogram!("sober_workspace_blob_duration_seconds", "operation" => "store")
                    .record(start.elapsed().as_secs_f64());
                WorkspaceError::Filesystem(e)
            })?;

        counter!("sober_workspace_blob_operations_total", "operation" => "store", "status" => "success")
            .increment(1);
        counter!("sober_workspace_blob_bytes_total").increment(data.len() as u64);
        histogram!("sober_workspace_blob_duration_seconds", "operation" => "store")
            .record(start.elapsed().as_secs_f64());

        Ok(key)
    }

    /// Retrieve blob data by key.
    pub async fn retrieve(&self, key: &str) -> Result<Vec<u8>, WorkspaceError> {
        let start = Instant::now();
        let path = self.blob_path(key);
        match fs::read(&path).await {
            Ok(data) => {
                counter!("sober_workspace_blob_operations_total", "operation" => "retrieve", "status" => "success")
                    .increment(1);
                histogram!("sober_workspace_blob_duration_seconds", "operation" => "retrieve")
                    .record(start.elapsed().as_secs_f64());
                Ok(data)
            }
            Err(e) => {
                let status = if e.kind() == std::io::ErrorKind::NotFound {
                    "not_found"
                } else {
                    "error"
                };
                counter!("sober_workspace_blob_operations_total", "operation" => "retrieve", "status" => status)
                    .increment(1);
                histogram!("sober_workspace_blob_duration_seconds", "operation" => "retrieve")
                    .record(start.elapsed().as_secs_f64());
                Err(WorkspaceError::Filesystem(e))
            }
        }
    }

    /// Check if a blob exists.
    pub async fn exists(&self, key: &str) -> bool {
        self.blob_path(key).exists()
    }

    /// Delete a blob by key.
    pub async fn delete(&self, key: &str) -> Result<(), WorkspaceError> {
        let start = Instant::now();
        let path = self.blob_path(key);
        if path.exists() {
            fs::remove_file(&path)
                .await
                .map_err(|e| {
                    counter!("sober_workspace_blob_operations_total", "operation" => "delete", "status" => "error")
                        .increment(1);
                    histogram!("sober_workspace_blob_duration_seconds", "operation" => "delete")
                        .record(start.elapsed().as_secs_f64());
                    WorkspaceError::Filesystem(e)
                })?;
            counter!("sober_workspace_blob_operations_total", "operation" => "delete", "status" => "success")
                .increment(1);
        } else {
            counter!("sober_workspace_blob_operations_total", "operation" => "delete", "status" => "not_found")
                .increment(1);
        }
        histogram!("sober_workspace_blob_duration_seconds", "operation" => "delete")
            .record(start.elapsed().as_secs_f64());
        Ok(())
    }

    /// Lists all blob keys and their modification times.
    ///
    /// Walks the blob directory tree (`{root}/{prefix}/{key}`) and returns
    /// `(key, modified_time)` pairs for every blob file found.
    pub async fn list_keys(&self) -> Result<Vec<(String, std::time::SystemTime)>, WorkspaceError> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let mut entries = Vec::new();
            let read_dir = match std::fs::read_dir(&root) {
                Ok(d) => d,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(entries),
                Err(e) => return Err(WorkspaceError::Filesystem(e)),
            };
            for prefix_entry in read_dir {
                let prefix_entry = prefix_entry.map_err(WorkspaceError::Filesystem)?;
                if !prefix_entry.path().is_dir() {
                    continue;
                }
                let sub_dir =
                    std::fs::read_dir(prefix_entry.path()).map_err(WorkspaceError::Filesystem)?;
                for blob_entry in sub_dir {
                    let blob_entry = blob_entry.map_err(WorkspaceError::Filesystem)?;
                    let path = blob_entry.path();
                    if path.is_file() {
                        let key = path
                            .file_name()
                            .expect("blob file always has a name")
                            .to_string_lossy()
                            .into_owned();
                        let modified = blob_entry
                            .metadata()
                            .map_err(WorkspaceError::Filesystem)?
                            .modified()
                            .map_err(WorkspaceError::Filesystem)?;
                        entries.push((key, modified));
                    }
                }
            }
            Ok(entries)
        })
        .await
        .expect("spawn_blocking join")
    }

    /// Returns the total size in bytes of all stored blobs.
    pub async fn total_size(&self) -> Result<u64, WorkspaceError> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let mut total: u64 = 0;
            let read_dir = match std::fs::read_dir(&root) {
                Ok(d) => d,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
                Err(e) => return Err(WorkspaceError::Filesystem(e)),
            };
            for prefix_entry in read_dir {
                let prefix_entry = prefix_entry.map_err(WorkspaceError::Filesystem)?;
                if !prefix_entry.path().is_dir() {
                    continue;
                }
                let sub_dir =
                    std::fs::read_dir(prefix_entry.path()).map_err(WorkspaceError::Filesystem)?;
                for blob_entry in sub_dir {
                    let blob_entry = blob_entry.map_err(WorkspaceError::Filesystem)?;
                    if blob_entry.path().is_file() {
                        total += blob_entry
                            .metadata()
                            .map_err(WorkspaceError::Filesystem)?
                            .len();
                    }
                }
            }
            Ok(total)
        })
        .await
        .expect("spawn_blocking join")
    }

    /// Lists blob keys in batches, filtering out files newer than `grace_period`.
    ///
    /// Returns a vector of batches (each batch is a `Vec<String>` of blob keys).
    pub async fn list_keys_batched(
        &self,
        batch_size: usize,
        grace_period: Duration,
    ) -> Result<Vec<Vec<String>>, WorkspaceError> {
        let cutoff = SystemTime::now() - grace_period;
        let all = self.list_keys().await?;
        let eligible: Vec<String> = all
            .into_iter()
            .filter(|(_, modified)| *modified <= cutoff)
            .map(|(key, _)| key)
            .collect();
        Ok(eligible
            .chunks(batch_size)
            .map(|chunk| chunk.to_vec())
            .collect())
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

    #[tokio::test]
    async fn list_keys_returns_stored_blobs() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path().to_path_buf());

        let key1 = store.store(b"blob one").await.unwrap();
        let key2 = store.store(b"blob two").await.unwrap();

        let keys = store.list_keys().await.unwrap();
        let key_strs: Vec<&str> = keys.iter().map(|(k, _)| k.as_str()).collect();

        assert_eq!(keys.len(), 2);
        assert!(key_strs.contains(&key1.as_str()));
        assert!(key_strs.contains(&key2.as_str()));
    }

    #[tokio::test]
    async fn list_keys_empty_store() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path().to_path_buf());

        let keys = store.list_keys().await.unwrap();
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn total_size_sums_all_blobs() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path().to_path_buf());

        let data1 = b"hello";
        let data2 = b"world!!!";
        store.store(data1).await.unwrap();
        store.store(data2).await.unwrap();

        let size = store.total_size().await.unwrap();
        assert_eq!(size, (data1.len() + data2.len()) as u64);
    }

    #[tokio::test]
    async fn total_size_empty_store() {
        let tmp = TempDir::new().unwrap();
        let store = BlobStore::new(tmp.path().to_path_buf());

        let size = store.total_size().await.unwrap();
        assert_eq!(size, 0);
    }
}
