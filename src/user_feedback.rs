//! Tier 4: User feedback and learning.
//!
//! When all automated tiers fail, ask the user for clarification and learn
//! from their response to improve future classifications.

use crate::tools::TOOLS;

/// Generate user feedback prompt with tool suggestions.
pub fn generate_feedback_prompt(input: &str) -> String {
    let mut prompt = format!(
        "I'm not sure what you want to do with: \"{}\"\n\n\
         Did you mean:\n",
        input
    );
    
    // Show top tools as options (first 8 tools)
    for (idx, tool) in TOOLS.iter().take(8).enumerate() {
        prompt.push_str(&format!(
            "  [{}] {} - {}\n",
            idx + 1,
            tool.name,
            tool.description
        ));
    }
    
    prompt.push_str("\nOptions:\n");
    prompt.push_str("  • Type a number to select a tool\n");
    prompt.push_str("  • Describe what you want in plain English (I'll generate the command)\n");
    prompt.push_str("    Example: \"stage and commit only, no push\"\n");
    prompt.push_str("    Example: \"deploy to staging server\"\n");
    prompt.push_str("  • Type 'cmd: <commands>' for explicit shell commands\n");
    prompt.push_str("    Example: cmd: git add -A && git commit -m 'update'\n");
    prompt.push_str("  • Type 'none' to skip\n");
    prompt.push_str("\nI'll remember your choice for next time.");
    
    prompt
}

/// Feedback response type.
pub enum FeedbackResponse {
    ToolSelection(usize),
    ExplicitCommand(String),
    NaturalLanguageDescription(String),
    None,
    Invalid,
}

/// Parse user's feedback response.
pub fn parse_feedback_response(response: &str) -> FeedbackResponse {
    let trimmed = response.trim();
    
    // Check for "none"
    if trimmed.to_lowercase() == "none" {
        return FeedbackResponse::None;
    }
    
    // Check for explicit command definition (starts with "cmd:" or legacy "macro:")
    let explicit_cmd = trimmed.strip_prefix("cmd:").or_else(|| trimmed.strip_prefix("cmd "))
        .or_else(|| trimmed.strip_prefix("macro:"))
        .or_else(|| trimmed.strip_prefix("macro "));
    
    if let Some(cmd_text) = explicit_cmd {
        let cmd = cmd_text.trim();
        if !cmd.is_empty() {
            return FeedbackResponse::ExplicitCommand(cmd.to_string());
        }
    }
    
    // Try to parse as number
    if let Ok(num) = trimmed.parse::<usize>() {
        if num > 0 && num <= TOOLS.len() {
            return FeedbackResponse::ToolSelection(num - 1); // Convert to 0-based index
        }
    }
    
    // If it's not empty and not a number, treat as natural language description
    if !trimmed.is_empty() {
        return FeedbackResponse::NaturalLanguageDescription(trimmed.to_string());
    }
    
    FeedbackResponse::Invalid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_number() {
        match parse_feedback_response("1") {
            FeedbackResponse::ToolSelection(0) => {},
            _ => panic!("Expected ToolSelection(0)"),
        }
        match parse_feedback_response("5") {
            FeedbackResponse::ToolSelection(4) => {},
            _ => panic!("Expected ToolSelection(4)"),
        }
    }

    #[test]
    fn test_parse_none() {
        match parse_feedback_response("none") {
            FeedbackResponse::None => {},
            _ => panic!("Expected None"),
        }
    }

    #[test]
    fn test_parse_explicit_command() {
        match parse_feedback_response("cmd: git status") {
            FeedbackResponse::ExplicitCommand(cmd) => assert_eq!(cmd, "git status"),
            _ => panic!("Expected ExplicitCommand"),
        }
        match parse_feedback_response("cmd git add -A") {
            FeedbackResponse::ExplicitCommand(cmd) => assert_eq!(cmd, "git add -A"),
            _ => panic!("Expected ExplicitCommand"),
        }
        // Test legacy "macro:" prefix still works
        match parse_feedback_response("macro: git status") {
            FeedbackResponse::ExplicitCommand(cmd) => assert_eq!(cmd, "git status"),
            _ => panic!("Expected ExplicitCommand for legacy macro:"),
        }
    }

    #[test]
    fn test_parse_natural_language() {
        match parse_feedback_response("stage and commit only") {
            FeedbackResponse::NaturalLanguageDescription(desc) => {
                assert_eq!(desc, "stage and commit only")
            }
            _ => panic!("Expected NaturalLanguageDescription"),
        }
        match parse_feedback_response("deploy to staging") {
            FeedbackResponse::NaturalLanguageDescription(desc) => {
                assert_eq!(desc, "deploy to staging")
            }
            _ => panic!("Expected NaturalLanguageDescription"),
        }
    }

    #[test]
    fn test_parse_invalid() {
        // Invalid number (out of range) -> treated as natural language
        match parse_feedback_response("0") {
            FeedbackResponse::NaturalLanguageDescription(_) => {},
            _ => panic!("Expected NaturalLanguageDescription for '0'"),
        }
        match parse_feedback_response("999") {
            FeedbackResponse::NaturalLanguageDescription(_) => {},
            _ => panic!("Expected NaturalLanguageDescription for '999'"),
        }
        // Empty string is invalid
        match parse_feedback_response("") {
            FeedbackResponse::Invalid => {},
            _ => panic!("Expected Invalid for empty string"),
        }
    }

    #[test]
    fn test_generate_prompt() {
        let prompt = generate_feedback_prompt("do something");
        assert!(prompt.contains("I'm not sure"));
        assert!(prompt.contains("do something"));
        assert!(prompt.contains("[1]"));
        assert!(prompt.contains("cmd:"));
    }
}

