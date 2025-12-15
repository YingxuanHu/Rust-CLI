# Tiered Intent Resolution System

This document explains how the LLM CLI determines what you want to do when you type a command.

## Overview

The system uses **3 tiers** of intent resolution, from fastest to most flexible:

```
User Input → Tier 1 → Tier 2 → Tier 3 → Execute or Chat
              (< 1ms)   (~50ms)  (~500ms)
```

Each tier tries to understand your intent. If successful, it executes immediately. If uncertain, it falls through to the next tier, ultimately defaulting to conversational chat.

---

## Architecture

### Tier 1: Fuzzy Matching (< 1ms)
**Method:** String similarity matching  
**Technology:** 100% Rust, `strsim` crate  
**Confidence:** High (exact or near-exact matches)

Checks in order:
1. **Learned aliases** (highest priority)
   - Your previously confirmed commands
   - Stored in `.llm-cli/learned.toml` in project directory

2. **Exact tool names**
   - `status` → `status` tool
   - `commit` → `commit` tool

3. **Exact tool examples**
   - `save work` → `save_work` tool
   - `git status` → `status` tool

4. **Fuzzy matches** (typo-tolerant)
   - `stauts` → `status` (85%+ similarity)
   - `comit` → `commit`

**Why this tier:** Instant feedback for common commands and learned patterns.

---

### Tier 2: Keyword + Embedding Classifier (~50ms)
**Method:** Hybrid scoring (deterministic keywords + semantic embeddings)  
**Technology:** 100% Rust, Ollama embedding API  
**Confidence:** Threshold 0.7+

Combines two approaches:

**A. Keyword Scoring (60% weight)**
- Analyzes word overlap between your input and tool examples
- Deterministic and explainable
- Example:
  ```
  Input: "push my changes"
  → Contains: "push", "changes"
  → Matches "save_work" examples: "push my changes", "sync with remote"
  → High keyword score
  ```

**B. Embedding Similarity (40% weight)**
- Computes semantic similarity using vector embeddings
- Handles paraphrasing and synonyms
- Cached for speed
- Example:
  ```
  Input: "upload code"
  → Embedding similar to "push to github"
  → Matches "save_work" tool
  ```

**Combined Score:** `0.6 * keyword_score + 0.4 * embedding_score`

**Why this tier:** Balances speed and flexibility. Handles most natural language queries without LLM latency.

---

### Tier 3: Small LLM Classifier (~500ms)
**Method:** Structured prompt to lightweight LLM  
**Technology:** Ollama API with `qwen2:0.5b` (default)  
**Confidence:** Threshold 0.5+

When Tier 2 is uncertain, asks a small, fast LLM:

```
You are a command classifier. Respond with ONLY the tool name, nothing else.
Available tools: save_work, status, commit, stage, ...

User input: "ship it to production"
Tool name:
```

**Supported Models:**
- `qwen2:0.5b` - Fastest (default, ~200-300ms)
- `qwen2:1.5b` - Better accuracy (~400-500ms)
- `phi3:mini` - Best accuracy (~800ms-1s)

**Why this tier:** Handles novel phrasing and edge cases that don't match patterns. Falls back to conversational chat when uncertain, providing a natural user experience.

---

## Learned Aliases

### File Locations

**Project aliases** (stored in project directory):
```
.llm-cli/learned.toml
```

### Format

```toml
[[aliases]]
phrase = "yeet my changes"
tool = "save_work"
timestamp = "1702053600"
source = "user_feedback"

[[aliases]]
phrase = "what's up"
tool = "status"
timestamp = "1702053700"
source = "user_feedback"
```

### Storage

- All learned aliases are stored per-project in `.llm-cli/learned.toml`
- Each project has its own set of learned commands
- This allows different meanings for the same phrase across different projects (e.g., "deploy" might mean different things in different repos)

---

## Performance Characteristics

| Tier | Latency | Accuracy | Technology | Fallback |
|------|---------|----------|------------|----------|
| 1 | < 1ms | Very High | Fuzzy matching | Yes |
| 2 | ~50ms | High | Keyword + Embeddings | Yes |
| 3 | ~500ms | High | Small LLM | Chat |

**Real-world performance:**
- **90% of commands:** Resolved in Tier 1 (< 1ms)
- **9% of commands:** Resolved in Tier 2 (~50ms)
- **1% of commands:** Resolved in Tier 3 or fall back to chat
- **Over time:** More commands move to Tier 1 through custom command learning

---

## Configuration

### Example Config (`.llm-cli/config.toml`)

