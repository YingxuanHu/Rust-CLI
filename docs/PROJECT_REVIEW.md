# Product and Engineering Review

Reviewed 2026-09-12. Baseline: `3c26eda` (before this revision).

## Follow-up: execution, discovery, and failure evidence

Current source now runs **tests, builds, shell commands, and built-in Git
commands** through a dedicated async runner: explicit command arguments,
immutable directory/task identity, live bounded stdout/stderr tails (64 KiB
each), elapsed time, exit status, cancellation, and deadline coverage for both
process exit and output pipes. Ctrl+C cancels an active command without closing
the assistant; with no command task it exits. Escape waits for bounded cleanup
before exit. Unix process-group cleanup covers ordinary descendants, not
deliberately detached processes; Windows currently stops only the direct child.

One command task runs at a time. Help, directory switching, and Chat-mode
questions stay available. Task results remain attached to their original
message and directory; changing directories clears old output context. Git
status is nonmodal, `diff <path>` shows an unstaged path diff, and `save work`
requires a separate confirmation to push after its local commit. Cancellation
does not undo completed staging, commits, file changes, or remote side effects.
Shell tasks close stdin and have no PTY. Interactive-command warnings are
heuristic, not complete detection. `diff <path>` is relative to the detected Git
root and retains Git pathspec semantics.

`tasks` now lists actual test/build commands from the nearest supported project.
Node scripts must exist, with a supported local package-manager declaration
or unambiguous local lockfile; npm is the fallback when neither exists. Ancestor
workspace manager inheritance and Python environment detection are not inferred.
Recorded output now reaches explanation prompts: up to five labeled head/tail
excerpts, strict byte limits, untrusted-data framing, and project isolation.

Fake-process, Git-fixture, UI-state, project-fixture, and prompt regressions cover
these contracts. This is progress through Phases 1 and 2, not their completion.
TODO search and file/patch work still contain synchronous operations. Readiness
probes, independent cancellation of model chat/pending model preparation,
searchable task selection, and durable rerun/recipe history remain unfinished.
These improvements are in source; no claim is made that v0.1.0 includes them.

## Recommendation

Focus on **making recurring repository tasks easy to complete and repeat**:
discover the right command, run it visibly, understand its result, make a
reviewed correction when needed, verify the outcome, and save the successful
sequence. The primary users are developers moving between projects and users
who understand their goal but do not remember every project's tooling.

Rust, local inference, natural-language commands, streaming, and saved workflows
are useful foundations, but individually they are not a market differentiator.
The opportunity is a particularly reliable, low-friction combination for daily
repo chores. This is a product hypothesis to validate, not a claim of exclusive
functionality or demonstrated superiority.

The first revision fixes correctness problems that would undermine that goal.
It does not complete all work in this review. The largest remaining efficiency
gaps are remaining synchronous operations, basic editing, incomplete task
discovery/reuse, and source context beyond bounded recent output.

## Current market comparison

These are source-verified capabilities, not hands-on comparative performance
benchmarks. Product documentation was checked on the review date. Price, model
quality, market share, and relative memory use were not evaluated.

