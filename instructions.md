# Instructions

This CLI is a Rust-native, terminal-first assistant with a Ratatui UI. It talks to a local LLM via Ollama, keeps session context (cwd/repo/history), and can orchestrate developer tasks (e.g., git workflows, file/search helpers). It uses semantic embedding-based intent matching to understand natural language commands.

## Prerequisites
- Rust toolchain: `rustup`, `cargo`, `rustc`.
- Ollama 0.13+ installed and running (`ollama serve`).
- Model pulled locally, e.g. `ollama pull "llama3"` (or `llama3:8b`).
- (Optional) Embedding model for semantic intent matching: `ollama pull "nomic-embed-text"`. If not available, the CLI falls back to direct chat mode.

## Health Check
- Verify setup with:

```bash
bash scripts/health.sh llama3
```

- Or use the built-in command: `cargo run -- health --model llama3`
- The script checks `rustc`, `cargo`, `ollama`, daemon reachability, and whether the model (tagged or bare) is present. Override the model with `MODEL=llama3 bash scripts/health.sh` (or another tag) or by passing an argument.

## Defaults
- Model: `llama3` (you can use `llama3:8b`/`llama3:latest`).
- Timeouts: LLM 45s; command 60s; request 60s.
- Max context: 4096 tokens.
- Streaming responses: on by default.
- All data stored in project's `.llm-cli/` directory (history, learned commands, embeddings, config).
- Generate commit message: on by default.
- Embedding model: `nomic-embed-text` (optional, for semantic intent matching).
- Classifier model: `qwen2:1.5b` (for intent classification fallback).

## Setup and Usage
- Prereqs: Rust toolchain (rustup/cargo), Ollama 0.13+ running (`ollama serve`).
- Pull the default model: `ollama pull "llama3"` (or `llama3:8b`).
- (Optional) Pull embedding model: `ollama pull "nomic-embed-text"` for semantic intent matching.
- (Optional) Pull classifier model: `ollama pull "qwen2:1.5b"` for intent classification.
- Health check: `cargo run -- health --model llama3` (or use `bash scripts/health.sh llama3`).
- Run the TUI: `cargo run -- run` or simply `cargo run`.
- Config file: Create `.llm-cli/config.toml` in your project directory, or pass `--config path`. See `config.example.toml` for available options.
- All files (history, learned commands, embeddings) are stored in `.llm-cli/` directory in your project.
- Env overrides: `LLM_CLI_MODEL`, `LLM_CLI_SYSTEM_PROMPT`, `LLM_CLI_LLM_TIMEOUT_SECS`, `LLM_CLI_CMD_TIMEOUT_SECS`, `LLM_CLI_MAX_CONTEXT_TOKENS`, `LLM_CLI_STREAMING`, `LLM_CLI_HISTORY_PATH`, `LLM_CLI_REQUEST_TIMEOUT_SECS`, `LLM_CLI_GENERATE_COMMIT_MESSAGE`.

## Notes
- If the health check warns the model is missing, pull it with `ollama pull <model>`.
- If the daemon is unreachable, start it with `ollama serve` before running the CLI.
- If embeddings are not available, the CLI will show "embeddings: disabled" and fall back to direct chat mode. This is fine for basic usage, but semantic intent matching works better with embeddings enabled.

## Input Modes

The CLI supports two input modes that can be toggled with `Ctrl+S`:

- **Chat Mode** (default): Natural language input is processed through semantic intent matching (if embeddings are available) or sent directly to the LLM for chat.
- **Shell Mode**: Commands are executed directly as shell commands. Prefix commands with `$` or `!` in Chat mode, or switch to Shell mode.

## Keyboard Shortcuts
- `Ctrl+S`: Toggle between Chat and Shell modes
- `Enter`: Submit input
- `↑`/`↓`: Navigate input history (mode-aware: shows only Chat or Shell commands based on current mode)
- `PgUp`/`PgDn`: Scroll conversation log
- `Esc`/`q`/`Ctrl+C`: Quit

## Shell Commands and Bang Shortcuts

In Chat mode, you can prefix commands with `$` or `!` to execute them as shell commands:
- `$ ls -la` - Execute a shell command
- `!pwd` - Execute a shell command (alternative syntax)

