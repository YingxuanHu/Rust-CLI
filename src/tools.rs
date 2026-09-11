//! Tool catalog for the agentic workflow.
//!
//! Each tool has metadata used by the LLM to understand when to invoke it.

use serde::{Deserialize, Serialize};

/// Metadata describing a tool the agent can invoke.
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: &'static str,
    #[allow(dead_code)] // Used for LLM prompting
    pub description: &'static str,
    pub examples: &'static [&'static str],
    #[allow(dead_code)] // Reserved for future validation
    pub requires_repo: bool,
}

/// Arguments parsed from LLM intent response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolArgs {
    /// File path argument (for stage, show_file, etc.)
    #[serde(default)]
    pub path: Option<String>,
    /// Generic query/message argument
    #[serde(default)]
    pub query: Option<String>,
    /// Shell command to execute
    #[serde(default)]
    pub command: Option<String>,
    /// File content (for write operations)
    #[serde(default)]
    pub content: Option<String>,
}

impl ToolArgs {
    /// Merge another ToolArgs, filling only fields that are currently None.
    pub fn merge_missing(&mut self, other: ToolArgs) {
        if self.path.is_none() {
            self.path = other.path;
        }
        if self.query.is_none() {
            self.query = other.query;
        }
        if self.command.is_none() {
            self.command = other.command;
        }
        if self.content.is_none() {
            self.content = other.content;
        }
    }
}

/// All available tools in the system.
pub static TOOLS: &[Tool] = &[
    Tool {
        name: "getting_started",
        description: "Show a short, contextual guide to the easiest first actions",
        examples: &[
            "getting started",
            "show me around",
            "what should I do first",
            "start",
        ],
        requires_repo: false,
    },
    Tool {
        name: "save_work",
        description: "Stage all changes, generate a commit message, commit, and push to remote",
        examples: &[
            "push my changes",
            "sync with remote",
            "upload code",
            "commit and push",
            "push to github",
            "save to remote",
            "push changes to remote",
            "sync changes",
            "save locally",
            "save local",
            "save changes locally",
        ],
        requires_repo: true,
    },
    Tool {
        name: "stage",
        description: "Stage files for commit using git add",
        examples: &[
            "stage all",
            "stage changes",
            "git add",
            "add all files",
            "stage src/main.rs",
            "add this file",
        ],
        requires_repo: true,
    },
    Tool {
        name: "commit",
        description: "Commit staged changes with a message (without pushing)",
        examples: &[
            "commit",
            "commit changes",
            "commit only",
            "make a commit",
            "commit staged",
            "create commit",
            "commit locally",
            "local commit",
        ],
        requires_repo: true,
    },
    Tool {
        name: "status",
        description: "Show git status and diff summary",
        examples: &[
            "status",
            "git status",
            "show status",
            "what changed",
            "show changes",
            "diff",
            "show diff",
        ],
        requires_repo: true,
    },
    Tool {
        name: "find_todos",
        description: "Search for TODO and FIXME comments in the codebase",
        examples: &[
            "find todos",
            "show todos",
            "list todos",
            "find fixme",
            "search for todos",
            "what needs to be done",
            "todo list",
            "any todos",
        ],
        requires_repo: false,
    },
    Tool {
        name: "run_tests",
        description: "Run the project's test suite using cargo test",
        examples: &[
            "run tests",
            "test",
            "cargo test",
            "run the tests",
            "execute tests",
            "check tests",
        ],
        requires_repo: true,
    },
    Tool {
        name: "show_file",
        description: "Display the contents of a file",
        examples: &[
            "show file",
            "read file",
            "open file",
            "show src/main.rs",
            "show Cargo.toml",
            "show README.md",
            "show docs/guide.md",
            "show scripts/health.sh",
            "cat file",
            "display file",
            "view file",
        ],
        requires_repo: false,
    },
    Tool {
        name: "draft_commit_message",
        description: "Generate a commit message based on staged changes",
        examples: &[
            "draft commit message",
            "generate commit message",
            "suggest commit message",
            "what should I commit",
            "commit message",
        ],
        requires_repo: true,
    },
    Tool {
        name: "shell",
        description: "Execute a shell command in the current directory",
        examples: &[
            "run ls",
            "execute command",
            "shell command",
            "run a command",
        ],
        requires_repo: false,
    },
    Tool {
        name: "list_files",
        description: "List files and directories in a given path",
        examples: &[
            "list directory",
            "ls",
            "dir",
            "list files in src",
            "show files in this directory",
            "what's in src",
            "show directory contents",
        ],
        requires_repo: false,
    },
    Tool {
        name: "write_file",
        description: "Create or overwrite a file with given content (requires confirmation)",
        examples: &[
            "write file",
            "create file",
            "save to file",
            "write to main.rs",
            "create new file",
        ],
        requires_repo: false,
    },
    Tool {
        name: "edit_file",
        description: "Propose a reviewed unified diff for one existing repository file",
        examples: &[
            "edit src/main.rs: add a version flag",
            "edit README.md: document installation",
            "edit Cargo.toml: add package metadata",
        ],
        requires_repo: true,
    },
    Tool {
        name: "rollback_edit",
        description: "Reverse the most recently applied reviewed patch",
        examples: &["rollback last edit", "undo last edit"],
        requires_repo: true,
    },
    Tool {
        name: "build",
        description: "Build the project using the appropriate build system",
        examples: &[
            "build",
            "compile",
            "build project",
            "cargo build",
            "npm run build",
        ],
        requires_repo: true,
    },
    Tool {
        name: "explain_project",
        description: "Show project type, dependencies, and structure",
        examples: &[
            "what project is this",
            "project info",
            "what kind of project",
            "project type",
            "show project info",
        ],
        requires_repo: true,
    },
    Tool {
        name: "chat",
        description: "Have a conversation with the AI assistant about anything",
        examples: &[
            "explain",
            "what is",
            "how do I",
            "help me",
            "tell me about",
            "can you",
            "files related to",
            "where are the settings",
            "which files handle",
            "find files about",
        ],
        requires_repo: false,
    },
    Tool {
        name: "help",
        description: "Show comprehensive help information about the CLI, commands, and key bindings",
        examples: &[
            "help",
            "?",
            "how to use",
            "usage",
            "show help",
            "commands",
            "what can you do",
        ],
        requires_repo: false,
    },
];

/// Build a prompt describing all available tools for the LLM.
#[allow(dead_code)] // Reserved for LLM-based intent parsing fallback
pub fn build_tool_catalog_prompt() -> String {
    let mut prompt = String::from("Available tools:\n\n");
    
    for tool in TOOLS {
        prompt.push_str(&format!("- **{}**: {}\n", tool.name, tool.description));
        prompt.push_str("  Examples: ");
        prompt.push_str(&tool.examples.join(", "));
        prompt.push('\n');
    }
    
    prompt
}

/// Get a tool by name.
#[allow(dead_code)] // Reserved for future validation
pub fn get_tool(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_catalog_not_empty() {
        assert!(!TOOLS.is_empty());
    }

    #[test]
    fn test_get_tool() {
        assert!(get_tool("save_work").is_some());
        assert!(get_tool("nonexistent").is_none());
    }
}
