use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

use tokio::{
    process::Command as TokioCommand,
    sync::mpsc,
};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use serde::{Deserialize, Serialize};

use std::sync::Arc;

use crate::{
    completion::CompletionProvider,
    config::Config,
    context,
    embedding::EmbeddingCache,
    frecency::FrecencyTracker,
    handlers::{dispatch_intent, AssistantEvent, IntentDispatcher},
    input::{expand_bang_shortcut, HistoryNavigation},
    intent::{self, ParsedIntent},
    learned::LearnedAliases,
    patch::{self, AppliedPatch, PatchReview},
    session::{Message, Role, SessionState},
    task_runner::{self, TaskHandle, TaskId, TaskOutcome, TaskSnapshot, TaskSpec},
    ui::{render_ui, AppView, InputMode, TerminalGuard},
    workflow::{generate_commit_message_async, handle_workflow_response, WorkflowResponder, WorkflowState},
};

pub async fn run(config: Config) -> Result<()> {
    // Initialize embedding cache for semantic intent matching
    let mut embedding_cache = EmbeddingCache::new(
        Some(&config.embedding_model),
        config.request_timeout_secs,
        &config.ollama_host,
    );
    tracing::info!("Initializing embedding cache (this may take a moment)...");
    if let Err(e) = embedding_cache.initialize(Some(&config.embedding_cache_path)).await {
        tracing::warn!(
            "Warning: Could not initialize embeddings: {}. Falling back to direct chat.",
            e
        );
        tracing::info!("Tip: Run 'ollama pull {}' to enable semantic matching.", config.embedding_model);
    } else {
        tracing::info!("Embedding cache ready!");
    }
    let embedding_cache = Arc::new(embedding_cache);

    let mut terminal = TerminalGuard::new().context("setting up terminal")?;
    let mut app = App::new(config, Arc::clone(&embedding_cache));
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    let result = async {
        loop {
            terminal
                .terminal
                .draw(|f| {
                    let view = app.create_view();
                    render_ui(f, view);
                })
                .context("drawing frame")?;

            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or(Duration::from_millis(0));

            if event::poll(timeout)? {
                match event::read()? {
                    Event::Key(key) => {
                        if key.kind == KeyEventKind::Press {
                            handle_key_event(&mut app, key);
                        }
                    }
                    Event::Mouse(mouse) => handle_mouse_event(&mut app, mouse),
                    _ => {}
                }
            }

            app.poll_assistant();
            app.poll_command_task();

            if app.should_quit {
                app.save_input_history();
                let _ = app.frecency.save();
                break;
            }

            if last_tick.elapsed() >= tick_rate {
                last_tick = Instant::now();
            }
        }
        Ok(())
    }.await;
    // This also runs when rendering or input fails, not just on Escape.
    app.shutdown_command_task().await;
    result
}

const MAX_INPUT_HISTORY: usize = 500;

#[derive(Debug, Serialize, Deserialize)]
struct PersistedInput {
    input: String,
}

struct ActiveTask {
    id: TaskId,
    label: String,
    command: String,
    cwd: PathBuf,
    request_cwd: PathBuf,
    message_idx: usize,
    started: Instant,
    cancelling: bool,
    handle: TaskHandle,
}

impl ActiveTask {
    fn status(&self) -> String {
        format!("{} #{} • running {:.1}s • {}", self.label, self.id.0,
            self.started.elapsed().as_secs_f64(),
            if self.cancelling { "cancelling…" } else { "Ctrl+C cancel" })
    }

    fn render(&self, snapshot: &TaskSnapshot) -> String {
        let state = match &snapshot.outcome {
            Some(outcome) => task_outcome_label(outcome),
            None if self.cancelling => "CANCELLING — stopping child processes".to_string(),
            None => "RUNNING — Ctrl+C or `cancel task` to stop".to_string(),
        };
        let mut text = format!("{} #{} — {}\nDirectory: {}\n{} • {:.1}s\n",
            self.label, self.id.0, self.command, self.cwd.display(), state,
            self.started.elapsed().as_secs_f64());
        for (name, output, truncated) in [
            ("stdout", &snapshot.stdout, snapshot.stdout_truncated),
            ("stderr", &snapshot.stderr, snapshot.stderr_truncated),
        ] {
            if !output.is_empty() || truncated {
                text.push_str(&format!("\n{name}:\n"));
                if truncated {
                    text.push_str("[Earlier output truncated; showing the retained tail]\n");
                }
                // Do not pass terminal-control bytes from a subprocess through
                // Ratatui. Newlines and tabs remain useful in test diagnostics.
                text.extend(output.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')));
                text.push('\n');
            }
        }
        if snapshot.stdout.is_empty() && snapshot.stderr.is_empty() {
            text.push_str(if snapshot.outcome.is_some() { "\n(no output)" } else { "\nWaiting for output…" });
        }
        if snapshot.outcome.is_some() {
            text.push_str("\nType `run tests` to run the current project's suite again.");
        }
        text
    }
}

fn task_outcome_label(outcome: &TaskOutcome) -> String {
    match outcome {
        TaskOutcome::Succeeded => "PASSED (exit 0)".to_string(),
        TaskOutcome::Failed { code: Some(code) } => format!("FAILED (exit {code})"),
        TaskOutcome::Failed { code: None } => "FAILED (terminated by signal)".to_string(),
        TaskOutcome::Cancelled => "CANCELLED".to_string(),
        TaskOutcome::TimedOut => "TIMED OUT".to_string(),
        TaskOutcome::Error(error) => format!("TASK ERROR: {error}"),
    }
}

struct App {
    config: Config,
    session: SessionState,
    input: String,
    messages: Vec<Message>,
    scroll: usize,
    scroll_locked: bool, // When true, don't auto-reset scroll to bottom
    pending_idxs: Vec<usize>,
    input_history: Vec<String>,
    history_idx: Option<usize>,
    pending_workflow: Option<WorkflowState>,
    last_applied_patch: Option<AppliedPatch>,
    assistant_tx: mpsc::UnboundedSender<AssistantEvent>,
    assistant_rx: mpsc::UnboundedReceiver<AssistantEvent>,
    embedding_cache: Arc<EmbeddingCache>,
    input_mode: InputMode,
    should_quit: bool,
    viewing_history: bool,
    // Autocompletion state
    completion_provider: CompletionProvider,
    ghost_text: Option<String>,
    frecency: FrecencyTracker,
    active_task: Option<ActiveTask>,
    next_task_id: u64,
}

impl App {
    fn new(config: Config, embedding_cache: Arc<EmbeddingCache>) -> Self {
        let (assistant_tx, assistant_rx) = mpsc::unbounded_channel();
        let embeddings_ready = embedding_cache.is_initialized();
        let input_history = load_input_history(&config.history_path);
        
        // Prepare frecency tracker path before moving config
        let frecency_path = config.learned_path
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("frecency.toml");
        
        let mut app = Self {
            config,
            session: SessionState::new(),
            input: String::new(),
            messages: Vec::new(),
            scroll: 0,
            scroll_locked: false,
            pending_idxs: Vec::new(),
            input_history,
            history_idx: None,
            pending_workflow: None,
            last_applied_patch: None,
            assistant_tx,
            assistant_rx,
            embedding_cache,
            input_mode: InputMode::Chat,
            should_quit: false,
            viewing_history: false,
            completion_provider: CompletionProvider::new(),
            ghost_text: None,
            frecency: FrecencyTracker::load(&frecency_path),
            active_task: None,
            next_task_id: 1,
        };

        let status = if embeddings_ready {
            "embeddings: ready"
        } else {
            "embeddings: disabled"
        };
        let system_msg = format!(
            "Ready. Local model: {} ({}). Type naturally; `help` lists commands. Press Esc to exit.",
            app.config.model, status
        );
        app.push_recorded(Role::System, system_msg);
        let welcome = crate::handlers::getting_started_message(
            app.session.repo_info.as_ref(),
            app.session.repo_root.is_some(),
        );
        app.reply(welcome);
        app
    }

