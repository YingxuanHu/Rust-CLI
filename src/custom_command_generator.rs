//! LLM-assisted custom command generation.
//!
//! Converts natural language descriptions into executable shell commands
//! using a local LLM. Supports composable handlers like {{GEN_COMMIT_MSG}}.

use anyhow::{Context, Result};
use std::path::Path;

use crate::config::Config;
use crate::workflow::generate_commit_message;

/// Generate a shell command from natural language description using LLM.
#[allow(dead_code)]
pub async fn generate_custom_command(
    description: &str,
    model: &str,
    repo_context: Option<&str>,
) -> Result<String> {
    let context_info = if let Some(ctx) = repo_context {
        format!("\n\nCurrent repository context: {}", ctx)
    } else {
        String::new()
    };
    
    let prompt = format!(
        "You are a shell command expert. Convert the user's natural language description into a valid shell command.\n\
         \n\
         Rules:\n\
         - Output ONLY the shell command, nothing else\n\
         - Use common git workflows when appropriate\n\
         - Use && to chain commands\n\
         - Be safe (avoid destructive commands without confirmation)\n\
         - Use standard tools: git, cargo, npm, docker, etc.\n\
         - IMPORTANT: Commands will run non-interactively. Use non-interactive flags:\n\
           * For git commit WITH generated message: use '{{{{GEN_COMMIT_MSG}}}}' placeholder\n\
           * For git commit with simple message: use 'git commit -m \"message\"'\n\
           * For commands that need user input: include appropriate flags\n\
         {}\n\
         \n\
         Examples:\n\
         User: \"stage and commit with generated message\"\n\
         Command: git add -A && {{{{GEN_COMMIT_MSG}}}} && echo \"Committed!\"\n\
         \n\
         User: \"stage and commit only, no push\"\n\
         Command: git add -A && git diff --cached --stat && git commit -m \"chore: staged changes\"\n\
         \n\
         User: \"deploy to staging server\"\n\
         Command: ssh staging 'cd /app && git pull && systemctl restart app'\n\
         \n\
         User: \"run tests then build\"\n\
         Command: cargo test && cargo build --release\n\
         \n\
         User: \"backup database with timestamp\"\n\
         Command: pg_dump mydb > backup_$(date +%Y%m%d_%H%M%S).sql\n\
         \n\
         User description: \"{}\"\n\
         Shell command:",
        context_info,
        description
    );
    
    // Call Ollama
    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:11434/api/generate")
        .json(&serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "temperature": 0.3,
                "num_predict": 200,
            }
        }))
        .send()
        .await
        .context("calling Ollama API for macro generation")?;
    
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Ollama API returned {}: {}", status, body);
    }
    
    let result: serde_json::Value = response.json().await?;
    let generated_cmd = result["response"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();
    
    // Clean up the response (remove any explanatory text)
    let cleaned = clean_generated_command(&generated_cmd);
    
    if cleaned.is_empty() {
        anyhow::bail!("LLM generated empty command");
    }
    
    Ok(cleaned)
}

/// Clean up LLM-generated command (remove markdown, explanations, etc.)
#[allow(dead_code)]
fn clean_generated_command(cmd: &str) -> String {
    let mut lines: Vec<&str> = cmd.lines().collect();
    
    // Remove markdown code blocks
    if lines.first().map_or(false, |l| l.starts_with("```")) {
        lines.remove(0);
    }
    if lines.last().map_or(false, |l| l.starts_with("```")) {
        lines.pop();
    }
    
    // Find the actual command line (skip explanatory text)
    for line in &lines {
        let trimmed = line.trim();
        // Skip empty lines and lines that look like explanations
        if trimmed.is_empty() 
            || trimmed.starts_with('#') 
            || trimmed.starts_with("//")
            || trimmed.to_lowercase().starts_with("note:")
            || trimmed.to_lowercase().starts_with("explanation:")
            || trimmed.to_lowercase().starts_with("this command")
        {
            continue;
        }
        
        // Found the command
        return trimmed.to_string();
    }
    
    // Fallback: just take the first non-empty line
    lines.iter()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Expand composable handlers in a command string.
/// 
/// Supported placeholders:
/// - {{GEN_COMMIT_MSG}} - Generate commit message from staged changes
pub fn expand_command_handlers(
    command: &str,
    config: &Config,
    repo_root: &Path,
) -> Result<String> {
    let mut expanded = command.to_string();
    
    // Handle {{GEN_COMMIT_MSG}} placeholder
    if expanded.contains("{{GEN_COMMIT_MSG}}") {
        let commit_msg = generate_commit_message(config, repo_root)
            .unwrap_or_else(|| "chore: update".to_string());
        
        // Escape quotes in commit message
        let escaped_msg = commit_msg.replace('"', "\\\"");
        
        // Split into subject and body
        let lines: Vec<&str> = escaped_msg.lines().collect();
        let subject = lines.first().copied().unwrap_or("chore: update");
        let body_lines: Vec<&str> = lines.iter().skip(1).copied().collect();
        
        // Build git commit command with proper multi-line message
        let mut commit_cmd = format!("git commit -m \"{}\"", subject);
        for line in body_lines {
            if !line.trim().is_empty() {
                commit_cmd.push_str(&format!(" -m \"{}\"", line.replace('"', "\\\"")));
            }
        }
        
        expanded = expanded.replace("{{GEN_COMMIT_MSG}}", &commit_cmd);
    }
    
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_simple_command() {
        let cmd = "git add -A && git commit";
        assert_eq!(clean_generated_command(cmd), "git add -A && git commit");
    }

    #[test]
    fn test_clean_with_markdown() {
        let cmd = "```bash\ngit add -A && git commit\n```";
        assert_eq!(clean_generated_command(cmd), "git add -A && git commit");
    }

    #[test]
    fn test_clean_with_explanation() {
        let cmd = "# This stages and commits\ngit add -A && git commit\nNote: This won't push";
        assert_eq!(clean_generated_command(cmd), "git add -A && git commit");
    }

    #[test]
    fn test_clean_with_comment() {
        let cmd = "git status  # Check status first";
        assert_eq!(clean_generated_command(cmd), "git status  # Check status first");
    }
}

