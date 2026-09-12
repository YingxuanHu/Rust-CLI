# Recent Output and Failure Explanations

Interactive chat can use actual recorded output when you refer to a recent
task. This describes current source, not the older v0.1.0 release.

## Use it

After `run tests`, `build`, a shell command, or a Git command finishes, ask:

```text
why did tests fail?
explain the failure
last build error
explain the output
```

The prompt includes labeled recent output excerpts, such as the task's kind,
result/directory summary, and captured diagnostics. A recorded file view or
TODO search can also supply evidence. The model's explanation remains a
generated answer, not a verified diagnosis or an automatic fix.

## What is retained?

- Session state keeps at most five outputs, newest first.
- Each stored output body has a strict 2,000-byte limit, including its
  truncation marker. Long captures preserve the opening and final sections;
  the middle may be omitted. UTF-8 characters are not split.
- Background tasks prepare a smaller, at-most-1,000-byte record containing
  their outcome and independently shortened stdout/stderr. Both streams' final
  diagnostics can survive the five-entry prompt budget instead of one noisy
  stream hiding the other's error.
- Prompt formatting uses at most 8 KiB total, including labels, summaries,
  content, and markers. Oversized summaries cannot consume the whole budget.
- Recorded text is framed as untrusted data for the model to use as evidence,
  not as instructions. This framing is not a security sandbox or a guarantee
  that a model cannot be influenced by malicious content.
- Changing the session directory clears recent-output context. Results from
  an older directory remain attached to their original task entry, but do not
  become evidence for the new directory's requests.

The live task display has a separate retention limit: the latest 64 KiB per
stdout/stderr stream. The model receives the smaller excerpt, not the full task
entry. Consult the displayed output or run a narrower command when an omitted
section matters. Captured output may contain sensitive text; model requests use
the configured Ollama endpoint, which can be remote if you configured it so.

## When is context added?

`context::contains_reference` recognizes complete words/phrases such as `it`,
`that`, `the diff`, and `the output`, plus task-failure questions and requests
to explain output. `it` no longer matches substrings in words such as `git` or
`iteration`. This is heuristic reference detection, not a semantic selector;
up to five recent outputs are supplied rather than one proven-relevant result.

`chat::compose_prompt` caps system and project metadata, preserves the current
request before older evidence, and gives remaining space to recent context.
The overall `max_context_tokens` setting is an approximate character-based
budget, not an exact count from the chosen model's tokenizer. Smaller budgets
or very long user requests can truncate or omit recent evidence.

## What this does not do

Recent-output context is session-only. It is not conversation history, a source
index, a durable task log, or cross-project memory. It does not resolve ambiguous
filenames into permission to modify them: use `stage src/main.rs`, not `stage it`,
when you intend to stage one path. It does not automatically rerun or repair
a command. `ask` does not share the interactive session's output context.

Clear explanation questions bypass action aliases and classifiers, so asking
`why did tests fail?` cannot be routed to `run tests` or `build`. The explanation
still needs the configured chat model; this is not an offline diagnostic engine.

## Regression coverage

Tests cover actual failure details reaching a default-budget prompt, all five
newest outputs, directory isolation, head/tail retention, whole-word reference
matching, empty captures, oversized metadata, and mixed-Unicode byte limits.
These tests establish prompt construction and capture behavior, not live-model
diagnostic accuracy.
