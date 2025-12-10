# LLM-Powered CLI

## Motivation

A lack of a lightweight, Rust-native LLM-powered CLI exists in the
current ecosystem. Existing solutions, such as Codex CLI and AIChat, are
often too heavy or tied to other ecosystems, leaving a gap for a simple,
Rust-based alternative. This gap matters because many developers who
choose Rust do so precisely for its speed, safety, and efficiency in
building small, reliable tools. When they want to experiment with
AI-driven workflows, they often have no choice but to rely on bulky
tools from other languages, which can feel out of place in the Rust
ecosystem.

This project is designed to address that gap while remaining enjoyable
to build. For us, it represents both a fun challenge and a learning
opportunity. It brings together three areas we want to practice more
deeply: systems programming in Rust, the design of text-based user
interfaces, and the integration of large language models into developer
tools. A project like this is not only rewarding to implement but also
relevant to ongoing conversations about how developers can interact with
AI in their day-to-day work.

Another reason this idea motivates us is its feasibility. By setting a
realistic scope, we believe two contributors working for one to two
weeks can create a polished prototype that demonstrates novelty without
being overwhelming. We see the novelty in its Rust-first design and in
its ability to demonstrate agentic workflows in a lightweight way.
Instead of being just a demonstration for this course, the project can
act as a base for something more ambitious if we decide to keep building
it later.

## Objective and Key Features

The main objective of this project is to build a lightweight, Rust-based
CLI powered by local LLM inference. The CLI will support context-aware
sessions, integrate with a small set of developer tools, and showcase
basic agentic workflows on a scale that feels practical but still novel.

We want the final tool to feel natural to use in the terminal. It should
behave less like an isolated demo and more like a familiar part of a
developer's toolkit. To achieve this, we have outlined a set of core
features. Each one is chosen not only for what it adds individually but
also for how it contributes to the overall experience.

### Stateful CLI Context (System+LLM) 

The CLI maintains both system-level and semantic context. On the
system side, it remembers the current working directory, repository
path, and command history, so interactions feel cohesive like in a shell
or REPL. On the semantic side, the CLI now tracks recent command outputs
(diffs, files, shell results) and resolves references in conversation.
For example, after running "show status", typing "commit it" correctly
links "it" to the shown diff. The system uses Cursor-style context
injection: when reference words are detected, recent outputs are added
to the LLM prompt for natural resolution. See
[SEMANTIC_CONTEXT.md](docs/SEMANTIC_CONTEXT.md) for details.

### Local Inference with Ollama (Stateless Prompts)

We will use Ollama to run LLM inference locally. Running locally avoids
the need for remote APIs, which improves speed and keeps code private.
For the initial version, prompts will be treated statelessly. This keeps
the system simple while still offering powerful functionality such as
generating summaries or drafting commit messages. Even with this limited
form, the tool gives developers the chance to use AI-powered queries
directly in their terminal.

### Ratatui TUI with Conversation Log

The interface will be built with Ratatui. This will give us a clean,
scrollable conversation log of inputs and outputs, along with a
straightforward input box for new commands. Having a polished interface
matters because it makes the tool more approachable. Instead of looking
like a wall of text, interactions feel organized, easy to follow, and
engaging.

### Agentic Workflow (Basic to Advanced)

As a demonstration of agentic behavior, the CLI will support multi-step
workflows. For example, "save work to remote" could trigger git add, git
commit with a generated message, and git push. Over time, this may
expand into a rule-based planner capable of dynamically sequencing
commands based on higher-level instructions. This progression shows how
the assistant can evolve from scripted helpers into more flexible
agents.

## Stretch Goals

If time allows, we have identified some additional features that would
add depth without shifting the project's main focus. They show how the
CLI could grow into something much more useful in the future.

### Developer Tool Wrappers

The CLI will also include features that make everyday work easier. It
will handle basic file tasks such as reading, writing, or listing files,
all within a safe environment so nothing important is changed by
accident. It will also make searching through code more straightforward
by connecting natural language requests to tools like ripgrep. For
instance, if a developer asks to find all TODOs in the project, the CLI
will return the results in a clear and organized way. These features do
not replace existing tools but make them easier to use, helping
developers stay focused on their main work without being slowed down by
routine steps.

### Repo Awareness 

The CLI is now repo-aware and automatically detects project types
(Rust, Node.js, Python, Go). It tailors commands based on the detected
project type (e.g., running `cargo test` for Rust or `npm test` for
Node.js). The infrastructure is also in place for proposing fixes as
diffs, with a workflow for users to accept, reject, or modify changes.
See [REPO_AWARENESS.md](REPO_AWARENESS.md) for details.

### Autocompletion Support