```toml
# Main chat model
model = "llama3"

# Embedding model for Tier 2
embedding_model = "nomic-embed-text"

# Classifier model for Tier 3
classifier_model = "qwen2:1.5b"  # or "qwen2:0.5b", "phi3:mini"

# Paths (all default to .llm-cli/ directory)
learned_path = ".llm-cli/learned.toml"
embedding_cache_path = ".llm-cli/embeddings.toml"
history_path = ".llm-cli/history.jsonl"
```

### Model Recommendations

**For speed-critical use (vim-like feel):**
```toml
classifier_model = "qwen2:0.5b"
```

**For better accuracy:**
```toml
classifier_model = "qwen2:1.5b"
```

**For best accuracy (slight delay acceptable):**
```toml
classifier_model = "phi3:mini"
```

---

## Explicit Learning Mode

You can also teach the system explicitly without waiting for Tier 4:

```
# This feature is planned for future implementation
> learn: yeet → save_work
✓ Learned: "yeet" → save_work

> learn: what's cooking → status
✓ Learned: "what's cooking" → status
```

---

## Shell Command Bypass

Shell commands with `$` or `!` prefix **bypass all tiers** for instant execution:

```
$ ls -la        # Direct shell execution
! git status    # Direct shell execution
!!              # Repeat last shell command
```

This ensures shell commands remain fast and predictable.

---

## Technical Details

### File Structure

```
src/
├── fuzzy.rs              # Tier 1: Fuzzy matching
├── keyword_classifier.rs # Tier 2: Keyword + embedding hybrid
├── llm_classifier.rs     # Tier 3: Small LLM fallback
├── learned.rs            # Learned alias management
└── intent.rs             # Orchestrates all tiers
```

### Data Flow

```rust
// Simplified pseudo-code

async fn resolve_intent(input: &str) -> Intent {
    // Tier 1: Fuzzy match
    if let Some(intent) = fuzzy_match(input) {
        return intent;  // < 1ms
    }
    
    // Tier 2: Keyword + embedding
    if let Some(intent) = keyword_classify(input).await {
        return intent;  // ~50ms
    }
    
    // Tier 3: LLM classifier
    if let Some(intent) = llm_classify(input).await {
        return intent;  // ~500ms
    }
    
    // Fallback: Default to chat
    Intent::Chat
}
```

---

## Advantages

✅ **Fast:** 90% of queries resolved in < 1ms  
✅ **Flexible:** Handles natural language and novel phrasing  
✅ **Natural fallback:** Defaults to conversational chat when uncertain  
✅ **Explainable:** Can see why each match succeeded  
✅ **100% Rust:** No Python, no C++ dependencies  
✅ **Offline:** Works without internet (Ollama runs locally)  
✅ **Clean:** All state in standard config directories  

---

## Future Enhancements

Potential improvements (not yet implemented):

1. **Explicit learning syntax**
   - `learn: yeet → save_work`

2. **Confidence display**
   - Show which tier matched and confidence score
   - `status [Tier 1, 100%]`

3. **Learning statistics**
   - `learned stats` - Show most used aliases

4. **Import/export aliases**
   - Share learned aliases between machines

5. **Negative learning**
   - `unlearn: yeet`
   - Remove incorrect aliases

---

## Troubleshooting

### Commands not being recognized

**Solution:** Lower Tier 2 threshold in `src/keyword_classifier.rs`:
```rust
const CONFIDENCE_THRESHOLD: f32 = 0.6;  // Default: 0.7
```

### Tier 3 is too slow

**Solution:** Use faster classifier model:
```toml
classifier_model = "qwen2:0.5b"  # Fastest
```

Or skip Tier 3 entirely (will default to chat):
```toml
classifier_model = ""  # Disables Tier 3
```

### Wrong tool keeps matching

**Solution:** Check learned aliases:
```bash
cat .llm-cli/learned.toml
```

Remove incorrect entry and re-learn correctly.

---

## Learning Custom Commands

When the LLM generates shell commands during chat, the system offers to save them as custom commands. These are stored in `.llm-cli/learned.toml` and matched instantly in Tier 1 (< 1ms).

---

## Summary

The 3-tier intent system provides:
1. **Speed** - Most queries resolve instantly (< 1ms)
2. **Flexibility** - Handles natural language via semantic matching and LLM
3. **Natural fallback** - Defaults to chat when uncertain
4. **Clarity** - Explainable, deterministic matches

It's designed to be **fast, local, and clean** - resolving most commands instantly while providing a natural conversational experience for everything else.

