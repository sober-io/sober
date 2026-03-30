//! Agent tools for managing encrypted secrets.
//!
//! Provides four tools for the agent to interact with the secret vault:
//!
//! - [`StoreSecretTool`] — encrypt and store a new secret
//! - [`ReadSecretTool`] — decrypt and retrieve a secret (internal, filtered from WS)
//! - [`ListSecretsTool`] — list secret metadata without encrypted data
//! - [`DeleteSecretTool`] — remove a secret

use std::sync::Arc;

use sober_core::types::tool::{
    BoxToolFuture, Tool, ToolError, ToolMetadata, ToolOutput, ToolVisibility,
};
use sober_core::types::{
    AuditLogRepo, ConversationId, CreateAuditLog, NewSecret, SecretRepo, UserId,
};
use sober_crypto::envelope::{Dek, EncryptedBlob, Mek};
use uuid::Uuid;

/// Shared context for all secret tools.
///
/// Holds the repos, MEK, and caller identity needed to encrypt/decrypt secrets.
pub struct SecretToolContext<S: SecretRepo, A: AuditLogRepo> {
    pub secret_repo: Arc<S>,
    pub audit_repo: Arc<A>,
    pub mek: Arc<Mek>,
    pub user_id: UserId,
    pub conversation_id: Option<ConversationId>,
}

/// Non-sensitive metadata keys that are safe to extract from the secret data
/// and store in plaintext for listing/filtering.
const METADATA_KEYS: &[&str] = &["provider", "server", "base_url", "model", "description"];

/// Resolves the `owner_id` injected by the agent loop into a [`UserId`].
fn resolve_user_id(input: &serde_json::Value) -> Result<UserId, ToolError> {
    let owner_id_str = input
        .get("owner_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidInput("missing owner_id context".into()))?;
    let uuid = Uuid::parse_str(owner_id_str)
        .map_err(|e| ToolError::InvalidInput(format!("invalid owner_id: {e}")))?;
    Ok(UserId::from_uuid(uuid))
}

/// Resolves the `conversation_id` injected by the agent loop.
fn resolve_conversation_id(input: &serde_json::Value) -> Option<ConversationId> {
    input
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .map(ConversationId::from_uuid)
}

/// Gets or creates the user's DEK, returning the unwrapped key.
async fn get_or_create_dek<S: SecretRepo>(
    repo: &S,
    mek: &Mek,
    user_id: UserId,
) -> Result<Dek, ToolError> {
    if let Some(stored) = repo
        .get_dek(user_id)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("failed to fetch DEK: {e}")))?
    {
        let blob = EncryptedBlob::from_bytes(&stored.encrypted_dek)
            .map_err(|e| ToolError::ExecutionFailed(format!("invalid stored DEK: {e}")))?;
        mek.unwrap_dek(&blob)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to unwrap DEK: {e}")))
    } else {
        let dek = Dek::generate()
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to generate DEK: {e}")))?;
        let wrapped = mek
            .wrap_dek(&dek)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to wrap DEK: {e}")))?;
        repo.store_dek(user_id, wrapped.to_bytes(), 1)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to store DEK: {e}")))?;
        Ok(dek)
    }
}

/// Extracts non-sensitive metadata fields from the secret data object.
fn extract_metadata(data: &serde_json::Value) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    if let Some(obj) = data.as_object() {
        for &key in METADATA_KEYS {
            if let Some(val) = obj.get(key) {
                metadata.insert(key.to_owned(), val.clone());
            }
        }
    }
    serde_json::Value::Object(metadata)
}

/// Writes an audit log entry, logging any failure without propagating it.
async fn write_audit<A: AuditLogRepo>(
    repo: &A,
    user_id: UserId,
    action: &str,
    target_id: Option<uuid::Uuid>,
    details: Option<serde_json::Value>,
) {
    let entry = CreateAuditLog {
        actor_id: Some(user_id),
        action: action.to_owned(),
        target_type: Some("secret".to_owned()),
        target_id,
        details,
        ip_address: None,
    };
    if let Err(e) = repo.create(entry).await {
        tracing::warn!("failed to write audit log for {action}: {e}");
    }
}

