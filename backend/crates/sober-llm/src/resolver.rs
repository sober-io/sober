//! Three-tier LLM key resolution service.
//!
//! Resolves the best available LLM provider key for a user by searching:
//!
//! 1. **User secrets** — encrypted keys stored per-user, ordered by priority.
//! 2. **System config** — `LLM_API_KEY` from the environment.
//!
//! The resolver decrypts secrets using the envelope encryption hierarchy
//! (MEK -> DEK -> secret).

use sober_core::config::LlmConfig;
use sober_core::error::AppError;
use sober_core::types::{SecretRepo, SecretScope, UserId};
use sober_crypto::envelope::{EncryptedBlob, Mek};
use tracing::warn;

/// A resolved LLM provider key with connection details.
#[derive(Debug, Clone)]
pub struct ResolvedLlmKey {
    /// Base URL for the provider API.
    pub base_url: String,
    /// API key for authentication.
    pub api_key: String,
    /// Provider name (e.g. `"anthropic"`, `"openrouter"`, `"system"`).
    pub provider: String,
}

/// Resolves LLM provider keys using a three-tier fallback chain.
///
/// Generic over `S: SecretRepo` because the repo trait uses RPITIT and is
/// not `dyn`-compatible.
pub struct LlmKeyResolver<S> {
    secret_repo: S,
    mek: Mek,
    system_config: LlmConfig,
}

impl<S: SecretRepo> LlmKeyResolver<S> {
    /// Creates a new resolver.
    pub fn new(secret_repo: S, mek: Mek, system_config: LlmConfig) -> Self {
        Self {
            secret_repo,
            mek,
            system_config,
        }
    }

    /// Resolves the best available LLM key for a user.
    ///
    /// Resolution order:
    /// 1. User's own secrets (type `"llm_provider"`, ordered by priority)
    /// 2. System config from environment variables
    pub async fn resolve(&self, user_id: UserId) -> Result<ResolvedLlmKey, AppError> {
        // 1. Try user's own keys (ordered by priority)
        if let Some(key) = self.try_scope(SecretScope::User(user_id)).await? {
            return Ok(key);
        }

        // 2. Fall back to system config
        let api_key = self.system_config.api_key.clone().unwrap_or_default();

        Ok(ResolvedLlmKey {
            base_url: self.system_config.base_url.clone(),
            api_key,
            provider: "system".into(),
        })
    }