    fn create_view(&self) -> AppView<'_> {
        AppView {
            messages: if self.viewing_history {
                &self.session.history
            } else {
                &self.messages
            },
            scroll: self.scroll,
            input: &self.input,
            input_mode: self.input_mode,
            model: &self.config.model,
            _streaming: self.config.streaming,
            cwd: self.session.cwd.to_string_lossy().to_string(),
            _repo_root: self
                .session
                .repo_root
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            pending_count: self.pending_idxs.len(),
            has_workflow: self.pending_workflow.is_some(),
            ghost_text: self.ghost_text.as_deref(),
            task_status: self.active_task.as_ref().map(ActiveTask::status),
        }
    }

    fn start_command_task(&mut self, label: &str, cwd: PathBuf, program: &str, args: &[&str]) {
        if self.active_task.is_some() {
            self.reply("A test task is already running. Press Ctrl+C or type `cancel task` before starting another.");
            return;
        }
        if self.pending_workflow.is_some() {
            self.reply("Finish or cancel the pending workflow before starting tests.");
            return;
        }
        let id = TaskId(self.next_task_id);
        self.next_task_id += 1;
        let spec = TaskSpec {
            id,
            cwd: cwd.clone(),
            program: program.to_string(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            timeout: Duration::from_secs(self.config.cmd_timeout_secs),
        };
        let idx = self.pending_placeholder();
        let task = ActiveTask {
            id,
            label: label.to_string(),
            command: std::iter::once(program).chain(args.iter().copied()).collect::<Vec<_>>().join(" "),
            cwd,
            request_cwd: self.session.cwd.clone(),
            message_idx: idx,
            started: Instant::now(),
            cancelling: false,
            handle: task_runner::spawn(spec),
        };
        let content = task.render(&task.handle.updates.borrow());
        self.upsert_message(idx, Role::Assistant, content);
        self.scroll = 0;
        self.viewing_history = false;
        self.active_task = Some(task);
    }

    fn cancel_command_task(&mut self) -> bool {
        if let Some(task) = self.active_task.as_mut() {
            task.cancelling = true;
            task.handle.cancel();
            true
        } else {
            false
        }
    }

    fn poll_command_task(&mut self) {
        let Some(task) = self.active_task.take() else { return; };
        // Latest-only snapshots bound both memory and work per UI tick. Read
        // even after the sender closes so the final result cannot be lost.
        let mut snapshot = task.handle.updates.borrow().clone();
        if snapshot.outcome.is_none() && task.handle.updates.has_changed().is_err() {
            snapshot.outcome = Some(TaskOutcome::Error(
                "Task worker stopped unexpectedly; completion could not be confirmed.".to_string()
            ));
        }
        if snapshot.id != task.id {
            self.active_task = Some(task);
            return;
        }
        let content = task.render(&snapshot);
        self.upsert_message(task.message_idx, Role::Assistant, content.clone());
        if let Some(outcome) = &snapshot.outcome {
            let has_later_messages = task.message_idx + 1 < self.messages.len();
            self.pending_idxs.retain(|&idx| idx != task.message_idx);
            self.session.record(Message { role: Role::Assistant, content });
            if self.session.cwd == task.request_cwd {
                let summary = format!("{} #{}: {} in {}", task.label, task.id.0, task_outcome_label(outcome), task.cwd.display());
                self.session.record_output("tests", &summary, &format!("stderr:\n{}\nstdout:\n{}", snapshot.stderr, snapshot.stdout));
            }
            if has_later_messages {
                self.reply(format!("{} #{} finished — {} ({:.1}s).\nDirectory: {}\nFull output is in its task entry above.",
                    task.label, task.id.0, task_outcome_label(outcome),
                    task.started.elapsed().as_secs_f64(), task.cwd.display()));
            }
        } else {
            self.active_task = Some(task);
        }
    }

    async fn shutdown_command_task(&mut self) {
        if let Some(task) = self.active_task.take() {
            task.handle.shutdown().await;
            self.pending_idxs.retain(|&idx| idx != task.message_idx);
        }
    }

    fn poll_assistant(&mut self) {
        for _ in 0..64 {
            let Ok(event) = self.assistant_rx.try_recv() else { break; };
            match event {
                AssistantEvent::Token { idx, chunk } => self.append_assistant_chunk(idx, chunk),
                AssistantEvent::Completed { idx, content, original_input, cwd } => {
                    self.finish_assistant(idx, content, original_input, cwd);
                }
                AssistantEvent::Failed { idx, error} => self.fail_assistant(idx, error),
                AssistantEvent::PatchFailed { idx, error } => {
                    if matches!(self.pending_workflow.as_ref().map(|workflow| &workflow.kind),
                        Some(crate::workflow::WorkflowKind::EditPatchPending { .. })) {
                        self.pending_workflow = None;
                    }
                    self.fail_assistant(idx, error);
                }
                AssistantEvent::PatchReady { idx, review } => self.finish_patch_review(idx, review),
                AssistantEvent::IntentResolved { idx, intent, original_input, cwd } => {
                    self.finish_intent(idx, intent, original_input, cwd);
                }
                AssistantEvent::CommitPlanReady { idx, suggested, repo_root, save_work } => {
                    self.finish_commit_plan(idx, suggested, repo_root, save_work);
                }
                AssistantEvent::CommitMessageReady { idx, message } => {
                    self.session.record_output("commit_msg", "Generated commit message", &message);
                    let content = format!("Suggested commit message:\n{message}");
                    self.upsert_message(idx, Role::Assistant, content.clone());
                    self.session.record(Message { role: Role::Assistant, content });
                    self.pending_idxs.retain(|&i| i != idx);
                }
            }
            // Only reset scroll if not locked
            if !self.scroll_locked {
                self.scroll = 0;
                self.viewing_history = false;
            }
        }
    }

    fn push_recorded(&mut self, role: Role, content: impl Into<String>) -> usize {
        let content = content.into();
        let idx = self.messages.len();
        self.messages.push(Message {
            role: role.clone(),
            content: content.clone(),
        });
        self.session.record(Message { role, content });
        self.scroll = 0;
        self.viewing_history = false;
        idx
    }

    fn reply(&mut self, content: impl Into<String>) {
        let content = content.into();
        self.messages.push(Message {
            role: Role::Assistant,
            content: content.clone(),
        });
        self.session.record(Message {
            role: Role::Assistant,
            content,
        });
        self.scroll = 0;
        self.viewing_history = false;
    }

    fn reply_scroll_to_top(&mut self, content: impl Into<String>) {
        let content = content.into();
        
        // Count lines in the message to set appropriate scroll
        // We need to estimate rendered lines considering wrapping
        let line_count = content.lines().count();
        
        self.messages.push(Message {
            role: Role::Assistant,
            content: content.clone(),
        });
        self.session.record(Message {
            role: Role::Assistant,
            content,
        });
        
        // Set scroll to a high value to show the top of the message
        // This will be clamped by the UI rendering logic
        self.scroll = line_count.saturating_add(100);
        self.scroll_locked = true; // Lock scroll so it doesn't auto-reset
        self.viewing_history = false;
    }

    fn append_assistant_chunk(&mut self, idx: usize, chunk: String) {
        if let Some(msg) = self.messages.get_mut(idx) {
            msg.role = Role::Assistant;
            msg.content.push_str(&chunk);
        }
    }

    fn finish_assistant(&mut self, idx: usize, content: Option<String>, original_query: String, cwd: PathBuf) {
        let fallback = self
            .messages
            .get(idx)
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let final_content = content.unwrap_or(fallback);

        self.upsert_message(idx, Role::Assistant, final_content.clone());
        self.session.record(Message {
            role: Role::Assistant,
            content: final_content.clone(),
        });
        self.pending_idxs.retain(|&i| i != idx);
        
        // Check if the response contains shell commands
        let commands = crate::workflow::extract_commands_from_text(&final_content);
        if !commands.is_empty() && self.pending_workflow.is_none() && self.active_task.is_none() && self.session.cwd == cwd {
            let combined = commands.join(" && ");
            
            let msg = if commands.len() == 1 {
                format!(
                    "\n💡 Found command: `{}`\n\n\
                     Options:\n\
                     • [y]es - Execute it\n\
                     • [s]ave - Save as custom workflow for \"{}\" \n\
                     • [n]o - Skip",
                    combined,
                    original_query
                )
            } else {
                format!(
                    "\n💡 Found {} commands:\n{}\n\n\
                     Combined: `{}`\n\n\
                     Options:\n\
                     • [y]es - Execute all\n\
                     • [s]ave - Save as custom workflow for \"{}\"\n\
                     • [n]o - Skip",
                    commands.len(),
                    commands.iter().map(|c| format!("  • {}", c)).collect::<Vec<_>>().join("\n"),
                    combined,
                    original_query
                )
            };
            
            self.reply(msg);
            self.pending_workflow = Some(WorkflowState {
                kind: crate::workflow::WorkflowKind::ChatCommandsConfirm {
                    original_query,
                    commands,
                    combined_command: combined,
                },
                repo_root: self.session.repo_root.clone().unwrap_or_else(|| self.session.cwd.clone()),
            });
        }
    }

    fn fail_assistant(&mut self, idx: usize, error: String) {
        let content = format!("Request failed: {error}");
        self.upsert_message(idx, Role::System, content.clone());
        self.session.record(Message {
            role: Role::System,
            content,
        });
        self.pending_idxs.retain(|&i| i != idx);
    }

    fn finish_intent(&mut self, idx: usize, intent: ParsedIntent, original_input: String, cwd: PathBuf) {
        if self.session.cwd != cwd {
            self.fail_assistant(idx, "the working directory changed while resolving this request; submit it again in the intended directory".to_string());
            return;
        }
        if self.pending_workflow.is_some() {
            self.fail_assistant(idx, "another workflow is awaiting your response; finish it before retrying this request".to_string());
            return;
        }
        // Outstanding events retain their message indices. Removing this entry
        // would redirect chunks for later requests to the wrong message.
        self.upsert_message(idx, Role::System, format!("Running workflow: {}", intent.tool));
        self.pending_idxs.retain(|&i| i != idx);
        dispatch_intent(self, &intent, &original_input);
    }

    fn finish_commit_plan(&mut self, idx: usize, suggested: String, repo_root: PathBuf, save_work: bool) {
        use crate::workflow::WorkflowKind;
        let expected = self.pending_workflow.as_ref().is_some_and(|workflow| {
            workflow.repo_root == repo_root && matches!(
                (&workflow.kind, save_work),
                (WorkflowKind::SaveWorkMessagePending, true) | (WorkflowKind::CommitMessagePending, false)
            )
        });
        if !expected {
            self.fail_assistant(idx, "this commit request was superseded before its suggestion arrived".to_string());
            return;
        }
        let content = format!(
            "Suggested commit message:\n{suggested}\nPress Enter (or type 'yes') to accept, or type a custom message. Type 'cancel' to abort."
        );
        self.upsert_message(idx, Role::Assistant, content.clone());
        self.session.record(Message { role: Role::Assistant, content });
        self.pending_idxs.retain(|&i| i != idx);
        self.pending_workflow = Some(WorkflowState {
            kind: if save_work {
                WorkflowKind::SaveWorkCommit { suggested }
            } else {
                WorkflowKind::CommitOnlyConfirm { suggested }
            },
            repo_root,
        });
    }

    fn finish_patch_review(&mut self, idx: usize, review: PatchReview) {
        let repo_root = match self.pending_workflow.take() {
            Some(WorkflowState {
                kind: crate::workflow::WorkflowKind::EditPatchPending { file },
                repo_root,
            }) if file == review.file => repo_root,
            workflow => {
                self.pending_workflow = workflow;
                self.fail_assistant(idx, "edit request was superseded before its patch arrived".to_string());
                return;
            }
        };

        if let Err(error) = patch::check_patch(
            &repo_root,
            &review.patch,
            Duration::from_secs(self.config.cmd_timeout_secs),
        ) {
            let content = format!("Patch review unavailable: {error}");
            self.upsert_message(idx, Role::System, content.clone());
            self.session.record(Message {
                role: Role::System,
                content,
            });
            self.pending_idxs.retain(|&i| i != idx);
            return;
        }

        let display_content = format!(
            "Edit plan for {}\nRequested change: {}\n\nChecked unified diff:\n{}\n\nPress Enter (or type 'yes') to apply this patch. Type 'no' to cancel.",
            review.file,
            review.description,
            patch::preview_patch(&review.patch, 12_000),
        );
        self.upsert_message(idx, Role::Assistant, display_content.clone());
        self.session.record(Message {
            role: Role::Assistant,
            content: display_content,
        });
        self.pending_idxs.retain(|&i| i != idx);
        self.pending_workflow = Some(WorkflowState {
            kind: crate::workflow::WorkflowKind::ApplyDiff { review },
            repo_root,
        });
    }

    fn upsert_message(&mut self, idx: usize, role: Role, content: String) {
        if let Some(msg) = self.messages.get_mut(idx) {
            msg.role = role;
            msg.content = content;
        } else {
            self.messages.push(Message { role, content });
        }
    }
    
    /// Update ghost text based on current input.
    fn update_ghost_text(&mut self) {
        if self.input.is_empty() {
            self.ghost_text = None;
            return;
        }
        
        // Load learned aliases
        let learned_global = self.config.learned_path.clone();
        let learned_project = self.session.repo_root.as_ref().map(|r| r.join(".llm-cli/learned.toml"));
        let learned = LearnedAliases::load(&learned_global, learned_project.as_deref())
            .unwrap_or_default();
        
        self.ghost_text = self.completion_provider.get_ghost_completion(
            &self.input,
            &self.session.cwd,
            &self.input_history,
            &learned,
            &self.frecency,
        );
    }
    
    /// Accept the ghost text completion.
    fn accept_ghost_text(&mut self) {
        if let Some(ghost) = self.ghost_text.take() {
            let completed = format!("{}{}", self.input, ghost);
            
            // Record command usage for frecency
            self.frecency.record_command(&completed);
            
            // Check if completed text contains a file path and record it
            self.record_files_in_text(&completed);
            
            self.input = completed;
            // Update ghost text again in case there's more to complete
            self.update_ghost_text();
        }
    }
    
    /// Extract and record any file paths mentioned in text.
    fn record_files_in_text(&mut self, text: &str) {
        // Extract potential file paths from the text
        for word in text.split_whitespace() {
            // Check if word looks like a file path and exists
            if self.is_valid_file_path(word) {
                self.frecency.record_file(word);
            }
        }
    }
    
    /// Check if a word is a valid file path that exists.
    fn is_valid_file_path(&self, word: &str) -> bool {
        // Must contain path separator or have file extension
        if !word.contains('/') && !word.contains('.') {
            return false;
        }
        
        // Skip URLs
        if word.starts_with("http://") || word.starts_with("https://") {
            return false;
        }
        
        // Check if file exists relative to cwd or repo root
        let path = std::path::Path::new(word);
        if path.is_absolute() {
            return path.exists();
        }
        
        // Try relative to cwd
        if self.session.cwd.join(path).exists() {
            return true;
        }
        
        // Try relative to repo root
        if let Some(repo) = &self.session.repo_root {
            if repo.join(path).exists() {
                return true;
            }
        }
        
        false
    }
    
    /// Record file access for frecency tracking.
    fn record_file_access(&mut self, file_path: &str) {
        self.frecency.record_file(file_path);
    }
    
    /// Record command usage for frecency tracking.
    fn record_command_usage(&mut self, command: &str) {
        self.frecency.record_command(command);
    }

    fn change_session_directory(&mut self, path_text: &str) {
        let path_text = path_text.trim();
        if path_text.is_empty() {
            self.reply("Usage: cd <directory>");
            return;
        }

        let requested = PathBuf::from(path_text);
        let candidate = if requested.is_absolute() {
            requested
        } else {
            self.session.cwd.join(requested)
        };
        let canonical = match candidate.canonicalize() {
            Ok(path) if path.is_dir() => path,
            Ok(_) => {
                self.reply(format!("Not a directory: {}", candidate.display()));
                return;
            }
            Err(error) => {
                self.reply(format!("Could not change directory to {}: {error}", candidate.display()));
                return;
            }
        };

        self.session.set_cwd(canonical.clone());
        self.update_ghost_text();
        let guide = crate::handlers::getting_started_message(
            self.session.repo_info.as_ref(),
            self.session.repo_root.is_some(),
        );
        self.reply(format!(
            "Working directory changed to {}.\n\n{}",
            canonical.display(), guide
        ));
    }

    fn record_input_history(&mut self, entry: String) {
        if self.input_history.last().is_some_and(|last| last == &entry) {
            return;
        }

        self.input_history.push(entry.clone());
        if self.input_history.len() > MAX_INPUT_HISTORY {
            let remove_count = self.input_history.len() - MAX_INPUT_HISTORY;
            self.input_history.drain(..remove_count);
        }

        let record = PersistedInput { input: entry };
        if let Err(error) = append_input_history(&self.config.history_path, &record) {
            tracing::warn!("Failed to persist input history: {error}");
        }
    }

    fn save_input_history(&self) {
        if let Err(error) = rewrite_input_history(&self.config.history_path, &self.input_history) {
            tracing::warn!("Failed to compact input history: {error}");
        }
    }
}

fn load_input_history(path: &std::path::Path) -> Vec<String> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            tracing::warn!("Failed to load input history from {}: {error}", path.display());
            return Vec::new();
        }
    };

    let mut entries: Vec<String> = contents
        .lines()
        .filter_map(|line| match serde_json::from_str::<PersistedInput>(line) {
            Ok(record) if !record.input.trim().is_empty() => Some(record.input),
            Ok(_) => None,
            Err(error) => {
                tracing::warn!("Skipping malformed input-history entry: {error}");
                None
            }
        })
        .collect();

    if entries.len() > MAX_INPUT_HISTORY {
        entries.drain(..entries.len() - MAX_INPUT_HISTORY);
    }
    entries
}

