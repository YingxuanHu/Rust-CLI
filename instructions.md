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

## Notes
- If the health check warns the model is missing, pull it with `ollama pull <model>`.
- If the daemon is unreachable, start it with `ollama serve` before running the CLI.
