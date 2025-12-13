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

**Studnet names:**\
Ruitong Li, \
Yingxuan Hu, 1006881377

**Contact email:**\
ruiton.li@mail.utoronto.ca\
alvin.hu@mail.utoronto.ca

### Repo Awareness 

The CLI is now repo-aware and automatically detects project types
(Rust, Node.js, Python, Go). It tailors commands based on the detected
project type (e.g., running `cargo test` for Rust or `npm test` for
Node.js). The infrastructure is also in place for proposing fixes as
diffs, with a workflow for users to accept, reject, or modify changes.
See [REPO_AWARENESS.md](REPO_AWARENESS.md) for details.

# Table of Contents

- [Final Report: Rust Terminal AI Assistant CLI](#final-report-rust-terminal-ai-assistant-cli)
- [Table of Contents](#table-of-contents)
- [1. Motivation](#1-motivation)
- [2. Objectives](#2-objectives)
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

# 2. Objectives

# 3. Features(add more to this)

The final deliverable is a Rust native terminal application that combines a full screen chat interface with a local language model running through Ollama and a set of developer commands that operate on the current directory or git repository. The user stays in a single conversation view. Normal text is treated as a prompt to the model and the reply appears in the same log, while inputs that begin with a colon are treated as explicit commands and are handled by deterministic code. This keeps the assistant tightly connected to repository context instead of feeling like a separate chatbot.

### Terminal UI and conversation layout

The interface is built with Ratatui and Crossterm and runs in a full screen terminal mode. Messages are displayed in a conversation panel and labeled as System, You, or LLM so it is clear where each message comes from. Long lines wrap to the terminal width to maintain readability as the session grows. The input area stays fixed at the bottom so the user can keep typing while output streams, and a status line shows the active model, current working directory, and detected repo root when applicable. When the tool is waiting on the model or running a command, an activity indicator is shown so the user can distinguish work in progress from a frozen interface.

### Chat model and streaming output

The chat feature uses local inference through Ollama. Prompts are sent to Ollama's local server and responses are displayed using streaming output, so text appears gradually rather than arriving all at once. This makes the interaction feel faster and more interactive, especially for longer responses. The default model is llama3, but users can switch models using an environment variable without changing code. The tool also checks that Ollama is reachable and reports clear errors in the conversation log if the server is not running or the model is missing.

### Session context and prompt iteration

To support real terminal workflows, the tool keeps lightweight session context. It tracks the current working directory and detects the git repo root when the user is inside a repository, which becomes the reference point for repo scoped commands and reduces confusion about where actions apply. The tool also maintains in memory prompt history that can be navigated with the keyboard, which supports iterative prompting by letting users recall, edit, and resend previous prompts quickly.

### Command routing and safety boundary

Command routing is a core part of the design and the main safety mechanism. The tool parses inputs that begin with a colon and routes them to deterministic implementations, while all other inputs are treated as model prompts. Because of this separation, model generated text is never treated as a command and cannot trigger execution. Tool actions only happen when the user explicitly types a supported command pattern.

### Git save workflow

The tool's git save workflow demonstrates multi step automation in a controlled way. Developers often repeat the same steps when saving work, including staging changes, committing, and pushing. The planning form, invoked with :save, prints a preview that includes repository and branch context, a status snapshot, and the intended sequence of git operations. The execution form, invoked with :save! followed by a commit message, runs the workflow and prints a report describing what was executed and how the repo state changed. This design supports convenience while keeping execution intentional and transparent.

### Search and read inside repo

To make code context gathering fast inside the TUI, the tool includes repo inspection commands. The search command, :find followed by a pattern, runs ripgrep and returns line numbered matches in the conversation. The file reading command, :read followed by a relative path, prints file contents into the same log so the user can reference them while asking the model questions. File reads are restricted to the repository or current directory to reduce accidental access outside the intended scope, and large outputs are truncated to keep the interface responsive.

### Diff inspection and diff summarization

For reviewing changes, the tool supports diff inspection and model assisted summarization. The :diff command displays a diff summary along with the current git diff. The :summarize-diff command captures that diff and asks the local model to produce a concise summary that is useful for commit messages or review notes. This feature combines deterministic output, which provides exact change context, with an LLM, which provides synthesis and clear writing.

### Polish and limits

Several design choices help the tool stay usable in real sessions. Streaming output reduces perceived latency, wrapped text prevents layout issues, and truncation avoids flooding the interface with large command output. The status bar keeps key context visible so users understand where commands apply. At the same time, the tool intentionally does not implement long term semantic memory beyond prompt history, it does not provide advanced scrollback controls beyond the wrapped conversation view, and it does not include a separate interactive confirmation screen for potentially destructive actions beyond the existing plan versus execute split. These limitations kept the implementation within scope while leaving clear room for future improvement.

# 4. User Guide and Developer Guide

# 5. Reproducibility Guide

This section explains how to set up and run the project from a clean environment on macOS Sonoma and on an Ubuntu Linux server. The steps are written so the instructor can follow them exactly without filling in missing details. The project is a Rust native full screen terminal application. You build it with Cargo and run it from a terminal. The chat features and diff summarization depend on a local Ollama server because the tool sends prompts to Ollama over HTTP. If Ollama is not installed, not running, or the model is missing, the UI will still launch but the LLM features will show a clear System error message instead of a response.

Repo related features such as viewing diffs and running the git save workflow require running the tool inside a git repository. If you run it in a directory that is not a repository, repo commands will either report that no repository was found or return empty output, which is expected behavior. The code search command uses ripgrep, so ripgrep must be installed if you want :find to work.

### Prerequisites on both macOS and Ubuntu

You need a working Rust toolchain so Cargo can build the project, and you need Ollama installed to provide local model inference. You also need to pull at least one model into Ollama. The project uses llama3 by default, so pulling llama3 is the simplest way to match expected behavior. Git should be installed if you want to use :diff, :summarize-diff, :save, or :save!, since those features call git under the hood. Ripgrep is optional and only required for :find.

The tool expects to connect to Ollama at the default local address. If Ollama is running elsewhere, you can set OLLAMA_HOST, and if you want a different model you can set OLLAMA_MODEL. Both options are described later.

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

At this point, Ollama is ready and the model is available locally. If you want to use the :find command, install ripgrep.

`brew install ripgrep`

Now build and run the project. These commands must be run from the project root directory, meaning the folder that contains Cargo.toml.

`cargo build
cargo run`

When the program starts, it switches the terminal into a full screen interface. You can type normal text and press Enter to chat with the model. You can press Up and Down to move through prompt history and resend a modified prompt. You can also use explicit commands. For example, :diff shows the current git diff when you are inside a repository, and :summarize-diff asks the model to summarize that diff into commit or review style text. The :save command prints a plan for a git add commit push workflow, and :save! followed by a commit message executes that workflow. The :read command prints the contents of a file given a relative path, and :find searches the repository using ripgrep if it is installed.

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

If you want to use the :find command, install ripgrep.

`sudo apt install -y ripgrep`

Now build and run the project from the repository root directory.

`cargo build
cargo run`

The program will open the full screen TUI inside the terminal session. The basic interaction is the same as on macOS. Normal input becomes a model prompt, and colon commands trigger deterministic tools. For repo commands, ensure you are running inside a git repository. If you are testing on a server and want to avoid pushing to a real remote, you can still run :save as a safe preview without executing anything.

### Optional configuration on both platforms

The tool supports selecting a different Ollama model. To do that, set OLLAMA_MODEL to the desired model name, pull it into Ollama, then run the tool. The pull step is important because setting the environment variable alone does not download the model.

`export OLLAMA_MODEL=llama3.2:3b
ollama pull llama3.2:3b
cargo run`

The tool also supports changing the Ollama server address. This is useful if Ollama is bound to a different interface, if you are running inside a container, or if your environment routes Ollama through a proxy. The host value should include both the protocol and the port. The default port is 11434, so an example override looks like this.

`export OLLAMA_HOST=http://127.0.0.1:11434
cargo run`

This avoids a common mistake of omitting the port, which would cause the tool to connect to the wrong address and fail.

### Quick sanity checks after launch

After the UI opens, first verify model chat by typing a short prompt such as "Give me a two sentence description of this tool" and confirm that the response streams into the conversation. If you do not see a response, check that Ollama is running and that the model has been pulled.

Next verify prompt history by pressing Up to recall the last prompt, editing it slightly, and resending it. This confirms that the session history is active.

If you are inside a git repository with changes, verify repo features by running :diff to show the diff and then :summarize-diff to generate a summary. If you want to test the workflow planning feature safely, run :save and confirm that a plan is printed without executing git operations.

Finally, verify developer tooling by running :read Cargo.toml or another small file and by running :find TODO if ripgrep is installed.

### Common issues and expected behavior

If the tool reports that it cannot reach Ollama, the server is likely not running. Start ollama serve and try again. If the tool reports that the model is missing, run ollama pull with the model name you are using. If :find does not work, confirm that ripgrep is installed by running rg --version. If git related commands do not work, confirm that git is installed and that you launched the tool inside a git repository.
# 6.Contributions by Each Team Member
### Yingxuan

Yingxuan worked on the parts of the project that connect the chat experience to local inference and to practical repository workflows. A large portion of this work was around the Ollama integration, where the focus was making the assistant feel responsive and reliable in real use. Model requests were implemented with streaming output so replies appear progressively in the interface instead of arriving all at once, and common failure cases such as Ollama not running or a model not being available were handled with clear, user facing error messages inside the conversation log.

Yingxuan also contributed to features that make repeated use comfortable in a terminal setting. This included prompt history navigation so users can recall and refine earlier prompts quickly, and session context tracking such as the current working directory and detection of a git repo root when the tool is used inside a repository. These elements help the assistant behave less like a one off chatbot and more like a tool that understands the user’s working context.

On the developer tooling side, Yingxuan implemented and refined the explicit command routing that separates normal chat prompts from deterministic tool commands. This routing enabled repo oriented features such as code search through a ripgrep wrapper, safe file reading limited to the repository or current directory, and diff inspection that surfaces current changes without leaving the TUI. Yingxuan also implemented the diff summarization flow that captures a git diff and composes a structured prompt for the local model, producing a concise summary that is useful for commit messages or review notes. In addition, Yingxuan helped implement the git save workflow that demonstrates controlled agent like behavior by supporting a planning mode and a separate execution mode, then formatting the results so the workflow is transparent to the user.

To support reliability and grading expectations, Yingxuan added a regression test around the git save planning behavior using a temporary repository to confirm that the expected structured output remains stable as the code evolves. Yingxuan also contributed to documentation updates so the project is easy to reproduce and demo, including quickstart steps, configuration options such as the model selection, and a clear command reference for users and instructors.
### Ruitong


# 7. Lessons Learned and Concluding Remarks

We learned that streaming output is not just a nice extra feature but a major part of how users judge responsiveness. Even when a model takes the same amount of time to finish, showing text as it arrives makes the tool feel faster and reduces uncertainty. Implementing streaming also pushed us to design the program with clearer separation between rendering, session state, and background work, because the UI must update smoothly while new tokens continue to arrive.

We also learned that “agentic” behavior only feels useful when it is easy to trust. Developers are comfortable with automation when they can predict what will happen and when execution is clearly intentional. If command execution is triggered indirectly through free form text, users lose confidence quickly because the consequences can be real, especially in a git repository. Our plan and execute approach reinforced that it is possible to demonstrate multi step workflows while still keeping the user in control.

Another lesson was that repository awareness can be valuable without being complicated. We did not need deep project analysis to create something useful. Simply detecting the repo root, exposing diff inspection, and adding a diff summarization command already supports common tasks like writing commit messages and preparing review notes. This incremental approach is practical because it delivers value early and provides a clear path for adding deeper repo features later without redesigning the system.

We also gained a better understanding of how much work goes into a terminal UI that feels stable. A TUI is easy to get working at a basic level, but small details determine whether it feels polished. Handling raw mode correctly, keeping layout consistent, wrapping text, truncating large outputs, and showing clear error messages all matter for reliability and usability. These details became especially important because our tool needs to remain readable while it is actively streaming output and printing tool results.

Overall, the project showed us that the best way to build an LLM assisted developer tool is to be deliberate about boundaries. The model is strongest at explanation and summarization, while deterministic commands are best for actions like reading files, searching code, and running git operations. Keeping those roles separate made the tool easier to reason about and safer to use, and it gives us a clear direction for future improvements.