fn append_input_history(path: &std::path::Path, record: &PersistedInput) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating history directory at {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("opening history file at {}", path.display()))?;
    serde_json::to_writer(&mut file, record).context("serializing input-history entry")?;
    file.write_all(b"\n").context("terminating input-history entry")?;
    Ok(())
}

fn rewrite_input_history(path: &std::path::Path, history: &[String]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating history directory at {}", parent.display()))?;
    }
    let mut content = String::new();
    for input in history.iter().rev().take(MAX_INPUT_HISTORY).rev() {
        let line = serde_json::to_string(&PersistedInput {
            input: input.clone(),
        })?;
        content.push_str(&line);
        content.push('\n');
    }
    fs::write(path, content).with_context(|| format!("writing history file at {}", path.display()))
}

// Implement IntentDispatcher for App
impl IntentDispatcher for App {
    fn reply(&mut self, content: impl Into<String>) {
        self.reply(content);
    }

    fn reply_scroll_to_top(&mut self, content: impl Into<String>) {
        self.reply_scroll_to_top(content);
    }

    fn push_recorded(&mut self, role: Role, content: impl Into<String>) -> usize {
        self.push_recorded(role, content)
    }

    fn set_pending_workflow(&mut self, workflow: WorkflowState) {
        self.pending_workflow = Some(workflow);
    }

