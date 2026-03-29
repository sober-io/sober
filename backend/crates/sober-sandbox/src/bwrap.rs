//! Bubblewrap sandbox builder and process execution.
//!
//! [`BwrapSandbox`] assembles bwrap arguments from a resolved [`SandboxPolicy`]
//! and manages process lifecycle including timeout enforcement.

use std::collections::HashMap;
use std::time::Instant;

use metrics::{counter, histogram};
use tokio::process::{Child, Command};
use tokio::time::{Duration, timeout};
use tracing::{debug, info, warn};

use crate::audit::{ExecutionTrigger, SandboxAuditEntry};
use crate::detect::{detect_bwrap, detect_socat};
use crate::error::SandboxError;
use crate::policy::{NetMode, SandboxPolicy};
use crate::proxy::ProxyBridge;

/// Fixed port for the inner socat bridge inside the sandbox namespace.
/// The sandboxed process's HTTP_PROXY points here.
const SANDBOX_PROXY_PORT: u16 = 18080;

/// Result of a completed sandbox execution.
#[derive(Debug, Clone)]
pub struct SandboxResult {
    /// Process exit code (0 = success).
    pub exit_code: i32,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// Domains that were denied by the network proxy (if active).
    pub denied_network_requests: Vec<String>,
}

/// Sandbox execution engine backed by bubblewrap (bwrap).
pub struct BwrapSandbox {
    policy: SandboxPolicy,
}

impl BwrapSandbox {
    /// Create a new sandbox from a resolved policy.
    pub fn new(policy: SandboxPolicy) -> Self {
        Self { policy }
    }

