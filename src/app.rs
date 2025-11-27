use anyhow::Result;

use crate::{config::Config, session::SessionState};

pub fn run(config: Config) -> Result<()> {
    let _session = SessionState::new();
    println!(
        "LLM CLI starting with model '{}' (streaming: {}) – TUI wiring to be added.",
        config.model, config.streaming
    );
    Ok(())
}