    fn get_session_cwd(&self) -> PathBuf {
        self.session.cwd.clone()
    }

    fn get_session_repo_root(&self) -> Option<PathBuf> {
        self.session.repo_root.clone()
    }

    fn get_session_repo_info(&self) -> Option<crate::repo::RepoInfo> {
        self.session.repo_info.clone()
    }

    fn get_input_history(&self) -> &[String] {
        &self.input_history
    }

    fn get_config(&self) -> &Config {
        &self.config
    }

    fn get_assistant_tx(&self) -> mpsc::UnboundedSender<AssistantEvent> {
        self.assistant_tx.clone()
    }

    fn pending_placeholder(&mut self) -> usize {
        let idx = self.messages.len();
        self.messages.push(Message {
            role: Role::Assistant,
            content: "…".to_string(),
        });
        self.pending_idxs.push(idx);
        idx
    }

    fn record_output(&mut self, kind: &'static str, summary: &str, content: &str) {
        self.session.record_output(kind, summary, content);
    }

    fn record_file_access(&mut self, file_path: &str) {
        self.record_file_access(file_path);
    }
    
    fn record_command_usage(&mut self, command: &str) {
        self.record_command_usage(command);
    }

    fn get_last_applied_patch(&self) -> Option<AppliedPatch> {
        self.last_applied_patch.clone()
    }

    fn clear_last_applied_patch(&mut self) {
        self.last_applied_patch = None;
    }

