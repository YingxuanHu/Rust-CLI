# Original Course Project Report

This is the historical course submission, preserved for attribution and
project history. Its performance estimates, setup instructions, and competitor
claims are not current product guarantees. See [the README](../README.md) for
current usage and [the engineering review](PROJECT_REVIEW.md) for verified
limitations and the improvement plan.

**Student names:**\
Ruitong Li, 1006912815\
Yingxuan Hu, 1006881377

**Contact email:**\
ruiton.li@mail.utoronto.ca\
alvin.hu@mail.utoronto.ca

---

## Start in One Command

From this repository, run `bash scripts/install.sh`. Then, from the project
you want to work in, run:

```bash
llm_cli
```

On its first launch, the CLI checks Ollama and asks before downloading a
missing chat model. It then opens with context-aware suggestions such as
`explain this project`, `run tests`, and `what changed`; type `show me around`
at any time to reopen them. Use `llm_cli init --full` to add the optional
routing models, `llm_cli doctor --full` to inspect setup without changing
anything, or `llm_cli setup` to customize configuration.

For one quick, read-only answer without opening the full-screen interface:

```bash
llm_cli ask "explain Rust ownership"
llm_cli ask "give me a concise testing checklist" --json
```

`ask` streams normal output for people and emits one JSON object with `--json`
for scripts. It does not run shell commands, edit files, or start Git workflows.

