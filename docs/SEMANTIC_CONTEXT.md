# Semantic Context Extension

## Overview

Enables natural reference resolution in conversation. After running a command like "show status", you can say "commit it" and the system understands "it" refers to the diff.

**Approach**: Cursor-style - application tracks recent outputs and injects context into LLM prompts when references are detected.

## Architecture

```
User: "show status"
  ↓
Handler executes → Records output to session.recent_outputs
  ↓
Shows result to user

User: "commit it"
  ↓
Detects reference word ("it") → Injects recent context into LLM prompt
  ↓
LLM receives: "[Recent context] 1. [diff] Git status: 3 files changed"
  ↓
LLM understands reference and responds appropriately
```

## Key Components

### 1. Context Tracking (`src/context.rs`)

- **`RecentOutput`**: Stores output type ("diff", "file", "command", etc.), summary, and a truncated content copy for future expansion
- **`contains_reference()`**: Detects reference words ("it", "that", "the diff", etc.)
- **`format_context_for_prompt()`**: Formats recent-output summaries for LLM injection

### 2. Session State (`src/session.rs`)

- **`recent_outputs`**: VecDeque maintaining last 5 outputs (max 2000 chars each)
- **`record_output()`**: Adds new output to the queue, auto-truncates old ones

### 3. Handlers (`src/handlers.rs`)

Each handler records relevant outputs:
- **status** → "diff" (git status + diff stat)
- **show_file** → "file" (file path + line count)
- **shell** → "command" (command + output)
- **find_todos** → "todos" (count + list)
- **draft_commit** → "commit_msg" (generated message)

### 4. App Integration (`src/app.rs`)

Before calling LLM in `submit_input()`:
```rust
let context_injection = if context::contains_reference(&prompt) {
    context::format_context_for_prompt(&session.recent_outputs)
} else {
    String::new()
};
```

## Reference Words

- Pronouns: `it`, `that`, `this`, `them`, `those`
- Explicit: `the diff`, `the changes`, `the file`, `the output`, `the status`, `the result`, `the command`, `the message`

## Example Workflows

### Workflow 1: Commit after status
```
> show status
[Shows git diff]
Context stored: [diff] Git status: 3 files changed

> commit it
Reference detected → Context injected
LLM understands: commit the shown diff
```

### Workflow 2: Multiple outputs
```
> show file src/main.rs
Context: [file] src/main.rs (150 lines)

> show status
Context: [file] src/main.rs, [diff] Git status: 2 files changed

> commit the changes
Reference detected → Both contexts available
LLM prioritizes: "changes" → diff output
```

### Workflow 3: Shell command reference
```
> $ cargo test
Context: [command] Ran: cargo test (success)

> run it again
Reference detected → Context shows last command
LLM can suggest: cargo test
```

## Implementation Notes

- **Session-only**: Context cleared on exit (not persisted)
- **Max 5 outputs**: Oldest automatically removed
- **Max 2000 bytes**: Long stored outputs are truncated with "[truncated]"; prompts currently receive summaries rather than full output text
- **No extra LLM calls**: Main LLM handles disambiguation
- **Fast**: Simple string matching + concatenation

## Performance

- Reference detection: O(1) - simple string contains check
- Context formatting: O(n) where n ≤ 5 recent outputs
- No blocking operations - all synchronous
- Minimal memory overhead (~10KB for 5 outputs)