// ---------------------------------------------------------------------------
// StoreSecretTool
// ---------------------------------------------------------------------------

/// Encrypts and stores a new secret in the vault.
pub struct StoreSecretTool<S: SecretRepo, A: AuditLogRepo> {
    ctx: Arc<SecretToolContext<S, A>>,
}

impl<S: SecretRepo, A: AuditLogRepo> StoreSecretTool<S, A> {
    /// Creates a new store secret tool.
    pub fn new(ctx: Arc<SecretToolContext<S, A>>) -> Self {
        Self { ctx }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let user_id = resolve_user_id(&input)?;
        let ctx_conversation_id = resolve_conversation_id(&input);

        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'name'".into()))?
            .to_owned();

        let secret_type = input
            .get("secret_type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'secret_type'".into()))?
            .to_owned();

        let data = input
            .get("data")
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'data'".into()))?;

        if !data.is_object() {
            return Err(ToolError::InvalidInput(
                "'data' must be a JSON object".into(),
            ));
        }

        // Determine scope: "user" = None (available across conversations),
        // "conversation" (default) = scoped to current conversation.
        let scope = input
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("conversation");

        let conversation_id = match scope {
            "user" => None,
            _ => ctx_conversation_id.or(self.ctx.conversation_id),
        };

        // Get or create DEK for this user.
        let dek = get_or_create_dek(self.ctx.secret_repo.as_ref(), &self.ctx.mek, user_id).await?;

        // Serialize and encrypt the secret data.
        let json_bytes = serde_json::to_vec(data)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to serialize data: {e}")))?;
        let encrypted = dek
            .encrypt(&json_bytes)
            .map_err(|e| ToolError::ExecutionFailed(format!("encryption failed: {e}")))?;

        // Extract non-sensitive metadata for listing.
        let metadata = extract_metadata(data);

        let new_secret = NewSecret {
            user_id,
            conversation_id,
            name: name.clone(),
            secret_type,
            metadata,
            encrypted_data: encrypted.to_bytes(),
            priority: None,
        };

        let secret_id = self
            .ctx
            .secret_repo
            .store_secret(new_secret)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to store secret: {e}")))?;

        write_audit(
            self.ctx.audit_repo.as_ref(),
            user_id,
            "secret_store",
            Some(*secret_id.as_uuid()),
            Some(serde_json::json!({ "name": name })),
        )
        .await;

        Ok(ToolOutput {
            content: format!("Secret '{name}' stored successfully."),
            is_error: false,
        })
    }
}

impl<S: SecretRepo + 'static, A: AuditLogRepo + 'static> Tool for StoreSecretTool<S, A> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "store_secret".to_owned(),
            description: "Encrypt and store a secret (API key, credentials, tokens) in the \
                secure vault. Secrets are AES-256-GCM encrypted at rest. Use scope 'conversation' \
                (default) to limit access to this conversation, or 'user' for cross-conversation \
                access."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name for the secret (e.g. 'openai-api-key', 'github-token')."
                    },
                    "secret_type": {
                        "type": "string",
                        "description": "Type of secret: 'llm_provider', 'mcp_server', 'api_key', 'oauth_app'."
                    },
                    "data": {
                        "type": "object",
                        "description": "Key-value pairs to encrypt. All values are stored encrypted."
                    },
                    "scope": {
                        "type": "string",
                        "enum": ["conversation", "user"],
                        "description": "Scope: 'conversation' (default) limits to this chat, 'user' makes it available everywhere."
                    }
                },
                "required": ["name", "secret_type", "data"]
            }),
            context_modifying: false,
            redacted: false,
            visibility: ToolVisibility::Public,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

// ---------------------------------------------------------------------------
// ReadSecretTool
// ---------------------------------------------------------------------------

/// Decrypts and returns a secret. Marked `redacted: true` so results are
/// filtered from WebSocket delivery.
pub struct ReadSecretTool<S: SecretRepo, A: AuditLogRepo> {
    ctx: Arc<SecretToolContext<S, A>>,
}

