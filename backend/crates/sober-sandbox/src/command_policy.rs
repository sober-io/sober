//! Command risk classification for shell execution.
//!
//! Classifies shell commands into risk tiers based on pattern matching.
//! Used by the permission system to decide whether commands auto-approve
//! or require user confirmation.
//!
//! **Limitations:** The classifier uses naive string splitting and does NOT
//! handle quoted strings, command substitution, escape sequences, or heredocs.
//! The sandbox (bwrap) is the real security boundary — this classifier is a
//! UX convenience for the permission system, not a security gate.

use crate::RiskLevel;
use std::collections::HashMap;

/// Classifies shell commands into risk tiers based on pattern matching.
#[derive(Default)]
pub struct CommandPolicy {
    overrides: HashMap<String, RiskLevel>,
    denied: Vec<String>,
}

impl CommandPolicy {
    /// Create a policy with user-defined command risk overrides.
    pub fn with_overrides(overrides: HashMap<String, String>) -> Self {
        let parsed = overrides
            .into_iter()
            .filter_map(|(cmd, level)| {
                let risk = match level.as_str() {
                    "safe" => RiskLevel::Safe,
                    "moderate" => RiskLevel::Moderate,
                    "dangerous" => RiskLevel::Dangerous,
                    _ => return None,
                };
                Some((cmd, risk))
            })
            .collect();
        Self {
            overrides: parsed,
            denied: Vec::new(),
        }
    }

    /// Create a policy with admin-level denied commands.
    pub fn with_denied(denied: Vec<String>) -> Self {
        Self {
            overrides: HashMap::new(),
            denied,
        }
    }

    /// Create a policy with both overrides and denied commands.
    pub fn new(overrides: HashMap<String, String>, denied: Vec<String>) -> Self {
        let mut policy = Self::with_overrides(overrides);
        policy.denied = denied;
        policy
    }

    /// Check if a command is on the admin deny list.
    pub fn is_denied(&self, command: &str) -> bool {
        let cmd = command.trim();
        self.denied
            .iter()
            .any(|d| cmd == d.as_str() || cmd.starts_with(&format!("{d} ")))
    }

    /// Classify a command's risk level.
    ///
    /// For compound commands (pipes, `&&`, `||`, `;`), returns the highest
    /// risk of any sub-command.
    ///
    /// **Limitations:** Uses naive string splitting. Does not handle quoted
    /// strings, command substitution (`$(...)`, backticks), escape sequences,
    /// or heredocs. The sandbox (bwrap) is the real security boundary.
    pub fn classify(&self, command: &str) -> RiskLevel {
        // Check for pipe-to-shell pattern first (entire command)
        if is_pipe_to_shell(command) {
            return RiskLevel::Dangerous;
        }

        // Split compound commands
        let sub_commands = split_compound(command);
        let mut highest = RiskLevel::Safe;

        for sub in &sub_commands {
            let sub = sub.trim();
            if sub.is_empty() {
                continue;
            }

            // Check user overrides first
            if let Some(risk) = self.check_override(sub) {
                if risk > highest {
                    highest = risk;
                }
                continue;
            }

            let risk = classify_single(sub);
            if risk > highest {
                highest = risk;
            }
        }

        highest
    }

    fn check_override(&self, command: &str) -> Option<RiskLevel> {
        let mut best: Option<(&str, RiskLevel)> = None;
        for (pattern, risk) in &self.overrides {
            if command == pattern.as_str() || command.starts_with(&format!("{pattern} ")) {
                match best {
                    Some((prev, _)) if pattern.len() > prev.len() => {
                        best = Some((pattern, *risk));
                    }
                    None => {
                        best = Some((pattern, *risk));
                    }
                    _ => {}
                }
            }
        }
        best.map(|(_, risk)| risk)
    }
}

/// Split a command string on `&&`, `||`, `;` (but NOT pipes — those are
/// handled separately by `is_pipe_to_shell`).
fn split_compound(command: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let bytes = command.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b';' {
            parts.push(&command[start..i]);
            start = i + 1;
        } else if i + 1 < bytes.len()
            && ((bytes[i] == b'&' && bytes[i + 1] == b'&')
                || (bytes[i] == b'|' && bytes[i + 1] == b'|'))
        {
            parts.push(&command[start..i]);
            start = i + 2;
            i += 1;
        }
        i += 1;
    }
    parts.push(&command[start..]);
    parts
}