    /// Run a command to completion inside the sandbox.
    ///
    /// Returns the process output, captured stdout/stderr, and any denied
    /// network requests (if using `AllowedDomains` mode).
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError::BwrapNotFound`] if bwrap is not installed,
    /// [`SandboxError::SpawnFailed`] if the process can't start,
    /// [`SandboxError::Timeout`] if the process exceeds `max_execution_seconds`,
    /// or [`SandboxError::Killed`] if the process had to be killed.
    pub async fn execute(
        &self,
        command: &[String],
        env: &HashMap<String, String>,
    ) -> Result<(SandboxResult, SandboxAuditEntry), SandboxError> {
        let bwrap_path = detect_bwrap()?;
        let start = Instant::now();

        // Start proxy if needed.
        let proxy = match &self.policy.net_mode {
            NetMode::AllowedDomains(domains) if !domains.is_empty() => {
                Some(ProxyBridge::start(domains.clone()).await?)
            }
            _ => None,
        };

        let args = self.build_args(command, proxy.as_ref());
        let mut env_vars = env.clone();

        // Set proxy env vars if proxy is active.
        // Points to the inner socat on sandbox loopback (SANDBOX_PROXY_PORT).
        if proxy.is_some() {
            let proxy_url = format!("http://127.0.0.1:{SANDBOX_PROXY_PORT}");
            env_vars.insert("HTTP_PROXY".to_owned(), proxy_url.clone());
            env_vars.insert("HTTPS_PROXY".to_owned(), proxy_url.clone());
            env_vars.insert("http_proxy".to_owned(), proxy_url.clone());
            env_vars.insert("https_proxy".to_owned(), proxy_url);
        }

        debug!(
            bwrap = %bwrap_path.display(),
            args_count = args.len(),
            policy = %self.policy.name,
            "spawning sandboxed process"
        );

        let child = Command::new(&bwrap_path)
            .args(&args)
            .envs(&env_vars)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| SandboxError::SpawnFailed(format!("bwrap spawn failed: {e}")))?;

        // Apply timeout.
        let max_secs = self.policy.max_execution_seconds;
        let result = timeout(
            Duration::from_secs(u64::from(max_secs)),
            child.wait_with_output(),
        )
        .await;

        let denied = if let Some(proxy) = proxy {
            proxy.stop().await.unwrap_or_default()
        } else {
            vec![]
        };

        let duration_ms = start.elapsed().as_millis() as u64;
        let profile = self.policy.name.clone();
        let elapsed_secs = start.elapsed().as_secs_f64();

        match result {
            Ok(Ok(output)) => {
                let sandbox_result = SandboxResult {
                    exit_code: output.status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    duration_ms,
                    denied_network_requests: denied,
                };

                // Metrics.
                counter!(
                    "sober_sandbox_executions_total",
                    "profile" => profile.clone(),
                    "status" => "success",
                )
                .increment(1);
                histogram!(
                    "sober_sandbox_execution_duration_seconds",
                    "profile" => profile.clone(),
                )
                .record(elapsed_secs);

                for domain in &sandbox_result.denied_network_requests {
                    counter!(
                        "sober_sandbox_policy_violations_total",
                        "profile" => profile.clone(),
                        "violation" => format!("network_denied:{domain}"),
                    )
                    .increment(1);
                }

                // Build audit entry.
                let entry = SandboxAuditEntry::from_result(
                    self.policy.clone(),
                    command.to_vec(),
                    ExecutionTrigger::Agent,
                    &sandbox_result,
                );
                info!(
                    execution_id = %entry.execution_id,
                    policy = %entry.policy.name,
                    command = ?&entry.command[..entry.command.len().min(3)],
                    duration_ms = entry.duration_ms,
                    exit_code = ?entry.exit_code,
                    outcome = ?entry.outcome,
                    denied_domains = ?entry.denied_network_requests,
                    "sandbox execution audit"
                );

                Ok((sandbox_result, entry))
            }
            Ok(Err(e)) => {
                counter!(
                    "sober_sandbox_executions_total",
                    "profile" => profile,
                    "status" => "error",
                )
                .increment(1);
                Err(SandboxError::SpawnFailed(format!(
                    "failed to wait for process: {e}"
                )))
            }
            Err(_) => {
                counter!(
                    "sober_sandbox_executions_total",
                    "profile" => profile,
                    "status" => "timeout",
                )
                .increment(1);
                warn!(
                    seconds = max_secs,
                    policy = %self.policy.name,
                    "sandbox execution timed out, killing process"
                );
                Err(SandboxError::Timeout { seconds: max_secs })
            }
        }
    }

    /// Spawn a long-running sandboxed process with piped stdin/stdout.
    ///
    /// Used by `sober-mcp` for MCP server processes that communicate over stdio.
    /// The caller is responsible for managing the child process lifecycle.
    ///
    /// # Errors
    ///
    /// Returns [`SandboxError::BwrapNotFound`] or [`SandboxError::SpawnFailed`].
    pub async fn spawn(
        &self,
        command: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Child, SandboxError> {
        let bwrap_path = detect_bwrap()?;
        let args = self.build_args(command, None);

        debug!(
            bwrap = %bwrap_path.display(),
            policy = %self.policy.name,
            "spawning long-running sandboxed process"
        );

        Command::new(&bwrap_path)
            .args(&args)
            .envs(env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| SandboxError::SpawnFailed(format!("bwrap spawn failed: {e}")))
    }

    /// Build the bwrap argument list from the current policy.
    fn build_args(&self, command: &[String], proxy: Option<&ProxyBridge>) -> Vec<String> {
        let mut args = Vec::new();

        // Always applied: PID isolation, die-with-parent, new session.
        args.extend(["--unshare-pid", "--die-with-parent", "--new-session"].map(String::from));

        // Always: proc, dev, tmpfs /tmp.
        args.extend(["--proc", "/proc"].map(String::from));
        args.extend(["--dev", "/dev"].map(String::from));
        args.extend(["--tmpfs", "/tmp"].map(String::from));

        // Always: system library bind-mounts (read-only).
        for sys_path in &["/usr", "/lib", "/lib64", "/bin", "/sbin"] {
            if std::path::Path::new(sys_path).exists() {
                args.extend(["--ro-bind", sys_path, sys_path].map(String::from));
            }
        }

        // Always: DNS and essential /etc files (read-only).
        for etc_file in &[
            "/etc/resolv.conf",
            "/etc/hosts",
            "/etc/nsswitch.conf",
            "/etc/ssl",
        ] {
            if std::path::Path::new(etc_file).exists() {
                args.extend(["--ro-bind", etc_file, etc_file].map(String::from));
            }
        }

        // Always: deny sensitive paths.
        for sensitive in Self::sensitive_paths() {
            if std::path::Path::new(&sensitive).exists() {
                args.extend(["--ro-bind", "/dev/null", &sensitive].map(String::from));
            }
        }

        // Per-policy: read-only bind mounts.
        for path in &self.policy.fs_read {
            let p = path.to_string_lossy();
            args.extend(["--ro-bind".to_owned(), p.to_string(), p.to_string()]);
        }

        // Per-policy: read-write bind mounts.
        for path in &self.policy.fs_write {
            let p = path.to_string_lossy();
            args.extend(["--bind".to_owned(), p.to_string(), p.to_string()]);
        }

        // Per-policy: denied paths.
        for path in &self.policy.fs_deny {
            let p = path.to_string_lossy();
            args.extend([
                "--ro-bind".to_owned(),
                "/dev/null".to_owned(),
                p.to_string(),
            ]);
        }

        // Network isolation.
        match &self.policy.net_mode {
            NetMode::None => {
                args.push("--unshare-net".to_owned());
            }
            NetMode::AllowedDomains(_) => {
                args.push("--unshare-net".to_owned());
                if let Some(proxy) = proxy {
                    // Bind-mount the UDS socket into the sandbox.
                    let sock = proxy.socket_path().to_string_lossy().to_string();
                    args.extend(["--bind".to_owned(), sock.clone(), sock.clone()]);

                    // Bind-mount socat binary so the inner bridge can run.
                    if let Ok(socat_path) = detect_socat() {
                        let socat_str = socat_path.to_string_lossy().to_string();
                        args.extend([
                            "--ro-bind".to_owned(),
                            socat_str,
                            "/usr/bin/socat".to_owned(),
                        ]);
                    }

                    // Wrap the command: start inner socat bridge, then exec the real command.
                    // Inner socat: TCP on sandbox loopback → UDS socket → host proxy.
                    args.push("--".to_owned());
                    let inner_cmd = command
                        .iter()
                        .map(|a| shell_escape(a))
                        .collect::<Vec<_>>()
                        .join(" ");
                    args.extend([
                        "sh".to_owned(),
                        "-c".to_owned(),
                        format!(
                            "/usr/bin/socat TCP-LISTEN:{SANDBOX_PROXY_PORT},bind=127.0.0.1,fork,reuseaddr UNIX-CONNECT:{sock} & exec {inner_cmd}"
                        ),
                    ]);
                    return args;
                }
            }
            NetMode::Full => {
                // No network namespace restriction.
            }
        }

        // Separator before the actual command.
        args.push("--".to_owned());
        args.extend(command.iter().cloned());

        args
    }

    /// Paths that are always denied (bound to /dev/null) regardless of profile.
    fn sensitive_paths() -> Vec<String> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_owned());
        vec![
            format!("{home}/.ssh"),
            format!("{home}/.aws"),
            format!("{home}/.gnupg"),
        ]
    }
}

/// Escape a string for safe inclusion in a shell command.
fn shell_escape(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_./=:@".contains(c))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn test_policy(net_mode: NetMode) -> SandboxPolicy {
        SandboxPolicy {
            name: "test".into(),
            fs_read: vec![PathBuf::from("/workspace/src")],
            fs_write: vec![PathBuf::from("/workspace/output")],
            fs_deny: vec![PathBuf::from("/secrets")],
            net_mode,
            max_execution_seconds: 30,
            allow_spawn: false,
        }
    }

    #[test]
    fn args_always_include_pid_isolation() {
        let sandbox = BwrapSandbox::new(test_policy(NetMode::None));
        let args = sandbox.build_args(&["echo".into(), "hello".into()], None);

        assert!(args.contains(&"--unshare-pid".to_owned()));
        assert!(args.contains(&"--die-with-parent".to_owned()));
        assert!(args.contains(&"--new-session".to_owned()));
    }

    #[test]
    fn args_include_system_mounts() {
        let sandbox = BwrapSandbox::new(test_policy(NetMode::None));
        let args = sandbox.build_args(&["echo".into()], None);

        // Should have ro-bind for /usr (which always exists).
        let usr_idx = args.iter().position(|a| a == "/usr");
        assert!(usr_idx.is_some());
    }

    #[test]
    fn args_include_fs_read_write_deny() {
        let sandbox = BwrapSandbox::new(test_policy(NetMode::None));
        let args = sandbox.build_args(&["echo".into()], None);

        // fs_read: --ro-bind /workspace/src /workspace/src
        assert!(
            args.windows(3)
                .any(|w| w == ["--ro-bind", "/workspace/src", "/workspace/src"])
        );

        // fs_write: --bind /workspace/output /workspace/output
        assert!(
            args.windows(3)
                .any(|w| w == ["--bind", "/workspace/output", "/workspace/output"])
        );

        // fs_deny: --ro-bind /dev/null /secrets
        assert!(
            args.windows(3)
                .any(|w| w == ["--ro-bind", "/dev/null", "/secrets"])
        );
    }

    #[test]
    fn args_unshare_net_for_none_mode() {
        let sandbox = BwrapSandbox::new(test_policy(NetMode::None));
        let args = sandbox.build_args(&["echo".into()], None);
        assert!(args.contains(&"--unshare-net".to_owned()));
    }

    #[test]
    fn args_unshare_net_for_allowed_domains_without_proxy() {
        // Without a proxy, AllowedDomains still gets --unshare-net.
        let sandbox = BwrapSandbox::new(test_policy(NetMode::AllowedDomains(vec![
            "example.com".into(),
        ])));
        let args = sandbox.build_args(&["echo".into()], None);
        assert!(args.contains(&"--unshare-net".to_owned()));
    }

    #[test]
    fn shell_escape_simple() {
        assert_eq!(shell_escape("echo"), "echo");
        assert_eq!(shell_escape("hello world"), "'hello world'");
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn args_no_unshare_net_for_full_mode() {
        let sandbox = BwrapSandbox::new(test_policy(NetMode::Full));
        let args = sandbox.build_args(&["echo".into()], None);
        assert!(!args.contains(&"--unshare-net".to_owned()));
    }

    #[test]
    fn args_end_with_command() {
        let sandbox = BwrapSandbox::new(test_policy(NetMode::None));
        let cmd = vec!["python3".to_owned(), "script.py".to_owned()];
        let args = sandbox.build_args(&cmd, None);

        let separator = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(&args[separator + 1..], &["python3", "script.py"]);
    }

    #[test]
    fn sensitive_paths_always_denied() {
        let sandbox = BwrapSandbox::new(test_policy(NetMode::Full));
        let args = sandbox.build_args(&["echo".into()], None);

        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_owned());
        let ssh_path = format!("{home}/.ssh");

        // If ~/.ssh exists, it should be denied.
        if std::path::Path::new(&ssh_path).exists() {
            assert!(
                args.windows(3)
                    .any(|w| w[0] == "--ro-bind" && w[1] == "/dev/null" && w[2] == ssh_path)
            );
        }
    }
}
