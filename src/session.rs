use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use crate::context::RecentOutput;
use crate::repo::RepoInfo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct SessionState {
    pub cwd: PathBuf,
    pub repo_root: Option<PathBuf>,
    pub repo_info: Option<RepoInfo>,
    pub history: Vec<Message>,
    pub recent_outputs: VecDeque<RecentOutput>,
}

impl SessionState {
    pub fn new() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let repo_root = find_git_root(&cwd);
        let repo_info = RepoInfo::detect(&cwd);
        Self {
            cwd,
            repo_root,
            repo_info,
            history: Vec::new(),
            recent_outputs: VecDeque::new(),
        }
    }

    pub fn record(&mut self, message: Message) {
        self.history.push(message);
    }

    /// Update all location-dependent session metadata after an in-app `cd`.
    pub fn set_cwd(&mut self, cwd: PathBuf) {
        if self.cwd != cwd {
            self.recent_outputs.clear();
        }
        self.repo_root = find_git_root(&cwd);
        self.repo_info = RepoInfo::detect(&cwd);
        self.cwd = cwd;
    }

    /// Record a command output for semantic reference resolution
    pub fn record_output(&mut self, kind: &'static str, summary: &str, content: &str) {
        const MAX_OUTPUTS: usize = 5;
        const MAX_CONTENT: usize = 2000;

        let truncated = truncate_to_bytes(content, MAX_CONTENT);

        self.recent_outputs.push_front(RecentOutput {
            kind,
            summary: summary.to_string(),
            content: truncated,
        });

        while self.recent_outputs.len() > MAX_OUTPUTS {
            self.recent_outputs.pop_back();
        }
    }
}

fn truncate_to_bytes(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }

    // Keep the opening command context and final diagnostics, where test and
    // build tools usually explain a failure. The marker shares the same limit.
    const MARKER: &str = "\n...[output truncated]...\n";
    if max_bytes < MARKER.len() {
        return MARKER[..max_bytes].to_owned();
    }
    let available = max_bytes - MARKER.len();
    let mut head = available.div_ceil(2);
    while !content.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = content.len() - available / 2;
    while !content.is_char_boundary(tail) {
        tail += 1;
    }
    format!("{}{MARKER}{}", &content[..head], &content[tail..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changing_directory_clears_output_context_from_the_previous_project() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = SessionState::new();
        session.record_output("tests", "old suite failed", "old project diagnostic");
        session.set_cwd(directory.path().to_path_buf());
        assert!(session.recent_outputs.is_empty());
        session.record_output("tests", "current suite passed", "current diagnostic");
        session.set_cwd(directory.path().to_path_buf());
        assert_eq!(session.recent_outputs.len(), 1);
    }

    #[test]
    fn output_truncation_preserves_utf8_boundaries() {
        let content = "🦀".repeat(1_000);
        let truncated = truncate_to_bytes(&content, 2_000);
        assert!(truncated.contains("...[output truncated]..."));
        assert!(truncated.len() <= 2_000);
        assert!(truncated.starts_with("🦀"));
        assert!(truncated.ends_with("🦀"));
        assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());
    }

    #[test]
    fn recording_large_output_keeps_the_start_and_final_failure() {
        let mut session = SessionState::new();
        let content = format!(
            "running 500 tests\n{}\nFAILURE: expected 42 but received 0",
            "test parser::roundtrip ... ok\n".repeat(500),
        );
        session.record_output("tests", "cargo test failed", &content);
        let stored = &session.recent_outputs.front().unwrap().content;
        assert!(stored.starts_with("running 500 tests"));
        assert!(stored.ends_with("FAILURE: expected 42 but received 0"));
        assert!(stored.contains("...[output truncated]..."));
        assert!(stored.len() <= 2_000);
    }

    #[test]
    fn small_capture_budgets_are_strict_and_utf8_safe() {
        let content = "ASCII🦀é漢字".repeat(100);
        for budget in 0..256 {
            let truncated = truncate_to_bytes(&content, budget);
            assert!(truncated.len() <= budget);
            assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());
        }
        assert_eq!(truncate_to_bytes("short🦀", 2_000), "short🦀");
        assert_eq!(truncate_to_bytes("", 0), "");
    }

    #[test]
    fn recording_outputs_keeps_only_the_five_newest() {
        let mut session = SessionState::new();
        for index in 0..10 {
            session.record_output("tests", &format!("run {index}"), &format!("result {index}"));
        }
        assert_eq!(session.recent_outputs.len(), 5);
        assert_eq!(session.recent_outputs.front().unwrap().content, "result 9");
        assert_eq!(session.recent_outputs.back().unwrap().content, "result 5");
    }
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    while let Some(parent) = current.parent() {
        if current.join(".git").exists() {
            return Some(current.to_path_buf());
        }
        current = parent;
    }
    None
}
