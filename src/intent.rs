//! Intent parsing for the agentic workflow.
//!
//! Uses embedding-based semantic matching to determine user intent,
//! with quick heuristic matching for obvious cases.

use anyhow::Result;

use crate::embedding::EmbeddingCache;
use crate::tools::ToolArgs;

/// The result of parsing user intent.
#[derive(Debug, Clone)]
pub struct ParsedIntent {
    pub tool: String,
    pub args: ToolArgs,
    pub confidence: f32,
}

/// Parse user intent using embedding similarity.
///
/// Returns the parsed intent or falls back to "chat" if no good match.
pub async fn parse_intent_with_embeddings(
    cache: &EmbeddingCache,
    user_input: &str,
) -> Result<ParsedIntent> {
    // Try embedding-based matching
    if let Some(embedding_match) = cache.find_match(user_input).await? {
        let args = extract_args_from_input(user_input, &embedding_match.tool);
        return Ok(ParsedIntent {
            tool: embedding_match.tool,
            args,
            confidence: embedding_match.similarity,
        });
    }

    // Fall back to chat
    Ok(ParsedIntent {
        tool: "chat".to_string(),
        args: ToolArgs {
            query: Some(user_input.to_string()),
            ..Default::default()
        },
        confidence: 0.3,
    })
}

/// Extract arguments from user input based on the matched tool.
fn extract_args_from_input(input: &str, tool: &str) -> ToolArgs {
    let mut args = ToolArgs::default();

    match tool {
        "show_file" => {
            // Try to extract file path from input
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
            // Try to extract file path for staging
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
            // Extract shell command
            let trimmed = input.trim();
            if let Some(cmd) = trimmed.strip_prefix('$').or_else(|| trimmed.strip_prefix('!')) {
                args.command = Some(cmd.trim().to_string());
            }
        }
        "list_files" => {
            // Try to extract path from list command
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
            // Try to extract path and content from write command
            let lower = input.to_lowercase();
            
            // Extract path
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
            
            // Extract content if "with content:" is present
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
/// Everything else should go through embedding-based semantic matching.
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

    // Everything else goes to embedding-based matching
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
    fn test_quick_match_delegates_to_embeddings() {
        // Non-shell commands should return None and use embedding matching
        assert!(quick_match("save my work").is_none());
        assert!(quick_match("status").is_none());
        assert!(quick_match("explain how rust works").is_none());
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

    #[test]
    fn test_extract_args_list_files() {
        let args = extract_args_from_input("list files in src/", "list_files");
        assert_eq!(args.path, Some("src/".to_string()));

        let args = extract_args_from_input("ls target", "list_files");
        assert_eq!(args.path, Some("target".to_string()));
    }

    #[test]
    fn test_extract_args_write_file() {
        let args = extract_args_from_input("write to test.txt with content: Hello world", "write_file");
        assert_eq!(args.path, Some("test.txt".to_string()));
        assert_eq!(args.content, Some("Hello world".to_string()));

        let args = extract_args_from_input("create main.rs with content: fn main() {}", "write_file");
        assert_eq!(args.path, Some("main.rs".to_string()));
        assert_eq!(args.content, Some("fn main() {}".to_string()));
    }
}