    fn has_active_task(&self) -> bool {
        self.active_task.is_some()
    }

    fn start_command_task(&mut self, label: &str, cwd: PathBuf, program: &str, args: &[&str]) {
        self.start_command_task(label, cwd, program, args);
    }
}

// Implement WorkflowResponder for App
impl WorkflowResponder for App {
    fn reply(&mut self, content: impl Into<String>) {
        self.reply(content);
    }
    
    fn execute_shell_command(&mut self, cmd: &str) {
        crate::handlers::handle_shell_dispatch(self, cmd);
    }

    fn execute_approved_shell_command(&mut self, cmd: &str) {
        crate::handlers::handle_approved_shell_dispatch(self, cmd);
    }

    fn command_timeout_secs(&self) -> u64 {
        self.config.cmd_timeout_secs
    }

    fn set_last_applied_patch(&mut self, patch: AppliedPatch) {
        self.last_applied_patch = Some(patch);
    }
}

fn handle_key_event(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if !app.cancel_command_task() {
                app.should_quit = true;
            }
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Toggle input mode
            app.input_mode = match app.input_mode {
                InputMode::Chat => InputMode::Shell,
                InputMode::Shell => InputMode::Chat,
            };
        }
        KeyCode::Esc => app.should_quit = true,
        KeyCode::Enter => submit_input(app),
        KeyCode::Tab => {
            // Accept ghost text completion
            if app.ghost_text.is_some() {
                app.accept_ghost_text();
            }
        }
        KeyCode::Up => {
            recall_history_prev(app);
        }
        KeyCode::Down => {
            recall_history_next(app);
        }
        KeyCode::PageUp => {
            scroll_session_history_up(app);
        }
        KeyCode::PageDown => {
            scroll_session_history_down(app);
        }
        KeyCode::Backspace => {
            app.input.pop();
            app.update_ghost_text();
        }
        KeyCode::Char(ch) => {
            app.input.push(ch);
            app.update_ghost_text();
        }
        _ => {}
    }
}

fn handle_mouse_event(app: &mut App, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::ScrollUp => {
            scroll_session_history_up(app);
        }
        MouseEventKind::ScrollDown => {
            scroll_session_history_down(app);
        }
        _ => {}
    }
}

fn submit_input(app: &mut App) {
    let raw_input = app.input.trim().to_string();
    
    // Allow empty input only if we have a pending workflow
    if raw_input.is_empty() && app.pending_workflow.is_none() {
        return;
    }
    
    // Track any file paths mentioned in the input
    app.record_files_in_text(&raw_input);

    app.history_idx = None;
    app.input.clear();
    
    // User is submitting new input, unlock scroll so new responses appear at bottom
    app.scroll_locked = false;

    // Task control works in either input mode and never enters a shell or LLM.
    if raw_input.eq_ignore_ascii_case("cancel task") {
        app.push_recorded(Role::User, raw_input);
        if !app.cancel_command_task() {
            app.reply("There is no active test task to cancel.");
        }
        return;
    }
    if app.active_task.is_some() && matches!(raw_input.to_ascii_lowercase().as_str(), "help" | "?") {
        app.push_recorded(Role::User, raw_input.clone());
        dispatch_intent(app, &ParsedIntent::new("help", 1.0), &raw_input);
        return;
    }

    // Handle pending workflow confirmations first (before recording to history)
    if app.pending_workflow.is_some() {
        // Record the confirmation response (or empty for Enter)
        let display_input = if raw_input.is_empty() { 
            "[Enter]".to_string() 
        } else { 
            raw_input.clone() 
        };
        app.push_recorded(Role::User, display_input);
        handle_pending_workflow(app, &raw_input);
        return;
    }

    // Handle bang shortcuts (!! and !prefix)
    if let Some(expanded) = expand_bang_shortcut(&app.input_history, &raw_input) {
        // Show what we're expanding to
        app.push_recorded(Role::User, format!("{} → {}", raw_input, &expanded));
        // Execute the expanded command (it's a shell command)
        let cmd = expanded
            .trim()
            .strip_prefix('$')
            .or_else(|| expanded.trim().strip_prefix('!'))
            .map(|s| s.trim())
            .unwrap_or(&expanded);
        if let Some(path) = directory_change_target(cmd) {
            app.change_session_directory(path);
        } else {
            crate::handlers::handle_shell_dispatch(app, cmd);
        }
        return;
    }

    let prompt = raw_input;
    app.push_recorded(Role::User, prompt.clone());

    // Store in history with mode marker for filtering
    let history_entry = if app.input_mode == InputMode::Shell {
        format!("$ {}", prompt) // Mark as shell command internally
    } else {
        prompt.clone()
    };
    app.record_input_history(history_entry);

    if let Some(path) = directory_change_target(&prompt) {
        app.change_session_directory(path);
        return;
    }

    // If in Shell mode, execute as shell command directly
    if app.input_mode == InputMode::Shell {
        crate::handlers::handle_shell_dispatch(app, &prompt);
        return;
    }

    // In Chat mode: try quick match first (for $ prefix and !! shortcuts)
    if let Some(intent) = intent::quick_match(&prompt) {
        if dispatch_intent(app, &intent, &prompt) {
            return;
        }
    }

    // Use tiered intent resolution in background
    let tx = app.assistant_tx.clone();
    let model = app.config.model.clone();
    let classifier_model = app.config.classifier_model.clone();
    let ollama_host = app.config.ollama_host.clone();
    let system_prompt = app.config.system_prompt.clone();
    let request_timeout_secs = app.config.request_timeout_secs;
    let llm_timeout_secs = app.config.llm_timeout_secs;
    let streaming = app.config.streaming;
    let max_context_tokens = app.config.max_context_tokens;
    let prompt_for_task = prompt.clone();
    let embedding_cache = Arc::clone(&app.embedding_cache);
    
    // Load learned aliases
    let learned_global = app.config.learned_path.clone();
    let learned_project = app.session.repo_root.as_ref().map(|r| r.join(".llm-cli/learned.toml"));
    
    // Capture lightweight repository context before spawning.
    let repo_context = crate::chat::project_context(app.session.repo_info.as_ref());
    let request_cwd = app.session.cwd.clone();
    
    // Inject recent context if user input contains references
    let context_injection = if context::contains_reference(&prompt_for_task) {
        context::format_context_for_prompt(&app.session.recent_outputs)
    } else {
        String::new()
    };

    // Insert placeholder for response
    let placeholder_idx = app.messages.len();
    app.messages.push(Message {
        role: Role::Assistant,
        content: String::new(),
    });
    app.pending_idxs.push(placeholder_idx);

    tokio::spawn(async move {
        // Try tiered intent resolution
        let learned = LearnedAliases::load(&learned_global, learned_project.as_deref()).unwrap_or_default();
        
        if let Ok(parsed) = intent::resolve_intent(
            &prompt_for_task,
            &embedding_cache,
            &learned,
            &classifier_model,
            request_timeout_secs,
            &ollama_host,
        )
        .await
        {
            // Non-chat intents get dispatched via signal to main thread
            if parsed.tool != "chat" && parsed.confidence >= 0.3 {
                let _ = tx.send(AssistantEvent::IntentResolved {
                    idx: placeholder_idx,
                    intent: parsed,
                    original_input: prompt_for_task,
                    cwd: request_cwd,
                });
                return;
            }
        }

        // Fall through to regular LLM chat
        let composed_prompt = crate::chat::compose_prompt(
            &system_prompt,
            &repo_context,
            &context_injection,
            &prompt_for_task,
            max_context_tokens,
        );

        let mut command = TokioCommand::new("ollama");
        command.arg("run")
            .arg(&model)
            .arg(&composed_prompt)
            .env("OLLAMA_HOST", &ollama_host);
        let response = crate::model_stream::run(
            command,
            Duration::from_secs(llm_timeout_secs),
            |chunk| {
                if streaming {
                    tx.send(AssistantEvent::Token {
                        idx: placeholder_idx,
                        chunk: chunk.to_string(),
                    }).map_err(|_| anyhow::anyhow!("the session was closed"))?;
                }
                Ok(())
            },
        ).await;
        match response {
            Ok(content) => {
                let _ = tx.send(AssistantEvent::Completed {
                    idx: placeholder_idx,
                    content: Some(content),
                    original_input: prompt_for_task,
                    cwd: request_cwd,
                });
            }
            Err(err) => {
                let _ = tx.send(AssistantEvent::Failed {
                    idx: placeholder_idx,
                    error: format!("{err}"),
                });
            }
        }
    });
}

