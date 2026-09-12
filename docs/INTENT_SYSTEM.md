# Intent Routing

The application maps plain language to a catalog of built-in workflows. The
pipeline is a heuristic router, not an autonomous planner or a calibrated
probability model.

## Routing order

1. Explicit syntax: shell prefixes, `edit path: instruction`, rollback and
   starter-guide phrases have direct handling in `intent::quick_match`.
2. Learned aliases, exact tool names, exact examples, then FZF subsequence
   matching in `fuzzy.rs`.
3. Keyword/embedding scoring in `keyword_classifier.rs` when the cache is ready.
4. A small Ollama model classifies into an exact catalog tool name or `chat`.
5. Unrecognized requests fall through to chat.

Arguments are extracted from supported forms and then validated. Classification
does not currently supply a general structured argument schema. For staging,
only a named path or an explicit whole-project phrase is accepted; missing
scope cannot silently become `git add -A`. Known local-save phrases resolve to
staged-only `commit`, not the combined stage/commit/push workflow.

## Exact and fuzzy matching

The catalog in `src/tools.rs` supplies examples and metadata. Learned aliases
have highest precedence. Exact names/examples take priority over fuzzy matching.
FZF matches ordered subsequences; it is not a general spellchecker and cannot
be assumed to handle every transposition. `staus` is a tested match for `status`.

## Semantic scoring

The embedding cache contains the catalog examples and tool assignments. A warm
lookup requests an embedding for the current input once, then compares the
stored vectors in memory. The best example similarity per tool is combined
with keyword overlap:

```text
score = 0.6 × keyword_score + 0.4 × embedding_similarity
```

The current acceptance threshold is 0.7. These weights are implementation
heuristics, not measured intent accuracy. An embedding request failure allows
the model-classification fallback to continue. See [cache behavior](EMBEDDING_CACHE.md)
for catalog validation and persistence.

## Model fallback and execution boundary

The default classifier is `qwen2:1.5b`; configure `classifier_model` or
`LLM_CLI_CLASSIFIER_MODEL` to change it. Its reply must match a known tool name
exactly after simple formatting normalization. For example,
`draft_commit_message` remains that tool and cannot match `commit` by substring.
Ambiguous prose such as `commit or status` is rejected.

Typed background events carry resolved intent, original input, and directory
back to the UI. Model chat text is never parsed as an internal `__INTENT__`
control signal. A result for a different current directory or an already-busy
workflow is rejected with an actionable message.

Each tool still owns its review behavior. The shell risk policy is a heuristic
review aid, not a shell parser or sandbox. Natural-language confidence alone
must not be treated as authorization to expand a requested action's scope.

## Learned workflows

The TUI can save reviewed command suggestions for a phrase. Current workflow
data uses `.llm-cli/custom_workflows.toml`; legacy `custom_commands`/`macros`
data is accepted. Malformed existing data causes save failure without replacing
the file with empty state. Saved workflows are currently shell strings rather
than parameterized, resumable typed steps.

## Testing and performance

Regression tests cover exact classifier output, local-save scope, staging
arguments, Unicode extraction, embedding-failure fallback, cache validity and
the one-request warm-cache contract. These do not establish live-model routing
accuracy across unseen phrasing.

Earlier latency and 90%/9%/1% tier-distribution estimates were not backed by a
reproducible benchmark in this repository and are not product guarantees.
Measure p50/p95 and incorrect-action rates on a published corpus, recording
hardware and warm/cold model state, before making comparative speed claims.

The planned next step is structured task discovery and explicit arguments,
followed by reusable verified recipes. See [the project review](PROJECT_REVIEW.md).
