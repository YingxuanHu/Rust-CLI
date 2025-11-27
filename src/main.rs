use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod app;
mod config;
mod ollama;
mod session;

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
    },
    /// Launch the TUI (default)
    Run,
}

fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let config = config::Config::load(cli.config.clone()).context("loading config")?;

    match cli.command.unwrap_or(Command::Run) {
        Command::Health { model } => {
            let model = model.unwrap_or_else(|| config.model.clone());
            let status = ollama::check_status(&model)?;
            if !status.reachable {
                eprintln!("Ollama daemon unreachable. Start it with: ollama serve");
                std::process::exit(2);
            }
            if !status.has_model {
                eprintln!(
                    "Model '{model}' not found locally. Pull it with: ollama pull \"{model}\""
                );
                std::process::exit(3);
            }
            println!("Ollama is reachable and model '{model}' is available.");
        }
        Command::Run => {
            app::run(config)?;
        }
    }

    Ok(())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
