# Quick Start Guide: Tiered Intent System

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

# For LLM classification (Tier 3) - choose one:
ollama pull qwen2:0.5b      # Recommended: Fastest
# OR
ollama pull qwen2:1.5b      # Better accuracy
# OR
ollama pull phi3:mini       # Best accuracy
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
Modes: Chat/Shell (Ctrl+S). History: ↑/↓. Enter to submit; Esc/q to exit.
```

This will create the embedding cache in `.cache/embeddings.toml` (takes a few seconds, only happens once per project).

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

### Tier 4: Learning

**Option 1: Map to existing tool**
```
> yeet
→ "I'm not sure what you want to do with: 'yeet'"
→ "Did you mean:"
→ "  [1] save_work - Stage all changes, commit, and push"
→ "  [2] status - Show git status"
→ "  ..."

> 1
→ "✓ Learned: 'yeet' → save_work"
→ Executes save_work

Next time:
> yeet
→ Instantly matches (< 1ms via Tier 1)
```

**Option 2: Define custom macro**
```
> deploy staging
→ "I'm not sure what you want to do with: 'deploy staging'"
→ "Options:"
→ "  • Type a number to select a tool"
→ "  • Type 'macro: <commands>' to define a shell command sequence"
→ "  • Type 'none' to skip"

> macro: ssh staging 'cd /app && git pull && systemctl restart app'
→ "✓ Learned macro: 'deploy staging' → ssh staging 'cd /app && git pull && systemctl restart app'"
→ Executes the macro

Next time:
> deploy staging
→ Instantly executes your custom command (< 1ms via Tier 1)
```

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

## View Learned Aliases and Commands

```bash
# Tool aliases
cat .llm-cli/learned.toml

# Custom commands
cat .llm-cli/custom_commands.toml
```

Example **learned.toml** (tool mappings):

```toml
[[aliases]]
phrase = "yeet"
tool = "save_work"
timestamp = "1702053600"
source = "user_feedback"
```

Example **custom_commands.toml** (shell commands):

```toml
[[custom_commands]]
phrase = "deploy staging"
command = "ssh staging 'cd /app && git pull'"
timestamp = "1702053700"
source = "user_custom_generated"
```

## Project-Specific Storage

All data is stored per-project in the `.llm-cli/` directory:
- **learned.toml**: Tool aliases (e.g., "yeet" → save_work)
- **custom_commands.toml**: Custom shell commands
- **embeddings.toml**: Cached embeddings for faster intent matching
- **history.jsonl**: Conversation history
- **config.toml**: Project-specific configuration (optional)

This means each project can have its own learned commands and history!

## Tips

1. **Let it learn** - When uncertain, select the right tool. Next time it'll be instant.

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

### "I'm not sure what you want to do" appears often

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
# Remove incorrect entries and re-learn
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
| Unknown | 4 | Manual | User selects |

Over time, more queries move to Tier 1 through learning!

## Macros (Custom Commands)

Define your own shell command sequences:

```
> backup database
→ "I'm not sure what you want to do..."

> macro: pg_dump mydb > backup_$(date +%Y%m%d).sql && echo "Done"
→ "✓ Learned macro"
→ Executes backup

Next time:
> backup database
→ Runs your backup command instantly
```

See [MACROS.md](MACROS.md) for detailed macro documentation.

## Next Steps

- Read [INTENT_SYSTEM.md](INTENT_SYSTEM.md) for detailed architecture
- Read [MACROS.md](MACROS.md) for custom command workflows
- Read [IMPLEMENTATION_SUMMARY.md](IMPLEMENTATION_SUMMARY.md) for technical details
- Customize your config in `.llm-cli/config.toml` (optional)
- Start using and let it learn your preferences!

## Example Session

```
> status
[Tier 1] → git status output

> push my work
[Tier 2] → Planned git workflow: ...

> yeet this code
[Tier 4] → I'm not sure what you want to do...
→ Did you mean: [1] save_work ...
> 1
→ ✓ Learned: "yeet this code" → save_work

> yeet this code
[Tier 1] → Planned git workflow: ...
```

The system gets smarter the more you use it. Enjoy!

