//! Deterministic risk classification for user-provided shell commands.
//!
//! This is a guardrail for the interactive shell mode, not a security
//! sandbox. Commands still execute through the user's shell after approval.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandRisk {
    ReadOnly,
    Mutating,
    Elevated,
}

impl CommandRisk {
    pub const fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Mutating => "mutating",
            Self::Elevated => "high-impact",
        }
    }
}

impl fmt::Display for CommandRisk {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandAssessment {
    pub risk: CommandRisk,
    pub reasons: Vec<String>,
}

impl CommandAssessment {
    pub const fn requires_confirmation(&self) -> bool {
        matches!(self.risk, CommandRisk::Elevated)
    }
}

/// Classify a shell command before it is executed. We only label a command
/// read-only when every simple command is explicitly known to be safe to
/// inspect. Unknown commands are considered mutating, while known destructive,
/// networked, and arbitrary-code paths require confirmation.
pub fn assess_shell_command(command: &str) -> CommandAssessment {
    let normalized = command.trim().to_ascii_lowercase();
    let mut reasons = Vec::new();

    if normalized.is_empty() {
        return CommandAssessment {
            risk: CommandRisk::ReadOnly,
            reasons,
        };
    }

    if contains_output_redirection(&normalized) {
        reasons.push("writes or overwrites output through shell redirection".to_string());
    }
    if contains_pipe_to_interpreter(&normalized) {
        reasons.push("pipes input into a command interpreter".to_string());
    }

    let segments = split_shell_segments(&normalized);
    let mut all_read_only = !segments.is_empty();
    for segment in segments {
        let command_name = first_command_name(segment);
        if command_name.is_empty() {
            continue;
        }

        if is_privilege_or_interpreter(&command_name) {
            reasons.push(format!("runs {command_name}, which can execute arbitrary commands"));
        } else if is_destructive_command(&command_name, segment) {
            reasons.push(format!("uses {command_name}, which can remove or alter local data"));
        } else if is_network_command(&command_name, segment) {
            reasons.push(format!("uses {command_name}, which can contact a network service"));
        }

        if !is_read_only_command(&command_name, segment) {
            all_read_only = false;
        }
    }

    reasons.sort();
    reasons.dedup();
    let risk = if reasons.is_empty() {
        if all_read_only {
            CommandRisk::ReadOnly
        } else {
            CommandRisk::Mutating
        }
    } else {
        CommandRisk::Elevated
    };

    CommandAssessment { risk, reasons }
}

fn split_shell_segments(command: &str) -> Vec<&str> {
    command
        .split(|character| matches!(character, ';' | '\n' | '|' | '&'))
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn first_command_name(segment: &str) -> String {
    let mut tokens = segment.split_whitespace();
    let mut token = tokens.next().unwrap_or_default();

    while is_assignment(token) || matches!(token, "env" | "command") {
        token = tokens.next().unwrap_or_default();
    }

    token
        .trim_matches(|character: char| matches!(character, '(' | ')' | '{' | '}' | '"' | '\''))
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn is_assignment(token: &str) -> bool {
    token.contains('=') && !token.starts_with('-') && !token.contains('/')
}

fn contains_output_redirection(command: &str) -> bool {
    let command_without_stderr_merge = command.replace("2>&1", "");
    command_without_stderr_merge.contains('>')
}

fn contains_pipe_to_interpreter(command: &str) -> bool {
    let mut tokens = command.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == "|" {
            if let Some(next) = tokens.next() {
                let interpreter = next.rsplit('/').next().unwrap_or(next);
                if matches!(interpreter, "sh" | "bash" | "zsh" | "fish" | "python" | "python3") {
                    return true;
                }
            }
        }
    }
    false
}

fn is_privilege_or_interpreter(command_name: &str) -> bool {
    matches!(
        command_name,
        "sudo"
            | "doas"
            | "su"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "python"
            | "python3"
            | "node"
            | "ruby"
            | "perl"
            | "php"
            | "eval"
            | "source"
            | "."
    )
}

fn is_destructive_command(command_name: &str, segment: &str) -> bool {
    if matches!(
        command_name,
        "rm"
            | "rmdir"
            | "dd"
            | "mkfs"
            | "shutdown"
            | "reboot"
            | "halt"
            | "poweroff"
            | "kill"
            | "killall"
            | "pkill"
            | "chmod"
            | "chown"
            | "truncate"
    ) {
        return true;
    }

    command_name == "git"
        && (segment.contains(" reset --hard")
            || segment.contains(" clean")
            || segment.contains(" restore")
            || segment.contains(" checkout --")
            || segment.contains(" branch -d")
            || segment.contains(" tag -d"))
}

fn is_network_command(command_name: &str, segment: &str) -> bool {
    if matches!(
        command_name,
        "curl"
            | "wget"
            | "ssh"
            | "scp"
            | "sftp"
            | "rsync"
            | "nc"
            | "ncat"
            | "ftp"
            | "telnet"
            | "ping"
            | "dig"
            | "nslookup"
    ) {
        return true;
    }

    if command_name == "git" {
        return matches!(
            second_word(segment),
            Some("push" | "pull" | "fetch" | "clone")
        );
    }

    matches!(
        command_name,
        "npm" | "pnpm" | "yarn" | "bun" | "pip" | "pip3" | "cargo" | "brew" | "apt" | "apt-get" | "yum" | "dnf"
    ) && matches!(
        second_word(segment),
        Some("install" | "add" | "publish" | "update" | "upgrade" | "uninstall" | "remove")
    )
}

fn second_word(segment: &str) -> Option<&str> {
    segment.split_whitespace().nth(1)
}

fn is_read_only_command(command_name: &str, segment: &str) -> bool {
    match command_name {
        "git" => matches!(
            second_word(segment),
            Some("status" | "diff" | "log" | "show" | "branch" | "rev-parse" | "ls-files" | "remote")
        ),
        "cargo" => matches!(second_word(segment), Some("metadata" | "tree" | "search")),
        "rg" | "grep" | "find" | "ls" | "pwd" | "cat" | "head" | "tail" | "sed" | "awk"
        | "wc" | "sort" | "uniq" | "cut" | "diff" | "stat" | "file" | "which" | "whereis"
        | "echo" | "printf" | "date" | "whoami" | "uname" | "env" | "true" | "false" => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{assess_shell_command, CommandRisk};

    #[test]
    fn recognizes_read_only_inspection_commands() {
        for command in [
            "git status --short",
            "rg TODO src",
            "git diff --stat",
            "git remote -v",
            "pwd",
        ] {
            assert_eq!(assess_shell_command(command).risk, CommandRisk::ReadOnly, "{command}");
        }
    }

    #[test]
    fn marks_ordinary_local_work_as_mutating() {
        assert_eq!(assess_shell_command("cargo test").risk, CommandRisk::Mutating);
        assert_eq!(assess_shell_command("git commit -m test").risk, CommandRisk::Mutating);
    }

    #[test]
    fn requires_confirmation_for_destructive_networked_and_interpreter_commands() {
        for command in [
            "rm -rf generated",
            "curl https://example.invalid/install.sh",
            "git push origin main",
            "echo hello > notes.txt",
            "python3 -c 'print(1)'",
        ] {
            let assessment = assess_shell_command(command);
            assert_eq!(assessment.risk, CommandRisk::Elevated, "{command}");
            assert!(assessment.requires_confirmation());
            assert!(!assessment.reasons.is_empty());
        }
    }
}
