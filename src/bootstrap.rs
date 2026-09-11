//! First-run local-runtime setup.
//!
//! The bootstrap deliberately changes only Ollama's model inventory, and only
//! after a person explicitly confirms the download. Installing or starting
//! system software is left to the operating system and the user.

use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result};

use crate::{config::Config, ollama};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapOutcome {
    Ready,
    NeedsOllamaInstallation,
    NeedsOllamaDaemon,
    NeedsChatModel,
    Cancelled,
}

impl BootstrapOutcome {
    pub const fn needs_user_action(self) -> bool {
        matches!(
            self,
            Self::NeedsOllamaInstallation | Self::NeedsOllamaDaemon | Self::NeedsChatModel
        )
    }

    pub const fn machine_error(self) -> &'static str {
        match self {
            Self::NeedsOllamaInstallation => {
                "Ollama is not installed; install it from https://ollama.com"
            }
            Self::NeedsOllamaDaemon => "Ollama is not responding; start or open Ollama and retry",
            Self::NeedsChatModel => "the configured chat model is missing; run `llm_cli init`",
            Self::Ready | Self::Cancelled => "local setup is not ready",
        }
    }
}

/// Prepare the minimum local runtime necessary to launch the TUI. When a
/// model is absent, this interactively asks before downloading it.
pub fn prepare_for_launch(config: &Config) -> Result<BootstrapOutcome> {
    prepare(config, false, false, false, true)
}

/// Check whether a non-interactive caller can safely emit machine-readable
/// output. This never prints a setup prompt or initiates a download.
pub fn prepare_for_machine_use(config: &Config) -> Result<BootstrapOutcome> {
    prepare(config, false, false, false, false)
}

/// Run the explicit `llm_cli init` flow. `full` includes the optional embedding
/// and intent-classifier models; `assume_yes` is for scripts.
pub fn initialize(config: &Config, full: bool, assume_yes: bool) -> Result<BootstrapOutcome> {
    prepare(config, full, assume_yes, true, true)
}

fn prepare(
    config: &Config,
    full: bool,
    assume_yes: bool,
    announce_when_ready: bool,
    show_messages: bool,
) -> Result<BootstrapOutcome> {
    if !ollama::command_available() {
        if show_messages {
            println!(
                "Ollama is not installed yet.\n\
                 \n1. Install it from https://ollama.com\n\
                 2. Open Ollama, then rerun `llm_cli`."
            );
        }
        return Ok(BootstrapOutcome::NeedsOllamaInstallation);
    }

    let mut missing = Vec::new();
    for model in models_to_prepare(config, full) {
        let status = match ollama::check_status(model, &config.ollama_host) {
            Ok(status) if status.reachable => status,
            Ok(status) => {
                if show_messages {
                    let detail = status.raw_output.trim();
                    if detail.is_empty() {
                        println!(
                            "Ollama is installed but is not responding at {}.\n\
                             Start or open Ollama, then rerun `llm_cli`.",
                            config.ollama_host
                        );
                    } else {
                        println!(
                            "Ollama is installed but is not responding at {} ({detail}).\n\
                             Start or open Ollama, then rerun `llm_cli`.",
                            config.ollama_host
                        );
                    }
                }
                return Ok(BootstrapOutcome::NeedsOllamaDaemon);
            }
            Err(error) => {
                if show_messages {
                    println!(
                        "Could not check Ollama at {}: {error}\n\
                         Start or open Ollama, then rerun `llm_cli`.",
                        config.ollama_host
                    );
                }
                return Ok(BootstrapOutcome::NeedsOllamaDaemon);
            }
        };
        if !status.has_model {
            missing.push(model);
        }
    }

    if missing.is_empty() {
        if announce_when_ready && show_messages {
            println!("AI setup is ready. Start the assistant with `llm_cli`.");
        }
        return Ok(BootstrapOutcome::Ready);
    }

    if !show_messages {
        return Ok(BootstrapOutcome::NeedsChatModel);
    }

    println!(
        "The following model{} {} needed:",
        plural_suffix(missing.len()),
        if missing.len() == 1 { "is" } else { "are" }
    );
    for model in &missing {
        println!("  - {model}");
    }
    println!("Ollama will download them through {}.", config.ollama_host);

    if !assume_yes && !confirm_download()? {
        println!("No models were downloaded. Rerun `llm_cli` whenever you are ready.");
        return Ok(BootstrapOutcome::Cancelled);
    }

    for model in missing {
        println!("\nDownloading '{model}'...");
        ollama::pull_model(model, &config.ollama_host)?;
    }

    if announce_when_ready {
        println!("\nAI setup is ready. Start the assistant with `llm_cli`.");
    } else {
        println!("\nAI setup is ready. Starting llm_cli...");
    }
    Ok(BootstrapOutcome::Ready)
}

fn models_to_prepare(config: &Config, full: bool) -> Vec<&str> {
    let mut models = vec![config.model.as_str()];
    if full {
        for model in [
            config.embedding_model.as_str(),
            config.classifier_model.as_str(),
        ] {
            if !models.contains(&model) {
                models.push(model);
            }
        }
    }
    models
}

fn plural_suffix(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn confirm_download() -> Result<bool> {
    if !io::stdin().is_terminal() {
        println!("No interactive terminal is available. To download now, rerun `llm_cli init --yes`.");
        return Ok(false);
    }

    print!("Download now? [y/N] ");
    io::stdout().flush().context("showing download confirmation")?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .context("reading download confirmation")?;
    Ok(matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[test]
    fn normal_bootstrap_only_requires_the_chat_model() {
        let config = Config::default();
        assert_eq!(super::models_to_prepare(&config, false), vec!["llama3"]);
    }

    #[test]
    fn full_bootstrap_adds_unique_optional_models() {
        let mut config = Config::default();
        config.embedding_model = config.model.clone();
        assert_eq!(
            super::models_to_prepare(&config, true),
            vec!["llama3", "qwen2:1.5b"]
        );
    }

    #[test]
    fn plural_suffix_is_human_readable() {
        assert_eq!(super::plural_suffix(1), "");
        assert_eq!(super::plural_suffix(2), "s");
    }

    #[test]
    fn machine_errors_are_actionable() {
        assert!(super::BootstrapOutcome::NeedsChatModel
            .machine_error()
            .contains("llm_cli init"));
        assert!(super::BootstrapOutcome::NeedsOllamaInstallation
            .machine_error()
            .contains("ollama.com"));
    }
}