Bang shortcuts (bash-style):
- `!!` - Repeat the last shell command from history
- `!prefix` - Find and execute the last shell command starting with `prefix`

Built-in shell commands:
- `cd <path>` - Change directory (supports `~`, `~/path`, absolute and relative paths)
- `pwd` - Print current working directory

## Built-in Commands/Intents

The CLI uses semantic embedding-based intent matching to understand natural language. The following commands are supported:

### Git Workflows
- **Save work**: `save work`, `save my work`, `push my changes`, `sync with remote`, `commit and push` → Shows git plan + status preview; reply `yes` to stage/commit/push; then accept or override the suggested commit message.
- **Git status**: `status`, `git status`, `show status`, `what changed`, `show changes`, `diff` → Shows `status --short` and `diff --stat`; reply with a file path to view its diff, or `yes` to continue.
- **Stage changes**: `stage all`, `stage changes`, `git add`, `add all files` (confirms before running) or `stage <path>`, `git add <path>` (confirms before running).
- **Commit only**: `commit`, `commit changes`, `commit only`, `make a commit` → Prompts with a suggested message, lets you accept or type your own, then runs `git commit` without push.
- **Draft commit message**: `draft commit message`, `generate commit message`, `suggest commit message` → Uses staged diff; returns a subject + bullets suggestion.

### File Operations
- **Show file**: `show file <path>`, `read file <path>`, `open file <path>`, `show <path>`, `read <path>`, `cat <path>` → Displays file contents (relative to repo root or cwd).
- **List files**: `list files`, `show files`, `list directory`, `ls`, `dir`, `list files in <path>`, `show files in <path>` → Lists files and directories in the specified path (or current directory if no path given).
- **Write file**: `write file`, `create file`, `save to file`, `write to <path>`, `create <path>` → Creates or overwrites a file with given content (requires confirmation). Use format: `write to <path> with content: <content>`.

### Code Search
- **Find TODOs**: `find todos`, `show todos`, `list todos`, `find fixme`, `search for todos`, `todo list` → Searches for TODO and FIXME comments (requires `rg`/ripgrep on PATH).

### Testing
- **Run tests**: `run tests`, `test`, `cargo test`, `run the tests`, `execute tests` → Runs `cargo test` in the repo.

### Chat & Command Extraction
- **General chat**: Any input that doesn't match a specific intent will be sent to the LLM for conversational response.
- **Command extraction**: If the LLM's chat response contains shell commands (in code blocks or bullet points), the CLI will detect them and offer to:
  - `[y]es` - Execute the commands
  - `[s]ave` - Save as a custom workflow for future use
  - `[n]o` - Skip execution
- **Custom workflows**: Saved workflows can be reused by typing the exact phrase you used originally (e.g., "discard current changes").

## Semantic Intent Matching

The CLI uses a 3-tier intent resolution system:
1. **Tier 1**: Fuzzy matching against learned commands and custom workflows (< 1ms)
2. **Tier 2**: Keyword + embedding hybrid classifier (~50ms, requires `nomic-embed-text`)
3. **Tier 3**: Small LLM classifier (~500ms, uses `qwen2:1.5b`) → falls back to chat if uncertain

If all tiers are uncertain, the system defaults to conversational chat mode. Custom workflows are stored in `.llm-cli/custom_workflows.toml` (legacy `.llm-cli/custom_commands.toml`) and have highest priority in matching.

## Workflow Confirmations

Many operations require confirmation before execution:
- **Save work workflow**: Shows a preview plan and asks for confirmation
- **Stage operations**: Shows what will be staged and asks for confirmation
- **Commit operations**: Shows suggested commit message and lets you accept or customize
- **File write operations**: Shows content preview and asks for confirmation before creating/overwriting

Reply with `yes`/`y` to confirm, `cancel`/`no`/`n` to abort, or type custom input (e.g., custom commit message).

## Tooling Dependencies
- **Ripgrep (`rg`)**: Required for TODO search. Install via Homebrew: `brew install ripgrep` (or your package manager). Ensure `rg` is on `PATH`.
- **Git**: Required for status/save-work. Ensure your repo is initialized and remotes set up for push.
- **Ollama**: Required for LLM inference. Must be running (`ollama serve`).
- **Embedding model (optional)**: `nomic-embed-text` for semantic intent matching. Install with `ollama pull nomic-embed-text`.