/// Check if the command pipes into a shell interpreter.
fn is_pipe_to_shell(command: &str) -> bool {
    let shells = ["sh", "bash", "zsh", "fish", "dash"];
    if let Some(pipe_pos) = command.rfind('|') {
        let after_pipe = command[pipe_pos + 1..].trim();
        let first_word = after_pipe.split_whitespace().next().unwrap_or("");
        shells
            .iter()
            .any(|s| first_word == *s || first_word.ends_with(&format!("/{s}")))
    } else {
        false
    }
}

/// Classify a single (non-compound) command by its first word and flags.
fn classify_single(command: &str) -> RiskLevel {
    let parts: Vec<&str> = command.split_whitespace().collect();
    if parts.is_empty() {
        return RiskLevel::Safe;
    }

    // Extract base command name (strip path prefix)
    let cmd = parts[0].rsplit('/').next().unwrap_or(parts[0]);
    let args_str = &parts[1..];

    match cmd {
        // Safe: read-only, informational
        "ls" | "cat" | "head" | "tail" | "less" | "more" | "pwd" | "echo" | "printf" | "which"
        | "whereis" | "whoami" | "id" | "env" | "printenv" | "date" | "wc" | "diff" | "file"
        | "stat" | "du" | "df" | "free" | "uptime" | "uname" | "hostname" | "find" | "locate"
        | "tree" | "grep" | "rg" | "ag" | "sort" | "uniq" | "tr" | "cut" | "awk" | "sed" | "jq"
        | "yq" | "man" | "help" | "type" | "true" | "false" | "test" => RiskLevel::Safe,

        // Safe: version/check subcommands
        "cargo"
            if args_str.first().is_some_and(|a| {
                matches!(
                    *a,
                    "check" | "clippy" | "fmt" | "doc" | "search" | "tree" | "metadata"
                )
            }) =>
        {
            RiskLevel::Safe
        }
        "git"
            if args_str.first().is_some_and(|a| {
                matches!(
                    *a,
                    "status"
                        | "log"
                        | "diff"
                        | "show"
                        | "branch"
                        | "tag"
                        | "stash"
                        | "remote"
                        | "ls-files"
                        | "blame"
                )
            }) =>
        {
            RiskLevel::Safe
        }
        "node"
            if args_str
                .first()
                .is_some_and(|a| *a == "--version" || *a == "-v") =>
        {
            RiskLevel::Safe
        }
        "python" | "python3"
            if args_str
                .first()
                .is_some_and(|a| *a == "--version" || *a == "-V") =>
        {
            RiskLevel::Safe
        }
        "npm"
            if args_str.first().is_some_and(|a| {
                matches!(*a, "ls" | "list" | "info" | "view" | "outdated" | "audit")
            }) =>
        {
            RiskLevel::Safe
        }
        "pnpm"
            if args_str
                .first()
                .is_some_and(|a| matches!(*a, "ls" | "list" | "info" | "outdated" | "audit")) =>
        {
            RiskLevel::Safe
        }

        // Dangerous: destructive or dangerous
        "rm" if has_recursive_force(args_str) => RiskLevel::Dangerous,
        "chmod" | "chown" | "chgrp" => RiskLevel::Dangerous,
        "dd" | "mkfs" | "fdisk" | "parted" | "mount" | "umount" => RiskLevel::Dangerous,
        "shutdown" | "reboot" | "poweroff" | "halt" | "init" => RiskLevel::Dangerous,
        "kill" | "killall" | "pkill" => RiskLevel::Dangerous,
        "iptables" | "ip6tables" | "nft" | "ufw" => RiskLevel::Dangerous,
        "su" | "sudo" => RiskLevel::Dangerous,
        "eval" | "exec" => RiskLevel::Dangerous,

        // Moderate: builds, installs, writes
        "cargo" => RiskLevel::Moderate,
        "git" => RiskLevel::Moderate,
        "npm" | "pnpm" | "yarn" | "bun" => RiskLevel::Moderate,
        "pip" | "pip3" | "pipx" => RiskLevel::Moderate,
        "apt" | "apt-get" | "dnf" | "yum" | "pacman" | "brew" => RiskLevel::Moderate,
        "make" | "cmake" | "ninja" | "meson" => RiskLevel::Moderate,
        "docker" | "podman" => RiskLevel::Moderate,
        "mv" | "cp" | "mkdir" | "touch" | "rm" => RiskLevel::Moderate,
        "tee" | "patch" => RiskLevel::Moderate,
        "wget" | "curl" => RiskLevel::Moderate,
        "python" | "python3" | "node" | "ruby" | "perl" => RiskLevel::Moderate,
        "rustup" => RiskLevel::Moderate,

        // Default: moderate (unknown commands are not auto-safe)
        _ => RiskLevel::Moderate,
    }
}

