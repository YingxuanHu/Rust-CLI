# Quick Start

Install with the [installation guide](INSTALL.md), open Ollama, and start
`llm_cli` in your project. The first launch offers to download the configured
chat model. Optional models can be added later with `llm_cli init --full`.

## First useful actions

Type `show me around`. The assistant suggests actions based on your detected
project. Use `tasks` to see the actual test/build commands before starting one.
Then try `project info`, `what changed`, `run tests`, or `build`.

You can read a file with `show src/main.rs`, list a directory with `list files
in src`, or ask a general question. The assistant currently has only limited
project context; it does not automatically read or understand the entire repo.

These instructions describe current source. Release v0.1.0 predates the
background-task and discovery improvements; see the installation guide's
source-install option until a newer release includes them.

## Run, inspect, and explain a task

1. Enter `tasks` to inspect the available actions. Missing scripts are reported
   as not configured, not run as successful no-op commands.
2. Enter `run tests` or `build`. The task shows its command, working directory,
   live stdout/stderr, elapsed time, and completion status.
3. If it fails, ask `why did tests fail?`, `last build error`, or
   `explain the output`. The model receives labeled recent output excerpts.

Tests, builds, shell commands, and built-in Git commands use the same background
runner. Ctrl+C or `cancel task` stops the active command without closing the
assistant. Esc exits after bounded cleanup. Cancellation is not rollback: any
completed file changes, staging, commits, or remote effects remain possible.

Only one command task runs at a time. Chat questions, help, and directory changes
remain available. Additional commands are not queued; enter them after the
current task finishes.
Tasks default to a 60-second timeout; configure `cmd_timeout_secs` for longer
work. Shell tasks close stdin and do not provide a PTY. Interactive-command
warnings are heuristic, so commands requiring an editor or prompt belong in
your normal terminal even when no warning appears.

Node tasks use nonempty `scripts.test`/`scripts.build` and the declared package
manager, then unambiguous local lockfiles, then npm if neither exists. Conflicting
managers are not guessed. An ancestor workspace's manager is not inferred for
a nested package. See [project detection](REPO_AWARENESS.md) for the full rules.

The assistant keeps at most five recent outputs, with bounded head/tail excerpts
for long output. Context is session-only and cleared when directories change;
explanations may not include omitted middle sections or earlier commands.

## Prepare work for a local commit

1. Use `what changed` to inspect status and the unstaged diff summary. It does
   not open a modal prompt. Use `diff src/main.rs` for that path's unstaged diff.
2. Use `stage src/main.rs` to review and stage a specific path, or `stage all`
   when you intend to include every change.
3. Use `commit` to review the suggested message. Accept it, enter a replacement,
   or type `cancel`. This commits staged changes locally.

`diff <path>` runs from the Git root when one is detected. Its argument is
repository-relative and follows Git pathspec semantics, not literal-only file
matching. Use an explicit shell command for staged diff inspection when needed.

`save locally` means the same staged-only commit. `save work` is the separate
combined workflow. It shows status, asks before staging **all** changes, prepares
a message for review, and commits locally. Only a separate `yes` at the final
push prompt publishes the commit; Enter or any other response keeps it local.
If interrupted, inspect status/log before retrying: cancelling does not unstage
files or remove a commit that already succeeded.

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
- `help` lists supported actions. Esc exits; Ctrl+C cancels an active command
  without exiting, or exits when no command task is running. Pending model
  preparation does not yet have its own cancellation control.
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
