//! Access mask application for prompt content filtering.
//!
//! Strips internal-only sections from the assembled prompt based on the
//! caller's [`TriggerKind`]. This ensures that human users cannot see
//! self-reasoning blocks, evolution state, or internal tool documentation.

use sober_core::types::access::{CallerContext, TriggerKind};

/// Section markers that delimit internal-only content in the resolved SOUL.md.
///
/// Content between `<!-- INTERNAL:START -->` and `<!-- INTERNAL:END -->` is
/// only visible to `Scheduler` and `Admin` triggers.
const INTERNAL_START: &str = "<!-- INTERNAL:START -->";
const INTERNAL_END: &str = "<!-- INTERNAL:END -->";

/// Applies an access mask to the prompt, stripping internal-only sections
/// based on the caller's trigger kind.
///
/// # Access rules
///
/// | Trigger    | Behavior                                          |
/// |------------|---------------------------------------------------|
/// | Scheduler  | Full access — no stripping                        |
/// | Admin      | Full access — no stripping                        |
/// | Human      | Internal sections removed                         |
/// | Replica    | Internal sections removed, scoped to grants       |
#[must_use]
pub fn apply_access_mask(prompt: &str, caller: &CallerContext) -> String {
    match caller.trigger {
        TriggerKind::Scheduler | TriggerKind::Admin => {
            // Full access — return as-is, just strip the marker comments.
            prompt.replace(INTERNAL_START, "").replace(INTERNAL_END, "")
        }
        TriggerKind::Human | TriggerKind::Replica => {
            // Strip internal sections entirely.
            strip_internal_sections(prompt)
        }
    }
}

/// Removes all content between `INTERNAL_START` and `INTERNAL_END` markers,
/// including the markers themselves.
fn strip_internal_sections(prompt: &str) -> String {
    let mut result = String::with_capacity(prompt.len());
    let mut remaining = prompt;

    loop {
        match remaining.find(INTERNAL_START) {
            Some(start_idx) => {
                // Add everything before the internal section.
                result.push_str(&remaining[..start_idx]);

                // Find the end marker.
                let after_start = &remaining[start_idx + INTERNAL_START.len()..];
                match after_start.find(INTERNAL_END) {
                    Some(end_idx) => {
                        // Skip past the end marker.
                        remaining = &after_start[end_idx + INTERNAL_END.len()..];
                    }
                    None => {
                        // No closing marker — strip everything after start marker
                        // (treat as unclosed internal section).
                        break;
                    }
                }
            }
            None => {
                // No more internal sections.
                result.push_str(remaining);
                break;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use sober_core::types::ids::UserId;

    fn make_caller(trigger: TriggerKind) -> CallerContext {
        CallerContext {
            user_id: Some(UserId::new()),
            trigger,
            permissions: vec![],
            scope_grants: vec![],
        }
    }

    #[test]
    fn scheduler_sees_everything() {
        let prompt = "Public content.\n<!-- INTERNAL:START -->\nSecret reasoning.\n<!-- INTERNAL:END -->\nMore public.";
        let caller = make_caller(TriggerKind::Scheduler);
        let result = apply_access_mask(prompt, &caller);
        assert!(result.contains("Secret reasoning."));
        assert!(result.contains("Public content."));
        assert!(result.contains("More public."));
        // Markers themselves should be removed.
        assert!(!result.contains("INTERNAL:START"));
    }

    #[test]
    fn admin_sees_everything() {
        let prompt = "Public.\n<!-- INTERNAL:START -->\nInternal.\n<!-- INTERNAL:END -->";
        let caller = make_caller(TriggerKind::Admin);
        let result = apply_access_mask(prompt, &caller);
        assert!(result.contains("Internal."));
    }

    #[test]
    fn human_sees_no_internal() {
        let prompt = "Public content.\n<!-- INTERNAL:START -->\nSecret reasoning.\n<!-- INTERNAL:END -->\nMore public.";
        let caller = make_caller(TriggerKind::Human);
        let result = apply_access_mask(prompt, &caller);
        assert!(!result.contains("Secret reasoning."));
        assert!(result.contains("Public content."));
        assert!(result.contains("More public."));
    }

    #[test]
    fn replica_sees_no_internal() {
        let prompt =
            "Delegated task.\n<!-- INTERNAL:START -->\nEvolution state.\n<!-- INTERNAL:END -->";
        let caller = make_caller(TriggerKind::Replica);
        let result = apply_access_mask(prompt, &caller);
        assert!(!result.contains("Evolution state."));
        assert!(result.contains("Delegated task."));
    }

    #[test]
    fn no_internal_sections_passes_through() {
        let prompt = "Just regular content with no markers.";
        let caller = make_caller(TriggerKind::Human);
        let result = apply_access_mask(prompt, &caller);
        assert_eq!(result, prompt);
    }

    #[test]
    fn multiple_internal_sections() {
        let prompt = "A\n<!-- INTERNAL:START -->\nB\n<!-- INTERNAL:END -->\nC\n<!-- INTERNAL:START -->\nD\n<!-- INTERNAL:END -->\nE";
        let caller = make_caller(TriggerKind::Human);
        let result = apply_access_mask(prompt, &caller);
        assert!(result.contains('A'));
        assert!(!result.contains('B'));
        assert!(result.contains('C'));
        assert!(!result.contains('D'));
        assert!(result.contains('E'));
    }

    #[test]
    fn unclosed_internal_section_stripped_to_end() {
        let prompt = "Visible.\n<!-- INTERNAL:START -->\nHidden forever.";
        let caller = make_caller(TriggerKind::Human);
        let result = apply_access_mask(prompt, &caller);
        assert!(result.contains("Visible."));
        assert!(!result.contains("Hidden forever."));
    }
}
