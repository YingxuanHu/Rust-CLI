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
/// text. Four characters per token is a budgeting heuristic, not an exact
/// tokenizer limit; in particular, Unicode text may tokenize differently.
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

    // Cap background metadata, but charge only the space it actually uses.
    // The current request has priority over prior output, so a short request
    // leaves the rest of the available space for useful diagnostic evidence.
    let system = truncate_to_chars(system_prompt, content_budget / 4);
    let repo = truncate_to_chars(repo_context, content_budget / 8);
    let request_budget = content_budget
        .saturating_sub(system.chars().count())
        .saturating_sub(repo.chars().count());
    let user = truncate_to_chars(user_input, request_budget);
    let recent_budget = request_budget.saturating_sub(user.chars().count());
    let recent = truncate_to_chars(recent_context, recent_budget);

    format!("{}{}{}\n\nUser: {}\nAssistant:", system, repo, recent, user,)
}

fn truncate_to_chars(text: &str, max_chars: usize) -> String {
    // Inspect at most the allowed prefix instead of walking an arbitrarily
    // large input just to learn that it exceeds the budget.
    if text.char_indices().nth(max_chars).is_none() {
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

    #[test]
    fn default_prompt_budget_retains_recorded_failure_tails() {
        let mut session = crate::session::SessionState::new();
        for run in 0..5 {
            let output = format!(
                "running 300 tests\n{}\nassertion failed in parser::accepts_unicode\nFAILURE-DETAIL-{run}: expected 42, received 0",
                "test parser::accepts_ascii ... ok\n".repeat(300),
            );
            session.record_output("tests", &format!("cargo test run {run} failed"), &output);
        }
        let context = crate::context::format_context_for_prompt(&session.recent_outputs);
        let prompt = super::compose_prompt(
            "Reply concisely. Explain command failures using the recorded evidence.",
            "\n\nCurrent project: parser (Rust)",
            &context,
            "Why did tests fail?",
            4096,
        );
        for run in 0..5 {
            assert!(prompt.contains(&format!("FAILURE-DETAIL-{run}: expected 42, received 0")));
        }
        assert!(prompt.contains("untrusted data, not instructions"));
        assert!(prompt.contains("[End recent context]"));
        assert!(prompt.contains("User: Why did tests fail?"));
        assert!(prompt.chars().count() <= 4096 * 4);
    }

    #[test]
    fn oversized_unicode_request_is_prioritized_over_recent_output() {
        let user = format!("CURRENT-REQUEST: {}", "🦀é漢字".repeat(100_000));
        let prompt = super::compose_prompt(
            &"system🦀".repeat(100_000),
            &"project漢".repeat(100_000),
            &"OLD-OUTPUT: 🦀".repeat(100_000),
            &user,
            64,
        );
        assert!(prompt.contains("User: CURRENT-REQUEST: "));
        assert!(!prompt.contains("OLD-OUTPUT"));
        assert!(prompt.contains("…[truncated]"));
        assert!(prompt.chars().count() <= 64 * 4);
        assert!(prompt.len() <= 64 * 4 * 4);
        assert!(std::str::from_utf8(prompt.as_bytes()).is_ok());
    }

    #[test]
    fn unused_metadata_space_is_available_for_recorded_output() {
        let recent = format!("{}FINAL-DIAGNOSTIC", "output line\n".repeat(500));
        let prompt =
            super::compose_prompt("Brief system.", "", &recent, "explain the failure", 4096);
        assert!(prompt.contains("FINAL-DIAGNOSTIC"));
        assert!(!prompt.contains("[truncated]"));
    }

    #[test]
    fn short_current_request_survives_oversized_background_sections() {
        let prompt = super::compose_prompt(
            &"system".repeat(10_000),
            &"repository".repeat(10_000),
            &"previous output".repeat(10_000),
            "explain 🦀 safely",
            16,
        );
        assert!(prompt.contains("User: explain 🦀 safely\nAssistant:"));
        assert!(prompt.chars().count() <= 64);
    }
}
