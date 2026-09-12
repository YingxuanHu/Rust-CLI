//! Read-only, one-shot chat for shells and scripts.

use std::{
    io::{self, Write},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tokio::process::Command;

use crate::{chat, config::Config, repo::RepoInfo};

#[derive(Serialize)]
struct AskResponse<'a> {
    model: &'a str,
    response: &'a str,
}

/// Join clap's positional prompt fragments while rejecting accidental empty
/// requests before contacting the model.
pub fn join_prompt(parts: &[String]) -> Result<String> {
    let prompt = parts.join(" ");
    if prompt.trim().is_empty() {
        bail!("provide a question, for example: `llm_cli ask \"explain Rust ownership\"`");
    }
    Ok(prompt)
}

/// Ask the configured model one question without launching the terminal UI.
/// This path is intentionally chat-only: it cannot execute tools or workflows.
pub async fn run(config: &Config, prompt: &str, json: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("reading current directory")?;
    let repo_context = chat::project_context(RepoInfo::detect(&cwd).as_ref());
    let composed_prompt = chat::compose_prompt(
        &config.system_prompt,
        &repo_context,
        "",
        prompt,
        config.max_context_tokens,
    );

    let mut command = Command::new("ollama");
    command.arg("run")
        .arg(&config.model)
        .arg(composed_prompt)
        .env("OLLAMA_HOST", &config.ollama_host);
    let response = crate::model_stream::run(
        command,
        Duration::from_secs(config.llm_timeout_secs),
        |chunk| {
            if !json {
                let mut output = io::stdout().lock();
                output
                    .write_all(chunk.as_bytes())
                    .context("writing streamed response")?;
                output.flush().context("flushing streamed response")?;
            }
            Ok(())
        }
    ).await?;

    if json {
        println!("{}", render_json(&config.model, &response)?);
    } else if !response.ends_with('\n') {
        println!();
    }
    Ok(())
}

fn render_json(model: &str, response: &str) -> Result<String> {
    serde_json::to_string(&AskResponse {
        model,
        response: response.trim_end(),
    })
    .context("serializing JSON response")
}

#[cfg(test)]
mod tests {
    #[test]
    fn joins_a_multi_word_prompt() {
        let parts = vec!["explain".to_string(), "this project".to_string()];
        assert_eq!(super::join_prompt(&parts).unwrap(), "explain this project");
    }

    #[test]
    fn rejects_an_empty_prompt() {
        assert!(super::join_prompt(&[]).is_err());
        assert!(super::join_prompt(&["   ".to_string()]).is_err());
    }

    #[test]
    fn json_output_is_one_valid_object() {
        let output = super::render_json("test-model", "Hello\n").unwrap();
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["model"], "test-model");
        assert_eq!(value["response"], "Hello");
    }
}