We may add autocompletion to improve usability when typing commands.
This could be implemented with existing Rust libraries such as reedline
or rustyline, offering suggestions as users type, for example, turning
"save" into "save work to remote." This feature would make the CLI feel
more interactive and approachable, similar to modern shell experiences.

### MCP Integration

We will add a minimal MCP client adapter that lets the CLI discover and
invoke a small, fixed set of MCP tools (e.g., search.ripgrep, fs.read,
and a composite git.save_work). The agentic workflow will treat MCP
tools and internal tools uniformly via a shared tool catalog, preserving
our existing safety rails (sandboxed paths, dry-run previews,
confirmations). If MCP discovery fails, the CLI will gracefully fall
back to internal implementations with no UX change. This integration
demonstrates standards-based composition without expanding scope beyond
a few well-defined tools.

## Operational Considerations

To keep things safe when running commands, we’ll show a preview before anything is executed, such as git add, git commit, or git push. Users will be able to confirm the action before it runs. This will be implemented during Phase 3, along with smooth error handling for any failures.
If Ollama isn’t available—whether it’s not installed, the server isn’t running, or the model isn’t found—the CLI will display a clear error message in the interface, such as “Can’t reach Ollama – is it running?”. We’ll introduce this basic detection in Phase 1 and refine the reconnect or retry experience in Phase 3.
We’ll also allow default settings, like the model name and timeout values, and optionally remember command history. This configuration and persistence will tie into the session state work during Phases 2 and 3.
For handling model responses, we’ll decide whether to stream output token-by-token or display it only after the full reply is generated. This choice will directly affect how we design the TUI and will be finalized in Phase 2.
Finally, we plan to include lightweight testing early on. This will cover how high-level user intentions are mapped to actual commands and how git workflows behave using temporary directories instead of real remotes. These tests will be added ahead of the broader testing and documentation effort in Phase 5.

## Tentative Plan

We have about two months to complete the project, which gives us enough
time to move carefully from basic foundations to a polished prototype.
To keep the workload balanced, both of us will handle aspects of
frontend and backend development. That way, each of us gains experience
across the full stack, and no one is locked into a single role.

### Phase 1: Foundations and Setup

The first step is to set up the environment and create the skeleton of
the CLI.

- Ruitong will prepare the Rust workspace, add the necessary dependencies such as Ratatui, and create the first structure of the CLI. This includes setting up a simple input-and-output loop so the CLI can take user commands.

- Yingxuan will focus on connecting Ollama for local inference. The goal here is to make sure the CLI can send prompts to the model and display the responses correctly.

By the end of this phase, we should have a basic CLI that runs locally,
accepts commands, and produces LLM output.

### Phase 2: Core Features

Once the groundwork is done, we move to the core features that define
the CLI.

- Ruitong will build session state management. The CLI will be able to remember the current directory, repository path, and a history of commands. This will make it feel closer to a real shell, where the user does not need to repeat the same information every time.

- Yingxuan will develop the Ratatui interface. The focus is on a scrollable conversation log that shows inputs and outputs in order, along with a clean input box for new commands. This step is important because it shapes how approachable and readable the tool feels.

During this phase, we will also connect the session logic and the model
output so that prompts can move naturally through the interface, giving
users a smooth interaction.

### Phase 3: Agentic Workflow and Polishing

In this phase, we will introduce simple agentic workflows to show the
practical value of the tool.

- Yingxuan will design the logic that maps high-level commands to sequences of smaller tool calls. For example, typing "save work to remote" should trigger git add, git commit with a generated message, and git push automatically.

- Ruitong connects the LLM so it can generate useful commit messages and then make sure the results are displayed clearly in the interface.

At the same time, we will polish the tool. This includes adding error
handling so the CLI responds gracefully to mistakes, cleaning up any
rough interactions, and making sure commands feel smooth and not clunky.

### Phase 4: Extra Features/Stretch Goal (If Time Is Permitted)

If progress is faster than expected, we will add extra features to make
the assistant more useful for daily work.

- Yingxuan may extend file operations. The CLI could support reading, writing, or editing small files, but only inside a safe sandbox to prevent accidental changes outside the project.

- Ruitong may add natural language wrappers for search tools such as ripgrep and integrate MCP tools. This would let a user type a request like "find all TODOs" and see the results displayed directly in the interface without having to remember the exact flags.

If time permits, we may also add features with repo awareness, and
autocompletion support to make typing commands faster and smoother.

### Phase 5: Testing, Documentation, and Demo Prep

In the final stage, we will both focus on testing and polish. We will
try the CLI on different systems and environments to make sure it
behaves consistently. We will also refine the documentation so that
anyone using the tool can understand how it works and what it can do.
Finally, we will prepare a demo script that highlights the strongest
features, including session memory, the interface, and the agentic
workflow. This stage is shared equally because it requires careful
review and clear communication from both of us.