/// Recognize `cd path` in either input mode, including the explicit `$ cd`
/// and `! cd` shell syntaxes. Unlike an ordinary shell child, this updates the
/// application's session directory for later commands.
fn directory_change_target(input: &str) -> Option<&str> {
    let input = input.trim();
    let command = input
        .strip_prefix('$')
        .or_else(|| input.strip_prefix('!'))
        .map(str::trim)
        .unwrap_or(input);

    if command == "cd" {
        return Some("");
    }
    command.strip_prefix("cd ").map(str::trim)
}

fn handle_pending_workflow(app: &mut App, prompt: &str) {
    if app.active_task.is_some() {
        app.reply("A test task is active. Cancel it before continuing this workflow.");
        return;
    }
    if let Some(workflow) = app.pending_workflow.take() {
        // Need to handle special case for SaveWorkPlan
        if matches!(
            &workflow.kind,
            crate::workflow::WorkflowKind::SaveWorkPlan
        ) {
            let confirmed = matches!(prompt.trim().to_lowercase().as_str(), "" | "y" | "yes");
            if confirmed {
                // Run git add -A first
                match crate::commands::run_command_with_timeout(
                    &workflow.repo_root,
                    "git",
                    &["add", "-A"],
                    Duration::from_secs(app.config.cmd_timeout_secs),
                ) {
                    Ok(out) => {
                        if !out.trim().is_empty() {
                            app.reply(format!("git add -A output:\n{out}"));
                        }
                    }
                    Err(err) => {
                        app.reply(format!("git add -A failed: {}", crate::commands::format_error(&err)));
                        return;
                    }
                }

                // Generate the commit message without freezing the event loop.
                let idx = app.pending_placeholder();
                let tx = app.assistant_tx.clone();
                let config = app.config.clone();
                let repo_root = workflow.repo_root.clone();
                app.pending_workflow = Some(WorkflowState {
                    kind: crate::workflow::WorkflowKind::SaveWorkMessagePending,
                    repo_root: repo_root.clone(),
                });
                tokio::spawn(async move {
                    let suggested = generate_commit_message_async(&config, &repo_root)
                        .await
                        .unwrap_or_else(|| "chore: save work".to_string());
                    let _ = tx.send(AssistantEvent::CommitPlanReady {
                        idx,
                        suggested,
                        repo_root,
                        save_work: true,
                    });
                });
            } else {
                app.reply("Workflow cancelled.");
            }
        } else if matches!(
            workflow.kind,
            crate::workflow::WorkflowKind::SaveWorkMessagePending
                | crate::workflow::WorkflowKind::CommitMessagePending
                | crate::workflow::WorkflowKind::EditPatchPending { .. }
        ) {
            app.pending_workflow = Some(workflow);
            app.reply("Generation is still in progress. Please wait.");
        } else if matches!(workflow.kind, crate::workflow::WorkflowKind::ApplyDiff { .. })
            && !matches!(prompt.trim().to_lowercase().as_str(), "" | "yes" | "y" | "no" | "n" | "cancel") {
            app.pending_workflow = Some(workflow);
            app.reply("Patch review is still open. Press Enter (or type 'yes') to apply, or 'no' to cancel.");
        } else {
            handle_workflow_response(app, workflow, prompt);
        }
    }
}

fn recall_history_prev(app: &mut App) {
    let nav = HistoryNavigation::new(&app.input_history, app.input_mode);
    if let Some((idx, display)) = nav.get_prev(app.history_idx) {
        app.history_idx = Some(idx);
        app.input = display;
    }
}

fn recall_history_next(app: &mut App) {
    let nav = HistoryNavigation::new(&app.input_history, app.input_mode);
    match nav.get_next(app.history_idx) {
        Some((idx, display)) => {
            app.history_idx = Some(idx);
            app.input = display;
        }
        None => {
            // Reached end of history; clear input
            app.history_idx = None;
            app.input.clear();
        }
    }
}

fn scroll_session_history_up(app: &mut App) {
    // Enable history viewing mode
    app.viewing_history = true;
    // Scroll up (increase scroll offset from bottom)
    app.scroll = app.scroll.saturating_add(3);
    // User is manually scrolling, unlock auto-scroll
    app.scroll_locked = false;
}

