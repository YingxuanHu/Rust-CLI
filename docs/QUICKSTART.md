# Quick Start

Install with the [installation guide](INSTALL.md), open Ollama, and start
`llm_cli` in your project. The first launch offers to download the configured
chat model. Optional models can be added later with `llm_cli init --full`.

## First useful actions

Type `show me around`. The assistant suggests actions based on your detected
project. Start with `project info`, `what changed`, or `run tests`.

You can read a file with `show src/main.rs`, list a directory with `list files
in src`, or ask a general question. The assistant currently has only limited
project context; it does not automatically read or understand the entire repo.

## Prepare work for a local commit

1. Use `what changed` to inspect status. If a diff prompt is open, press Enter
   to close it before starting another task.
2. Use `stage src/main.rs` to review and stage a specific path, or `stage all`
   when you intend to include every change.
3. Use `commit` to review the suggested message. Accept it, enter a replacement,
   or type `cancel`. This commits staged changes locally.

`save locally` means the same staged-only commit. `save work` is the separate
combined workflow that stages all changes, prepares a message, commits, and
pushes. Review its plan before proceeding.

## Make a reviewed edit

```text
edit src/main.rs: improve the error message
```

Wait for the proposed diff, review it, and accept or cancel. `rollback last
edit` checks and reverses the most recent reviewed edit in this session. Edits
are single-file; tests are not run automatically after applying one.

## Quick questions without the full-screen interface

Available in current source; release v0.1.0 predates these commands:

```bash
llm_cli ask "explain Rust ownership"
llm_cli ask "give me a concise testing checklist" --json
```

Plain output streams. JSON mode emits one response object, or an error object
and exit status 2 for setup/configuration/runtime failures. `ask` never executes
tools. Files and stdin are not automatically attached as context.

## Input and setup shortcuts

- Tab accepts ghost text; Up/Down recalls prior inputs.
- Ctrl+S switches between chat and direct shell input.
- `cd /path/to/project` changes the session directory.
- `help` lists supported actions. Esc exits; Ctrl+C cancels active tests without
  exiting, or exits when no test task is running.
- `llm_cli init` retries setup; `llm_cli doctor --full` explains prerequisites.
- `llm_cli setup` creates an optional commented project config.

For a different model, set `LLM_CLI_MODEL` or the `model` field in
`.llm-cli/config.toml`. Use an installed model tag. The embedding and classifier
models are optional; their absence should not prevent basic chat.

If an action is misunderstood, use the explicit forms above. For example,
`stage README.md` is unambiguous, while `add this file` lacks a named file and
will not automatically stage everything. Inspect saved aliases if the same
phrase keeps selecting an unexpected workflow.

See [intent routing](INTENT_SYSTEM.md) for implementation details and
[the review](PROJECT_REVIEW.md) for limitations and planned improvements.
