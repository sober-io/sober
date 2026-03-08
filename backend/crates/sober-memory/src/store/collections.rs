//! Qdrant collection naming helpers.

use sober_core::UserId;

/// Returns the collection name for a user's personal memory.
///
/// Uses the simple (unhyphenated) UUID format to keep collection names
/// DNS-safe.
#[must_use]
pub fn user_collection_name(user_id: UserId) -> String {
    format!("user_{}", user_id.as_uuid().simple())
}

/// Returns the collection name for system-level shared memory.
#[must_use]
pub fn system_collection_name() -> &'static str {
    "system"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_collection_is_deterministic() {
        let id = UserId::new();
        let a = user_collection_name(id);
        let b = user_collection_name(id);
        assert_eq!(a, b);
    }

    #[test]
    fn user_collection_contains_no_hyphens() {
        let name = user_collection_name(UserId::new());
        assert!(!name.contains('-'));
    }

    #[test]
    fn user_collection_starts_with_prefix() {
        let name = user_collection_name(UserId::new());
        assert!(name.starts_with("user_"));
    }

    #[test]
    fn system_collection_is_constant() {
        assert_eq!(system_collection_name(), "system");
    }
}
