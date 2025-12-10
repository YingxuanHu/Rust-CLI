# Autocompletion

Ghost-text completion with smart path detection and frecency-based ranking.

## Core Structure

```
src/completion.rs     CompletionProvider - fuzzy matching + path completion
src/frecency.rs       FrecencyTracker - tracks files AND commands
src/app.rs           Ghost text state + Tab key handling  
src/ui.rs            Ghost text rendering (dim gray after cursor)
src/handlers.rs      Records: file access + command usage
```

## Completion Sources

1. **Tool examples** - 50+ phrases from `TOOLS[]` (e.g., "push my changes", "show status")
2. **Command history** - Recent commands (last 50)
3. **Learned aliases** - User-taught commands from `.llm-cli/learned.toml`
4. **File/folder paths** - Smart path completion when typing paths

## Smart Detection

The system decides completion mode based on input:

### Full Input Matching
When typing commands, it matches the entire input:
- `"sav"` → ghost: `"e work"` (fuzzy matches "save work")
- `"push my ch"` → ghost: `"anges"` (completes to "push my changes")

### Path Completion
When the last word looks like a path, it completes files/folders:
- `"show src"` → ghost: `"/"` (directory exists)
- `"read src/ma"` → ghost: `"in.rs"` (file completion)
- `"ls ./"` → ghost: `"src/"` (relative path)

A word is path-like if it:
- Contains `/` or `\`
- Starts with `.` or `~`
- Matches an existing file/folder in cwd

## Usage

- **Tab** - Accept ghost text
- **Type normally** - Ghost text updates in real-time
- **Backspace** - Ghost text updates as you edit

## Frecency-Based Ranking

Both **files** and **commands** are ranked by **frecency score** (frequency × recency decay):

```rust
score = access_count * time_decay

time_decay:
  0-1 hours ago:   4.0×  (recent boost)
  2-24 hours:      2.0×  (daily boost)
  1-7 days:        1.0×  (normal)
  1-4 weeks:       0.5×  (halved)
  older:           0.25× (quarter)
```

### Example: File Completion

| File | Accesses | Last Used | Score | Rank |
|------|----------|-----------|-------|------|
| `src/main.rs` | 15 | 1 hour ago | 60.0 | 1st |
| `src/app.rs` | 30 | 2 days ago | 30.0 | 2nd |
| `src/old_test.rs` | 50 | 1 month ago | 25.0 | 3rd |

### Example: Command Completion

```
You type: "sav"

Without frecency:
  Ghost: "e work to remote" (alphabetically first)

With frecency (20 uses, 1 hour ago):
  Ghost: "e work to remote" (score: 80.0)

After 1 week of using "save changes locally" instead:
  Ghost: "e changes locally" (now higher score)
```

### Cross-Session Memory

Access patterns persist in `.llm-cli/frecency.toml`:

```toml
[[files]]
path = "src/main.rs"
count = 15.0
last_accessed = 1733875200

[[commands]]
path = "save work to remote"
count = 20.0
last_accessed = 1733875300
```

- **Bounded**: Max 500 files + 500 commands
- **Count capped**: At 100 per entry
- **Auto-pruning**: Entries with score < 0.5 removed on load
- **Per-project**: Each project tracks its own patterns

### What Gets Recorded

| Event | Tracked |
|-------|---------|
| Tab accepted | The completed command/phrase |
| Tool executed | Original user input |
| File opened | File path |

## Implementation

`CompletionProvider::get_ghost_completion()` returns only the **suffix** to append. Fuzzy matching uses `SkimMatcherV2`, path completion uses `std::fs::read_dir()` sorted by frecency score.

