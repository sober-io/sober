//! Property-based tests for CommandPolicy (CLAUDE.md security requirement).

use proptest::prelude::*;
use sober_sandbox::{CommandPolicy, RiskLevel};

proptest! {
    /// Any command containing "rm -rf /" should be classified as Dangerous.
    #[test]
    fn rm_rf_root_always_dangerous(prefix in "[a-z ]{0,20}") {
        let policy = CommandPolicy::default();
        let cmd = format!("{prefix}; rm -rf /");
        prop_assert_eq!(policy.classify(&cmd), RiskLevel::Dangerous);
    }

    /// Pipe to shell is always dangerous regardless of the command before the pipe.
    #[test]
    fn pipe_to_shell_always_dangerous(cmd in "[a-zA-Z0-9 /._-]{1,50}") {
        let policy = CommandPolicy::default();
        let piped = format!("{cmd} | sh");
        prop_assert_eq!(policy.classify(&piped), RiskLevel::Dangerous);
    }

    /// Safe commands should never be classified as Dangerous.
    #[test]
    fn safe_commands_never_dangerous(args in "[a-zA-Z0-9 ./_-]{0,30}") {
        let policy = CommandPolicy::default();
        for safe_cmd in ["ls", "cat", "pwd", "echo", "whoami"] {
            let cmd = format!("{safe_cmd} {args}");
            prop_assert_ne!(policy.classify(&cmd), RiskLevel::Dangerous);
        }
    }

    /// Admin deny list should always deny, regardless of arguments.
    #[test]
    fn deny_list_always_denies(args in "[a-zA-Z0-9 ./_-]{0,30}") {
        let policy = CommandPolicy::with_denied(vec!["shutdown".to_string()]);
        let cmd = format!("shutdown {args}");
        prop_assert!(policy.is_denied(&cmd));
    }
}
