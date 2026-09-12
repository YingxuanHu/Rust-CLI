# CLI Assistant

A Rust terminal assistant for everyday development tasks: understand a project,
check changes, run tests, prepare commits, and review small edits using local
Ollama models.

The aim is to reduce repeated command lookup and context switching. Familiar
actions use built-in workflows; open-ended questions use the configured model.
This is an actively developed project, with important limitations listed below.

## Install and start

For macOS or Linux x86_64, download the installer, install a verified release,
and put its binary directory on your current terminal's PATH:

```bash
curl -fLO https://raw.githubusercontent.com/YingxuanHu/Rust-CLI/main/scripts/install.sh
bash install.sh --binary
export PATH="$HOME/.local/bin:$PATH"
llm_cli
```

Install and open [Ollama](https://ollama.com) if it is not already available.
The first launch checks the configured chat model and asks before downloading
it. Run from the project you want to work on. Type `show me around` for starter
actions. Esc exits; Ctrl+C cancels an active test task, or exits when none is running.

Windows archives and platform details are in the
[installation guide](docs/INSTALL.md). Binary releases can lag behind `main`:
**v0.1.0 does not include `ask`, shell completions, or the fixes documented in
the current engineering review.** Check your version with `llm_cli --version`.
To use the current source, from a clone with Rust installed:

```bash
cargo install --path . --locked
```

## Everyday tasks

Enter these in the interactive assistant:

| What you need | What to type | What happens |
| --- | --- | --- |
| Get oriented | `show me around` | Suggestions based on the detected project |
| Understand the project | `project info` | Manifest-derived project information |
| Inspect changes | `what changed` | Git status and a diff summary |
| Check the project | `run tests` | Live test output, elapsed time, and pass/fail status |
| Prepare one file | `stage src/main.rs` | Review and stage the named path |
| Commit staged work locally | `commit` or `save locally` | Review a message and commit; no push |
| Stage, commit, and push | `save work` | Review the combined Git workflow |
| Read a file | `show src/main.rs` | Display that file |
| Propose a small edit | `edit src/main.rs: describe the change` | Review a checked single-file diff |
| Reverse that edit | `rollback last edit` | Check and reverse the latest reviewed patch |
| Find unfinished work | `find todos` | Search TODO/FIXME comments with ripgrep |

Generated command suggestions can be reviewed, executed, skipped, or saved as
learned workflows. Tab accepts ghost text; Up/Down recalls inputs. Ctrl+S
switches between Chat and Shell modes. `cd path/to/project` changes the session
directory. `help` lists available actions.

`run tests` keeps the interface responsive and shows the command and its working
directory. Press Ctrl+C or type `cancel task` to stop it without exiting. Output
retains the latest 64 KiB from each of stdout and stderr, with a truncation
notice when needed. Tests use `cmd_timeout_secs` (60 seconds by default); increase
it in your configuration for longer suites. You can ask questions in Chat mode,
use help, or change directories while tests run; other command workflows wait
until the task finishes. Running `run tests` again uses the current project.

For one answer in the current terminal (current source):

```bash
llm_cli ask "explain Rust ownership"
llm_cli ask "give me a concise testing checklist" --json
```

`ask` streams plain text or emits one JSON object. Runtime/setup/configuration
failures emit an `error` object in JSON mode with exit status 2. It does not
execute workflows. Its project context is limited to name and type; it does
not inspect source files or attach piped input automatically.

## Setup when needed

Start with `llm_cli`. These commands are available for specific needs:

| Command | Purpose |
| --- | --- |
| `llm_cli init` | Retry model setup |
| `llm_cli init --full` | Also download optional embedding/classifier models |
| `llm_cli doctor --full` | Read-only setup and project-tooling report |
| `llm_cli setup` | Create a commented `.llm-cli/config.toml` |
| `llm_cli audit` | Inspect recent direct shell executions |
| `llm_cli completions zsh` | Generate shell completion definitions |

Defaults: chat `llama3`, embeddings `nomic-embed-text`, classifier `qwen2:1.5b`,
Ollama at `127.0.0.1:11434`. Only the chat model is required. Use a project
configuration or environment variables to select different models/endpoints:

```bash
LLM_CLI_MODEL=your-installed-model llm_cli
llm_cli ask "explain Rust ownership" --config /path/to/config.toml
```

An explicit missing configuration file is an error. Without `--config`, the
CLI looks for `.llm-cli/config.toml` in the launch directory. Project-wide
ancestor discovery and user-wide defaults are planned.

## Current boundaries

- Project awareness detects manifests and Git roots; it is not a source index
  or a full repository understanding system.
- Reviewed edits cover one existing file. There is no automatic test-and-repair
  loop or durable multi-file rollback.
- Tests now stream in the background with cancellation. Git, shell, build,
  and file workflows still need migration to the background runner. Model-chat
  cancellation remains planned.
- Test cancellation stops the process group on macOS/Linux; processes that
  deliberately leave that group are not covered. Windows currently stops only
  the direct test process, not its descendants.
- Command-risk checks are heuristic, not a sandbox. Review the displayed
  command. The audit log covers direct shell execution, not every workflow step.
- Input history can contain the text you entered, including shell arguments.
  Avoid entering secrets; log redaction is incomplete.
- Shell workflows use POSIX `sh`. Windows binaries are built, but native Windows
  workflow behavior needs broader testing.
- Cached intent examples now avoid repeated embedding calls. No end-to-end
  speedup percentage is claimed without reproducible measurements.

The [project review and roadmap](docs/PROJECT_REVIEW.md) covers each subsystem,
current competing tools, remaining defects, and the next implementation phases.

## Development

```bash
cargo check --locked
cargo test --locked
```

Tests include local Git fixtures, fake HTTP responses, and process-level checks;
they do not require downloading or running an LLM. Live model quality, clean
machine installation, and complete interactive workflows need separate testing.

Useful guides: [installation](docs/INSTALL.md), [quick start](docs/QUICKSTART.md),
[intent routing](docs/INTENT_SYSTEM.md), [embedding cache](docs/EMBEDDING_CACHE.md).

Created by Ruitong Li and Yingxuan Hu. The original attribution, demo links,
and course submission are preserved in [the project report](docs/PROJECT_REPORT.md).
