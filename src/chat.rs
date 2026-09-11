//! Shared prompt construction for interactive and one-shot chat.

use crate::repo::{ProjectType, RepoInfo};

/// Build a lightweight project description without reading source files.
pub fn project_context(repo_info: Option<&RepoInfo>) -> String {
    let Some(info) = repo_info else {
        return String::new();
    };

    let kind = match &info.project_type {
        ProjectType::Rust => "Rust",
        ProjectType::Node => "Node.js",
        ProjectType::Python => "Python",
        ProjectType::Go => "Go",
        ProjectType::Unknown => "Unknown",
    };
    let name = info.name.as_deref().unwrap_or("unnamed");
    format!("\n\nCurrent project: {name} ({kind})")
}

/// Build a prompt within an approximate token budget without splitting UTF-8
/// text. Ollama's tokenizer is model-specific, so four characters per token is
/// intentionally a conservative approximation rather than a false exactness.
pub fn compose_prompt(
    system_prompt: &str,
    repo_context: &str,
    recent_context: &str,
    user_input: &str,
    max_context_tokens: u32,
) -> String {
    const PROMPT_OVERHEAD: usize = "\n\nUser: \nAssistant:".len();
    let max_chars = (max_context_tokens as usize).saturating_mul(4).max(64);
    let content_budget = max_chars.saturating_sub(PROMPT_OVERHEAD);

    let system_budget = content_budget / 4;
    let repo_budget = content_budget / 8;
    let user_budget = content_budget / 2;
    let recent_budget = content_budget
        .saturating_sub(system_budget)
        .saturating_sub(repo_budget)
        .saturating_sub(user_budget);

    format!(
        "{}{}{}\n\nUser: {}\nAssistant:",
        truncate_to_chars(system_prompt, system_budget),
        truncate_to_chars(repo_context, repo_budget),
        truncate_to_chars(recent_context, recent_budget),
        truncate_to_chars(user_input, user_budget),
    )
}

fn truncate_to_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    const MARKER: &str = "…[truncated]";
    if max_chars <= MARKER.chars().count() {
        return text.chars().take(max_chars).collect();
    }

    let prefix: String = text
        .chars()
        .take(max_chars - MARKER.chars().count())
        .collect();
    format!("{prefix}{MARKER}")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::repo::{ProjectType, RepoInfo};

    #[test]
    fn prompt_budget_preserves_the_current_user_input_and_utf8_boundaries() {
        let prompt = super::compose_prompt(
            "System instructions that are intentionally long.",
            " repository context",
            " recent context",
            "explain 🦀 safely",
            16,
        );

        assert!(prompt.contains("User: explain 🦀 safely"));
        assert!(prompt.chars().count() <= 64);
        assert!(std::str::from_utf8(prompt.as_bytes()).is_ok());
    }

    #[test]
    fn project_context_is_small_and_descriptive() {
        let project = RepoInfo {
            project_type: ProjectType::Rust,
            root: PathBuf::from("/project"),
            name: Some("demo".to_string()),
            source_dirs: Vec::new(),
        };

        assert_eq!(
            super::project_context(Some(&project)),
            "\n\nCurrent project: demo (Rust)"
        );
        assert!(super::project_context(None).is_empty());
    }
}
