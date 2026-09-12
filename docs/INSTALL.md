# Install and First Run

`llm_cli` is a local terminal application. It needs Ollama to run local
models, but does not require an API key or cloud account. Building from source
also needs Rust; release binaries do not.

## Fast path

Install the latest verified binary on macOS or Linux x86_64:

```bash
curl -fLO https://raw.githubusercontent.com/YingxuanHu/Rust-CLI/main/scripts/install.sh
bash install.sh --binary
export PATH="$HOME/.local/bin:$PATH"
```

Install and open [Ollama](https://ollama.com). On the first launch, `llm_cli`
checks the local runtime and asks before downloading its chat model. Add the
PATH line to your shell's startup file to keep it in future terminals.

**Version note:** v0.1.0 predates `ask`, shell completion, and the current review
fixes. These newer features require installing current source until the next
release: `cargo install --path . --locked` from this repository.

Then, in the repository where you want to use the assistant:

```bash
llm_cli
```

The CLI opens with a short set of context-aware starter actions, so no command
syntax is required before asking your first question. Use `llm_cli init --full`
when you also want its optional embedding and intent-classifier models.
`setup` is optional: it creates `.llm-cli/config.toml` without replacing an
existing file. Use `llm_cli setup --force` only when you intentionally want a
fresh starter configuration. Run `llm_cli doctor --full` only if startup
reports a problem or you want a read-only readiness report for the configured
models, Ollama, project tooling, Git, ripgrep, and the audit-log location.
`health --full` remains available when you only want to check all three
configured local models.

For a single answer without opening the full-screen interface, use:

```bash
llm_cli ask "explain Rust ownership"
llm_cli ask "give me a concise testing checklist" --json
```

Normal output streams directly to the terminal. `--json` prints one JSON object
for scripts (and returns a JSON error with status 2 for setup, configuration,
or runtime failures). Piped and redirected invocations do not prompt for model
downloads; run `llm_cli init` first if setup is incomplete.
`ask` is read-only and does not execute natural-language requests as commands
or workflows.

In the interactive assistant, `tasks` lists the detected test/build commands.
Tests, builds, shell commands, and built-in Git commands show live output while
keeping input available. Ctrl+C (or `cancel task`) stops the active command
without exiting; when no command task is active, Ctrl+C exits. Esc exits after
bounded cleanup. Cancellation does not undo completed file, Git, or remote
changes. Tasks use `cmd_timeout_secs`, defaulting to 60 seconds; set a larger
value in `.llm-cli/config.toml` for longer work.

Shell tasks have closed stdin and no PTY. Detection of interactive commands is
heuristic; use a normal terminal for commands that need prompts or editors.

These controls require current source until a newer release is published.
TODO searches and file/patch operations still have synchronous work. Pending
model preparation and model chat do not yet have independent cancellation.

## Binary release install (no Cargo required)

Download the installer and run it locally (rather than piping it to a shell):

```bash
curl -fLO https://raw.githubusercontent.com/YingxuanHu/Rust-CLI/main/scripts/install.sh
bash install.sh --binary
```

The installer selects the matching macOS (Apple Silicon or Intel) or Linux
x86_64 archive, downloads the release's `SHA256SUMS`, verifies the archive
before installing it, and places `llm_cli` in `~/.local/bin` by default. Use
`--version vX.Y.Z` for a specific release and `--install-dir DIRECTORY` to
choose a different location. Windows releases are published as `.zip` files;
extract the matching archive and add its folder to `PATH`.

Direct shell activity is recorded locally after commands run. Inspect it with
`llm_cli audit` (the latest 20 records) or `llm_cli audit --tail 0` (all
records); neither command modifies the audit log.

## Shell completion

The command can complete its subcommands, flags, and shell names with Tab. It
does not alter shell startup files automatically. Choose the command for your
shell:

For Bash, enable this terminal; add the same line to `~/.bashrc` to persist it:

```bash
source <(llm_cli completions bash)
```

For Zsh, generate into a user-owned directory:

```zsh
mkdir -p ~/.zsh/completions
llm_cli completions zsh > ~/.zsh/completions/_llm_cli
```

Add these lines to `~/.zshrc`, placing `fpath` before any existing `compinit`:

```zsh
fpath=(~/.zsh/completions $fpath)
autoload -Uz compinit && compinit
```

For Fish:

```fish
mkdir -p ~/.config/fish/completions
llm_cli completions fish > ~/.config/fish/completions/llm_cli.fish
```

For PowerShell, add the following line to your PowerShell profile to enable it
in future sessions:

```powershell
llm_cli completions powershell | Out-String | Invoke-Expression
```

`--config` is global, so it works before or after a subcommand. For example:

```bash
llm_cli ask "explain Rust ownership" --config /path/to/config.toml
```

## Manual path

1. Install Rust through [rustup](https://rustup.rs).
2. Install and start [Ollama](https://ollama.com).
3. Install the checked-out source:

   ```bash
   cargo install --path . --locked
   ```

4. Run `llm_cli` from the project directory. It will offer to download its
   missing chat model. Use `llm_cli init --full` if you also want the optional
   routing models.

## Verification and troubleshooting

Use `llm_cli --help` for CLI help and `llm_cli doctor --full` for the complete
read-only dependency check. The repository also provides
`bash scripts/health.sh`, which checks the source-tree toolchain and default models.

If `llm_cli` is not found after a binary install, use
`export PATH="$HOME/.local/bin:$PATH"`. For a Cargo source install, use
`export PATH="$HOME/.cargo/bin:$PATH"` (or the custom install location).
If the embedding or classifier model is missing, chat still starts, but intent
matching falls back to the remaining tiers.