The installed command can also complete its subcommands and flags with Tab in
your shell. See [shell completion setup](INSTALL.md#shell-completion) for
one copy-and-paste command per shell.

The full manual and troubleshooting guide is in [docs/INSTALL.md](INSTALL.md).
The same installer supports a verified no-Cargo path after downloading it:
`bash install.sh --binary`.

---

# Video Slide Presentation

https://drive.google.com/file/d/1k9EWQem0jMkhU5A1f3u6RnVxP2bLZsGG/view?usp=sharing

---

# Video Demo

https://drive.google.com/file/d/1Zn_lr4jAE4M_RqhSJq5vAKVMID7bcl23/view?usp=drive_link

---

# Final Report

## Table of Contents

- [1. Motivation](#1-motivation)
- [2. Objectives](#2-objectives)
- [3. Features](#3-features)
- [4. User Guide](#4-user-guide)
- [5. Reproducibility Guide](#5-reproducibility-guide)
- [6. Contributions by Each Team Member](#6-contributions-by-each-team-member)
- [7. Lessons Learned and Concluding Remarks](#7-lessons-learned-and-concluding-remarks)

---

## 1. Motivation

A lack of a lightweight, LLM-powered CLI exists in the current ecosystem. Existing solutions like Codex CLI and AIChat are often too heavy or tied to other ecosystems, leaving a gap for a simple alternative. This gap matters because many developers who choose Rust do so for its speed, safety, and efficiency in building small, reliable tools. When they want to experiment with AI-driven workflows, they often have no choice but to rely on bulky tools.

This project addresses that gap while remaining enjoyable to build. It brings together three areas we want to practice deeply: systems programming in Rust, text-based user interface design, and integration of large language models into developer tools. The project is both rewarding to implement and relevant to ongoing conversations about how developers can interact with AI in their day-to-day work.

By setting a realistic scope, we created a polished prototype that demonstrates novelty without being overwhelming. The novelty lies in its Rust-first design, its ability to demonstrate agentic workflows in a lightweight way, and its practical integration into terminal-based development workflows.

---

## 2. Objectives

Build a lightweight, Rust-native terminal assistant that improves day-to-day developer workflows while staying **local**, **fast**, and **safe**.

**Outcome-focused objectives:**

1. **Keep inference local and private**: Provide a usable LLM experience without relying on hosted APIs, so code and prompts stay on the user’s machine.

2. **Feel native in the terminal**: Deliver a responsive, full-screen TUI experience that supports real iterative work (quick follow-ups, scrollback, and history), not a one-off chatbot demo.

3. **Reduce friction for common repo tasks**: Help users move from “what changed?” to “ready to save/share work” with fewer manual steps and clearer, more structured workflows.

4. **Handle natural language reliably**: Translate a range of everyday phrasing into the right action, while still providing a graceful fallback to normal chat when intent is unclear.

5. **Make automation trustworthy**: Ensure potentially destructive actions are transparent and user-controlled via previews and explicit confirmations.

---

## 3. Features

### Terminal UI and Conversation Layout

Built with Ratatui, the full-screen interface displays messages labeled as System, User, or Assistant. Long lines wrap properly. The input area stays fixed at the bottom, and a status bar shows the active model, working directory, and runtime state. Users can scroll with PgUp/PgDn and navigate input history with Up/Down arrows.

### Chat Model and Streaming Output

Uses Ollama for local inference with streaming output—text appears progressively rather than all at once. Default model is `llama3`, configurable via environment variable or config file. Startup health checks verify Ollama connectivity and model availability.

### Session Context and Semantic References

Tracks working directory, repository root, and project type. The repo-awareness layer auto-detects manifests (Cargo.toml, package.json, etc.) so we can label the session (`Current project: llm_cli (Rust)`), route “run tests” or “build” to the correct toolchain, and keep workflows scoped to the right tree. Maintains input history and a window of recent outputs (last 5, max 2000 chars each). When reference words like "it", "that", or "the diff" are detected, relevant context is injected into the LLM prompt.

### Intent Routing, Workflows, and Command Learning

In this project, a **workflow** means a user-facing action that expands into a **series of individual commands** (e.g., multiple `git` steps) with **previews + explicit confirmation** before execution. We use this term throughout the CLI because many “smart” behaviors (git automation, running commands suggested by the LLM, and saving custom workflows) share the same plan/confirm/execute pattern.

#### Tiered Intent Resolution (how text becomes an action)

**Tier 1 (< 1ms)**: Fuzzy matching against learned aliases, exact tool names, and examples. Resolves 90% of common commands instantly.

**Tier 2 (~50ms)**: Hybrid scoring combining keyword matching (60% weight) and semantic embeddings (40% weight). Requires `nomic-embed-text` model. Handles paraphrasing and synonyms.

**Tier 3 (~500ms)**: Small LLM classifier (default: `qwen2:1.5b`) handles novel phrasing. Falls back to chat when uncertain.

This tiered routing is what decides whether your input should start a **workflow** (e.g., a git workflow), run a single tool action, run a shell command, or fall back to chat.

### Command Routing and Safety

Supports two modes toggled with Ctrl+S:
- **Chat Mode**: Natural language processed through intent system or sent to LLM
- **Shell Mode**: Direct shell command execution

In Chat mode, prefix commands with `$` or `!` for shell execution. Bang shortcuts (`!!` for last command, `!prefix` for last command starting with prefix) provide bash-style convenience. When the LLM suggests shell commands, users must confirm before execution.

The direct shell path classifies commands before running them. Read-only commands
run normally; high-impact commands—such as deletion, permission changes,
redirection, command interpreters, pushes/publishes, and network tools—show the
exact expanded command and require a second explicit confirmation. Every direct
shell execution is appended to a project-local JSON Lines audit log (default:
`.llm-cli/audit.jsonl`). The log redacts common credential flags and headers;
it is an execution record, not a security sandbox.

Use `llm_cli audit` to inspect the 20 most recent direct shell executions in
the active project's log, or `llm_cli audit --tail 0` to show the complete
local history. Reading the log never changes it.

#### Git Workflows (multi-step)

**Save work**: Shows plan with status preview, asks confirmation, stages changes, generates commit message based on staged diff, lets user accept/override message, commits, and pushes. Execution report shows results.

**Status**: Shows `git status --short` and `diff --stat` summary. User can type a file path to view that file's diff.

**Commit**: Commits without push, with LLM-generated message suggestion.

**Stage**: Stages changes with confirmation.

**Draft commit message**: Generates commit message from staged changes.

### File Operations and Code Search

**Show file**: Displays file contents in conversation (e.g., "show src/main.rs")

**List files**: Lists directory contents (e.g., "list files in src")

**Write file**: Creates/overwrites files with confirmation (e.g., "write to notes.txt with content: ...")

**Reviewed code edit**: `edit path/to/file: describe the change` asks the model
for a single-file unified diff. The CLI rejects patches that touch another file,
performs a Git dry run before preview and application, and leaves the exact
patch available for `rollback last edit` while the file remains unchanged.

**Find TODOs**: Ripgrep-based search for TODO/FIXME comments (requires `rg` on PATH)

#### Custom Command Learning (workflow special case)

When the LLM generates shell commands, users can execute them (`y`), save them as learned commands (`s`), or skip (`n`). Learned commands are matched instantly (< 1ms) in Tier 1 on subsequent uses, enabling personalized workflows.

### Ghost Text Completion

As users type, ghost text suggestions appear and can be accepted with Tab. Completions are mode-aware (Chat vs Shell) and come from three sources: tool examples, recent history, and learned phrases.

**How the suggestion is chosen (high-level algorithm):**
- **Path completion first**: If the last token looks like a path, the CLI suggests filesystem completions from the current directory, prioritizing candidates by a **file frecency score** (recent/frequent file paths), and appending `/` for directories.
- **Otherwise, command completion**:
  - **Candidate pool**: tool examples + the most recent history entries + learned phrases.
  - **Scoring**: each candidate gets a **fuzzy-match score** against your current input, then receives a **frecency boost** so frequently/recently used items are preferred.
  - **Display rule**: ghost text only appears when the chosen candidate **starts with your current input**, and the UI shows only the **remaining suffix** (so Tab completes what you’ve already typed).

### Embedding Cache

Pre-computed embeddings for tool examples are cached in `.llm-cli/embeddings.toml`, providing 10-20x startup speedup (from ~1-2s to ~0.1s). Cache invalidates automatically if the embedding model changes.

### Help / Manual

Typing `help` (or `?`) shows an in-app manual with modes, key bindings, and common commands. The CLI also supports standard Clap help output via `--help`.

---

## 4. User Guide

### Getting Started (Setup + Run)

This section is a “from zero to running” setup. For OS-specific install details (macOS vs Ubuntu), see **Reproducibility Guide**; the steps below describe the required components and the exact commands you’ll run once they’re installed.

#### 1) Install prerequisites

- **Rust toolchain**: `rustc` + `cargo` (via `rustup`)
- **Ollama**: installed and running locally
- **Git**: recommended (required for git-related workflows)
- **ripgrep (`rg`)**: optional (required only for `find todos`)

### Using the CLI (after it launches)

After starting, the full-screen interface opens with a conversation panel at top and an input area at bottom.

A `help` page is available inside the app with 'help' or '?' in `Chat` mode, showing all available commands and key bindings.

For a one-off explanation in a shell or script, use `llm_cli ask "your
question"`. It uses the configured model and lightweight project identity, but
does not execute tools or make changes. Pass `--json` for a single structured
response instead of streamed text.

If the CLI cannot start, it explains whether Ollama must be installed or
started. Run `llm_cli init` again to retry setup, or `llm_cli doctor` for a
read-only readiness report covering Ollama, configured models, optional
developer tools, project tooling, and the shell audit-log path. Use
`llm_cli doctor --full` to include the embedding and intent-classifier models.
`llm_cli health --full` remains a focused compatibility check for the model
runtime.

Use `cd <directory>` inside the TUI to change its session directory. The CLI
then re-detects the project and Git root; `$ cd` and `! cd` use the same
persistent session change rather than a disposable shell child process.

**Essential keyboard shortcuts:**
- Type naturally and press Enter to chat
- Up/Down: Navigate input history
- Ctrl+S: Toggle between Chat and Shell modes
- Tab: Accept ghost text completion
- PgUp/PgDn: Scroll conversation
- Esc/Ctrl+C: Quit; `q` quits only when the input is empty

**Git workflows:**
Pre-configured git workflows are available when running inside a git repository.

```
status                    # Show git status and diffstat
save work                 # Stage, commit with generated message, and push
commit                    # Commit with generated message
stage all                 # Stage all changes
draft commit message      # Generate commit message from staged changes
```

If you forget commands or key bindings at any time, type `help` (or `?`) inside the app.

**File operations:**
```
show src/main.rs                         # Display file contents
list files in src                        # List directory
write to <file> with content: <content>  # Create/overwrite file
edit src/main.rs: add a --version flag    # Generate and review a checked patch
rollback last edit                         # Reverse the latest reviewed patch
```

**Code search:**
```
find todos               # Search TODO/FIXME (requires ripgrep)
```

**Shell mode:**
- Toggle with Ctrl+S to run shell commands directly
- `cd <directory>` changes the TUI session directory in either mode

**Bang shortcuts in Chat mode:**
```
$ ls -la                 # Execute shell command
! pwd                    # Execute shell command (alternative)
!!                       # Repeat last shell command
!cargo                   # Run last command starting with "cargo"
```

**Custom commands:**

When the LLM suggests shell commands, type:
- `y` or `yes`: Execute
- `s` or `save`: Save as learned command
- `n` or `no`: Skip

**Configuration (`.llm-cli/config.toml`):**

- Run `llm_cli setup` to create `.llm-cli/config.toml` without overwriting an existing file, or copy `config.example.toml`. Pass an explicit file with `--config /path/to/config.toml` before or after a subcommand, such as `llm_cli ask "explain this project" --config /path/to/config.toml`.
- When the file is missing, the CLI falls back to built-in defaults so it still works out of the box. Those defaults are:
  - `model = "llama3"`
  - `ollama_host = "127.0.0.1:11434"` (or an HTTPS API base)
  - `system_prompt = "Reply in concise bullets. Use short sentences. Break lines for each bullet. Be direct."`
  - `llm_timeout_secs = 45`
  - `cmd_timeout_secs = 60`
  - `max_context_tokens = 4096`
  - `streaming = true`
  - `request_timeout_secs = 60`
  - `generate_commit_message = true`
  - `history_path = ".llm-cli/history.jsonl"`
  - `audit_path = ".llm-cli/audit.jsonl"` (or `LLM_CLI_AUDIT_PATH`)
  - `embedding_cache_path = ".llm-cli/embeddings.toml"`
  - `embedding_model = "nomic-embed-text"`
  - `classifier_model = "qwen2:1.5b"`
  - `learned_path = ".llm-cli/learned.toml"`

You can also override any field temporarily with environment variables:
```bash
export LLM_CLI_MODEL="llama3:8b"
export LLM_CLI_STREAMING="true"
# Use a daemon on another reachable host or port.
export LLM_CLI_OLLAMA_HOST="192.0.2.10:11434"
# For Ollama Cloud, use an HTTPS base plus your API key:
# export LLM_CLI_OLLAMA_HOST="https://ollama.com"
# export OLLAMA_API_KEY="..."
```

All project-specific files are stored in `.llm-cli/` directory (history, learned commands, embeddings, frecency data).

An example config file with comments is provided as `config.example.toml`. Feel free to copy it to `.llm-cli/config.toml` and modify as needed.

---

## 5. Reproducibility Guide

### Prerequisites (Both Platforms)

- Rust toolchain (via rustup)
- Ollama 0.13+ installed and running
- At least one model pulled (we use `llama3`)
- Git (for repository features)
- Ripgrep (optional, for TODO search)

Run `bash scripts/health.sh` whenever you want to verify the setup—it checks for the Rust/Ollama toolchain plus the chat, embedding, and classifier models you plan to use.

### macOS Sonoma Setup

**1. Install Rust:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Restart terminal after installation
```

**2. Verify installation:**
```bash
rustc --version
cargo --version
```

**3. Install Ollama:**
```bash
brew install --cask ollama
```

**4. Pull models:**
```bash
ollama pull llama3
ollama pull nomic-embed-text
ollama pull qwen2:1.5b
```

**5. Install ripgrep (optional, used for 'find todos' only):**
```bash
brew install ripgrep
```

**6. Build and run:**
```bash
cd /path/to/project
cargo build
cargo run
```

### Ubuntu Linux Server Setup

**1. Install dependencies:**
```bash
sudo apt update
sudo apt install -y build-essential curl git
```

**2. Install Rust:**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

**3. Verify installation:**
```bash
rustc --version
cargo --version
```

**4. Install Ollama:**
```bash
curl -fsSL https://ollama.com/install.sh | sh
```

**5. Start Ollama (in separate terminal session):**
```bash
ollama serve
```

**6. Pull models (in second terminal):**
```bash
ollama pull llama3
ollama pull nomic-embed-text
ollama pull qwen2:1.5b
```

**7. Install ripgrep (optional):**
```bash
sudo apt install -y ripgrep
```

**8. Build and run:**
```bash
cd /path/to/project
cargo build
cargo run
```

### Optional Configuration

**Use a different model:**
```bash
export LLM_CLI_MODEL=llama3.2:3b
ollama pull llama3.2:3b
cargo run
```

**Custom config:**

Create `.llm-cli/config.toml` in your project directory (see User Guide for format).

### Quick Sanity Checks

After the UI opens:

1. **Test chat**: Type "describe this repo" and confirm streaming response appears
2. **Test history**: Press Up to recall last prompt, edit, and resend
3. **Test git workflow** (if in repo): Type `save work` to show git status then stage, commit with generated message, and push
4. **Test file operations**: Type `show Cargo.toml` or `list files in src`
5. **Test TODO search** (if ripgrep installed): Type `find todos`

---

## 6. Contributions by Each Team Member

### Ruitong Li

**Developer Tools:**
- Implemented shell command execution with subprocess management
- Built file operations (read, write, list) with safety boundaries
- Created tools registry for centralized definitions
- Added proper error handling and user-facing messages

**Configuration and Session:**
- Built hierarchical configuration system (defaults, TOML file, env vars)
- Implemented session state tracking (cwd, repo root, messages, outputs)
- Added project type detection integration
- Created message streaming state management

**Intent Resolution System:**
- Designed 3-tier intent resolution architecture
- Built Tier 2 keyword+embedding hybrid classifier
- Implemented Tier 3 LLM-based classifier
- Created embedding cache system for 10-20x startup speedup
- Documented system in `docs/INTENT_SYSTEM.md` and `docs/EMBEDDING_CACHE.md`

**Repository Features:**
- Implemented git workflows (status, save work, commit, stage, draft message)
- Built commit message generation from staged diffs
- Added TODO/FIXME scanning via ripgrep
- Implemented repository and project type detection

**Semantic Context Resolution:**
- Designed Cursor-style context injection system
- Implemented reference word detection and recent outputs tracking
- Enabled natural follow-up interactions ("commit it" after "show status")
- Documented in `docs/SEMANTIC_CONTEXT.md`

**Custom Command Learning:**
- Built command extraction from LLM responses
- Implemented learned aliases with TOML persistence
- Integrated custom workflows into Tier 1 for instant recall

**Testing and Documentation:**
- Added unit tests for key modules
- Wrote comprehensive documentation
- Created health check script and reproducibility guide
- Created the demo video

### Yingxuan Hu

**Ollama Integration and Streaming:**
- Implemented Ollama client with streaming output and health checks
- Built server-sent events parsing for real-time responses
- Added error categorization (unreachable vs model missing)

**Terminal UI System:**
- Designed and implemented full-screen Ratatui interface
- Built three-panel layout (conversation, input, status bar)
- Implemented text wrapping, scroll management, and ghost text rendering
- Created `TerminalGuard` RAII pattern for reliable cleanup

**Application Core:**
- Architected main event loop and state coordination
- Implemented async channel-based background task system
- Built keyboard and mouse event handling
- Created workflow state machine for multi-step confirmations
- Added graceful shutdown handling

**Input Processing:**
- Implemented mode-aware input history with Up/Down navigation
- Built bang shortcuts (`!!`, `!prefix`) with bash-style expansion
- Improved shell shortcut handling
- Created input deduplication

**Completion System:**
- Designed ghost text completion provider
- Built frecency-based ranking for suggestions
- Integrated with history, learned commands, and tool examples
- Added Tab key acceptance

**Mode Switching and Polish:**
- Implemented Chat/Shell mode toggle with visual indicators
- Added shell command shortcuts (`$`, `!`) in Chat mode
- Fixed terminal state issues, race conditions, and scroll bugs
- Improved error messages throughout codebase

**Testing and Documentation:**
- Added unit tests for key modules
- Wrote comprehensive documentation
- Created health check script and reproducibility guide
- Created the slide presentation video

---

## 7. Lessons Learned and Concluding Remarks

**Streaming output matters for perceived performance.** Even when a model takes the same time to finish, showing text as it arrives makes the tool feel faster and more responsive. Implementing streaming pushed us to design clearer separation between rendering, state, and background work.

**Agentic behavior must be trustworthy.** Developers are comfortable with automation when they can predict outcomes and execution is intentional. Our plan-and-execute approach with confirmations demonstrates that multi-step workflows can be powerful while keeping users in control.

**Repository awareness doesn't need complexity.** Simply detecting the repo root, exposing status/diff previews, and generating commit messages from staged diffs already supports common tasks effectively. This incremental approach delivers value early and provides a clear path for future enhancements.

**Tiered intent resolution balances speed and flexibility.** By combining fuzzy matching (< 1ms), semantic embeddings (~50ms), and LLM classification (~500ms) with natural fallback to chat, we created a system that feels fast for common commands while gracefully handling novel phrasing. Most users develop habits, so optimizing for the common case while providing flexibility for exploration creates a system that adapts to user behavior over time.

**Terminal UI stability requires attention to detail.** Handling raw mode correctly, keeping layout consistent, wrapping text, truncating large outputs, and showing clear error messages all matter for reliability. These details became especially important because the tool must remain readable while actively streaming output.

**Startup performance has outsized impact.** The embedding cache system's 10-20x speedup, fundamentally changing how willing we were to restart it frequently. Performance optimizations that improve iteration speed matter significantly for developer tools.

**Simple heuristics can enable natural interaction.** Semantic context resolution via reference detection doesn't require sophisticated NLP. Simple string matching for pronouns and explicit references combined with a small queue of recent outputs created a surprisingly effective system for follow-up interactions.

**Clear boundaries between LLM and deterministic code matter.** The model is strongest at explanation and summarization, while deterministic commands are best for actions like reading files, searching code, and running git operations. Keeping those roles separate made the tool easier to reason about and safer to use.

**Future directions** include deeper repository analysis (AST parsing, symbol indexing), multi-file editing with diff previews, integration with external tools via MCP (Model Context Protocol), collaborative features (sharing learned commands), and performance optimization for large repositories.

The project achieved its goal of creating a lightweight, Rust-native LLM-powered CLI that feels natural in developer workflows. More importantly, it demonstrated that local inference, semantic intent matching, and careful UX design can combine to create tools that augment rather than replace human decision-making.
