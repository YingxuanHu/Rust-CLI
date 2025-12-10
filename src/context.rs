//! Semantic context tracking for reference resolution.
//!
//! Tracks recent command outputs (diffs, files, etc.) and detects when user input
//! contains references ("it", "that", "the diff") to inject context into LLM prompts.

use std::collections::VecDeque;

/// A recent command output that can be referenced in conversation
#[derive(Debug, Clone)]
pub struct RecentOutput {
    pub kind: &'static str,  // "diff", "file", "status", "command", "commit_msg", "todos"
    pub summary: String,     // Brief description for context
    #[allow(dead_code)]      // Stored for potential detailed context expansion
    pub content: String,     // Full output (truncated if needed)
}

/// Reference words that indicate user is referring to recent context
const REFERENCE_WORDS: &[&str] = &[
    "it",
    "that",
    "this",
    "them",
    "those",
    "the diff",
    "the changes",
    "the file",
    "the output",
    "the status",
    "the result",
    "the command",
    "the message",
    "these changes",
    "those files",
];

/// Check if user input contains reference words
pub fn contains_reference(input: &str) -> bool {
    let lower = input.to_lowercase();
    REFERENCE_WORDS.iter().any(|r| lower.contains(r))
}

/// Format recent outputs as context for LLM prompt
pub fn format_context_for_prompt(outputs: &VecDeque<RecentOutput>) -> String {
    if outputs.is_empty() {
        return String::new();
    }

    let mut ctx = String::from("\n\n[Recent context for reference resolution]\n");
    for (i, out) in outputs.iter().enumerate() {
        ctx.push_str(&format!("{}. [{}] {}\n", i + 1, out.kind, out.summary));
    }
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_reference() {
        assert!(contains_reference("commit it"));
        assert!(contains_reference("show me that"));
        assert!(contains_reference("what does the diff say"));
        assert!(!contains_reference("show status"));
        assert!(!contains_reference("run tests"));
    }

    #[test]
    fn test_format_context() {
        let mut outputs = VecDeque::new();
        outputs.push_back(RecentOutput {
            kind: "diff",
            summary: "3 files changed".to_string(),
            content: "...".to_string(),
        });
        outputs.push_back(RecentOutput {
            kind: "file",
            summary: "src/main.rs (150 lines)".to_string(),
            content: "...".to_string(),
        });

        let ctx = format_context_for_prompt(&outputs);
        assert!(ctx.contains("[Recent context"));
        assert!(ctx.contains("[diff]"));
        assert!(ctx.contains("[file]"));
    }
}

