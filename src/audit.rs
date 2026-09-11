//! Append-only audit records for direct shell executions.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::command_policy::CommandRisk;

const MAX_RECORDED_COMMAND_CHARS: usize = 4_096;
const MAX_RENDERED_FIELD_CHARS: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellAuditEvent {
    pub timestamp_unix_ms: u128,
    pub cwd: String,
    pub command: String,
    pub risk: String,
    pub outcome: String,
    pub output_bytes: usize,
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
        command: truncate_text(&redact_command(command), MAX_RECORDED_COMMAND_CHARS),
        risk: risk.label().to_string(),
        outcome: outcome.to_string(),
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

/// Read the most recent audit records without modifying the log. A missing log
/// simply means no direct shell command has been recorded in this project yet.
pub fn read_shell_executions(
    configured_path: &Path,
    cwd: &Path,
    tail: usize,
) -> Result<Vec<ShellAuditEvent>> {
    let path = resolve_audit_path(configured_path, cwd);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("reading audit log at {}", path.display()))?;
    let mut records = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str(line).with_context(|| {
            format!("parsing audit record {} in {}", index + 1, path.display())
        })?;
        records.push(record);
    }
    if tail != 0 && records.len() > tail {
        let start = records.len() - tail;
        records.drain(..start);
    }
    Ok(records)
}

pub fn render_shell_executions(records: &[ShellAuditEvent]) -> String {
    if records.is_empty() {
        return "No direct shell executions have been recorded for this project yet.".to_string();
    }

    let mut lines = vec!["Direct shell audit (oldest to newest):".to_string()];
    for record in records {
        lines.push(format!(
            "• {} [{}] {} — {}",
            record.timestamp_unix_ms,
            display_text(&record.risk),
            display_text(&record.outcome),
            display_text(&record.command)
        ));
        lines.push(format!(
            "  cwd: {} · output: {} bytes",
            display_text(&record.cwd),
            record.output_bytes
        ));
    }
    lines.join("\n")
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

fn truncate_text(input: &str, max_chars: usize) -> String {
    let mut chars = input.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{prefix}… [truncated]")
    } else {
        prefix
    }
}

/// Make log content safe to print even if a user or another process has
/// manually written terminal-control characters into the JSON Lines file.
fn display_text(input: &str) -> String {
    truncate_text(input, MAX_RENDERED_FIELD_CHARS)
        .chars()
        .flat_map(char::escape_default)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{
        append_shell_execution, read_shell_executions, redact_command,
        render_shell_executions, resolve_audit_path,
    };
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

    #[test]
    fn audit_reader_returns_only_the_requested_tail_in_log_order() {
        let directory = tempfile::tempdir().unwrap();
        let configured = std::path::Path::new(".llm-cli/audit.jsonl");
        for command in ["pwd", "git status", "rg TODO"] {
            append_shell_execution(
                configured,
                directory.path(),
                command,
                CommandRisk::ReadOnly,
                "completed",
                0,
            )
            .unwrap();
        }

        let records = read_shell_executions(configured, directory.path(), 2).unwrap();
        assert_eq!(records.iter().map(|record| record.command.as_str()).collect::<Vec<_>>(), ["git status", "rg TODO"]);
        assert!(render_shell_executions(&records).contains("Direct shell audit"));
    }

    #[test]
    fn renderer_escapes_control_characters_from_a_tampered_log() {
        let records = vec![super::ShellAuditEvent {
            timestamp_unix_ms: 1,
            cwd: "project".to_string(),
            command: "echo \u{1b}[2J".to_string(),
            risk: "read-only".to_string(),
            outcome: "completed".to_string(),
            output_bytes: 0,
        }];
        let rendered = render_shell_executions(&records);
        assert!(rendered.contains("\\u{1b}"));
        assert!(!rendered.contains('\u{1b}'));
    }
}