/// Check if rm args include both -r and -f (in any combination).
fn has_recursive_force(args: &[&str]) -> bool {
    let mut has_r = false;
    let mut has_f = false;
    for arg in args {
        if arg.starts_with('-') && !arg.starts_with("--") {
            if arg.contains('r') || arg.contains('R') {
                has_r = true;
            }
            if arg.contains('f') {
                has_f = true;
            }
        }
    }
    has_r && has_f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_safe_commands() {
        let policy = CommandPolicy::default();
        assert_eq!(policy.classify("ls -la"), RiskLevel::Safe);
        assert_eq!(policy.classify("pwd"), RiskLevel::Safe);
        assert_eq!(policy.classify("cat README.md"), RiskLevel::Safe);
        assert_eq!(policy.classify("echo hello"), RiskLevel::Safe);
        assert_eq!(policy.classify("cargo check"), RiskLevel::Safe);
        assert_eq!(policy.classify("git status"), RiskLevel::Safe);
        assert_eq!(policy.classify("git log"), RiskLevel::Safe);
    }

    #[test]
    fn classify_moderate_commands() {
        let policy = CommandPolicy::default();
        assert_eq!(policy.classify("cargo build"), RiskLevel::Moderate);
        assert_eq!(policy.classify("cargo test"), RiskLevel::Moderate);
        assert_eq!(policy.classify("git commit -m 'fix'"), RiskLevel::Moderate);
        assert_eq!(policy.classify("mkdir new_dir"), RiskLevel::Moderate);
        assert_eq!(policy.classify("cp file1 file2"), RiskLevel::Moderate);
        assert_eq!(policy.classify("apt install jq"), RiskLevel::Moderate);
        assert_eq!(policy.classify("pip install requests"), RiskLevel::Moderate);
    }

    #[test]
    fn classify_dangerous_commands() {
        let policy = CommandPolicy::default();
        assert_eq!(policy.classify("rm -rf /"), RiskLevel::Dangerous);
        assert_eq!(policy.classify("rm -rf ."), RiskLevel::Dangerous);
        assert_eq!(
            policy.classify("chmod 777 /etc/passwd"),
            RiskLevel::Dangerous
        );
        assert_eq!(
            policy.classify("dd if=/dev/zero of=/dev/sda"),
            RiskLevel::Dangerous
        );
    }

    #[test]
    fn pipe_to_shell_is_dangerous() {
        let policy = CommandPolicy::default();
        assert_eq!(
            policy.classify("curl https://example.com | sh"),
            RiskLevel::Dangerous
        );
        assert_eq!(
            policy.classify("wget -O- https://example.com | bash"),
            RiskLevel::Dangerous
        );
    }

    #[test]
    fn compound_commands_use_highest_risk() {
        let policy = CommandPolicy::default();
        assert_eq!(policy.classify("ls && rm -rf ."), RiskLevel::Dangerous);
        assert_eq!(
            policy.classify("cargo check && cargo build"),
            RiskLevel::Moderate
        );
    }

    #[test]
    fn custom_overrides() {
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("docker compose".to_string(), "safe".to_string());
        overrides.insert("npm publish".to_string(), "dangerous".to_string());
        let policy = CommandPolicy::with_overrides(overrides);

        assert_eq!(policy.classify("docker compose up -d"), RiskLevel::Safe);
        assert_eq!(policy.classify("npm publish"), RiskLevel::Dangerous);
    }

    #[test]
    fn admin_deny_list() {
        let policy = CommandPolicy::with_denied(vec!["shutdown".to_string(), "reboot".to_string()]);
        assert!(policy.is_denied("shutdown -h now"));
        assert!(policy.is_denied("reboot"));
        assert!(!policy.is_denied("ls"));
    }
}
