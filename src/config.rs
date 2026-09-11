use std::{env, fs, path::{Path, PathBuf}};

use anyhow::{Context, Result};
use serde::Deserialize;

const DEFAULT_MODEL: &str = "llama3";
const DEFAULT_OLLAMA_HOST: &str = "127.0.0.1:11434";

/// The starter configuration written by `llm_cli setup`.
///
/// Keep this as a plain TOML template instead of serializing `Config`: it is
/// deliberately commented and is useful to people editing it for the first
/// time.
pub const DEFAULT_CONFIG_TEMPLATE: &str = r#"# Default configuration for llm-cli.
# Created by `llm_cli setup`. Environment variables prefixed with LLM_CLI_
# override these values for one invocation.

model = "llama3"
system_prompt = "Reply in concise bullets. Use short sentences. Break lines for each bullet. Be direct."
# Ollama daemon address or HTTP(S) API base. This is passed to both the
# `ollama` command and its HTTP APIs, so local, remote, and Cloud hosts use
# the same setting.
ollama_host = "127.0.0.1:11434"

# Maximum duration for a chat or commit-message generation request.
llm_timeout_secs = 45
# Maximum duration for a tool or shell command.
cmd_timeout_secs = 60
# Approximate prompt budget. The application reserves space for the system
# prompt and current input before adding recent-context summaries.
max_context_tokens = 4096
streaming = true
# HTTP timeout used by Ollama's embedding and intent-classifier APIs.
request_timeout_secs = 60
generate_commit_message = true

embedding_model = "nomic-embed-text"
classifier_model = "qwen2:1.5b"

# Optional custom paths (defaults are shown for reference):
# history_path = ".llm-cli/history.jsonl"
# audit_path = ".llm-cli/audit.jsonl"
# embedding_cache_path = ".llm-cli/embeddings.toml"
# learned_path = ".llm-cli/learned.toml"
"#;

#[derive(Debug, Clone)]
pub struct Config {
    pub model: String,
    pub system_prompt: String,
    pub ollama_host: String,
    pub llm_timeout_secs: u64,
    pub cmd_timeout_secs: u64,
    pub max_context_tokens: u32,
    pub streaming: bool,
    pub history_path: PathBuf,
    pub audit_path: PathBuf,
    pub request_timeout_secs: u64,
    pub generate_commit_message: bool,
    pub embedding_cache_path: PathBuf,
    pub embedding_model: String,
    pub classifier_model: String,
    pub learned_path: PathBuf,
}

#[derive(Debug, Deserialize, Default)]
struct PartialConfig {
    model: Option<String>,
    system_prompt: Option<String>,
    ollama_host: Option<String>,
    llm_timeout_secs: Option<u64>,
    cmd_timeout_secs: Option<u64>,
    max_context_tokens: Option<u32>,
    streaming: Option<bool>,
    history_path: Option<PathBuf>,
    audit_path: Option<PathBuf>,
    request_timeout_secs: Option<u64>,
    generate_commit_message: Option<bool>,
    embedding_cache_path: Option<PathBuf>,
    embedding_model: Option<String>,
    classifier_model: Option<String>,
    learned_path: Option<PathBuf>,
}

impl Config {
    pub fn load(config_path: Option<PathBuf>) -> Result<Self> {
        let mut cfg = Config::default();

        let path = config_path.unwrap_or_else(default_config_path);
        if path.exists() {
            let contents = fs::read_to_string(&path)
                .with_context(|| format!("reading config at {path:?}"))?;
            let partial: PartialConfig =
                toml::from_str(&contents).context("parsing config file as TOML")?;
            cfg.apply_partial(partial);
        }

        cfg.apply_env_overrides();
        cfg.ollama_host = normalize_ollama_host(&cfg.ollama_host)?;
        Ok(cfg)
    }

