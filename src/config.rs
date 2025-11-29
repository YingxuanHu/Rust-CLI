use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::Deserialize;

const DEFAULT_MODEL: &str = "llama3";

#[derive(Debug, Clone)]
pub struct Config {
    pub model: String,
    pub system_prompt: String,
    pub llm_timeout_secs: u64,
    pub cmd_timeout_secs: u64,
    pub max_context_tokens: u32,
    pub streaming: bool,
    pub history_path: PathBuf,
    pub request_timeout_secs: u64,
    pub generate_commit_message: bool,
}

#[derive(Debug, Deserialize, Default)]
struct PartialConfig {
    model: Option<String>,
    system_prompt: Option<String>,
    llm_timeout_secs: Option<u64>,
    cmd_timeout_secs: Option<u64>,
    max_context_tokens: Option<u32>,
    streaming: Option<bool>,
    history_path: Option<PathBuf>,
    request_timeout_secs: Option<u64>,
    generate_commit_message: Option<bool>,
}

impl Config {
    pub fn load(config_path: Option<PathBuf>) -> Result<Self> {
        let mut cfg = Config::default();

        if let Some(path) = config_path.or_else(default_config_path) {
            if path.exists() {
                let contents = fs::read_to_string(&path)
                    .with_context(|| format!("reading config at {path:?}"))?;
                let partial: PartialConfig =
                    toml::from_str(&contents).context("parsing config file as TOML")?;
                cfg.apply_partial(partial);
            }
        }

        cfg.apply_env_overrides();
        Ok(cfg)
    }

    fn apply_partial(&mut self, partial: PartialConfig) {
        if let Some(model) = partial.model {
            self.model = model;
        }
        if let Some(system_prompt) = partial.system_prompt {
            self.system_prompt = system_prompt;
        }
        if let Some(llm_timeout_secs) = partial.llm_timeout_secs {
            self.llm_timeout_secs = llm_timeout_secs;
        }
        if let Some(cmd_timeout_secs) = partial.cmd_timeout_secs {
            self.cmd_timeout_secs = cmd_timeout_secs;
        }
        if let Some(max_context_tokens) = partial.max_context_tokens {
            self.max_context_tokens = max_context_tokens;
        }
        if let Some(streaming) = partial.streaming {
            self.streaming = streaming;
        }
        if let Some(history_path) = partial.history_path {
            self.history_path = history_path;
        }
        if let Some(request_timeout_secs) = partial.request_timeout_secs {
            self.request_timeout_secs = request_timeout_secs;
        }
        if let Some(generate_commit_message) = partial.generate_commit_message {
            self.generate_commit_message = generate_commit_message;
        }
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(val) = env::var("LLM_CLI_MODEL") {
            if !val.is_empty() {
                self.model = val;
            }
        }
        if let Ok(val) = env::var("LLM_CLI_SYSTEM_PROMPT") {
            if !val.is_empty() {
                self.system_prompt = val;
            }
        }
        if let Ok(val) = env::var("LLM_CLI_LLM_TIMEOUT_SECS") {
            if let Ok(parsed) = val.parse() {
                self.llm_timeout_secs = parsed;
            }
        }
        if let Ok(val) = env::var("LLM_CLI_CMD_TIMEOUT_SECS") {
            if let Ok(parsed) = val.parse() {
                self.cmd_timeout_secs = parsed;
            }
        }
        if let Ok(val) = env::var("LLM_CLI_MAX_CONTEXT_TOKENS") {
            if let Ok(parsed) = val.parse() {
                self.max_context_tokens = parsed;
            }
        }
        if let Ok(val) = env::var("LLM_CLI_STREAMING") {
            if let Ok(parsed) = parse_bool(&val) {
                self.streaming = parsed;
            }
        }
        if let Ok(val) = env::var("LLM_CLI_HISTORY_PATH") {
            if !val.is_empty() {
                self.history_path = PathBuf::from(val);
            }
        }
        if let Ok(val) = env::var("LLM_CLI_REQUEST_TIMEOUT_SECS") {
            if let Ok(parsed) = val.parse() {
                self.request_timeout_secs = parsed;
            }
        }
        if let Ok(val) = env::var("LLM_CLI_GENERATE_COMMIT_MESSAGE") {
            if let Ok(parsed) = parse_bool(&val) {
                self.generate_commit_message = parsed;
            }
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            system_prompt: "Reply in concise bullets. Use short sentences. Break lines for each bullet. Be direct.".to_string(),
            llm_timeout_secs: 45,
            cmd_timeout_secs: 60,
            max_context_tokens: 4096,
            streaming: true,
            history_path: default_history_path(),
            request_timeout_secs: 60,
            generate_commit_message: true,
        }
    }
}

fn default_config_path() -> Option<PathBuf> {
    project_dirs().map(|dirs| dirs.config_dir().join("config.toml"))
}

fn default_history_path() -> PathBuf {
    if let Some(dirs) = project_dirs() {
        if let Some(state) = dirs.state_dir() {
            return state.join("history.jsonl");
        }
        return dirs.data_dir().join("history.jsonl");
    }
    PathBuf::from("~/.local/state/llm-cli/history.jsonl")
}

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("dev", "llm-cli", "llm-cli")
}

fn parse_bool(input: &str) -> Result<bool, std::str::ParseBoolError> {
    input.parse::<bool>()
}
