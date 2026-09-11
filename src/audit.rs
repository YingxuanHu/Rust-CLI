//! Append-only audit records for direct shell executions.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::command_policy::CommandRisk;

#[derive(Debug, Serialize)]
struct ShellAuditEvent<'a> {
    timestamp_unix_ms: u128,
    cwd: String,
    command: String,
    risk: &'a str,
    outcome: &'a str,
    output_bytes: usize,
}

/// Resolve a configured audit path against the active session directory.
/// This keeps the default `.llm-cli/audit.jsonl` with the project even after a
/// user changes the TUI's session directory.
pub fn resolve_audit_path(configured_path: &Path, cwd: &Path) -> PathBuf {
    if configured_path.is_absolute() {
        configured_path.to_path_buf()
    } else {
        cwd.join(configured_path)
    }
}

/// Append a compact, redacted JSON Lines record. The operation is best-effort
/// at the call site so an unavailable audit directory never blocks a command
/// the user has explicitly approved.
pub fn append_shell_execution(
    configured_path: &Path,
    cwd: &Path,
    command: &str,
    risk: CommandRisk,
    outcome: &'static str,
    output_bytes: usize,
) -> Result<()> {
    let path = resolve_audit_path(configured_path, cwd);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating audit directory at {}", parent.display()))?;
    }

    let timestamp_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_millis();
    let event = ShellAuditEvent {
        timestamp_unix_ms,
        cwd: cwd.display().to_string(),
        command: redact_command(command),
        risk: risk.label(),
        outcome,
        output_bytes,
    };

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening audit log at {}", path.display()))?;
    serde_json::to_writer(&mut file, &event).context("serializing audit event")?;
    file.write_all(b"\n").context("terminating audit event")?;
    Ok(())
}

/// Retain enough of a command to explain what ran without keeping common
/// credential-bearing flags in a project-local log.
fn redact_command(command: &str) -> String {
    if command.to_ascii_lowercase().contains("authorization:") {
        return "[redacted command containing an Authorization header]".to_string();
    }

    let mut redact_next = false;
    command
        .split_whitespace()
        .map(|token| {
            if redact_next {
                redact_next = false;
                return "[REDACTED]".to_string();
            }

            let lower = token.to_ascii_lowercase();
            if matches!(lower.as_str(), "--token" | "--password" | "--api-key" | "--secret" | "-u" | "--user") {
                redact_next = true;
                return token.to_string();
            }
            if let Some((key, _)) = token.split_once('=') {
                let key_lower = key.to_ascii_lowercase();
                if key_lower.contains("token")
                    || key_lower.contains("password")
                    || key_lower.contains("secret")
                    || key_lower.contains("api_key")
                {
                    return format!("{key}=[REDACTED]");
                }
            }
            token.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{append_shell_execution, redact_command, resolve_audit_path};
    use crate::command_policy::CommandRisk;

    #[test]
    fn audit_records_are_json_lines_in_the_session_project() {
        let directory = tempfile::tempdir().unwrap();
        let configured = std::path::Path::new(".llm-cli/audit.jsonl");
        append_shell_execution(
            configured,
            directory.path(),
            "git status --short",
            CommandRisk::ReadOnly,
            "completed",
            14,
        )
        .unwrap();

        let path = resolve_audit_path(configured, directory.path());
        let record: serde_json::Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(record["risk"], "read-only");
        assert_eq!(record["outcome"], "completed");
        assert_eq!(record["command"], "git status --short");
    }

    #[test]
    fn audit_redacts_common_credential_forms() {
        assert_eq!(redact_command("curl --token abc123 https://example.invalid"), "curl --token [REDACTED] https://example.invalid");
        assert_eq!(redact_command("API_TOKEN=abc tool run"), "API_TOKEN=[REDACTED] tool run");
        assert!(redact_command("curl -H 'Authorization: Bearer abc' https://example.invalid").starts_with("[redacted"));
    }
}
