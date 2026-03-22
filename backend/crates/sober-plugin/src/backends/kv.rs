//! Key-value backend trait and implementations.
//!
//! [`KvBackend`] is an object-safe trait for plugin-scoped key-value
//! storage.  Two implementations are provided:
//!
//! - [`InMemoryKvBackend`] — for tests and offline mode.
//! - [`PgKvBackend`] — PostgreSQL-backed persistent storage.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;

use sober_core::types::ids::PluginId;

/// Object-safe backend for plugin-scoped key-value storage.
///
/// Implementations can be in-memory (for tests) or database-backed
/// (production).  Each method returns a boxed future to maintain
/// object safety while supporting async database operations.
pub trait KvBackend: Send + Sync {
    /// Retrieves a value by plugin ID and key.
    fn get(
        &self,
        plugin_id: PluginId,
        key: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<serde_json::Value>, String>> + Send + '_>>;

    /// Stores a value for the given plugin ID and key.
    fn set(
        &self,
        plugin_id: PluginId,
        key: &str,
        value: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;

    /// Deletes a key from the given plugin's store.
    fn delete(
        &self,
        plugin_id: PluginId,
        key: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>>;

    /// Lists keys for the given plugin, optionally filtered by prefix.
    fn list_keys(
        &self,
        plugin_id: PluginId,
        prefix: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, String>> + Send + '_>>;
}

// ---------------------------------------------------------------------------
// InMemoryKvBackend
// ---------------------------------------------------------------------------

/// In-memory KV backend for tests and offline mode.
///
/// Data is keyed by `(PluginId, key)` and stored in a `HashMap` behind
/// a `Mutex`.  All operations resolve synchronously (returned futures
/// are immediately ready).
#[derive(Debug, Default)]
pub struct InMemoryKvBackend {
    store: Mutex<HashMap<(PluginId, String), serde_json::Value>>,
}

impl InMemoryKvBackend {
    /// Creates a new empty in-memory backend.
    pub fn new() -> Self {
        Self::default()
    }
}

impl KvBackend for InMemoryKvBackend {
    fn get(
        &self,
        plugin_id: PluginId,
        key: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<serde_json::Value>, String>> + Send + '_>> {
        let result = self
            .store
            .lock()
            .map(|store| store.get(&(plugin_id, key.to_owned())).cloned())
            .map_err(|e| format!("kv lock poisoned: {e}"));
        Box::pin(std::future::ready(result))
    }

    fn set(
        &self,
        plugin_id: PluginId,
        key: &str,
        value: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let result = self
            .store
            .lock()
            .map(|mut store| {
                store.insert((plugin_id, key.to_owned()), value);
            })
            .map_err(|e| format!("kv lock poisoned: {e}"));
        Box::pin(std::future::ready(result))
    }

    fn delete(
        &self,
        plugin_id: PluginId,
        key: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let result = self
            .store
            .lock()
            .map(|mut store| {
                store.remove(&(plugin_id, key.to_owned()));
            })
            .map_err(|e| format!("kv lock poisoned: {e}"));
        Box::pin(std::future::ready(result))
    }

    fn list_keys(
        &self,
        plugin_id: PluginId,
        prefix: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, String>> + Send + '_>> {
        let result = self
            .store
            .lock()
            .map(|store| {
                store
                    .keys()
                    .filter(|(pid, k)| *pid == plugin_id && prefix.is_none_or(|p| k.starts_with(p)))
                    .map(|(_, k)| k.clone())
                    .collect()
            })
            .map_err(|e| format!("kv lock poisoned: {e}"));
        Box::pin(std::future::ready(result))
    }
}

// ---------------------------------------------------------------------------
// PgKvBackend
// ---------------------------------------------------------------------------

/// PostgreSQL-backed KV backend for production use.
///
/// Uses the `plugin_kv_data` table with a single JSONB `data` column
/// per plugin.  Keys are stored as top-level JSONB object keys within
/// the `data` column.
#[derive(Debug, Clone)]
pub struct PgKvBackend {
    pool: sqlx::PgPool,
}

impl PgKvBackend {
    /// Creates a new PostgreSQL-backed KV backend.
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }
}

impl KvBackend for PgKvBackend {
    fn get(
        &self,
        plugin_id: PluginId,
        key: &str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<serde_json::Value>, String>> + Send + '_>> {
        let pool = self.pool.clone();
        let key = key.to_owned();
        Box::pin(async move {
            let row: Option<(Option<serde_json::Value>,)> =
                sqlx::query_as("SELECT data->$2 FROM plugin_kv_data WHERE plugin_id = $1")
                    .bind(plugin_id.as_uuid())
                    .bind(&key)
                    .fetch_optional(&pool)
                    .await
                    .map_err(|e| format!("kv get failed: {e}"))?;

            Ok(row.and_then(|(v,)| v))
        })
    }

    fn set(
        &self,
        plugin_id: PluginId,
        key: &str,
        value: serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let pool = self.pool.clone();
        let key = key.to_owned();
        Box::pin(async move {
            sqlx::query(
                "INSERT INTO plugin_kv_data (plugin_id, data, updated_at) \
                 VALUES ($1, jsonb_build_object($2::text, $3), now()) \
                 ON CONFLICT (plugin_id) DO UPDATE \
                 SET data = plugin_kv_data.data || jsonb_build_object($2::text, $3), \
                     updated_at = now()",
            )
            .bind(plugin_id.as_uuid())
            .bind(&key)
            .bind(&value)
            .execute(&pool)
            .await
            .map_err(|e| format!("kv set failed: {e}"))?;

            Ok(())
        })
    }

    fn delete(
        &self,
        plugin_id: PluginId,
        key: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        let pool = self.pool.clone();
        let key = key.to_owned();
        Box::pin(async move {
            sqlx::query(
                "UPDATE plugin_kv_data SET data = data - $2, updated_at = now() \
                 WHERE plugin_id = $1",
            )
            .bind(plugin_id.as_uuid())
            .bind(&key)
            .execute(&pool)
            .await
            .map_err(|e| format!("kv delete failed: {e}"))?;

            Ok(())
        })
    }

    fn list_keys(
        &self,
        plugin_id: PluginId,
        prefix: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, String>> + Send + '_>> {
        let pool = self.pool.clone();
        let prefix = prefix.map(String::from);
        Box::pin(async move {
            let rows: Vec<(String,)> = sqlx::query_as(
                "SELECT k FROM plugin_kv_data, jsonb_object_keys(data) AS k \
                 WHERE plugin_id = $1",
            )
            .bind(plugin_id.as_uuid())
            .fetch_all(&pool)
            .await
            .map_err(|e| format!("kv list failed: {e}"))?;

            let mut keys: Vec<String> = rows.into_iter().map(|(k,)| k).collect();
            if let Some(ref prefix) = prefix {
                keys.retain(|k| k.starts_with(prefix.as_str()));
            }

            Ok(keys)
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_get_missing_key() {
        let backend = InMemoryKvBackend::new();
        let result = backend.get(PluginId::new(), "missing").await;
        assert_eq!(result.unwrap(), None);
    }

    #[tokio::test]
    async fn in_memory_set_and_get() {
        let backend = InMemoryKvBackend::new();
        let pid = PluginId::new();
        let value = serde_json::json!("hello");

        backend.set(pid, "key1", value.clone()).await.unwrap();
        let result = backend.get(pid, "key1").await.unwrap();
        assert_eq!(result, Some(value));
    }

    #[tokio::test]
    async fn in_memory_delete() {
        let backend = InMemoryKvBackend::new();
        let pid = PluginId::new();

        backend
            .set(pid, "key1", serde_json::json!(42))
            .await
            .unwrap();
        backend.delete(pid, "key1").await.unwrap();
        let result = backend.get(pid, "key1").await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn in_memory_list_keys_all() {
        let backend = InMemoryKvBackend::new();
        let pid = PluginId::new();

        backend
            .set(pid, "alpha", serde_json::json!(1))
            .await
            .unwrap();
        backend
            .set(pid, "beta", serde_json::json!(2))
            .await
            .unwrap();

        let mut keys = backend.list_keys(pid, None).await.unwrap();
        keys.sort();
        assert_eq!(keys, vec!["alpha", "beta"]);
    }

    #[tokio::test]
    async fn in_memory_list_keys_with_prefix() {
        let backend = InMemoryKvBackend::new();
        let pid = PluginId::new();

        backend
            .set(pid, "user:name", serde_json::json!("alice"))
            .await
            .unwrap();
        backend
            .set(pid, "user:age", serde_json::json!(30))
            .await
            .unwrap();
        backend
            .set(pid, "config:theme", serde_json::json!("dark"))
            .await
            .unwrap();

        let mut keys = backend.list_keys(pid, Some("user:")).await.unwrap();
        keys.sort();
        assert_eq!(keys, vec!["user:age", "user:name"]);
    }

    #[tokio::test]
    async fn in_memory_plugin_isolation() {
        let backend = InMemoryKvBackend::new();
        let pid1 = PluginId::new();
        let pid2 = PluginId::new();

        backend
            .set(pid1, "key", serde_json::json!("from_plugin_1"))
            .await
            .unwrap();
        backend
            .set(pid2, "key", serde_json::json!("from_plugin_2"))
            .await
            .unwrap();

        assert_eq!(
            backend.get(pid1, "key").await.unwrap(),
            Some(serde_json::json!("from_plugin_1"))
        );
        assert_eq!(
            backend.get(pid2, "key").await.unwrap(),
            Some(serde_json::json!("from_plugin_2"))
        );
    }

    #[tokio::test]
    async fn in_memory_overwrite() {
        let backend = InMemoryKvBackend::new();
        let pid = PluginId::new();

        backend
            .set(pid, "key", serde_json::json!("old"))
            .await
            .unwrap();
        backend
            .set(pid, "key", serde_json::json!("new"))
            .await
            .unwrap();

        assert_eq!(
            backend.get(pid, "key").await.unwrap(),
            Some(serde_json::json!("new"))
        );
    }

    // Compile-time: KvBackend is object-safe and dyn-compatible.
    #[allow(dead_code)]
    const _: () = {
        fn assert_object_safe(_: &dyn KvBackend) {}
    };

    // Compile-time: Arc<dyn KvBackend> is Send + Sync.
    #[allow(dead_code)]
    const _: () = {
        fn assert_send_sync<T: Send + Sync>() {}
        fn check() {
            assert_send_sync::<std::sync::Arc<dyn KvBackend>>();
        }
    };
}
