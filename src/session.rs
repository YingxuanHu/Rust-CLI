use std::path::PathBuf;
use std::collections::VecDeque;

use crate::repo::RepoInfo;
use crate::context::RecentOutput;

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

    /// Change the current working directory and update repo_root if needed.
    pub fn set_cwd(&mut self, new_cwd: PathBuf) {
        self.cwd = new_cwd;
        self.repo_root = find_git_root(&self.cwd);
        self.repo_info = RepoInfo::detect(&self.cwd);
    }

    /// Record a command output for semantic reference resolution
    pub fn record_output(&mut self, kind: &'static str, summary: &str, content: &str) {
        const MAX_OUTPUTS: usize = 5;
        const MAX_CONTENT: usize = 2000;

        let truncated = if content.len() > MAX_CONTENT {
            format!("{}...[truncated]", &content[..MAX_CONTENT])
        } else {
            content.to_string()
        };

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

fn find_git_root(start: &PathBuf) -> Option<PathBuf> {
    let mut current = start.as_path();
    while let Some(parent) = current.parent() {
        if current.join(".git").exists() {
            return Some(current.to_path_buf());
        }
        current = parent;
    }
    None
}
