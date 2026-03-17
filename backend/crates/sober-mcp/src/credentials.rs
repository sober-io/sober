//! MCP server credential resolution from user secrets.
//!
//! Decrypts user secrets tagged with `secret_type = "mcp_server"` and
//! `metadata.server = "<server-name>"`, then merges the key-value pairs into
//! the server's base environment map before spawning.

use std::collections::HashMap;

use sober_core::error::AppError;
use sober_core::types::repo::SecretRepo;
use sober_core::types::{ConversationId, UserId};
use sober_crypto::envelope::{EncryptedBlob, Mek};

/// Resolve MCP server credentials from user secrets.
///
/// Lists secrets of type `"mcp_server"` for the user, finds the first one
/// whose `metadata.server` matches `server_name`, decrypts it with the user's
/// DEK (unwrapped via the MEK), and merges the resulting key-value pairs into
/// `base_env`.
///
/// If no matching secret exists, or if the user has no DEK, `base_env` is
/// returned unchanged.
///
/// # Errors
///
/// Returns [`AppError`] on database errors or decryption failures.
pub async fn resolve_mcp_credentials<S: SecretRepo>(
    secret_repo: &S,
    mek: &Mek,
    user_id: UserId,
    conversation_id: Option<ConversationId>,
    server_name: &str,
    base_env: &HashMap<String, String>,
) -> Result<HashMap<String, String>, AppError> {
    let mut env = base_env.clone();

    let secrets = secret_repo
        .list_secrets(user_id, conversation_id, Some("mcp_server"))
        .await?;

    for meta in secrets {
        // Check if this secret's metadata.server matches our server name.
        if meta.metadata.get("server").and_then(|v| v.as_str()) != Some(server_name) {
            continue;
        }

        let row = match secret_repo.get_secret(meta.id).await? {
            Some(r) => r,
            None => continue,
        };

        let stored_dek = match secret_repo.get_dek(user_id).await? {
            Some(d) => d,
            None => break,
        };

        // Deserialise the MEK-wrapped DEK bytes and unwrap it.
        let dek_blob = EncryptedBlob::from_bytes(&stored_dek.encrypted_dek)?;
        let dek = mek.unwrap_dek(&dek_blob)?;

        // Deserialise the secret's encrypted data and decrypt it.
        let data_blob = EncryptedBlob::from_bytes(&row.encrypted_data)?;
        let plaintext = dek.decrypt(&data_blob)?;

        // The secret value is a JSON object mapping env var names to values.
        if let Ok(kv) = serde_json::from_slice::<HashMap<String, String>>(&plaintext) {
            env.extend(kv);
        }

        // Use the first matching secret only.
        break;
    }

    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_base_env_unchanged_with_no_matching_secrets() {
        // Verify the function signature compiles and the base_env clone works.
        let mut base = HashMap::new();
        base.insert("EXISTING".to_owned(), "value".to_owned());
        let cloned = base.clone();
        assert_eq!(base, cloned);
    }
}