    /// Attempts to resolve a key from the given scope.
    async fn try_scope(&self, scope: SecretScope) -> Result<Option<ResolvedLlmKey>, AppError> {
        let secrets = self
            .secret_repo
            .list_secrets(scope, Some("llm_provider"))
            .await?;

        if secrets.is_empty() {
            return Ok(None);
        }

        // Get the DEK for this scope
        let stored_dek = match self.secret_repo.get_dek(scope).await? {
            Some(dek) => dek,
            None => {
                warn!("secrets exist for scope but no DEK found");
                return Ok(None);
            }
        };

        let dek_blob = EncryptedBlob::from_bytes(&stored_dek.encrypted_dek)
            .map_err(|e| AppError::Internal(e.into()))?;
        let dek = self
            .mek
            .unwrap_dek(&dek_blob)
            .map_err(|e| AppError::Internal(e.into()))?;

        // Try each secret by priority
        for meta in &secrets {
            if let Some(row) = self.secret_repo.get_secret(meta.id).await? {
                let secret_blob = EncryptedBlob::from_bytes(&row.encrypted_data)
                    .map_err(|e| AppError::Internal(e.into()))?;
                let plaintext = match dek.decrypt(&secret_blob) {
                    Ok(pt) => pt,
                    Err(e) => {
                        warn!(secret_id = %meta.id, "failed to decrypt secret: {e}");
                        continue;
                    }
                };
                let secret_data: serde_json::Value = match serde_json::from_slice(&plaintext) {
                    Ok(v) => v,
                    Err(e) => {
                        warn!(secret_id = %meta.id, "failed to parse decrypted secret: {e}");
                        continue;
                    }
                };

                if let Some(api_key) = secret_data.get("api_key").and_then(|v| v.as_str()) {
                    let base_url = row
                        .metadata
                        .get("base_url")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let provider = row
                        .metadata
                        .get("provider")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    return Ok(Some(ResolvedLlmKey {
                        base_url,
                        api_key: api_key.to_string(),
                        provider,
                    }));
                }
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::types::{
        EncryptionKeyId, NewSecret, SecretId, SecretMetadata, SecretRow, StoredDek, UpdateSecret,
    };
    use sober_crypto::envelope::Dek;

    /// In-memory mock of `SecretRepo` for testing.
    struct MockSecretRepo {
        stored_dek: Option<StoredDek>,
        secrets: Vec<SecretRow>,
    }

    impl MockSecretRepo {
        fn empty() -> Self {
            Self {
                stored_dek: None,
                secrets: vec![],
            }
        }

        fn with_secret(
            user_id: UserId,
            dek: &Dek,
            mek: &Mek,
            name: &str,
            metadata: serde_json::Value,
            api_key: &str,
            priority: Option<i32>,
        ) -> Self {
            let wrapped_dek = mek.wrap_dek(dek).expect("wrap DEK");
            let secret_json = serde_json::json!({"api_key": api_key});
            let plaintext = serde_json::to_vec(&secret_json).expect("serialize");
            let encrypted = dek.encrypt(&plaintext).expect("encrypt");

            let secret_id = SecretId::new();
            Self {
                stored_dek: Some(StoredDek {
                    id: EncryptionKeyId::new(),
                    user_id,
                    encrypted_dek: wrapped_dek.to_bytes(),
                    mek_version: 1,
                    created_at: sober_core::Utc::now(),
                    rotated_at: None,
                }),
                secrets: vec![SecretRow {
                    id: secret_id,
                    user_id,
                    name: name.to_string(),
                    secret_type: "llm_provider".to_string(),
                    metadata,
                    encrypted_data: encrypted.to_bytes(),
                    priority,
                    created_at: sober_core::Utc::now(),
                    updated_at: sober_core::Utc::now(),
                }],
            }
        }
    }

    impl SecretRepo for MockSecretRepo {
        async fn get_dek(&self, _scope: SecretScope) -> Result<Option<StoredDek>, AppError> {
            Ok(self.stored_dek.clone())
        }

        async fn store_dek(
            &self,
            _scope: SecretScope,
            _encrypted_dek: Vec<u8>,
            _mek_version: i32,
        ) -> Result<(), AppError> {
            Ok(())
        }

        async fn list_secrets(
            &self,
            _scope: SecretScope,
            secret_type: Option<&str>,
        ) -> Result<Vec<SecretMetadata>, AppError> {
            Ok(self
                .secrets
                .iter()
                .filter(|s| secret_type.is_none() || Some(s.secret_type.as_str()) == secret_type)
                .map(|s| SecretMetadata {
                    id: s.id,
                    name: s.name.clone(),
                    secret_type: s.secret_type.clone(),
                    metadata: s.metadata.clone(),
                    priority: s.priority,
                    created_at: s.created_at,
                    updated_at: s.updated_at,
                })
                .collect())
        }

        async fn get_secret(&self, id: SecretId) -> Result<Option<SecretRow>, AppError> {
            Ok(self.secrets.iter().find(|s| s.id == id).cloned())
        }

        async fn get_secret_by_name(
            &self,
            _scope: SecretScope,
            name: &str,
        ) -> Result<Option<SecretRow>, AppError> {
            Ok(self.secrets.iter().find(|s| s.name == name).cloned())
        }

        async fn store_secret(&self, _secret: NewSecret) -> Result<SecretId, AppError> {
            Ok(SecretId::new())
        }

        async fn update_secret(
            &self,
            _id: SecretId,
            _update: UpdateSecret,
        ) -> Result<(), AppError> {
            Ok(())
        }

        async fn delete_secret(&self, _id: SecretId) -> Result<(), AppError> {
            Ok(())
        }

        async fn list_secret_ids(&self, _scope: SecretScope) -> Result<Vec<SecretId>, AppError> {
            Ok(self.secrets.iter().map(|s| s.id).collect())
        }
    }

    fn test_user_id() -> UserId {
        UserId::new()
    }

    fn test_mek() -> Mek {
        Mek::from_hex(&"aa".repeat(32)).expect("valid MEK hex")
    }

    fn default_llm_config() -> LlmConfig {
        LlmConfig {
            base_url: "https://openrouter.ai/api/v1".into(),
            api_key: Some("system-key".into()),
            model: "anthropic/claude-sonnet-4".into(),
            max_tokens: 4096,
            embedding_model: "text-embedding-3-small".into(),
        }
    }

    #[tokio::test]
    async fn resolves_user_key_first() {
        let mek = test_mek();
        let user_id = test_user_id();
        let dek = Dek::generate().expect("generate DEK");

        let repo = MockSecretRepo::with_secret(
            user_id,
            &dek,
            &mek,
            "My Anthropic Key",
            serde_json::json!({"provider": "anthropic", "base_url": "https://api.anthropic.com/v1"}),
            "sk-ant-user-key",
            Some(1),
        );

        let resolver = LlmKeyResolver::new(repo, test_mek(), default_llm_config());
        let result = resolver.resolve(user_id).await.expect("resolve");

        assert_eq!(result.api_key, "sk-ant-user-key");
        assert_eq!(result.provider, "anthropic");
        assert_eq!(result.base_url, "https://api.anthropic.com/v1");
    }

    #[tokio::test]
    async fn falls_back_to_system_config() {
        let repo = MockSecretRepo::empty();
        let resolver = LlmKeyResolver::new(repo, test_mek(), default_llm_config());
        let result = resolver.resolve(test_user_id()).await.expect("resolve");

        assert_eq!(result.api_key, "system-key");
        assert_eq!(result.provider, "system");
        assert_eq!(result.base_url, "https://openrouter.ai/api/v1");
    }

    #[tokio::test]
    async fn system_config_with_no_api_key() {
        let repo = MockSecretRepo::empty();
        let config = LlmConfig {
            base_url: "https://openrouter.ai/api/v1".into(),
            api_key: None,
            model: "anthropic/claude-sonnet-4".into(),
            max_tokens: 4096,
            embedding_model: "text-embedding-3-small".into(),
        };
        let resolver = LlmKeyResolver::new(repo, test_mek(), config);
        let result = resolver.resolve(test_user_id()).await.expect("resolve");

        assert_eq!(result.api_key, "");
        assert_eq!(result.provider, "system");
    }
}
