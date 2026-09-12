use std::{io::IsTerminal, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod app;
mod ask;
mod audit;
mod bootstrap;
mod chat;
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
mod model_stream;
mod ollama;
mod patch;
mod repo;
mod session;
mod shell_completion;
#[cfg(test)]
mod test_support;
mod tools;
mod ui;
mod workflow;

#[derive(Debug, Parser)]
#[command(author, version, about = "LLM-powered CLI (Rust + Ollama)")]
struct Cli {
    /// Optional config file path (TOML)
    #[arg(long, global = true)]
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
    /// Check local AI setup and offer to download missing models
    Init {
        /// Also download the optional embedding and intent-classifier models
        #[arg(long)]
        full: bool,
        /// Download missing models without asking for confirmation
        #[arg(long)]
        yes: bool,
    },
    /// Ask one read-only question without opening the terminal UI
    Ask {
        /// Question for the configured model
        #[arg(required = true, num_args = 1..)]
        prompt: Vec<String>,
        /// Emit one JSON object instead of streaming plain text
        #[arg(long)]
        json: bool,
    },
    /// Inspect local prerequisites and project tooling without changing anything
    Doctor {
        /// Also check the embedding and intent-classifier models
        #[arg(long)]
        full: bool,
    },
    /// Show recent direct shell executions from the project audit log
    Audit {
        /// Number of recent entries to display; use 0 for the entire log
        #[arg(long, default_value_t = 20)]
        tail: usize,
    },
    /// Generate a shell completion script
    Completions {
        /// Shell to generate completion for
        shell: shell_completion::CompletionShell,
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
                println!("Next: run `llm_cli`. It will guide you through missing local setup.");
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
        Command::Init { full, yes } => {
            let config = config::Config::load(config_path).context("loading config")?;
            let outcome = bootstrap::initialize(&config, full, yes)?;
            if outcome.needs_user_action() {
                std::process::exit(2);
            }
        }
        Command::Ask { prompt, json } => {
            if let Err(error) = run_question(config_path, &prompt, json).await {
                if json {
                    println!("{}", serde_json::json!({ "error": format!("{error:#}") }));
                } else {
                    eprintln!("Error: {error:#}");
                }
                std::process::exit(2);
            }
        }
        Command::Audit { tail } => {
            let config = config::Config::load(config_path).context("loading config")?;
            let cwd = std::env::current_dir().context("reading current directory")?;
            let records = audit::read_shell_executions(&config.audit_path, &cwd, tail)?;
            println!("{}", audit::render_shell_executions(&records));
        }
        Command::Completions { shell } => {
            print!("{}", shell_completion::script(shell));
        }
        Command::Run => {
            let config = config::Config::load(config_path).context("loading config")?;
            let outcome = bootstrap::prepare_for_launch(&config)?;
            if outcome.needs_user_action() {
                std::process::exit(2);
            }
            if matches!(outcome, bootstrap::BootstrapOutcome::Cancelled) {
                return Ok(());
            }
            app::run(config).await?;
        }
    }

    Ok(())
}

async fn run_question(config_path: Option<PathBuf>, parts: &[String], json: bool) -> Result<()> {
    let prompt = ask::join_prompt(parts)?;
    let config = config::Config::load(config_path).context("loading config")?;
    let interactive = !json && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let outcome = if interactive {
        bootstrap::prepare_for_launch(&config)?
    } else {
        bootstrap::prepare_for_machine_use(&config)?
    };
    if outcome.needs_user_action() {
        anyhow::bail!("{}", outcome.machine_error());
    }
    if matches!(outcome, bootstrap::BootstrapOutcome::Cancelled) {
        return Ok(());
    }
    ask::run(&config, &prompt, json).await
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_env_filter(filter).init();
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, Command};

    #[test]
    fn parses_a_one_shot_question_and_json_flag() {
        let cli = Cli::try_parse_from([
            "llm_cli",
            "ask",
            "explain",
            "this project",
            "--json",
        ])
        .expect("ask command should parse");

        match cli.command.expect("subcommand") {
            Command::Ask { prompt, json } => {
                assert_eq!(
                    prompt,
                    vec!["explain".to_string(), "this project".to_string()]
                );
                assert!(json);
            }
            command => panic!("unexpected command: {command:?}"),
        }
    }

    #[test]
    fn accepts_config_after_a_subcommand() {
        let cli = Cli::try_parse_from([
            "llm_cli",
            "ask",
            "explain",
            "this",
            "--config",
            "project.toml",
        ])
        .expect("global config should parse after a subcommand");

        assert_eq!(cli.config, Some(PathBuf::from("project.toml")));
        assert!(matches!(cli.command, Some(Command::Ask { .. })));
    }

    #[test]
    fn parses_a_shell_completion_command() {
        let cli = Cli::try_parse_from(["llm_cli", "completions", "fish"])
            .expect("completion command should parse");
        assert!(matches!(
            cli.command,
            Some(Command::Completions {
                shell: crate::shell_completion::CompletionShell::Fish
            })
        ));
    }
}
