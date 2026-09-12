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
    let has_model = listed_model_available(&stdout, model);

    Ok(OllamaStatus {
        reachable: true,
        has_model,
        raw_output: stdout,
    })
}

fn listed_model_available(output: &str, model: &str) -> bool {
    let requested = normalize_model_tag(model);
    output.lines().skip(1).any(|line| {
        line.split_whitespace()
            .next()
            .is_some_and(|name| normalize_model_tag(name) == requested)
    })
}

fn normalize_model_tag(model: &str) -> String {
    // A registry address can contain a port; only the final path component
    // determines whether a model tag was supplied.
    if model.rsplit('/').next().unwrap_or(model).contains(':') {
        model.to_string()
    } else {
        format!("{model}:latest")
    }
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

#[cfg(test)]
mod tests {
    use super::listed_model_available;

    #[test]
    fn a_different_model_tag_does_not_satisfy_the_requested_model() {
        let inventory = "NAME ID SIZE MODIFIED\nllama3:8b abc 4GB now\n";
        assert!(listed_model_available(inventory, "llama3:8b"));
        assert!(!listed_model_available(inventory, "llama3:70b"));
        assert!(!listed_model_available(inventory, "llama3"));
        assert!(!listed_model_available(inventory, "llama3:latest"));
    }

    #[test]
    fn an_untagged_model_means_latest() {
        let inventory = "NAME ID SIZE MODIFIED\nllama3:latest abc 4GB now\n";
        assert!(listed_model_available(inventory, "llama3"));
        assert!(listed_model_available(inventory, "llama3:latest"));
        assert!(!listed_model_available(inventory, "llama3:8b"));
        assert!(!listed_model_available(inventory, "llama3.2"));
    }

    #[test]
    fn registry_ports_are_not_model_tags() {
        let inventory =
            "NAME ID SIZE MODIFIED\nlocalhost:5000/team/model:latest abc 4GB now\n";
        assert!(listed_model_available(inventory, "localhost:5000/team/model"));
        assert!(!listed_model_available(
            inventory,
            "localhost:5000/team/model:small"
        ));
    }

    #[test]
    fn an_empty_inventory_has_no_models() {
        assert!(!listed_model_available("NAME ID SIZE MODIFIED\n", "llama3"));
        assert!(!listed_model_available("", "llama3"));
    }
}
