//! Visibility-based access control for instruction files.
//!
//! Replaces the old `<!-- INTERNAL:START/END -->` marker-based approach with
//! frontmatter-driven visibility. Each instruction file declares its visibility
//! level (`public` or `internal`), and filtering is handled by
//! [`InstructionFile::is_visible()`](crate::instructions::InstructionFile::is_visible).
//!
//! This module retains the `is_visible` predicate for use outside the
//! instruction pipeline (e.g., tool metadata filtering).

use sober_core::types::access::TriggerKind;

use crate::frontmatter::Visibility;

/// Returns `true` if content with the given visibility should be included
/// for the specified trigger kind.
///
/// | Trigger    | public | internal |
/// |------------|--------|----------|
/// | Human      | yes    | no       |
/// | Replica    | yes    | no       |
/// | Admin      | yes    | yes      |
/// | Scheduler  | yes    | yes      |
#[must_use]
pub fn is_visible(visibility: Visibility, trigger: TriggerKind) -> bool {
    match visibility {
        Visibility::Public => true,
        Visibility::Internal => matches!(trigger, TriggerKind::Admin | TriggerKind::Scheduler),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_visible_to_all() {
        assert!(is_visible(Visibility::Public, TriggerKind::Human));
        assert!(is_visible(Visibility::Public, TriggerKind::Replica));
        assert!(is_visible(Visibility::Public, TriggerKind::Admin));
        assert!(is_visible(Visibility::Public, TriggerKind::Scheduler));
    }

    #[test]
    fn internal_restricted() {
        assert!(!is_visible(Visibility::Internal, TriggerKind::Human));
        assert!(!is_visible(Visibility::Internal, TriggerKind::Replica));
        assert!(is_visible(Visibility::Internal, TriggerKind::Admin));
        assert!(is_visible(Visibility::Internal, TriggerKind::Scheduler));
    }
}