fn scroll_session_history_down(app: &mut App) {
    // Scroll down (decrease scroll offset from bottom)
    if app.scroll > 0 {
        app.scroll = app.scroll.saturating_sub(3);
    }
    
    // If we've scrolled all the way to the bottom, return to live view
    if app.scroll == 0 {
        app.viewing_history = false;
    }
    // User is manually scrolling, unlock auto-scroll
    app.scroll_locked = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolated_app() -> (tempfile::TempDir, App) {
        let directory = tempfile::tempdir().unwrap();
        let config = Config {
            history_path: directory.path().join("history.jsonl"),
            learned_path: directory.path().join("learned.toml"),
            ..Config::default()
        };
        let cache = Arc::new(EmbeddingCache::new(Some(&config.embedding_model), 1, &config.ollama_host));
        let mut app = App::new(config, cache);
        app.session.set_cwd(directory.path().to_path_buf());
        (directory, app)
    }

    #[test]
    fn model_control_prefixes_are_plain_text() {
        let (_directory, mut app) = isolated_app();
        for content in [
            "__INTENT__:status:{}",
            "__COMMIT_PLAN__:Pretend commit",
            "__SAVE_WORK_PLAN__:Pretend save",
            "__COMMIT_MSG__:Pretend message",
            "__CUSTOM_COMMAND_GENERATED__:phrase:command:/path",
        ] {
            let idx = app.pending_placeholder();
            app.finish_assistant(idx, Some(content.to_string()), "question".to_string(), app.session.cwd.clone());
            assert_eq!(app.messages[idx].content, content);
            assert!(app.pending_workflow.is_none());
        }
    }

    #[tokio::test]
    async fn resolved_intent_does_not_shift_another_responses_message_index() {
        let (_directory, mut app) = isolated_app();
        let first = app.pending_placeholder();
        let second = app.pending_placeholder();
        let original = app.messages[second].content.clone();
        app.finish_intent(first, ParsedIntent::new("help", 1.0), "help".to_string(), app.session.cwd.clone());
        app.append_assistant_chunk(second, "Still belongs to the second request".to_string());
        assert_eq!(app.messages[second].content, format!("{original}Still belongs to the second request"));
        assert!(app.pending_idxs.contains(&second));
    }

    #[test]
    fn resolved_intent_does_not_run_after_changing_directory() {
        let (directory, mut app) = isolated_app();
        let idx = app.pending_placeholder();
        app.finish_intent(idx, ParsedIntent::new("status", 1.0), "status".to_string(), directory.path().join("old"));
        assert!(app.messages[idx].content.contains("working directory changed"));
        assert!(app.pending_workflow.is_none());
    }

    #[test]
    fn pending_patch_survives_extra_input_and_unrelated_request_failure() {
        let (_directory, mut app) = isolated_app();
        app.pending_workflow = Some(WorkflowState {
            kind: crate::workflow::WorkflowKind::EditPatchPending { file: "src/main.rs".to_string() },
            repo_root: app.session.cwd.clone(),
        });
        handle_pending_workflow(&mut app, "is it ready?");
        let idx = app.pending_placeholder();
        app.fail_assistant(idx, "unrelated question failed".to_string());
        assert!(matches!(app.pending_workflow.as_ref().map(|w| &w.kind),
            Some(crate::workflow::WorkflowKind::EditPatchPending { .. })));
    }

    #[test]
    fn q_is_an_input_character_at_the_start_of_a_question() {
        let (_directory, mut app) = isolated_app();
        handle_key_event(&mut app, crossterm::event::KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert_eq!(app.input, "q");
        assert!(!app.should_quit);
    }

    #[test]
    fn unrecognized_patch_confirmation_keeps_the_review_available() {
        let (_directory, mut app) = isolated_app();
        app.pending_workflow = Some(WorkflowState {
            kind: crate::workflow::WorkflowKind::ApplyDiff { review: PatchReview {
                file: "src/main.rs".to_string(),
                patch: "reviewed patch".to_string(),
                description: "requested change".to_string(),
            } },
            repo_root: app.session.cwd.clone(),
        });
        handle_pending_workflow(&mut app, "what does this change?");
        assert!(matches!(app.pending_workflow.as_ref().map(|w| &w.kind),
            Some(crate::workflow::WorkflowKind::ApplyDiff { .. })));
        handle_pending_workflow(&mut app, "no");
        assert!(app.pending_workflow.is_none());
    }

    #[test]
    fn input_history_round_trips_as_json_lines() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state/history.jsonl");
        append_input_history(
            &path,
            &PersistedInput {
                input: "status".to_string(),
            },
        )
        .unwrap();
        append_input_history(
            &path,
            &PersistedInput {
                input: "$ cargo test".to_string(),
            },
        )
        .unwrap();

        assert_eq!(load_input_history(&path), vec!["status", "$ cargo test"]);
        rewrite_input_history(&path, &["help".to_string()]).unwrap();
        assert_eq!(load_input_history(&path), vec!["help"]);
    }

    #[test]
    fn directory_change_syntax_supports_both_modes() {
        assert_eq!(directory_change_target("cd src"), Some("src"));
        assert_eq!(directory_change_target("$ cd ../other"), Some("../other"));
        assert_eq!(directory_change_target("! cd /tmp"), Some("/tmp"));
        assert_eq!(directory_change_target("cargo test"), None);
    }

    #[cfg(unix)]
    fn start_fake_test(app: &mut App, script: &str) -> usize {
        app.start_command_task("Tests", app.session.cwd.clone(), "sh", &["-c", script]);
        app.active_task.as_ref().unwrap().message_idx
    }

    #[cfg(unix)]
    async fn wait_for_task_state(app: &mut App, condition: impl Fn(&App) -> bool) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                app.poll_command_task();
                if condition(app) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("background task did not reach the expected state");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn live_test_output_keeps_typing_and_message_identity_responsive() {
        let (directory, mut app) = isolated_app();
        let idx = start_fake_test(&mut app,
            "printf 'live-marker\\n'; while [ ! -e release-test ]; do sleep 0.05; done; printf 'done-marker\\n'");
        wait_for_task_state(&mut app, |app| app.messages[idx].content.contains("stdout:\nlive-marker")).await;
        assert!(app.active_task.is_some(), "output must arrive before the process exits");
        assert!(app.pending_idxs.contains(&idx));
        assert!(app.messages[idx].content.contains("RUNNING"));

        for ch in "question".chars() {
            handle_key_event(&mut app, crossterm::event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(app.input, "question");
        assert!(!app.should_quit);

        let newer_idx = app.pending_placeholder();
        app.finish_assistant(newer_idx, Some("Independent chat reply".to_string()),
            "question".to_string(), app.session.cwd.clone());
        fs::write(directory.path().join("release-test"), "release").unwrap();
        wait_for_task_state(&mut app, |app| app.active_task.is_none()).await;

        assert!(app.messages[idx].content.contains("PASSED (exit 0)"));
        assert!(app.messages[idx].content.contains("done-marker"));
        assert_eq!(app.messages[newer_idx].content, "Independent chat reply");
        assert!(!app.pending_idxs.contains(&idx));
        assert_eq!(app.session.recent_outputs.front().unwrap().kind, "tests");
        assert!(app.session.recent_outputs.front().unwrap().content.contains("done-marker"));
        assert!(app.create_view().task_status.is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn repeated_ctrl_c_cancels_without_quitting_or_releasing_task_early() {
        let (_directory, mut app) = isolated_app();
        let idx = start_fake_test(&mut app, "printf 'started-marker\\n'; sleep 30");
        wait_for_task_state(&mut app, |app| app.messages[idx].content.contains("stdout:\nstarted-marker")).await;
        let id = app.active_task.as_ref().unwrap().id;
        let ctrl_c = crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        handle_key_event(&mut app, ctrl_c);
        handle_key_event(&mut app, ctrl_c);

        assert!(!app.should_quit);
        let task = app.active_task.as_ref().expect("cancellation must keep the task slot until final delivery");
        assert_eq!(task.id, id);
        assert!(task.cancelling);
        assert!(app.pending_idxs.contains(&idx));
        assert!(app.create_view().task_status.unwrap().contains("cancelling"));
        wait_for_task_state(&mut app, |app| app.active_task.is_none()).await;
        assert!(app.messages[idx].content.contains("CANCELLED"));
        assert!(!app.should_quit);
        assert!(!app.pending_idxs.contains(&idx));

        let next_idx = start_fake_test(&mut app, "printf 'second-test\\n'");
        assert_ne!(app.active_task.as_ref().unwrap().id, id);
        wait_for_task_state(&mut app, |app| app.active_task.is_none()).await;
        assert!(app.messages[next_idx].content.contains("PASSED (exit 0)"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn escape_shutdown_cleans_up_the_active_test() {
        let (_directory, mut app) = isolated_app();
        let idx = start_fake_test(&mut app, "printf 'started-marker\\n'; sleep 30");
        wait_for_task_state(&mut app, |app| app.messages[idx].content.contains("stdout:\nstarted-marker")).await;
        handle_key_event(&mut app, crossterm::event::KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.should_quit);
        tokio::time::timeout(Duration::from_secs(3), app.shutdown_command_task())
            .await.expect("exit should stop its child task promptly");
        assert!(app.active_task.is_none());
        assert!(!app.pending_idxs.contains(&idx));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn help_and_cd_work_while_tests_run_without_leaking_old_project_context() {
        let (directory, mut app) = isolated_app();
        let original_cwd = app.session.cwd.clone();
        let next_directory = directory.path().join("next-project");
        fs::create_dir(&next_directory).unwrap();
        let idx = start_fake_test(&mut app,
            "printf 'old-project-marker\\n'; while [ ! -e release-test ]; do sleep 0.05; done");
        wait_for_task_state(&mut app, |app| app.messages[idx].content.contains("stdout:\nold-project-marker")).await;

        for mode in [InputMode::Chat, InputMode::Shell] {
            app.input_mode = mode;
            app.input = "help".to_string();
            submit_input(&mut app);
            assert!(app.messages.last().unwrap().content.contains("cancel task"));
            assert!(app.active_task.is_some());
            assert!(app.pending_workflow.is_none());
            let help_scroll = app.scroll;
            app.poll_command_task();
            assert_eq!(app.scroll, help_scroll, "task output must not steal the help scroll position");
        }

        app.session.record_output("tests", "Old project output", "old context");
        app.input = "cd next-project".to_string();
        submit_input(&mut app);
        assert_eq!(app.session.cwd, next_directory.canonicalize().unwrap());
        assert!(app.session.recent_outputs.is_empty());
        assert!(app.active_task.is_some());
        fs::write(original_cwd.join("release-test"), "release").unwrap();
        wait_for_task_state(&mut app, |app| app.active_task.is_none()).await;

        assert!(app.session.recent_outputs.is_empty(), "old project results must not become the new project's recent output");
        assert!(app.messages[idx].content.contains(&format!("Directory: {}", original_cwd.display())));
        assert!(app.messages[idx].content.contains("PASSED (exit 0)"));
        assert!(app.messages[idx].content.contains("old-project-marker"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_tests_display_both_output_streams_and_exit_code() {
        let (_directory, mut app) = isolated_app();
        let idx = start_fake_test(&mut app, "printf 'assertion context\\n'; printf 'failure details\\n' >&2; exit 7");
        wait_for_task_state(&mut app, |app| app.active_task.is_none()).await;
        let result = &app.messages[idx].content;
        assert!(result.contains("FAILED (exit 7)"), "{result}");
        assert!(result.contains("stdout:\nassertion context"), "{result}");
        assert!(result.contains("stderr:\nfailure details"), "{result}");
        assert!(app.session.recent_outputs.front().unwrap().content.contains("failure details"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn chat_command_suggestions_do_not_open_workflows_during_tests() {
        let (_directory, mut app) = isolated_app();
        start_fake_test(&mut app, "sleep 30");
        let idx = app.pending_placeholder();
        let content = "Try this later:\n```sh\ncargo build\n```".to_string();
        app.finish_assistant(idx, Some(content.clone()), "How do I build?".to_string(), app.session.cwd.clone());
        assert_eq!(app.messages[idx].content, content);
        assert!(app.pending_workflow.is_none());
        assert!(app.active_task.is_some());
        app.shutdown_command_task().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_second_test_task_is_rejected_until_the_first_finishes() {
        let (directory, mut app) = isolated_app();
        let first_idx = start_fake_test(&mut app, "sleep 30");
        let first_id = app.active_task.as_ref().unwrap().id;
        let next_id = app.next_task_id;
        app.start_command_task("Tests", app.session.cwd.clone(), "sh", &["-c", "touch forbidden-second-task"]);
        assert_eq!(app.active_task.as_ref().unwrap().id, first_id);
        assert_eq!(app.active_task.as_ref().unwrap().message_idx, first_idx);
        assert_eq!(app.next_task_id, next_id);
        assert!(app.messages.last().unwrap().content.contains("already running"));
        app.shutdown_command_task().await;
        assert!(!directory.path().join("forbidden-second-task").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shell_bang_and_late_intents_cannot_bypass_the_active_test_guard() {
        let (directory, mut app) = isolated_app();
        start_fake_test(&mut app, "sleep 30");
        let task_id = app.active_task.as_ref().unwrap().id;
        for (mode, input) in [
            (InputMode::Shell, "touch forbidden-shell"),
            (InputMode::Chat, "$ touch forbidden-explicit"),
            (InputMode::Chat, "!!"),
        ] {
            app.input_history.push("$ touch forbidden-bang".to_string());
            app.input_mode = mode;
            app.input = input.to_string();
            submit_input(&mut app);
            assert!(app.messages.last().unwrap().content.contains("Tests are still running"));
            assert!(app.pending_workflow.is_none());
            assert_eq!(app.active_task.as_ref().unwrap().id, task_id);
        }
        let idx = app.pending_placeholder();
        let mut intent = ParsedIntent::new("shell", 1.0);
        intent.args.command = Some("touch forbidden-late-intent".to_string());
        app.assistant_tx.send(AssistantEvent::IntentResolved {
            idx,
            intent,
            original_input: "create a file".to_string(),
            cwd: app.session.cwd.clone(),
        }).unwrap();
        app.poll_assistant();
        assert!(app.messages.last().unwrap().content.contains("Tests are still running"));
        assert!(app.pending_workflow.is_none());
        assert_eq!(app.active_task.as_ref().unwrap().id, task_id);
        assert!(!app.pending_idxs.contains(&idx));
        app.shutdown_command_task().await;
        for name in ["forbidden-shell", "forbidden-explicit", "forbidden-bang", "forbidden-late-intent"] {
            assert!(!directory.path().join(name).exists(), "unexpected command ran: {name}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancel_task_input_works_in_chat_and_shell_modes() {
        for mode in [InputMode::Chat, InputMode::Shell] {
            let (_directory, mut app) = isolated_app();
            let idx = start_fake_test(&mut app, "sleep 30");
            app.input_mode = mode;
            app.input = "cancel task".to_string();
            submit_input(&mut app);
            assert!(app.active_task.as_ref().unwrap().cancelling);
            assert!(!app.should_quit);
            assert!(app.pending_workflow.is_none());
            wait_for_task_state(&mut app, |app| app.active_task.is_none()).await;
            assert!(app.messages[idx].content.contains("CANCELLED"));
        }
    }

    #[test]
    fn ctrl_c_without_a_task_still_exits() {
        let (_directory, mut app) = isolated_app();
        assert!(!app.cancel_command_task());
        handle_key_event(&mut app, crossterm::event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.should_quit);
    }

    #[test]
    fn cancel_task_without_an_active_task_is_a_noop_in_both_modes() {
        for mode in [InputMode::Chat, InputMode::Shell] {
            let (_directory, mut app) = isolated_app();
            app.input_mode = mode;
            app.input = "cancel task".to_string();
            submit_input(&mut app);
            assert!(app.active_task.is_none());
            assert!(!app.should_quit);
            assert!(app.pending_workflow.is_none());
            assert!(app.messages.last().unwrap().content.contains("no active test task"));
        }
    }
}
