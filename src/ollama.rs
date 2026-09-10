use std::process::Command;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone)]
pub struct OllamaStatus {
    pub reachable: bool,
    pub has_model: bool,
    pub raw_output: String,
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

pub fn ensure_available(model: &str, ollama_host: &str) -> Result<()> {
    let status = check_status(model, ollama_host)?;
    if !status.reachable {
        bail!(
            "Cannot reach Ollama daemon. Is it running? Raw output: {}",
            status.raw_output
        );
    }
    if !status.has_model {
        bail!("Model '{model}' not found locally. Pull it with: ollama pull \"{model}\"");
    }
    Ok(())
}
