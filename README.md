# LLM-Powered CLI

**Studnet names:**\
Ruitong Li, \
Yingxuan Hu, 1006881377

**Contact email:**\
ruiton.li@mail.utoronto.ca\
alvin.hu@mail.utoronto.ca

# Table of Contents

- [LLM-Powered CLI](#llm-powered-cli)
- [Table of Contents](#table-of-contents)
- [1. Motivation](#1-motivation)
- [2. Objectives](#2-objectives)
    - [Stateful CLI Context (System+LLM)](#stateful-cli-context-systemllm)
    - [Local Inference with Ollama (Stateless Prompts)](#local-inference-with-ollama-stateless-prompts)
    - [Ratatui TUI with Conversation Log](#ratatui-tui-with-conversation-log)
    - [Agentic Workflow (Basic to Advanced)](#agentic-workflow-basic-to-advanced)
    - [Repo Awareness](#repo-awareness)
    - [Stretch Goals](#stretch-goals)
- [3. Features(add more to this)](#3-featuresadd-more-to-this)
    - [Terminal UI and conversation layout](#terminal-ui-and-conversation-layout)
    - [Chat model and streaming output](#chat-model-and-streaming-output)
    - [Session context and prompt iteration](#session-context-and-prompt-iteration)
    - [Command routing and safety boundary](#command-routing-and-safety-boundary)
    - [Git save workflow](#git-save-workflow)
    - [Search and read inside repo](#search-and-read-inside-repo)
    - [Diff inspection and diff summarization](#diff-inspection-and-diff-summarization)
    - [Polish and limits](#polish-and-limits)
- [4. User Guide and Developer Guide](#4-user-guide-and-developer-guide)
- [5. Reproducibility Guide](#5-reproducibility-guide)
    - [Prerequisites on both macOS and Ubuntu](#prerequisites-on-both-macos-and-ubuntu)
    - [macOS Sonoma setup](#macos-sonoma-setup)
    - [Ubuntu Linux server setup](#ubuntu-linux-server-setup)
    - [Optional configuration on both platforms](#optional-configuration-on-both-platforms)
    - [Quick sanity checks after launch](#quick-sanity-checks-after-launch)
    - [Common issues and expected behavior](#common-issues-and-expected-behavior)
- [6.Contributions by Each Team Member](#6contributions-by-each-team-member)
    - [Yingxuan](#yingxuan)
    - [Ruitong](#ruitong)
- [7. Lessons Learned and Concluding Remarks](#7-lessons-learned-and-concluding-remarks)


# 1. Motivation

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

# 2. Objectives

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

### Repo Awareness 

The CLI is now repo-aware and automatically detects project types
(Rust, Node.js, Python, Go). It tailors commands based on the detected
project type (e.g., running `cargo test` for Rust or `npm test` for
Node.js). The infrastructure is also in place for proposing fixes as
diffs, with a workflow for users to accept, reject, or modify changes.
See [REPO_AWARENESS.md](REPO_AWARENESS.md) for details.

### Stretch Goals

If time allows, we have identified some additional features that would
add depth without shifting the project's main focus. They show how the
CLI could grow into something much more useful in the future.
# 3. Features(add more to this)

The final deliverable is a Rust native terminal application that combines a full screen chat interface with a local language model running through Ollama and a set of developer tools that operate on the current directory or git repository. The user stays in a single conversation view. In Chat mode, normal text is treated as a prompt to the model (or a recognized tool intent) and the reply appears in the same log. In Shell mode, inputs are executed as shell commands in the current working directory. This keeps the assistant tightly connected to repository context instead of feeling like a separate chatbot.

### Terminal UI and conversation layout

The interface is built with Ratatui and Crossterm and runs in a full screen terminal mode. Messages are displayed in a conversation panel and labeled as System, user, or assistant so it is clear where each message comes from. Long lines wrap to the terminal width to maintain readability as the session grows. The input area stays fixed at the bottom so the user can keep typing while output streams. A compact status line shows the active model, current working directory, and lightweight runtime state such as pending replies or an in progress workflow confirmation. The UI also supports simple quality of life controls such as scrolling the conversation and accepting inline (ghost) completions.

### Chat model and streaming output

The chat feature uses local inference through Ollama. Prompts are sent to Ollama and responses are displayed using streaming output, so text appears gradually rather than arriving all at once. This makes the interaction feel faster and more interactive, especially for longer responses. The default model is llama3, but users can switch models using configuration or the LLM_CLI_MODEL environment variable. On startup (and via the health subcommand), the tool checks whether Ollama is reachable and whether the selected model exists locally.

### Session context and prompt iteration

To support real terminal workflows, the tool keeps lightweight session context. It tracks the current working directory, detects a git repo root when applicable, and detects the project type (Rust, Node.js, Python, Go) to choose sensible build/test defaults. The tool also maintains in memory input history that can be navigated with the keyboard, which supports iterative prompting by letting users recall, edit, and resend previous prompts quickly. In addition, the tool records a small window of recent deterministic outputs (e.g., git status/diff previews and file reads) and can inject them into the model prompt when the user refers to “it”, “that”, or “the diff”.

### Command routing and safety boundary

Command routing is a core part of the design and the main safety mechanism. The tool supports two main interaction styles: Chat mode and Shell mode. In Chat mode, the input is interpreted either as normal chat (sent to the LLM) or as a recognized tool intent (handled by deterministic code). In Shell mode, inputs are executed as shell commands in the current working directory, with built in handling for cd and pwd and basic safeguards against obviously interactive commands (for example, a bare git commit without a message). When the assistant response contains shell commands, the tool does not execute them automatically; it prompts the user to confirm before executing or saving them.

### Git save workflow

The tool’s git save workflow demonstrates multi step automation in a controlled way. Developers often repeat the same steps when saving work, including staging changes, committing, and pushing. When the user asks to “save work” or “push changes”, the tool first prints a clear plan (including a short status preview) and asks for confirmation. If confirmed, it stages changes, suggests a commit message based on staged diffs, lets the user accept or override the message, then runs commit and push and prints a short execution report. This design supports convenience while keeping execution intentional and transparent.

### Search and read inside repo

To make code context gathering fast inside the TUI, the tool includes a small set of repo inspection actions. The tool can display a file’s contents inside the conversation log (for example, “show src/main.rs”) and can list directories (for example, “list files in src”). For lightweight search, it supports a focused TODO/FIXME scan (“find todos”) using ripgrep when available. File writing is supported via an explicit “write file … with content: …” action and always requires a confirmation step before creating or overwriting a file.

### Diff inspection and diff summarization

For reviewing changes, the tool supports diff inspection with lightweight follow ups. A “status” request shows git status and a diffstat summary, then allows the user to provide a file path to view that file’s git diff. For model assisted writing, the tool can generate a suggested commit message based on staged changes, which is useful for turning a diff into a readable summary for commits. A dedicated “summarize diff” command is not implemented; summarization is currently focused on commit message generation for staged diffs.

### Polish and limits

Several design choices help the tool stay usable in real sessions. Streaming output reduces perceived latency, wrapped text prevents layout issues, and confirmations are used for workflows and for executing shell commands suggested by the assistant. The status bar keeps key context visible so users understand where actions apply, and the tool supports basic scrolling and history navigation for longer sessions. At the same time, the tool intentionally keeps memory limited to the current session (history plus a small window of recent outputs), provides only basic scrollback controls, and focuses developer tooling on a small set of common actions rather than a large command surface. These limitations kept the implementation within scope while leaving clear room for future improvement.

# 4. User Guide and Developer Guide

A user can start the program from the repository root or from any directory they want to work in. The interface opens as a full screen terminal view. The main area shows the conversation log, and an input box at the bottom accepts text. When the user types normal text and presses Enter, the tool treats it as an LLM prompt. It sends the prompt to the local Ollama server and streams the reply into the conversation log. This is the standard mode for asking questions, getting explanations, drafting short text, and summarizing information.

Prompt history is available through the Up and Down arrow keys. Pressing Up brings the previous prompt back into the input box. The user can edit it and press Enter to send it again. This supports iterative prompting and makes it faster to refine instructions without retyping.

The tool supports two interaction styles. In Chat mode (default), the input is interpreted either as normal chat (sent to the LLM) or as a recognized tool intent (handled by deterministic code). In Shell mode (toggle with Ctrl+S), inputs are executed as shell commands in the current working directory. As a shortcut, you can also run explicit shell commands in Chat mode by prefixing the line with $ or !, and you can repeat the last shell command with !!.

Common developer actions are exposed through natural language. For example, “status” shows git status and a diffstat summary, and then you can type a file path to view that file’s diff. “save work” prints a plan and asks for confirmation before staging, generating a suggested commit message, committing, and pushing. “find todos” runs a TODO/FIXME scan (requires ripgrep). “show src/main.rs” prints file contents in the log, “list files in src” lists directory entries, and “write file notes.txt with content: …” creates or overwrites a file after a confirmation prompt.

From a developer perspective, the codebase is organized so each part has a clear job. The UI module handles terminal setup, layout, rendering, and input events. The session module tracks runtime context such as the working directory, repo root detection, and prompt history. The model module communicates with Ollama, including streaming responses. The command routing module resolves user intent (chat vs tool vs shell) and calls the deterministic tool implementations, while workflows use explicit confirmations before execution. This design keeps a clear boundary between model text generation and command execution, which supports both safety and easier maintenance.

# 5. Reproducibility Guide

This section explains how to set up and run the project from a clean environment on macOS Sonoma and on an Ubuntu Linux server. The steps are written so the instructor can follow them exactly without filling in missing details. The project is a Rust native full screen terminal application. You build it with Cargo and run it from a terminal. The chat features depend on a local Ollama daemon because the tool invokes the `ollama` CLI for inference. If Ollama is not installed, not running, or the selected model is missing, the program will fail fast at startup with a clear error message.

Repo related features such as viewing diffs and running the git save workflow require running the tool inside a git repository. If you run it in a directory that is not a repository, repo oriented actions (like status/save work/commit) will report that no repository was found, which is expected behavior. The TODO scan uses ripgrep, so ripgrep must be installed if you want “find todos” to work.

### Prerequisites on both macOS and Ubuntu

You need a working Rust toolchain so Cargo can build the project, and you need Ollama installed to provide local model inference. You also need to pull at least one model into Ollama. The project uses llama3 by default, so pulling llama3 is the simplest way to match expected behavior. Git should be installed if you want repo workflows (status, stage, commit, save work) since those features call git under the hood. Ripgrep is optional and only required for the “find todos” scan.

### macOS Sonoma setup

Begin by installing Rust using rustup. This is the standard method and ensures Cargo and the compiler are placed in your home directory in a predictable way.

`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

After installation, restart the terminal. This step matters on macOS because the installer updates your shell profile so Cargo can be found in new sessions. Confirm that both rustc and cargo are available.

`rustc --version
cargo --version`

Next install Ollama using Homebrew. This installs the Ollama application, which runs a background service that the CLI tool communicates with.

`brew install --cask ollama`

Launch Ollama once so the background service starts. You can do this by opening the Ollama app from Spotlight. Once it has been launched, pull the default model used by the project.

`ollama pull llama3`

At this point, Ollama is ready and the model is available locally. If you want to use the “find todos” scan, install ripgrep.

`brew install ripgrep`

Now build and run the project. These commands must be run from the project root directory, meaning the folder that contains Cargo.toml.

`cargo build
cargo run`

When the program starts, it switches the terminal into a full screen interface. You can type normal text and press Enter to chat with the model. You can press Up and Down to move through input history and resend a modified prompt. You can also trigger deterministic tools using natural language. For example, “status” shows git status and diffstat and then lets you type a file path to view that file’s diff. “find todos” runs a TODO/FIXME scan if ripgrep is installed. “show Cargo.toml” prints a file into the conversation, and “list files in src” lists directory entries. To save work, use “save work” (it prints a plan and asks for confirmation before staging, committing, and pushing).

### Ubuntu Linux server setup

Start by installing basic packages needed for building and using the tool. Build essential provides a standard compilation environment, curl is needed to download installers, and git is required for repository features.

`sudo apt update
sudo apt install -y build-essential curl git`

Install Rust using rustup in non interactive mode. Then load the Cargo environment into the current shell session so cargo is available immediately without logging out.

`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -y
source "$HOME/.cargo/env"`

Confirm that Rust is installed.

`rustc --version
cargo --version`

Install Ollama using the official Linux installer script.

`curl -fsSL https://ollama.com/install.sh | sh`

Start the Ollama server. On a server, you should run this in a dedicated terminal session because it needs to stay running while the TUI is in use.

`ollama serve`

In a second SSH session, pull the default model. This separates model downloads from the server process and makes it easier to see errors if the pull fails.

`ollama pull llama3`

If you want to use the “find todos” scan, install ripgrep.

`sudo apt install -y ripgrep`

Now build and run the project from the repository root directory.

`cargo build
cargo run`

The program will open the full screen TUI inside the terminal session. The basic interaction is the same as on macOS. Normal input becomes a model prompt (or a recognized tool intent), and you can toggle into Shell mode with Ctrl+S to run shell commands directly. For repo workflows, ensure you are running inside a git repository. If you are testing on a server and want to avoid pushing to a real remote, you can still type “save work” to see the plan and then cancel instead of confirming.

### Optional configuration on both platforms

The tool supports selecting a different model. To do that, set LLM_CLI_MODEL to the desired model name, pull it into Ollama, then run the tool. The pull step is important because setting the environment variable alone does not download the model.

`export LLM_CLI_MODEL=llama3.2:3b
ollama pull llama3.2:3b
cargo run`

You can also set values in `.llm-cli/config.toml` (or pass `--config <path>`), which is useful when you want a repeatable setup without exporting environment variables.

### Quick sanity checks after launch

After the UI opens, first verify model chat by typing a short prompt such as "Give me a two sentence description of this tool" and confirm that the response streams into the conversation. If you do not see a response, check that Ollama is running and that the model has been pulled.

Next verify prompt history by pressing Up to recall the last prompt, editing it slightly, and resending it. This confirms that the session history is active.

If you are inside a git repository with changes, verify repo features by typing “status” to show status and diffstat, then type a file path to view that file’s diff. To test the workflow planning feature safely, type “save work” and confirm that a plan is printed, then type anything other than Enter/yes to cancel.

Finally, verify developer tooling by typing “show Cargo.toml” (or another small file), “list files in src”, and “find todos” if ripgrep is installed.

### Common issues and expected behavior

If the tool reports that it cannot reach Ollama, the server is likely not running. Start ollama serve and try again. If the tool reports that the model is missing, run ollama pull with the model name you are using. If “find todos” does not work, confirm that ripgrep is installed by running rg --version. If git related workflows do not work, confirm that git is installed and that you launched the tool inside a git repository.
# 6.Contributions by Each Team Member
### Yingxuan
Yingxuan focused on connecting the chat experience to local inference and practical repository workflows. This included implementing the Ollama integration with streaming output so responses appear progressively in the UI, and adding startup and health checks so common setup problems (Ollama not running or a missing model) fail fast with clear messages.

Yingxuan also improved usability for repeated terminal use by implementing prompt history navigation and lightweight session context tracking, including the current working directory and git repo root detection when the tool is used inside a repository. This helps the tool feel consistent across a session and reduces confusion about where repo scoped commands apply.

On the developer tooling side, Yingxuan implemented and refined intent based routing that separates normal chat prompts from deterministic tool actions. This enabled repo oriented features such as git status and diff previews, TODO/FIXME scanning via a ripgrep wrapper, and a commit message suggestion feature based on staged diffs. Yingxuan also contributed to the git save workflow by supporting a plan and confirmation flow and ensuring the results are presented clearly so users can understand what will happen and what was done.

To support reliability, Yingxuan added unit tests for key helper modules (such as learned aliases, file operations, and context formatting) and contributed to documentation updates that explain setup, configuration options such as model selection, and the command reference used in reproducibility and demo materials.

### Ruitong


# 7. Lessons Learned and Concluding Remarks

We learned that streaming output is not just a nice extra feature but a major part of how users judge responsiveness. Even when a model takes the same amount of time to finish, showing text as it arrives makes the tool feel faster and reduces uncertainty. Implementing streaming also pushed us to design the program with clearer separation between rendering, session state, and background work, because the UI must update smoothly while new tokens continue to arrive.

We also learned that “agentic” behavior only feels useful when it is easy to trust. Developers are comfortable with automation when they can predict what will happen and when execution is clearly intentional. If command execution is triggered indirectly through free form text, users lose confidence quickly because the consequences can be real, especially in a git repository. Our plan and execute approach reinforced that it is possible to demonstrate multi step workflows while still keeping the user in control.

Another lesson was that repository awareness can be valuable without being complicated. We did not need deep project analysis to create something useful. Simply detecting the repo root, exposing status and diff previews, and generating commit message suggestions from staged diffs already supports common tasks like writing commit messages and preparing short review notes. This incremental approach is practical because it delivers value early and provides a clear path for adding deeper repo features later without redesigning the system.

We also gained a better understanding of how much work goes into a terminal UI that feels stable. A TUI is easy to get working at a basic level, but small details determine whether it feels polished. Handling raw mode correctly, keeping layout consistent, wrapping text, truncating large outputs, and showing clear error messages all matter for reliability and usability. These details became especially important because our tool needs to remain readable while it is actively streaming output and printing tool results.

Overall, the project showed us that the best way to build an LLM assisted developer tool is to be deliberate about boundaries. The model is strongest at explanation and summarization, while deterministic commands are best for actions like reading files, searching code, and running git operations. Keeping those roles separate made the tool easier to reason about and safer to use, and it gives us a clear direction for future improvements.
