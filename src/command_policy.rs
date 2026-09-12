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

/// Heuristically classify a shell command before execution. Known inspection
/// names can receive a read-only label; recognized destructive, networked,
/// scripted, and output-writing forms require confirmation. This deliberately
/// conservative check does not parse every shell expansion or tool option and
/// is not a security boundary. Unknown commands are considered mutating.
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
    // This is intentionally conservative, not a shell parser. Even inside
    // quotes these constructs warrant review rather than a read-only label.
    if normalized.contains("$(") || normalized.contains('`')
        || normalized.contains("<(") || normalized.contains(">(")
    {
        reasons.push("contains shell command or process substitution".to_string());
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
        if has_unsafe_inspection_options(&command_name, segment) {
            reasons.push(format!("uses {command_name} options or scripts that can write data or execute commands"));
        }
        if command_name.starts_with('-') {
            reasons.push("uses command-wrapper options that require manual review".to_string());
        }
        if segment.split_whitespace()
            .take_while(|word| matches!(*word, "env" | "command") || is_assignment(word))
            .next().is_some()
        {
            reasons.push("uses environment overrides or command wrappers that require manual review".to_string());
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
    let Some((name, _)) = token.split_once('=') else { return false; };
    let mut chars = name.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
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

fn has_unsafe_inspection_options(command_name: &str, segment: &str) -> bool {
    let words: Vec<_> = segment.split_whitespace().collect();
    match command_name {
        // Both tools support embedded programs with filesystem/process effects;
        // recognizing a harmless subset requires a real expression parser.
        "sed" | "awk" => true,
        "find" => words.iter().any(|word| matches!(*word,
            "-delete" | "-exec" | "-execdir" | "-ok" | "-okdir"
                | "-fprint" | "-fprint0" | "-fprintf" | "-fls"
        )),
        "rg" => words.iter().any(|word| *word == "--pre" || word.starts_with("--pre=")),
        "sort" => words.iter().any(|word| *word == "--output" || word.starts_with("--output=")
            || (*word != "--" && word.starts_with("-o"))),
        // uniq can overwrite a second positional output file. Keep every use
        // under review until its option/operand syntax is parsed structurally.
        "uniq" => true,
        "git" => {
            let subcommand = second_word(segment).unwrap_or_default();
            // Global options such as `-C` or `-c` change the target/context;
            // don't assume the next token is the real subcommand.
            subcommand.starts_with('-')
                || (subcommand == "remote" && words.iter().any(|word| matches!(*word,
                    "add" | "remove" | "rm" | "rename" | "set-url" | "set-head" | "set-branches" | "prune" | "update"
                )))
                || (subcommand == "branch" && words.iter().any(|word| matches!(*word,
                    "-f" | "--force" | "-m" | "-c" | "--move" | "--copy" | "--edit-description"
                )))
                || words.iter().any(|word| *word == "--output" || word.starts_with("--output="))
        }
        _ => false,
    }
}

fn is_read_only_command(command_name: &str, segment: &str) -> bool {
    match command_name {
        "git" => {
            // Branch creation changes local state even without a destructive
            // option. Only the bare listing is known to be read-only here.
            (second_word(segment) == Some("branch") && segment.split_whitespace().count() == 2)
                || matches!(second_word(segment),
                    Some("status" | "diff" | "log" | "show" | "rev-parse" | "ls-files" | "remote")
                )
        }
        "cargo" => matches!(second_word(segment), Some("metadata" | "tree" | "search")),
        "rg" | "grep" | "find" | "ls" | "pwd" | "cat" | "head" | "tail"
        | "wc" | "sort" | "cut" | "diff" | "stat" | "file" | "which" | "whereis"
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

    #[test]
    fn inspection_commands_with_effects_require_review() {
        for command in [
            "echo $(touch marker)",
            "printf '%s' `touch marker`",
            "cat <(curl https://example.invalid)",
            "find . -delete",
            "find . -exec touch marker \\;",
            "find . -fprint listing.txt",
            "sed -i 's/old/new/' notes.txt",
            "awk 'BEGIN { system(\"touch marker\") }'",
            "rg --pre=python3 TODO src",
            "sort -o results.txt input.txt",
            "uniq input.txt output.txt",
            "git -C other-repo push",
            "git remote set-url origin https://example.invalid/repo",
            "git branch --force main older-commit",
            "git log --output=history.txt",
            "env -i rm generated.txt",
            "env MODE=test git push",
        ] {
            assert!(assess_shell_command(command).requires_confirmation(), "{command}");
        }
        assert_ne!(assess_shell_command("git branch new-branch").risk, CommandRisk::ReadOnly);
    }
}