    /// Write the documented starter configuration without silently replacing a
    /// user's existing settings. Returns `true` when a file was created or
    /// replaced.
    pub fn initialize_file(path: &Path, overwrite: bool) -> Result<bool> {
        if path.exists() && !overwrite {
            return Ok(false);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating config directory at {parent:?}"))?;
        }
        fs::write(path, DEFAULT_CONFIG_TEMPLATE)
            .with_context(|| format!("writing starter config at {path:?}"))?;
        Ok(true)
    }

    fn apply_partial(&mut self, partial: PartialConfig) {
        if let Some(model) = partial.model {
            self.model = model;
        }
        if let Some(system_prompt) = partial.system_prompt {
            self.system_prompt = system_prompt;
        }
        if let Some(ollama_host) = partial.ollama_host {
            self.ollama_host = ollama_host;
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
        if let Some(audit_path) = partial.audit_path {
            self.audit_path = audit_path;
        }
        if let Some(request_timeout_secs) = partial.request_timeout_secs {
            self.request_timeout_secs = request_timeout_secs;
        }
        if let Some(generate_commit_message) = partial.generate_commit_message {
            self.generate_commit_message = generate_commit_message;
        }
        if let Some(embedding_cache_path) = partial.embedding_cache_path {
            self.embedding_cache_path = embedding_cache_path;
        }
        if let Some(embedding_model) = partial.embedding_model {
            self.embedding_model = embedding_model;
        }
        if let Some(classifier_model) = partial.classifier_model {
            self.classifier_model = classifier_model;
        }
        if let Some(learned_path) = partial.learned_path {
            self.learned_path = learned_path;
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
        if let Ok(val) = env::var("LLM_CLI_OLLAMA_HOST") {
            if !val.is_empty() {
                self.ollama_host = val;
            }
        } else if let Ok(val) = env::var("OLLAMA_HOST") {
            if !val.is_empty() {
                self.ollama_host = val;
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
        if let Ok(val) = env::var("LLM_CLI_AUDIT_PATH") {
            if !val.is_empty() {
                self.audit_path = PathBuf::from(val);
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
        if let Ok(val) = env::var("LLM_CLI_EMBEDDING_CACHE_PATH") {
            if !val.is_empty() {
                self.embedding_cache_path = PathBuf::from(val);
            }
        }
        if let Ok(val) = env::var("LLM_CLI_EMBEDDING_MODEL") {
            if !val.is_empty() {
                self.embedding_model = val;
            }
        }
        if let Ok(val) = env::var("LLM_CLI_CLASSIFIER_MODEL") {
            if !val.is_empty() {
                self.classifier_model = val;
            }
        }
        if let Ok(val) = env::var("LLM_CLI_LEARNED_PATH") {
            if !val.is_empty() {
                self.learned_path = PathBuf::from(val);
            }
        }
    }

}

impl Default for Config {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.to_string(),
            system_prompt: "Reply in concise bullets. Use short sentences. Break lines for each bullet. Be direct.".to_string(),
            ollama_host: DEFAULT_OLLAMA_HOST.to_string(),
            llm_timeout_secs: 45,
            cmd_timeout_secs: 60,
            max_context_tokens: 4096,
            streaming: true,
            history_path: default_history_path(),
            audit_path: default_audit_path(),
            request_timeout_secs: 60,
            generate_commit_message: true,
            embedding_cache_path: default_embedding_cache_path(),
            embedding_model: "nomic-embed-text".to_string(),
            classifier_model: "qwen2:1.5b".to_string(),
            learned_path: default_learned_path(),
        }
    }
}

pub fn default_config_path() -> PathBuf {
    // Look for config in .llm-cli directory in current project
    PathBuf::from(".llm-cli/config.toml")
}

fn default_history_path() -> PathBuf {
    // Store history in project's .llm-cli directory
    PathBuf::from(".llm-cli/history.jsonl")
}

fn default_audit_path() -> PathBuf {
    // Store direct-shell audit records in the project's .llm-cli directory.
    PathBuf::from(".llm-cli/audit.jsonl")
}

fn default_embedding_cache_path() -> PathBuf {
    // Store embedding cache in project's .llm-cli directory
    PathBuf::from(".llm-cli/embeddings.toml")
}

fn default_learned_path() -> PathBuf {
    // Store learned aliases in project's .llm-cli directory
    PathBuf::from(".llm-cli/learned.toml")
}

fn parse_bool(input: &str) -> Result<bool, std::str::ParseBoolError> {
    input.parse::<bool>()
}

fn normalize_ollama_host(value: &str) -> Result<String> {
    let host = value.trim().trim_end_matches('/');
    let is_http_url = host.starts_with("http://") || host.starts_with("https://");
    let invalid_url = host.contains("://") && !is_http_url;
    let address = host
        .strip_prefix("http://")
        .or_else(|| host.strip_prefix("https://"))
        .unwrap_or(host);
    if host.is_empty()
        || invalid_url
        || address.is_empty()
        || address.contains(['/', '?', '#', '@'])
        || address.chars().any(char::is_whitespace)
    {
        anyhow::bail!(
            "ollama_host must be a host and port such as `127.0.0.1:11434` or an http(s) API base"
        );
    }
    Ok(host.to_string())
}

/// Build an Ollama HTTP endpoint from a validated `OLLAMA_HOST`-style value.
pub fn ollama_api_url(ollama_host: &str, path: &str) -> String {
    let base = if ollama_host.starts_with("http://") || ollama_host.starts_with("https://") {
        ollama_host
    } else {
        return format!("http://{ollama_host}/{}", path.trim_start_matches('/'));
    };
    format!("{base}/{}", path.trim_start_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_config_overrides_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(
            &path,
            "model = \"llama3.2:3b\"\nstreaming = false\ncmd_timeout_secs = 12\n",
        )
        .unwrap();

        let config = Config::load(Some(path)).unwrap();
        assert_eq!(config.model, "llama3.2:3b");
        assert!(!config.streaming);
        assert_eq!(config.cmd_timeout_secs, 12);
        assert_eq!(config.embedding_model, "nomic-embed-text");
        assert_eq!(config.audit_path, PathBuf::from(".llm-cli/audit.jsonl"));
    }

    #[test]
    fn initialization_does_not_replace_existing_config_without_force() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested/config.toml");

        assert!(Config::initialize_file(&path, false).unwrap());
        assert!(path.exists());
        assert!(!Config::initialize_file(&path, false).unwrap());
        assert!(Config::initialize_file(&path, true).unwrap());
        assert!(fs::read_to_string(path)
            .unwrap()
            .contains("embedding_model = \"nomic-embed-text\""));
    }

    #[test]
    fn ollama_host_is_normalized_and_validates_its_scheme() {
        let directory = tempfile::tempdir().unwrap();
        let valid = directory.path().join("valid.toml");
        fs::write(&valid, "ollama_host = \" 127.0.0.1:18080/ \"\n").unwrap();
        let config = Config::load(Some(valid)).unwrap();
        assert_eq!(config.ollama_host, "127.0.0.1:18080");
        assert_eq!(
            ollama_api_url(&config.ollama_host, "/api/version"),
            "http://127.0.0.1:18080/api/version"
        );

        let cloud = directory.path().join("cloud.toml");
        fs::write(&cloud, "ollama_host = \"https://ollama.example.invalid/\"\n").unwrap();
        let cloud_config = Config::load(Some(cloud)).unwrap();
        assert_eq!(
            ollama_api_url(&cloud_config.ollama_host, "api/generate"),
            "https://ollama.example.invalid/api/generate"
        );

        let invalid = directory.path().join("invalid.toml");
        fs::write(&invalid, "ollama_host = \"ftp://example.invalid\"\n").unwrap();
        assert!(Config::load(Some(invalid)).is_err());
    }
}
