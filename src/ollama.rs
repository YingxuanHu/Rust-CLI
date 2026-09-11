use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct OllamaStatus {
    pub reachable: bool,
    pub has_model: bool,
    pub raw_output: String,
}

/// Whether the Ollama executable can be launched from the current PATH.
/// This deliberately does not require the daemon to be running.
pub fn command_available() -> bool {
    Command::new("ollama")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

pub fn check_status(model: &str, ollama_host: &str) -> Result<OllamaStatus> {
    let output = Command::new("ollama")
        .arg("list")
        .env("OLLAMA_HOST", ollama_host)
        .output()
        .context("invoking ollama list")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Ok(OllamaStatus {
            reachable: false,
            has_model: false,
            raw_output: stderr,
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let mut has_model = false;
    let target_base = model.split(':').next().unwrap_or(model);

    for line in stdout.lines().skip(1) {
        let name = line.split_whitespace().next().unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let base = name.split(':').next().unwrap_or(name);
        if name == model || base == target_base {
            has_model = true;
            break;
        }
    }

    Ok(OllamaStatus {
        reachable: true,
        has_model,
        raw_output: stdout,
    })
}

/// Download a model through Ollama while preserving its progress output for
/// the person running the bootstrap command. Model names are command
/// arguments, never shell input.
pub fn pull_model(model: &str, ollama_host: &str) -> Result<()> {
    let status = Command::new("ollama")
        .arg("pull")
        .arg(model)
        .env("OLLAMA_HOST", ollama_host)
        .status()
        .with_context(|| format!("starting download for model '{model}'"))?;
    if !status.success() {
        bail!("Ollama could not download model '{model}' (exit status {status})");
    }
    Ok(())
}
