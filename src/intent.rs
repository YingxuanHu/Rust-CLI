//! Intent parsing using a tiered resolution system.
//!
//! Tier 1: Fuzzy matching (< 1ms)
//! Tier 2: Keyword + embedding hybrid (~50ms)
//! Tier 3: Small LLM classifier (~500ms) → fallback to "chat"

use anyhow::Result;

use crate::{
    embedding::EmbeddingCache,
    fuzzy,
    keyword_classifier::KeywordClassifier,
    learned::LearnedAliases,
    llm_classifier,
    tools::ToolArgs,
};

/// The result of parsing user intent.
#[derive(Debug, Clone)]
pub struct ParsedIntent {
    pub tool: String,
    pub args: ToolArgs,
    pub confidence: f32,
}

impl ParsedIntent {
    pub fn new(tool: &str, confidence: f32) -> Self {
        Self {
            tool: tool.to_string(),
            args: ToolArgs::default(),
            confidence,
        }
    }
}

/// Main entry point: resolve intent through all 3 tiers.
pub async fn resolve_intent(
    input: &str,
    cache: &EmbeddingCache,
    learned: &LearnedAliases,
    llm_model: &str,
    request_timeout_secs: u64,
    ollama_host: &str,
) -> Result<ParsedIntent> {
    tracing::debug!("[Intent Resolution] Input: '{}'", input);
    
    // Tier 1: Fuzzy matching + learned aliases (< 1ms)
    if let Some(mut intent) = fuzzy::fuzzy_match(input, learned) {
        tracing::debug!("[Intent Resolution] ✓ Tier 1 (Fuzzy): {}", intent.tool);
        let extracted = extract_args_from_input(input, &intent.tool);
        intent.args.merge_missing(extracted);
        return Ok(intent);
    }
    
    // Tier 2: Keyword + embedding classifier (~50ms)
    if let Some(mut intent) = KeywordClassifier::classify(input, cache).await? {
        tracing::debug!("[Intent Resolution] ✓ Tier 2 (Keyword/Embedding): {}", intent.tool);
        let extracted = extract_args_from_input(input, &intent.tool);
        intent.args.merge_missing(extracted);
        return Ok(intent);
    }
    
    // Tier 3: Small LLM classifier (~500ms)
    // This can return "chat" if user is just chatting, or a tool name if they're trying to do something
    tracing::debug!("[Intent Resolution] Attempting Tier 3 (LLM) with model: '{}'", llm_model);
    if let Some(mut intent) = llm_classifier::classify_with_llm(
        input,
        llm_model,
        request_timeout_secs,
        ollama_host,
    )
    .await?
    {
        let extracted = extract_args_from_input(input, &intent.tool);
        intent.args.merge_missing(extracted);
        
        // If LLM classified as "chat", return it directly
        if intent.tool == "chat" {
            tracing::debug!("[Intent Resolution] ✓ Tier 3 (LLM): chat");
            return Ok(intent);
        }
        
        // If LLM found a tool match, return it
        tracing::debug!("[Intent Resolution] ✓ Tier 3 (LLM): {}", intent.tool);
        return Ok(intent);
    }
    
    // Fallback: Default to chat when LLM couldn't classify
    // This means the input is likely conversational rather than an action intent
    tracing::debug!("[Intent Resolution] → Fallback to chat");
    Ok(ParsedIntent {
        tool: "chat".to_string(),
        args: ToolArgs::default(),
        confidence: 0.5,
    })
}

/// Extract arguments from user input based on the matched tool.
fn extract_args_from_input(input: &str, tool: &str) -> ToolArgs {
    let mut args = ToolArgs::default();
    
    match tool {
        "show_file" => {
            let lower = input.to_lowercase();
            for prefix in ["show file ", "read file ", "open file ", "show ", "read ", "open "] {
                if lower.starts_with(prefix) {
                    let path = input[prefix.len()..].trim();
                    if !path.is_empty() {
                        args.path = Some(path.to_string());
                        break;
                    }
                }
            }
        }
        "stage" => {
            let lower = input.to_lowercase();
            if lower.starts_with("stage ") && !lower.starts_with("stage all") {
                let path = input[6..].trim();
                if !path.is_empty() {
                    args.path = Some(path.to_string());
                }
            } else if lower.starts_with("git add ") && !lower.contains("-a") && !lower.contains("-A") {
                let path = input[8..].trim();
                if !path.is_empty() {
                    args.path = Some(path.to_string());
                }
            }
        }
        "shell" => {
            let trimmed = input.trim();
            if let Some(cmd) = trimmed.strip_prefix('$').or_else(|| trimmed.strip_prefix('!')) {
                args.command = Some(cmd.trim().to_string());
            }
        }
        "list_files" => {
            let lower = input.to_lowercase();
            for prefix in ["list files in ", "show files in ", "ls ", "dir "] {
                if lower.starts_with(prefix) {
                    let path = input[prefix.len()..].trim();
                    if !path.is_empty() {
                        args.path = Some(path.to_string());
                        break;
                    }
                }
            }
        }
        "write_file" => {
            let lower = input.to_lowercase();
            
            for prefix in ["write to ", "save to ", "create ", "write file "] {
                if lower.starts_with(prefix) {
                    let rest = input[prefix.len()..].trim();
                    if let Some(space_idx) = rest.find(char::is_whitespace) {
                        args.path = Some(rest[..space_idx].to_string());
                    } else {
                        args.path = Some(rest.to_string());
                    }
                    break;
                }
            }
            
            if let Some(content_idx) = lower.find("with content:") {
                let content = input[content_idx + 13..].trim();
                if !content.is_empty() {
                    args.content = Some(content.to_string());
                }
            }
        }
        _ => {}
    }
    
    args
}

/// Quick match for shell commands only (special syntax that needs parsing).
/// This bypasses the tiered system for explicit shell command syntax.
pub fn quick_match(input: &str) -> Option<ParsedIntent> {
    let trimmed = input.trim();

    // Shell command prefix ($ or !) - needs special syntax parsing
    if trimmed.starts_with('$') || (trimmed.starts_with('!') && trimmed != "!!") {
        let cmd = trimmed[1..].trim();
        return Some(ParsedIntent {
            tool: "shell".to_string(),
            args: ToolArgs {
                command: Some(cmd.to_string()),
                ..Default::default()
            },
            confidence: 1.0,
        });
    }

    // Bang shortcuts (!!)
    if trimmed == "!!" {
        return Some(ParsedIntent {
            tool: "shell_repeat".to_string(),
            args: ToolArgs::default(),
            confidence: 1.0,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quick_match_shell() {
        let intent = quick_match("$ ls -la").unwrap();
        assert_eq!(intent.tool, "shell");
        assert_eq!(intent.args.command, Some("ls -la".to_string()));
    }

    #[test]
    fn test_quick_match_bang_repeat() {
        let intent = quick_match("!!").unwrap();
        assert_eq!(intent.tool, "shell_repeat");
    }

    #[test]
    fn test_extract_args_show_file() {
        let args = extract_args_from_input("show file src/main.rs", "show_file");
        assert_eq!(args.path, Some("src/main.rs".to_string()));
    }

    #[test]
    fn test_extract_args_stage() {
        let args = extract_args_from_input("stage src/lib.rs", "stage");
        assert_eq!(args.path, Some("src/lib.rs".to_string()));
    }
}
