//! Per-turn registry of known secret values for redaction.
//!
//! Populated by `read_secret` and `store_secret` during tool execution.
//! The dispatch layer uses [`SecretRegistry::redact`] to strip secret
//! values from strings before persistence and broadcast.

use std::sync::Mutex;

/// Collects plaintext secret values during a turn for redaction.
///
/// Thread-safe via internal [`Mutex`] — tools register values during
/// execution, and the dispatch layer reads them for redaction.
#[derive(Debug, Default)]
pub struct SecretRegistry {
    /// (plaintext_value, secret_name) pairs.
    entries: Mutex<Vec<(String, String)>>,
}

impl SecretRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a secret value for redaction.
    ///
    /// Short or empty values are ignored to avoid false-positive replacements.
    pub fn register(&self, plaintext: &str, secret_name: &str) {
        // Skip values that are too short to meaningfully redact — single
        // characters or empty strings would cause excessive false positives.
        if plaintext.len() < 4 {
            return;
        }
        let mut entries = self.entries.lock().expect("secret registry lock poisoned");
        // Deduplicate — don't register the same value twice.
        if !entries.iter().any(|(v, _)| v == plaintext) {
            entries.push((plaintext.to_owned(), secret_name.to_owned()));
        }
    }

    /// Replaces all registered secret values in `text` with `[REDACTED: name]`.
    ///
    /// Longer values are replaced first to avoid partial matches when one
    /// secret value is a substring of another.
    pub fn redact(&self, text: &str) -> String {
        let entries = self.entries.lock().expect("secret registry lock poisoned");
        if entries.is_empty() {
            return text.to_owned();
        }
        // Sort by length descending so longer matches are replaced first.
        let mut sorted: Vec<_> = entries.iter().collect();
        sorted.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        let mut result = text.to_owned();
        for (plaintext, name) in sorted {
            result = result.replace(plaintext.as_str(), &format!("[REDACTED: {name}]"));
        }
        result
    }

    /// Returns `true` if no secrets have been registered.
    pub fn is_empty(&self) -> bool {
        self.entries
            .lock()
            .expect("secret registry lock poisoned")
            .is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_returns_input_unchanged() {
        let reg = SecretRegistry::new();
        assert_eq!(reg.redact("hello world"), "hello world");
        assert!(reg.is_empty());
    }

    #[test]
    fn redacts_registered_value() {
        let reg = SecretRegistry::new();
        reg.register("sk-abc123", "my-api-key");
        assert_eq!(
            reg.redact("Authorization: Bearer sk-abc123"),
            "Authorization: Bearer [REDACTED: my-api-key]"
        );
    }

    #[test]
    fn redacts_multiple_values() {
        let reg = SecretRegistry::new();
        reg.register("sk-abc123", "openai-key");
        reg.register("ghp_xyz789", "github-token");
        let input = "keys: sk-abc123 and ghp_xyz789";
        let result = reg.redact(input);
        assert_eq!(
            result,
            "keys: [REDACTED: openai-key] and [REDACTED: github-token]"
        );
    }

    #[test]
    fn longer_values_replaced_first() {
        let reg = SecretRegistry::new();
        reg.register("sk-abc", "short-key");
        reg.register("sk-abc123456", "long-key");
        let result = reg.redact("token: sk-abc123456");
        assert_eq!(result, "token: [REDACTED: long-key]");
    }

    #[test]
    fn ignores_short_values() {
        let reg = SecretRegistry::new();
        reg.register("abc", "too-short");
        assert!(reg.is_empty());
        assert_eq!(reg.redact("abc def"), "abc def");
    }

    #[test]
    fn deduplicates_registrations() {
        let reg = SecretRegistry::new();
        reg.register("sk-abc123", "key-1");
        reg.register("sk-abc123", "key-2");
        // First registration wins.
        assert_eq!(reg.redact("sk-abc123"), "[REDACTED: key-1]");
    }

    #[test]
    fn redacts_in_json_string() {
        let reg = SecretRegistry::new();
        reg.register("secret-value-here", "my-secret");
        let json = r#"{"headers":{"Authorization":"Bearer secret-value-here"}}"#;
        let result = reg.redact(json);
        assert_eq!(
            result,
            r#"{"headers":{"Authorization":"Bearer [REDACTED: my-secret]"}}"#
        );
    }

    /// Simulates the dispatch flow: serialize JSON → redact → deserialize.
    /// Ensures the round-trip produces valid JSON with redacted values.
    #[test]
    fn redact_json_roundtrip_preserves_structure() {
        let reg = SecretRegistry::new();
        reg.register("sk-live-abc123", "openai-key");

        let input = serde_json::json!({
            "name": "openai-key",
            "data": {
                "api_key": "sk-live-abc123",
                "provider": "openai"
            }
        });

        let serialized = serde_json::to_string(&input).unwrap();
        let redacted = reg.redact(&serialized);
        let parsed: serde_json::Value = serde_json::from_str(&redacted).unwrap();

        assert_eq!(
            parsed["data"]["api_key"], "[REDACTED: openai-key]",
            "api_key should be redacted"
        );
        assert_eq!(
            parsed["data"]["provider"], "openai",
            "non-secret fields should be preserved"
        );
        assert_eq!(
            parsed["name"], "openai-key",
            "name field should be preserved"
        );
    }

    /// Simulates pre-execution redaction (empty registry) followed by
    /// post-execution re-redaction (populated registry). This is the
    /// store_secret timing fix flow.
    #[test]
    fn pre_and_post_execution_redaction() {
        let reg = SecretRegistry::new();

        let input = serde_json::json!({
            "name": "my-key",
            "data": {"key": "super-secret-value-123"}
        });
        let serialized = serde_json::to_string(&input).unwrap();

        // Pre-execution: registry is empty, redaction is a no-op.
        let pre_redacted = reg.redact(&serialized);
        assert_eq!(
            pre_redacted, serialized,
            "empty registry should not modify input"
        );

        // Tool executes and registers the secret.
        reg.register("super-secret-value-123", "my-key");

        // Post-execution: registry has values, input is now redacted.
        let post_redacted = reg.redact(&serialized);
        assert_ne!(
            post_redacted, serialized,
            "populated registry should modify input"
        );
        assert!(
            !post_redacted.contains("super-secret-value-123"),
            "secret should be redacted: {post_redacted}"
        );

        // The re-redacted JSON should still be valid.
        let parsed: serde_json::Value = serde_json::from_str(&post_redacted).unwrap();
        assert_eq!(parsed["data"]["key"], "[REDACTED: my-key]");
    }

    /// Verifies that redaction works on assistant message text that echoes secrets.
    #[test]
    fn redacts_assistant_text_with_secret() {
        let reg = SecretRegistry::new();
        reg.register("sk-proj-abc123xyz", "openai-key");

        let assistant_text = "I've stored your OpenAI API key sk-proj-abc123xyz securely.";
        let redacted = reg.redact(assistant_text);
        assert_eq!(
            redacted,
            "I've stored your OpenAI API key [REDACTED: openai-key] securely."
        );
    }

    /// Verifies that redaction works on user messages containing pasted secrets.
    #[test]
    fn redacts_user_message_with_pasted_secret() {
        let reg = SecretRegistry::new();
        reg.register("ghp_1234567890abcdef", "github-token");

        let user_msg = "store my github token ghp_1234567890abcdef as github-token";
        let redacted = reg.redact(user_msg);
        assert_eq!(
            redacted,
            "store my github token [REDACTED: github-token] as github-token"
        );
    }

    /// Ensures reasoning content is also redacted.
    #[test]
    fn redacts_reasoning_content() {
        let reg = SecretRegistry::new();
        reg.register("password123!", "db-password");

        let reasoning = "The user wants to store password123! as their database password. \
            I should call store_secret with the value password123!";
        let redacted = reg.redact(reasoning);
        assert!(
            !redacted.contains("password123!"),
            "reasoning should not contain secret: {redacted}"
        );
        assert_eq!(
            redacted.matches("[REDACTED: db-password]").count(),
            2,
            "both occurrences should be redacted"
        );
    }
}
