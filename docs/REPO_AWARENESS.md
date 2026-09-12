# Project Detection and Tasks

This guide describes current source. Release v0.1.0 predates the current task
discovery and background execution improvements.

## Start with `tasks`

Run `llm_cli` inside your project and type `tasks`. The assistant lists the
detected root and available test/build commands without executing them. Type
`run tests` or `build` to start an available action. `project info` shows the
project type, name when available, root, and conventional source directories.

Commands run in the detected project root, which may differ from your current
subdirectory. The task entry displays the actual command and working directory.
No package manager or test runner is installed automatically by these actions.

## Which project is selected?

Detection checks each directory for supported manifests before moving to its
parent. A nested Node frontend therefore wins over a more distant Rust
workspace. When multiple supported manifests occupy the same directory, the
tie-break order is Rust, Node, Python, then Go.

| Project | Manifest | Test task | Build task |
| --- | --- | --- | --- |
| Rust | `Cargo.toml` | `cargo test` | `cargo build` |
| Node | `package.json` | `<manager> run test`, if the script exists | `<manager> run build`, if the script exists |
| Python | `pyproject.toml` or `setup.py` | `pytest` | Not configured |
| Go | `go.mod` | `go test ./...` | `go build` |
| Unrecognized | No supported manifest | Not configured | Not configured |

Project names are parsed from Cargo's `[package].name` or the top-level JSON
`name`, not guessed from arbitrary matching lines. A malformed manifest can
still identify a project type, but does not produce an invented name. Invalid
Node JSON cannot supply a runnable script.

Project metadata refreshes on an in-app `cd`. Node task selection rereads the
package manifest and lockfiles, so changing scripts does not require restarting
the assistant. Git-root detection is separate from project-manifest detection.

## Node package-manager rules

Only nonempty string values in `scripts.test` and `scripts.build` produce tasks.
The assistant does not invent test/build scripts or use Bun's built-in test
runner in place of the declared script.

Selection uses the detected project's own directory:

1. A supported `packageManager` declaration such as `pnpm@9.15.0` selects npm,
   pnpm, Yarn, or Bun and takes precedence over lockfiles.
2. Otherwise, an unambiguous lockfile selects the manager: `pnpm-lock.yaml`,
   `yarn.lock`, `bun.lock`/`bun.lockb`, or
   `package-lock.json`/`npm-shrinkwrap.json`.
3. Without a declaration or lockfile, npm is the default.

Conflicting manager lockfiles, malformed declarations, and unsupported declared
managers leave the task unconfigured instead of silently choosing another tool.
Multiple lockfiles belonging to the same manager are not considered a conflict.
The declaration selects a command name; the assistant does not install or pin
that manager's version.

**Workspace boundary:** an ancestor workspace's manager or lockfile is not
inferred for a nested package. Declare its manager locally or run an explicit
command when necessary. Workspace membership, project overrides, Python virtual
environment/uv detection, and additional task types such as lint are future work.

## Execution and limitations

Tests and builds stream bounded stdout/stderr with elapsed time and an exit
result. Ctrl+C or `cancel task` cancels the active command; Esc exits after
bounded cleanup. Only one command task runs at a time. Cancellation does not
undo completed changes. Child stdin is closed and no PTY is provided. Warnings
catch some interactive commands, not every editor or prompt; use your normal
terminal for commands requiring input.

Manifest identity is not a repository source index. The assistant does not
automatically read README files, inspect every script's behavior, or verify
that dependencies are installed. `tasks` is a read-only listing, not yet a
searchable picker. Run tasks only in projects whose code you intend to execute.

See [quick start](QUICKSTART.md) for daily workflows and
[recent-output context](SEMANTIC_CONTEXT.md) for failure explanations.
