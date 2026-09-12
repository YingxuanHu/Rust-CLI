# Product and Engineering Review

Reviewed 2026-09-12. Baseline: `3c26eda` (before this revision).

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
gaps are synchronous command execution, weak context, basic editing, and
incomplete task discovery.

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
| Understand a repo | Manifest identity, structure, file view | No code index; manifest parsing is shallow | Nearest project detection; selected README/manifest/source context with exclusions |
| Explain a test error or file | Recent-output tracking and reference words | Prompt uses summaries and omits stored output; conversation history is absent | Include bounded actual output and recent turns, scoped to project |
| Run tests/build | Fixed recipes for Rust, Node, Python, Go | Synchronous execution; wrong manifest in nested mixed projects; hardcoded npm/pytest | Detect actual scripts/lockfiles/environments and stream task execution |
| Check changes | Git status/diff summary | Status creates a modal file prompt; staged and unstaged views are incomplete | Nonmodal status plus selectable files and explicit staged/unstaged drill-down |
| Save work | Stage, draft message, commit, optional combined push workflow | All-file staging is easy to choose; upstream/auth errors lack guided recovery | Selected-file plan, verification step, clear local save vs publish |
| Edit code | Checked single-file patch; last reviewed edit reversal | Whitespace paths problematic; preview can truncate; no verification loop | Full diff review, content preconditions, opt-in tests, retained rollback records |
| Create/overwrite a file | Path-constrained text writes and confirmation | Preview is partial; no stale-content check or atomic replacement | Full preview and write precondition; atomic save |
| Run shell commands | Shell mode, bang shortcuts, heuristic risk review | POSIX child shell, blocking execution, incomplete process-tree cleanup | One async executor with job identity, cancellation and output bounds |
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
| `app`, `ui`, `input`, `session`, `model_stream` | Background model work is a useful base. Typed events and shared streaming now protect response identity. A job manager, editor, bounded state, and reliable terminal teardown remain needed. |
| `intent`, `fuzzy`, `keyword_classifier`, `llm_classifier`, `embedding`, `tools` | Layered routing is reasonable. Cache use and exact classification were faulty. Confidence scores are heuristics, not calibrated probabilities; broad paraphrases still need argument extraction and evaluation. |
| `handlers`, `workflow`, `commands`, `command_policy`, `custom_command_generator` | Workflows encode useful chores but mix UI decisions, subprocess work, and persistence. Replace shell-string composition with typed steps. The custom-command generation function is dormant; its live placeholder expansion is distinct. |
| `repo`, `chat`, `context` | Manifest identity and output summaries are not repository understanding or conversation memory. Implement evidence selection and nearest-manifest logic before larger agent features. |
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

## Remaining engineering priorities

| Priority | Concrete issue and source | Required outcome |
| --- | --- | --- |
| P1 | Synchronous execution from `handlers.rs`, `app.rs`, `workflow.rs`; unbounded probes in `ollama.rs` | Input/rendering stay responsive during slow tools and unavailable runtimes; timeouts cover the entire operation |
| P1 | `commands.rs` kills a parent but joins readers that descendants can keep open; captures unbounded output | Cancel the process tree and bound/timebox capture, tested with inherited pipes and large output |
| P1 | `context.rs` omits `RecentOutput.content`; `chat.rs` has no history; project changes retain old context | Answers use the actual selected evidence and never silently reuse another project's output |
| P1 | `repo.rs` searches all ancestors for Cargo before nearer Node manifests | Nearest applicable project and correct task runner selected in mixed monorepos |
| P1 | `audit.rs` redaction misses forms such as quoted multiword values; raw input history retains secrets | Consistent redaction/retention policy across audit and history; do not promise secret-free logs |
| P2 | `frecency.rs` unordered snapshot writes and `app.rs` history replacement | Concurrent sessions preserve newer data; atomic writes and recoverable corruption |
| P2 | `ui.rs` raw-mode construction failure, character/cell-width mismatch; minimal editing in `app.rs` | Restored terminal on all exits, cursor editing, paste, horizontal scrolling and wide-character tests |
| P2 | `workflow.rs` command extraction joins illustrative alternatives; confirmation can also save aliases | Structured command proposals and separate Run once / Save actions |
| P2 | `patch.rs` whitespace path parsing and truncated review; `file_ops.rs` check/write race | Correct path handling, full review, content precondition and atomic writes |
| P2 | Config and state relative to launch directory; version stays 0.1.0 across source additions | Defined user/project precedence and provenance; versioned releases with migration notes |

## Implementation sequence and acceptance criteria

### Phase 1 — Reliable task execution

Introduce `TaskId`, immutable cwd/config context, typed command arguments,
streamed stdout/stderr, exit status, elapsed time, and cancellation in one
executor used by every tool. Keep workflow transitions separate from rendering
and process management. Lazy-load optional embeddings after the first usable
screen. Complete the P1 timeout/output/context issues above.

Acceptance: a slow test suite leaves input usable; cancellation ends child
processes; a disconnected model never leaves a permanent spinner; concurrent
results attach to the right request; local save never pushes; selected staging
never broadens. Test with fake processes and real temporary repositories.

### Phase 2 — Finish a task with fewer steps

Add a task picker populated from the nearest manifest, package-manager lockfile,
and project overrides. Start with test, lint, build, inspect changes and local
save. Show the actual command before execution. Include output-backed
`explain this failure`, `rerun`, and a bounded `ask --file`/stdin workflow.

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