impl<S: SecretRepo, A: AuditLogRepo> ReadSecretTool<S, A> {
    /// Creates a new read secret tool.
    pub fn new(ctx: Arc<SecretToolContext<S, A>>) -> Self {
        Self { ctx }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let user_id = resolve_user_id(&input)?;
        let conversation_id = resolve_conversation_id(&input).or(self.ctx.conversation_id);

        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'name'".into()))?;

        let secret = self
            .ctx
            .secret_repo
            .get_secret_by_name(user_id, conversation_id, name)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to look up secret: {e}")))?;

        let secret = match secret {
            Some(s) => s,
            None => {
                return Ok(ToolOutput {
                    content: format!("Secret '{name}' not found."),
                    is_error: true,
                });
            }
        };

        // Decrypt the secret data.
        let dek = get_or_create_dek(self.ctx.secret_repo.as_ref(), &self.ctx.mek, user_id).await?;
        let blob = EncryptedBlob::from_bytes(&secret.encrypted_data)
            .map_err(|e| ToolError::ExecutionFailed(format!("invalid encrypted data: {e}")))?;
        let plaintext = dek
            .decrypt(&blob)
            .map_err(|e| ToolError::ExecutionFailed(format!("decryption failed: {e}")))?;

        let decrypted_str = String::from_utf8(plaintext)
            .map_err(|e| ToolError::ExecutionFailed(format!("invalid UTF-8 in secret: {e}")))?;

        write_audit(
            self.ctx.audit_repo.as_ref(),
            user_id,
            "secret_read",
            Some(*secret.id.as_uuid()),
            Some(serde_json::json!({ "name": name })),
        )
        .await;

        Ok(ToolOutput {
            content: decrypted_str,
            is_error: false,
        })
    }
}

impl<S: SecretRepo + 'static, A: AuditLogRepo + 'static> Tool for ReadSecretTool<S, A> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "read_secret".to_owned(),
            description: "Decrypt and retrieve a stored secret by name. Use this when you need \
                credentials (API keys, tokens) to call external services. The decrypted value is \
                for internal use only and will not be shown to the user."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the secret to retrieve."
                    }
                },
                "required": ["name"]
            }),
            context_modifying: false,
            redacted: true,
            visibility: ToolVisibility::Public,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

// ---------------------------------------------------------------------------
// ListSecretsTool
// ---------------------------------------------------------------------------

/// Lists secret metadata (names, types) without decrypting any data.
pub struct ListSecretsTool<S: SecretRepo, A: AuditLogRepo> {
    ctx: Arc<SecretToolContext<S, A>>,
}

impl<S: SecretRepo, A: AuditLogRepo> ListSecretsTool<S, A> {
    /// Creates a new list secrets tool.
    pub fn new(ctx: Arc<SecretToolContext<S, A>>) -> Self {
        Self { ctx }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let user_id = resolve_user_id(&input)?;
        let conversation_id = resolve_conversation_id(&input).or(self.ctx.conversation_id);

        let secret_type = input.get("secret_type").and_then(|v| v.as_str());

        let secrets = self
            .ctx
            .secret_repo
            .list_secrets(user_id, conversation_id, secret_type)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to list secrets: {e}")))?;

        write_audit(
            self.ctx.audit_repo.as_ref(),
            user_id,
            "secret_list",
            None,
            Some(serde_json::json!({
                "secret_type": secret_type,
                "count": secrets.len(),
            })),
        )
        .await;

        if secrets.is_empty() {
            return Ok(ToolOutput {
                content: "No secrets found.".to_owned(),
                is_error: false,
            });
        }

        let entries: Vec<serde_json::Value> = secrets
            .into_iter()
            .map(|s| {
                serde_json::json!({
                    "name": s.name,
                    "secret_type": s.secret_type,
                    "metadata": s.metadata,
                    "scope": if s.conversation_id.is_some() { "conversation" } else { "user" },
                })
            })
            .collect();

        let output = serde_json::to_string_pretty(&entries)
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to format output: {e}")))?;

        Ok(ToolOutput {
            content: output,
            is_error: false,
        })
    }
}

