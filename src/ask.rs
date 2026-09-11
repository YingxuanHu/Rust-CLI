//! Read-only, one-shot chat for shells and scripts.

use std::{
    io::{self, Write},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use tokio::{io::AsyncReadExt, process::Command, time::timeout};

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
        bail!("provide a question, for example: `llm_cli ask \"explain this project\"`");
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

    let mut child = Command::new("ollama")
        .arg("run")
        .arg(&config.model)
        .arg(composed_prompt)
        .env("OLLAMA_HOST", &config.ollama_host)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .context("starting Ollama chat")?;
    let mut stdout = child.stdout.take().context("capturing Ollama response")?;
    let mut response = String::new();

    let read_response = async {
        let mut buffer = [0_u8; 1024];
        loop {
            let count = stdout.read(&mut buffer).await.context("reading Ollama response")?;
            if count == 0 {
                break;
            }
            let chunk = String::from_utf8_lossy(&buffer[..count]);
            if !json {
                let mut output = io::stdout().lock();
                output
                    .write_all(chunk.as_bytes())
                    .context("writing streamed response")?;
                output.flush().context("flushing streamed response")?;
            }
            response.push_str(&chunk);
        }
        Ok::<(), anyhow::Error>(())
    };

    match timeout(Duration::from_secs(config.llm_timeout_secs), read_response).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            bail!("Ollama chat timed out after {} seconds", config.llm_timeout_secs);
        }
    }

    let status = child.wait().await.context("waiting for Ollama chat")?;
    if !status.success() {
        bail!("Ollama chat exited with {status}");
    }

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
