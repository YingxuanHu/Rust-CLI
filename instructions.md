# Instructions

This CLI is a Rust-native, terminal-first assistant with a Ratatui UI. It talks to a local LLM via Ollama, keeps session context (cwd/repo/history), and can orchestrate developer tasks (e.g., git workflows, file/search helpers).

## Prerequisites
- Rust toolchain: `rustup`, `cargo`, `rustc`.
- Ollama 0.13+ installed and running (`ollama serve`).
- Model pulled locally, e.g. `ollama pull "llama3"` (or `llama3:8b`).

## Health Check
- Verify setup with:

```bash
bash scripts/health.sh llama3
```

- The script checks `rustc`, `cargo`, `ollama`, daemon reachability, and whether the model (tagged or bare) is present. Override the model with `MODEL=llama3 bash scripts/health.sh` (or another tag) or by passing an argument.

## Defaults
- Model: `llama3` (you can use `llama3:8b`/`llama3:latest`).
- Timeouts: LLM 45s; command 60s.
- Max context: 4096 tokens.
- Streaming responses: on by default.
- History file: `~/.local/state/llm-cli/history.jsonl` (or `~/Library/Application Support/llm-cli/history.jsonl` on macOS).
 - Generate commit message: on by default.

## Setup and Usage
- Prereqs: Rust toolchain (rustup/cargo), Ollama 0.13+ running (`ollama serve`).
- Pull the default model: `ollama pull "llama3"` (or `llama3:8b`).
- Health check: `cargo run -- health --model llama3` (or use `bash scripts/health.sh llama3`).
- Run (placeholder TUI for now): `cargo run -- run` or simply `cargo run`.
- Config file: place `config.toml` at `~/.config/llm-cli/config.toml` (Linux) or `~/Library/Application Support/llm-cli/config.toml` (macOS), or pass `--config path`. See `config.example.toml` for keys.
- Env overrides: `LLM_CLI_MODEL`, `LLM_CLI_LLM_TIMEOUT_SECS`, `LLM_CLI_CMD_TIMEOUT_SECS`, `LLM_CLI_MAX_CONTEXT_TOKENS`, `LLM_CLI_STREAMING`, `LLM_CLI_HISTORY_PATH`.

## Notes
- If the health check warns the model is missing, pull it with `ollama pull <model>`.
- If the daemon is unreachable, start it with `ollama serve` before running the CLI.

## Built-in commands/intents
- Chat: type any prompt; responses stream live.
- Save work: `save work` → shows git plan + status preview; reply `yes` to stage/commit/push; then accept or override the suggested commit message.
- Git status: `git status` or `status` (shows `status --short` and `diff --stat`; reply with a file path to view its diff).
- Find TODOs: `find todos`/`find todo`/`todos` (requires `rg`/ripgrep on PATH).
- Show file: `show file <path>`/`read file <path>`/`show <path>` (relative to repo root or cwd).
- Stage changes: `stage all`/`stage`/`git add -A` (confirms before running) or `stage <path>`/`git add <path>` (confirms before running).
- Draft commit message: `draft commit message` (uses staged diff; returns a subject + bullets suggestion).
- Commit only: `commit`/`commit only` (prompts with a suggested message, lets you accept or type your own, then runs `git commit` without push).
- Run tests: `run tests`/`tests`/`run test` (runs `cargo test` in the repo).
- Input history: `Ctrl+P`/`Ctrl+N`; scroll log with arrows/PgUp/PgDn; quit with Esc/q/Ctrl+C.

## Tooling dependencies
- Ripgrep (`rg`) for TODO search. Install via Homebrew: `brew install ripgrep` (or your package manager). Ensure `rg` is on `PATH`.
- Git for status/save-work. Ensure your repo is initialized and remotes set up for push.
