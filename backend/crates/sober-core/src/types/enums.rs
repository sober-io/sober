//! Domain enums that map to PostgreSQL custom types.
//!
//! Each enum uses `#[serde(rename_all = "lowercase")]` for consistent JSON
//! serialization and `#[sqlx(type_name, rename_all)]` (behind the `postgres`
//! feature) for direct mapping to PostgreSQL enum types.

use serde::{Deserialize, Serialize};

/// Memory/authorization scope level.
///
/// Maps to the `scope_kind` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "scope_kind", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum ScopeKind {
    /// Global system scope.
    System,
    /// Per-user scope.
    User,
    /// Shared group/team scope.
    Group,
    /// Ephemeral conversation scope.
    Session,
}

/// User account lifecycle status.
///
/// Maps to the `user_status` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "user_status", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus {
    /// Account created but not yet verified.
    Pending,
    /// Active account.
    Active,
    /// Administratively disabled.
    Disabled,
}

/// Author type of a message in a conversation.
///
/// Maps to the `message_role` PostgreSQL enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "postgres", derive(sqlx::Type))]
#[cfg_attr(
    feature = "postgres",
    sqlx(type_name = "message_role", rename_all = "lowercase")
)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// Human user message.
    User,
    /// AI assistant response.
    Assistant,
    /// System prompt or instruction.
    System,
    /// Tool invocation result.
    Tool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_kind_serde_roundtrip() {
        for variant in [
            ScopeKind::System,
            ScopeKind::User,
            ScopeKind::Group,
            ScopeKind::Session,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: ScopeKind = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn scope_kind_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&ScopeKind::System).unwrap(),
            "\"system\""
        );
        assert_eq!(serde_json::to_string(&ScopeKind::User).unwrap(), "\"user\"");
    }

    #[test]
    fn user_status_serde_roundtrip() {
        for variant in [
            UserStatus::Pending,
            UserStatus::Active,
            UserStatus::Disabled,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: UserStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn message_role_serde_roundtrip() {
        for variant in [
            MessageRole::User,
            MessageRole::Assistant,
            MessageRole::System,
            MessageRole::Tool,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: MessageRole = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, deserialized);
        }
    }

    #[test]
    fn message_role_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&MessageRole::Assistant).unwrap(),
            "\"assistant\""
        );
        assert_eq!(
            serde_json::to_string(&MessageRole::Tool).unwrap(),
            "\"tool\""
        );
    }
}
