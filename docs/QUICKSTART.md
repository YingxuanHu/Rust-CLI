# Quick Start Guide

For the shortest install path, see [INSTALL.md](INSTALL.md). This page focuses
on intent routing after the CLI is installed.

## Prerequisites

Make sure you have Ollama installed and running:

```bash
# Check if Ollama is running
ollama list

# If not running, start it
ollama serve
```

## Required Models

Download the required models for the intent system:

```bash
# For embeddings (Tier 2)
ollama pull nomic-embed-text

# For LLM classification (Tier 3)
ollama pull qwen2:1.5b      # Default: balanced speed and accuracy
```

## Build and Run

```bash
# Build the project
cargo build --release

# Run the TUI
./target/release/llm_cli

# Or just:
cargo run --release
```

## First Run

On first run, you'll see:

```
Initializing embedding cache (this may take a moment)...
Embedding cache ready!
LLM CLI ready. Model: llama3 (embeddings: ready). 
Modes: Chat/Shell (Ctrl+S). History: ↑/↓. Enter to submit; Esc or empty-input q to exit.
```

This creates `.llm-cli/embeddings.toml` (takes a few seconds, only happens once per project).

## Try It Out

### Tier 1: Exact/Fuzzy Matching (< 1ms)

```
> status
→ Instantly shows git status

> stauts
→ Fuzzy matches to "status" (typo tolerance)

> save work
→ Instantly matches "save_work" tool
```

### Tier 2: Natural Language (~50ms)

```
> push my changes
→ Matches "save_work" via keyword + embedding

> what changed
→ Matches "status" via semantic similarity

> upload code
→ Matches "save_work"
```

### Tier 3: Novel Phrasing (~500ms)

```
> ship it to production
→ LLM classifies as "save_work"

> show me the todos
→ LLM classifies as "find_todos"
```

### Saving a command suggested by chat

When a normal chat response includes recognizable shell commands, the CLI
shows a confirmation prompt. Type `y` to execute them, `s` to save the command
sequence as a reusable workflow for the same original phrase, or `n` to skip.
Saved workflows are matched before the normal intent tiers on later use.

## Configuration

### Optional: Customize Your Config

Create `.llm-cli/config.toml` in your project directory:

```toml
# Main chat model
model = "llama3"

# Embedding model for Tier 2
embedding_model = "nomic-embed-text"

# Classifier model for Tier 3 (choose based on speed vs accuracy)
classifier_model = "qwen2:1.5b"    # Recommended (default)
# classifier_model = "qwen2:0.5b"  # Faster
# classifier_model = "phi3:mini"   # More accurate

# Optional: custom paths (all default to .llm-cli/ directory)
# learned_path = ".llm-cli/learned.toml"
# embedding_cache_path = ".llm-cli/embeddings.toml"
# history_path = ".llm-cli/history.jsonl"
```

### Debug Logging

The application uses structured logging via the `tracing` crate. By default, only `info`, `warn`, and `error` messages are shown. To enable debug logging (useful for troubleshooting intent resolution, LLM classification, and embedding cache):

```bash
# Show all debug messages
RUST_LOG=debug cargo run --release

# Show debug messages for specific modules
RUST_LOG=llm_cli::intent=debug,llm_cli::llm_classifier=debug cargo run --release

# Show trace-level messages (very verbose)
RUST_LOG=trace cargo run --release

# Or set it before running
export RUST_LOG=debug
./target/release/llm_cli
```

Debug logs include:
- **Intent Resolution**: Which tier matched your input and why
- **LLM Classifier**: API calls to Ollama and response parsing
- **Keyword Classifier**: Scoring details for each tool
- **Embedding Cache**: Cache loading and saving operations

## Shell Commands

Shell commands bypass all tiers for instant execution:

```
$ ls -la              # Direct shell execution
$ git status          # Direct shell execution
! pwd                 # Alternative prefix
!!                    # Repeat last shell command
```

## View Learned Aliases and Workflows

```bash
# Tool aliases
cat .llm-cli/learned.toml

# Custom workflows
cat .llm-cli/custom_workflows.toml
```

Example **learned.toml** (tool mappings):

```toml
[[aliases]]
phrase = "yeet"
tool = "save_work"
timestamp = "1702053600"
source = "user_feedback"
```

Example **custom_workflows.toml** (shell command workflows):

```toml
[[custom_workflows]]
phrase = "deploy staging"
command = "ssh staging 'cd /app && git pull'"
timestamp = "1702053700"
source = "user_custom_generated"
```

## Project-Specific Storage

All data is stored per-project in the `.llm-cli/` directory:
- **learned.toml**: Tool aliases (e.g., "yeet" → save_work)
- **custom_workflows.toml**: Custom shell workflows
- **embeddings.toml**: Cached embeddings for faster intent matching
- **history.jsonl**: Submitted-input history for recall and completion
- **config.toml**: Project-specific configuration (optional)

This means each project can have its own learned commands and history!

## Tips

1. **Save useful suggestions** - Use `s` at a chat command confirmation to make a reusable workflow.

2. **Use natural language** - The system understands:
   - "push my code"
   - "what's going on"
   - "ship it"
   - "show me todos"

3. **Typos are fine** - Fuzzy matching handles:
   - "stauts" → "status"
   - "comit" → "commit"
   - "statsu" → "status"

4. **Shell commands stay fast** - Always use `$` prefix for shell commands to skip tiers.

5. **Check which tier matched** - Fast responses (< 50ms) = Tier 1 or 2. Slower (~500ms) = Tier 3.

## Troubleshooting

### A request falls back to chat instead of a tool

**Option 1:** Lower the confidence threshold in `src/keyword_classifier.rs`:
```rust
const CONFIDENCE_THRESHOLD: f32 = 0.6;  // Default: 0.7
```

**Option 2:** Use a better classifier model:
```toml
classifier_model = "qwen2:1.5b"  # or "phi3:mini"
```

### Tier 3 is too slow

Use the fastest classifier:
```toml
classifier_model = "qwen2:0.5b"
```

### Wrong tool keeps matching

Check and edit learned aliases:
```bash
cat .llm-cli/learned.toml
# Remove incorrect entries, then save a corrected workflow from a chat suggestion.
```

### Embeddings initialization is slow

This only happens once per project. The cache is stored in `.llm-cli/embeddings.toml` and reused on subsequent runs.

## Performance Expectations

| Query Type | Tier | Latency | Example |
|-----------|------|---------|---------|
| Exact match | 1 | < 1ms | "status" |
| Learned alias | 1 | < 1ms | "yeet" |
| Typo | 1 | < 1ms | "stauts" |
| Natural language | 2 | ~50ms | "push my changes" |
| Novel phrasing | 3 | ~500ms | "ship it to prod" |
| Unknown | Chat | Model response | Conceptual question or unsupported task |

Saved custom workflows move to Tier 1 on later use.

## Next Steps

- Read [INTENT_SYSTEM.md](INTENT_SYSTEM.md) for detailed architecture
- Read [AUTOCOMPLETION.md](AUTOCOMPLETION.md) for ghost-text behavior
- Read [SEMANTIC_CONTEXT.md](SEMANTIC_CONTEXT.md) for reference handling
- Customize your config in `.llm-cli/config.toml` (optional)
- Run `llm_cli health --full` before troubleshooting a model issue.

The system stays local, with deterministic routing for supported actions and
chat as the fallback for everything else.