impl<S: SecretRepo + 'static, A: AuditLogRepo + 'static> Tool for ListSecretsTool<S, A> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "list_secrets".to_owned(),
            description: "List stored secrets (names and types only, no decrypted values). \
                Use this to discover what credentials are available before using read_secret."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "secret_type": {
                        "type": "string",
                        "description": "Filter by type (e.g. 'llm_provider', 'api_key'). Omit to list all."
                    }
                }
            }),
            context_modifying: false,
            redacted: false,
            visibility: ToolVisibility::Public,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

// ---------------------------------------------------------------------------
// DeleteSecretTool
// ---------------------------------------------------------------------------

/// Deletes a secret from the vault by name.
pub struct DeleteSecretTool<S: SecretRepo, A: AuditLogRepo> {
    ctx: Arc<SecretToolContext<S, A>>,
}

impl<S: SecretRepo, A: AuditLogRepo> DeleteSecretTool<S, A> {
    /// Creates a new delete secret tool.
    pub fn new(ctx: Arc<SecretToolContext<S, A>>) -> Self {
        Self { ctx }
    }

    async fn execute_inner(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let user_id = resolve_user_id(&input)?;
        let conversation_id = resolve_conversation_id(&input).or(self.ctx.conversation_id);

        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing required field 'name'".into()))?;

        let secret = self
            .ctx
            .secret_repo
            .get_secret_by_name(user_id, conversation_id, name)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to look up secret: {e}")))?;

        let secret = match secret {
            Some(s) => s,
            None => {
                return Ok(ToolOutput {
                    content: format!("Secret '{name}' not found."),
                    is_error: true,
                });
            }
        };

        let secret_id = secret.id;

        self.ctx
            .secret_repo
            .delete_secret(secret_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to delete secret: {e}")))?;

        write_audit(
            self.ctx.audit_repo.as_ref(),
            user_id,
            "secret_delete",
            Some(*secret_id.as_uuid()),
            Some(serde_json::json!({ "name": name })),
        )
        .await;

        Ok(ToolOutput {
            content: format!("Secret '{name}' deleted."),
            is_error: false,
        })
    }
}

impl<S: SecretRepo + 'static, A: AuditLogRepo + 'static> Tool for DeleteSecretTool<S, A> {
    fn metadata(&self) -> ToolMetadata {
        ToolMetadata {
            name: "delete_secret".to_owned(),
            description: "Delete a stored secret by name. This permanently removes the \
                encrypted data from the vault."
                .to_owned(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the secret to delete."
                    }
                },
                "required": ["name"]
            }),
            context_modifying: false,
            redacted: false,
            visibility: ToolVisibility::Public,
        }
    }

    fn execute(&self, input: serde_json::Value) -> BoxToolFuture<'_> {
        Box::pin(self.execute_inner(input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_metadata_picks_safe_keys() {
        let data = serde_json::json!({
            "provider": "openai",
            "api_key": "sk-secret-value",
            "base_url": "https://api.openai.com",
            "model": "gpt-4",
            "token": "sensitive-token"
        });
        let meta = extract_metadata(&data);
        let obj = meta.as_object().expect("should be object");
        assert_eq!(obj.get("provider").unwrap(), "openai");
        assert_eq!(obj.get("base_url").unwrap(), "https://api.openai.com");
        assert_eq!(obj.get("model").unwrap(), "gpt-4");
        assert!(obj.get("api_key").is_none(), "api_key should be excluded");
        assert!(obj.get("token").is_none(), "token should be excluded");
    }

    #[test]
    fn extract_metadata_handles_non_object() {
        let data = serde_json::json!("just a string");
        let meta = extract_metadata(&data);
        assert!(meta.as_object().unwrap().is_empty());
    }

    #[test]
    fn extract_metadata_empty_object() {
        let data = serde_json::json!({});
        let meta = extract_metadata(&data);
        assert!(meta.as_object().unwrap().is_empty());
    }
}
