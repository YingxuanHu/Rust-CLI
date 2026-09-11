# Install and First Run

`llm_cli` is a local terminal application. It needs a Rust toolchain to build
and Ollama to run its models; it does not require an API key or cloud account.

## Fast path

From a checked-out copy of this repository:

```bash
bash scripts/install.sh --with-models
```

That installs the binary with Cargo and pulls the default chat, embedding, and
intent-classifier models. Omit `--with-models` if the models already exist or
you want to choose different ones.

Then, in the repository where you want to use the assistant:

```bash
llm_cli setup
llm_cli doctor --full
llm_cli
```

`setup` creates `.llm-cli/config.toml` without replacing an existing file.
Use `llm_cli setup --force` only when you intentionally want a fresh starter
configuration. `doctor --full` gives a read-only readiness report for the
configured models, Ollama, project tooling, Git, ripgrep, and the audit-log
location. `health --full` remains available when you only want to check all
three configured local models.

## Binary release install (no Cargo required)

Once this repository has a versioned GitHub release, download the installer and
run it locally (rather than piping it to a shell):

```bash
curl -fLO https://raw.githubusercontent.com/YingxuanHu/Rust-CLI/main/scripts/install.sh
bash install.sh --binary --with-models
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

## Manual path

1. Install Rust through [rustup](https://rustup.rs).
2. Install and start [Ollama](https://ollama.com).
3. Pull the default models:

   ```bash
   ollama pull llama3
   ollama pull nomic-embed-text
   ollama pull qwen2:1.5b
   ```

4. Install the checked-out source:

   ```bash
   cargo install --path . --locked
   ```

5. Run the three commands in the Fast path above from the project directory.

## Verification and troubleshooting

Use `llm_cli --help` for CLI help and `llm_cli doctor --full` for the complete
read-only dependency check. The repository also provides
`bash scripts/health.sh`, which checks the source-tree toolchain and default models.

If `llm_cli` is not found after installing, ensure Cargo's binary directory is
on your `PATH` (rustup prints the required setup command during installation).
If the embedding or classifier model is missing, chat still starts, but intent
matching falls back to the remaining tiers.
