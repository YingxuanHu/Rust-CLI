# Video Demo Outline (3–10 min)

| # | Feature | What to Show (Example) | Talking Points (while showing) |
|---|---------|------------------------|--------------------------------|
| 1 | **Rust-native TUI & local models** | Launch with `cargo run` while `ollama serve` is running | “We’re running fully local—Rust TUI plus Ollama. Notice the status bar showing model, repo, cwd, and a clear seperation between Conversation and Input areas.” |

To start with, we can go to the help menu, which shows user guidance and keybindings.

next, input area, we can toggle between Chat and Shell modes using Ctrl+S. in chat mode we can input natural language prompts like "how is it going", while in shell mode we can run shell commands directly, like ls or pwd. we can also run shell commands in chat mode by prefixing them with a $ sign, like $ ls.

we can also go back to the previous command or prompt using the Up arrow key, and go forward using the Down arrow key. this works in both chat and shell modes. the two modes have separate histories, so switching between them preserves the context.

| 2 | **Semantic chat & streaming** | Ask “summarize the repo structure” and watch streamed reply | “Chat goes through the same window; text streams token-by-token so you see progress. It auto-injects recent context like repo paths.” |

our tool also supports semantic chat, which means it can understand and respond to natural language prompts about the codebase. for example, we can ask "summarize the repo structure", and it will generate a summary based on the files and directories in the repo. the response streams token-by-token, so we can see it generating in real-time.

we also have auto-completion for workflows and file paths. for example, if we type "open file ", it will suggest relevant file paths in the repo based on frequency and recency. We can tab to accept suggestions.

| 3 | **Intent-based workflows** | Type “save work” to trigger git plan preview | “Natural language triggers workflows. ‘Save work’ expands into plan + confirmation before staging, committing, pushing.” |

One of our major features is workflows. which means we can trigger complex actions with simple natural language commands. for example, if we type "save work", it will generate a git plan that shows the steps it will take to stage, commit, and push our changes. we can review the plan and confirm before proceeding. 

Adding some changes to a file, we can see how the tool detects the changes and includes them in the git plan.

TODO: Delete this message before submission. 

| 4 | **LLM commit message suggestion** | After staging, accept generated message | “We partner with the model to draft commit messages. You can accept/edit before commit so nothing happens blindly.” |
| 5 | **Shell + bang shortcuts** | Use Ctrl+S to enter Shell mode, run `$ ls` and `!!` | “Toggle Shell mode for direct commands, or prefix with `$` in Chat. History-aware bang shortcuts mirror Bash muscle memory.” |
| 6 | **Custom command learning** | Trigger an LLM suggestion, choose `s` to save | “When the assistant proposes a command you like, press `s` to save it. Next time, typing its nickname resolves instantly (Tier 1).” |
| 7 | **Config & defaults** | Show `.llm-cli/config.toml` vs defaults | “Config lives in `.llm-cli`. If it’s missing, defaults kick in. You can tweak models, prompts, timeouts without recompiling.” |

## Flow Tips
- Open with quick project framing (goal: local AI assistant for daily repo work).
- Spend ~45–60 seconds per feature; linger longer on workflows if needed.
- Keep terminal font large, narrate what you type, and surface status bars/logs when they change.