| Tool | Relevant established capabilities | Implication for this project |
| --- | --- | --- |
| [AIChat](https://github.com/sigoden/aichat) | Rust CLI; Ollama and other providers; shell assistance; stdin/file inputs; REPL; RAG; agents; macros | Closest overlap. Rust/local chat alone is insufficient; make complete repo tasks easier. |
| [Aider](https://aider.chat/docs/repomap.html) | Repository map of relevant code symbols; [lint/test integration](https://aider.chat/docs/usage/lint-test.html) around edits | Context and verification are necessary before promoting this as a serious code-editing tool. |
| [Goose](https://github.com/aaif-goose/goose) | Rust-based CLI/desktop agent, local-provider support and extensions; [recipes](https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/recipes/recipe-reference.md) with parameters and retry validation | Reusable workflows are established. A narrow, predictable workflow experience must earn its place. |
| [ShellGPT](https://github.com/TheR1D/shell_gpt) | One-shot questions, piped input, shell-command generation and execution choices; documented local-model setup | Answering questions should fit naturally into existing terminal pipelines. |
| [Warp](https://docs.warp.dev/knowledge-and-collaboration/warp-drive/workflows) | Searchable, parameterized saved commands, defaults, descriptions and workflow sharing | Discovery and reuse need a visible UI; exact-phrase aliases alone are hard to find and maintain. |

The proposed advantage is **fast local task selection plus visible, reusable
execution and recovery in the user's existing terminal**. Test that against the
above tools on specific chores. Avoid claims that they are uniformly heavy,
cloud-only, or unable to support local models.

## Use-case review

| User goal | Current capability | Friction or limitation | Highest-value next change |
| --- | --- | --- | --- |
| Install and ask a first question | Verified binary archives, source installer, guided model download | Release v0.1.0 lags current source; PATH/setup requires manual steps; model checks may stall | Publish a tested follow-up release, bounded readiness probes, choose from installed models |
| Discover what is possible | Starter suggestions, help, ghost completion | Suggestions are prose; long help competes with conversation | Searchable task picker with short descriptions and prefilled inputs |
| Ask a quick question | `ask`, streaming, JSON | Name/type context only; no explicit file or stdin attachment | Bounded stdin and `--file`, readable source attribution and useful errors |
| Understand a repo | Nearest-manifest identity, parsed Rust/Node names, file view | No code index or automatic source reading | Selected README/manifest/source context with exclusions and provenance |
| Explain a test error or file | Bounded actual recent output with labels and failure-reference detection | Excerpts omit some text; no conversation history or verified diagnostic quality | Selected evidence, recent turns, and failure-specific follow-up actions |
| Run tests/build | Background execution; `tasks` listing; local Node script/manager detection | No searchable picker, workspace-manager inheritance, or Python environment detection | Searchable actions, project overrides, and environment-aware commands |
| Check changes | Background nonmodal status and unstaged summary; explicit `diff <path>` | Staged/unstaged selection and file discovery remain basic | Selectable files and explicit staged/unstaged drill-down |
| Save work | Reviewed all-file staging/local commit, then separate push confirmation | No selected-file save plan; upstream/auth errors lack guided recovery | Selected-file plan, optional verification, and guided publish recovery |
| Edit code | Checked single-file patch; last reviewed edit reversal | Whitespace paths problematic; preview can truncate; no verification loop | Full diff review, content preconditions, opt-in tests, retained rollback records |
| Create/overwrite a file | Path-constrained text writes and confirmation | Preview is partial; no stale-content check or atomic replacement | Full preview and write precondition; atomic save |
| Run shell commands | Background POSIX shell task with review, job identity, cancellation, output bounds | Noninteractive; risk/interactive-command detection heuristic; Windows cleanup limited | Clearer structured proposals, broader platform tests, better unsupported-input guidance |
| Repeat a successful operation | Learned aliases/workflows, input history, frecency | Exact phrases, hidden files, no arguments/step schema; concurrent writes can race | Parameterized recipes with list/edit/run, preconditions and outcomes |
| Inspect past activity | Direct-shell audit tail | Other workflow steps absent; redaction incomplete; entire log read for tail | Structured task history, redaction consistency, bounded tolerant log reader |
| Work without a network | Local Ollama inference after models are installed | Local host is default, but remote endpoints are configurable; optional models add startup work | Clearly show active endpoint/model; offline task mode for deterministic actions |

## Codebase coverage

The source review covered all Rust modules and the installation, CI/release,
configuration, and documentation surfaces. Coverage here means inspection,
not a claim that every branch was executed or all defects were found.

| Area and source modules | Assessment |
| --- | --- |
| `main`, `bootstrap`, `config`, `ollama`, `diagnostics` | Useful guided setup and read-only doctor. Consolidate runtime probes, distinguish readiness from installation, expose effective config and scope. |
| `app`, `ui`, `input`, `session`, `model_stream`, `task_runner` | Typed model events and one active background command protect response identity. The runner streams bounded output and cancels ordinary Unix descendants. Independent model cancellation, broader terminal/editor handling, bounded session history, and Windows process-tree handling remain needed. |
| `intent`, `fuzzy`, `keyword_classifier`, `llm_classifier`, `embedding`, `tools` | Layered routing is reasonable. Cache use and exact classification were faulty. Confidence scores are heuristics, not calibrated probabilities; broad paraphrases still need argument extraction and evaluation. |
| `handlers`, `workflow`, `commands`, `command_policy`, `custom_command_generator` | Workflows encode useful chores but mix UI decisions, subprocess work, and persistence. Replace shell-string composition with typed steps. The custom-command generation function is dormant; its live placeholder expansion is distinct. |
| `repo`, `chat`, `context` | Nearest-manifest selection, real Rust/Node name parsing, script/manager detection, and bounded actual output now work together. This is still not repository understanding or conversation memory; evidence selection and workspace/environment configuration remain needed. |
| `file_ops`, `patch` | Confinement and checked diffs help prevent accidents. Remaining work includes race-resistant writes, complete previews, Git quoted-path handling and multi-file transactions. |
| `learned`, `frecency`, `audit` | Useful local memory, but persistence needs atomic, ordered, recoverable writes and explicit retention/redaction behavior. |
| `ask`, `completion`, `shell_completion` | Useful shell ergonomics. Streaming/JSON/path issues are fixed in this revision; completions now derive from Clap instead of duplicated command lists. Input editing and pipeline context remain incomplete. |
| `test_support`, `tests/cli.rs`, `.github/workflows`, `scripts`, docs | Baseline had 83 unit tests and Ubuntu-only CI. Added real regression fixtures and CLI tests; CI now also runs tests on macOS and checks declared Rust 1.85. Windows workflow tests and clean-install acceptance tests remain absent. |

## Corrections implemented in this revision

1. **Use cached vectors during classification.** Previously a semantic lookup
   embedded the user input and every catalog example again, despite a populated
   cache. It now embeds the input once and compares examples in memory. A mock
   server accepting exactly one request makes the old behavior fail. Cache
   format 2 checks model, endpoint, full catalog/tool assignment, and vector
   validity/dimensions; incomplete initialization is not published as ready.

2. **Keep model text separate from control events.** Ordinary model responses
   beginning `__INTENT__:` or commit-control prefixes could previously be parsed
   as internal actions. Background results now use typed events. Original
   prompt/directory travel with the request, placeholders stay stable, and
   delayed intents cannot run in a different directory or replace a pending
   workflow. Pending patch review survives incidental input and unrelated errors.

3. **Bound chat execution and preserve Unicode.** A shared subprocess reader
   retains partial UTF-8 characters and applies one deadline to reads and process
   exit. Silent children and children that close pipes before exiting are tested.
   Stderr is drained with bounded retained diagnostics; model children are
   killed on failure/drop. This is not full process-tree or interactive task cancellation.

4. **Preserve requested action scope.** `draft_commit_message` no longer matches
   `commit` by substring. Explicit filenames such as `all-important.txt` and
   `file-a.rs` remain filenames, option-like paths use `git add --`, and ambiguous
   staging cannot silently expand to all files. `save locally` selects a local
   commit of staged work. Generated commit text is quoted as literal shell data.

5. **Correct setup and shell-facing contracts.** Exact Ollama tags are checked
   (`:8b` does not satisfy `:70b`), missing explicit config files fail, and JSON
   runtime/config/setup errors produce an error object with failure status.
   Noninteractive `ask` does not treat unavailable setup confirmation as success.
   Logs go to stderr. Bash/Zsh/Fish/PowerShell definitions are generated from the
   real command parser; Bash global-config behavior has an executable test.

6. **Protect everyday text/file interactions.** `q` can start a question; home,
   root and Unicode path completions preserve the input prefix. Dangling or
   outside-root symlink writes are rejected. Malformed saved aliases/workflows
   are not replaced with empty data; documented legacy workflow keys load correctly.

7. **Make project claims and onboarding accurate.** The README leads with tasks
   and installation. The course report is preserved separately. Unsupported
   latency percentages are removed from current guidance; release/source
   differences and limitations are explicit. CI includes macOS and an MSRV
   check, and future releases must pass tests plus binary smoke checks.

8. **Extend visible command execution.** Tests, builds, shell, and built-in Git
   commands share task identity, bounded output, timeout, cancellation, and
   cleanup. Git continuations advance only after successful uncancelled results
   in the originating directory. Status no longer opens a modal file prompt;
   saving work commits locally before a separate explicit push choice.

9. **Make detected tasks inspectable.** `tasks` lists commands without executing
   them. Nearest supported manifests beat distant projects of another language.
   Node requires actual scripts and selects a supported declared manager before
   unambiguous local lockfiles. Missing/ambiguous tasks report unavailable rather
   than succeeding through an `echo` placeholder.

10. **Explain captured evidence.** Session output uses 2,000-byte head/tail
    captures. Prompt context includes actual content from at most five outputs
    within 8 KiB total, with capped metadata and untrusted-data framing. Prompt
    budgeting reuses unused metadata space, and whole-word reference matching
    avoids false matches such as `it` inside `git`. Context clears on directory
    change; model diagnostic quality still requires separate evaluation.

## Remaining engineering priorities

| Priority | Concrete issue and source | Required outcome |
| --- | --- | --- |
| P1 | Remaining synchronous TODO search/file/patch execution; unbounded readiness probes in `ollama.rs` | Input/rendering stay responsive during every slow operation; timeouts cover preparation as well as child execution |
| P1 | Legacy synchronous helper in `commands.rs` still kills a parent, joins potentially inherited pipes, and captures unbounded output | Finish migrating remaining callers or replace the helper with end-to-end bounded execution |
| P1 | Model chat and pending commit/patch/model-backed command preparation have no independent cancellation | Cancel the intended request without quitting, stale completion, or accidental workflow continuation |
| P1 | `repo.rs` does not infer workspace membership/ancestor manager or Python environments | Correct task selection for nested workspaces and configured environments, with inspectable overrides |
| P2 | Output context is bounded recent evidence, not conversation memory or explicit source selection | Select relevant evidence with provenance and intentional retention instead of silently implying full repository knowledge |
| P1 | `audit.rs` redaction misses forms such as quoted multiword values; raw input history retains secrets | Consistent redaction/retention policy across audit and history; do not promise secret-free logs |
| P2 | `frecency.rs` unordered snapshot writes and `app.rs` history replacement | Concurrent sessions preserve newer data; atomic writes and recoverable corruption |
| P2 | `ui.rs` raw-mode construction failure, character/cell-width mismatch; minimal editing in `app.rs` | Restored terminal on all exits, cursor editing, paste, horizontal scrolling and wide-character tests |
| P2 | `workflow.rs` command extraction joins illustrative alternatives; confirmation can also save aliases | Structured command proposals and separate Run once / Save actions |
| P2 | `patch.rs` whitespace path parsing and truncated review; `file_ops.rs` check/write race | Correct path handling, full review, content precondition and atomic writes |
| P2 | Config and state relative to launch directory; version stays 0.1.0 across source additions | Defined user/project precedence and provenance; versioned releases with migration notes |

## Implementation sequence and acceptance criteria

### Phase 1 — Reliable task execution

Completed for test/build/shell/built-in Git commands: `TaskId`, immutable command
cwd, explicit arguments, streamed stdout/stderr, status, elapsed time, and
cancellation. Git workflow continuations are separate from the worker and
publication requires its own confirmation.

Remaining: migrate TODO/file/patch work, bound readiness probes, support model
request/preparation cancellation, and lazy-load optional embeddings after the
first usable screen. Finish the remaining timeout/output issues above before
calling every tool responsive.

Acceptance: a slow test suite leaves input usable; cancellation ends child
processes; a disconnected model never leaves a permanent spinner; concurrent
results attach to the right request; local save never pushes; selected staging
never broadens. Test with fake processes and real temporary repositories.

### Phase 2 — Finish a task with fewer steps

Completed first slice: nearest-manifest detection, local Node manager/script
selection, a read-only `tasks` list, actual commands in task entries, nonmodal
Git status, and output-backed failure explanations.

Next: a searchable task picker with project overrides, lint and verification
actions, environment/workspace awareness, explicit `rerun`, and bounded
`ask --file`/stdin inputs. The current task list is not yet that picker, and
failure explanations do not automatically fix or retry a command.

Acceptance: a new user runs the correct task without knowing Cargo/npm/pnpm/uv
syntax; an explanation quotes the captured error and identifies its source;
large inputs are bounded with an explicit truncation notice; read-only questions
cannot trigger actions.

### Phase 3 — Reuse successful work

Turn a successful task sequence into a discoverable recipe: parameters,
preconditions, explicit commands, model use only where needed, success checks,
and replay history. Begin with `check project`, `prepare local commit`, and
`explain then rerun failed tests`. Persist task/session history with project
boundaries and recoverable writes. Offer plain-language and named invocation.

Acceptance: the second execution needs fewer interactions, uses the saved
commands without re-generating them, fails clearly when prerequisites change,
and resumes only steps whose state can be verified. Recipe export/import must
show the steps before first use.

### Phase 4 — Verified edits and distribution

Add selected file context, full diff review, optional post-edit tests and durable
rollback records before multi-file edits. Publish a tested release matching the
docs, verify installation on clean supported systems, then consider Homebrew or
other package managers. Licensing is an owner decision; no license is selected
by this review.

Defer broad provider catalogs, marketplace/plugin systems, autonomous multi-agent
coding, and cloud collaboration until repeat-task usability is demonstrated.

## How to measure efficiency

Use a small reproducible benchmark repository and an intent corpus containing
exact commands, typos, paraphrases, negation, questions, ambiguous scope, Unicode,
and option-like filenames. Separately measure fresh/warm model and cache states.
Record hardware, versions, endpoint, model, and dataset with every result.

Track time to first successful task, total interactions, task completion rate,
incorrect-action rate, rerun time, startup/first-output p50 and p95, embedding
request counts, and recovery after failure. Compare the same tasks using plain
shell commands and selected competitors; obtain actual novice-user observations.
Proposed targets should be chosen after measuring the baseline, not presented
as current results. The cache regression proves call reduction only; it does
not substantiate the earlier 10–20x startup or 90% routing claims.

Local automated checks use fixtures and fake inference endpoints. They establish
specific contracts, not live-model accuracy, full Windows support, or complete
interactive usability. Those remain explicit acceptance work.
