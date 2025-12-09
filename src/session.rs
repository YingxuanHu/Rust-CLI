use std::path::PathBuf;

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
