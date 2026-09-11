use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod app;
mod audit;
mod command_policy;
mod commands;
mod completion;
mod config;
mod context;
mod custom_command_generator;
mod diagnostics;
mod embedding;
mod file_ops;
mod frecency;
mod fuzzy;
mod handlers;
mod input;
mod intent;
mod keyword_classifier;
mod learned;
mod llm_classifier;
mod ollama;
mod patch;
mod repo;
mod session;
#[cfg(test)]
mod test_support;
mod tools;
mod ui;
mod workflow;

#[derive(Debug, Parser)]
#[command(author, version, about = "LLM-powered CLI (Rust + Ollama)")]
struct Cli {
    /// Optional config file path (TOML)
    #[arg(long)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run a health check against Ollama and the selected model
    Health {
        /// Model name or tag, e.g. `llama3` or `llama3:8b`
        #[arg(long)]
        model: Option<String>,
        /// Also verify the embedding and intent-classifier models
        #[arg(long)]
        full: bool,
    },
    /// Create a commented per-project configuration file
    Setup {
        /// Replace an existing configuration file
        #[arg(long)]
        force: bool,
    },
    /// Inspect local prerequisites and project tooling without changing anything
    Doctor {
        /// Also check the embedding and intent-classifier models
        #[arg(long)]
        full: bool,
    },
    /// Launch the TUI (default)
    Run,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let config_path = cli.config.clone();

    match cli.command.unwrap_or(Command::Run) {
        Command::Setup { force } => {
            let path = config_path.unwrap_or_else(config::default_config_path);
            if config::Config::initialize_file(&path, force)? {
                println!("Created starter configuration at {}", path.display());
                println!("Next: pull the required Ollama models, then run `llm_cli doctor --full`.");
            } else {
                println!(
                    "Configuration already exists at {}. Use `llm_cli setup --force` to replace it.",
                    path.display()
                );
            }
        }
        Command::Health { model, full } => {
            let config = config::Config::load(config_path).context("loading config")?;
            let model = model.unwrap_or_else(|| config.model.clone());
            let status = ollama::check_status(&model, &config.ollama_host)?;
            if !status.reachable {
                eprintln!("Ollama daemon at {} is unreachable. Start it with: ollama serve", config.ollama_host);
                std::process::exit(2);
            }
            if !status.has_model {
                eprintln!(
                    "Model '{model}' not found locally. Pull it with: ollama pull \"{model}\""
                );
                std::process::exit(3);
            }
            if full {
                let required = [
                    ("embedding", config.embedding_model.as_str()),
                    ("classifier", config.classifier_model.as_str()),
                ];
                let mut missing = Vec::new();
                for (label, required_model) in required {
                    let status = ollama::check_status(required_model, &config.ollama_host)?;
                    if status.has_model {
                        println!("{label} model '{required_model}' is available.");
                    } else {
                        eprintln!(
                            "{label} model '{required_model}' not found. Pull it with: ollama pull \"{required_model}\""
                        );
                        missing.push(required_model);
                    }
                }
                if !missing.is_empty() {
                    std::process::exit(3);
                }
            }
            println!("Ollama at {} is reachable and chat model '{model}' is available.", config.ollama_host);
        }
        Command::Doctor { full } => {
            let config = config::Config::load(config_path).context("loading config")?;
            let report = diagnostics::run_doctor(&config, full);
            println!("{}", report.render());
            if report.has_blockers() {
                std::process::exit(2);
            }
        }
        Command::Run => {
            let config = config::Config::load(config_path).context("loading config")?;
            app::run(config).await?;
        }
    }

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
