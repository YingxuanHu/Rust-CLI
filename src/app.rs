use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use tokio::{
    io::AsyncReadExt,
    process::Command as TokioCommand,
    sync::mpsc,
};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use std::sync::Arc;

use crate::{
    config::Config,
    embedding::EmbeddingCache,
    handlers::{dispatch_intent, AssistantEvent, IntentDispatcher},
    input::{expand_bang_shortcut, HistoryNavigation},
    intent::{self, ParsedIntent},
    ollama,
    session::{Message, Role, SessionState},
    tools::ToolArgs,
    ui::{render_ui, AppView, InputMode, TerminalGuard},
    workflow::{generate_commit_message, handle_workflow_response, WorkflowResponder, WorkflowState},
};

pub async fn run(config: Config) -> Result<()> {
    ollama::ensure_available(&config.model)?;

    // Initialize embedding cache for semantic intent matching
    let mut embedding_cache = EmbeddingCache::new(None);
    eprintln!("Initializing embedding cache (this may take a moment)...");
    if let Err(e) = embedding_cache.initialize().await {
        eprintln!(
            "Warning: Could not initialize embeddings: {}. Falling back to direct chat.",
            e
        );
        eprintln!("Tip: Run 'ollama pull nomic-embed-text' to enable semantic matching.");
    }
    let embedding_cache = Arc::new(embedding_cache);

    let mut terminal = TerminalGuard::new().context("setting up terminal")?;
    let mut app = App::new(config, Arc::clone(&embedding_cache));
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

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
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key_event(&mut app, key);
                }
            }
        }

        app.poll_assistant();

        if app.should_quit {
            break;
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    Ok(())
}

struct App {
    config: Config,
    session: SessionState,
    input: String,
    messages: Vec<Message>,
    scroll: usize,
    pending_idxs: Vec<usize>,
    input_history: Vec<String>,
    history_idx: Option<usize>,
    pending_workflow: Option<WorkflowState>,
    assistant_tx: mpsc::UnboundedSender<AssistantEvent>,
    assistant_rx: mpsc::UnboundedReceiver<AssistantEvent>,
    embedding_cache: Arc<EmbeddingCache>,
    input_mode: InputMode,
    should_quit: bool,
}

impl App {
    fn new(config: Config, embedding_cache: Arc<EmbeddingCache>) -> Self {
        let (assistant_tx, assistant_rx) = mpsc::unbounded_channel();
        let embeddings_ready = embedding_cache.is_initialized();
        let mut app = Self {
            config,
            session: SessionState::new(),
            input: String::new(),
            messages: Vec::new(),
            scroll: 0,
            pending_idxs: Vec::new(),
            input_history: Vec::new(),
            history_idx: None,
            pending_workflow: None,
            assistant_tx,
            assistant_rx,
            embedding_cache,
            input_mode: InputMode::Chat,
            should_quit: false,
        };

        let status = if embeddings_ready {
            "embeddings: ready"
        } else {
            "embeddings: disabled"
        };
        let system_msg = format!(
            "LLM CLI ready. Model: {} ({}). Modes: Chat/Shell (Ctrl+S). History: ↑/↓. Enter to submit; Esc/q to exit.",
            app.config.model, status
        );
        app.push_recorded(Role::System, system_msg);
        app
    }

    fn create_view(&self) -> AppView<'_> {
        AppView {
            messages: &self.messages,
            scroll: self.scroll,
            input: &self.input,
            input_mode: self.input_mode,
            model: &self.config.model,
            streaming: self.config.streaming,
            cwd: self.session.cwd.to_string_lossy().to_string(),
            repo_root: self
                .session
                .repo_root
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            pending_count: self.pending_idxs.len(),
            has_workflow: self.pending_workflow.is_some(),
        }
    }

    fn poll_assistant(&mut self) {
        while let Ok(event) = self.assistant_rx.try_recv() {
            match event {
                AssistantEvent::Token { idx, chunk } => self.append_assistant_chunk(idx, chunk),
                AssistantEvent::Completed { idx, content } => self.finish_assistant(idx, content),
                AssistantEvent::Failed { idx, error } => self.fail_assistant(idx, error),
            }
            self.scroll = 0;
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
    }

    fn append_assistant_chunk(&mut self, idx: usize, chunk: String) {
        if let Some(msg) = self.messages.get_mut(idx) {
            msg.role = Role::Assistant;
            msg.content.push_str(&chunk);
        }
    }

    fn finish_assistant(&mut self, idx: usize, content: Option<String>) {
        let fallback = self
            .messages
            .get(idx)
            .map(|m| m.content.clone())
            .unwrap_or_default();
        let final_content = content.unwrap_or(fallback);

        // Check for intent signal from background task
        if final_content.starts_with("__INTENT__:") {
            self.pending_idxs.retain(|&i| i != idx);
            // Remove the placeholder message
            if idx < self.messages.len() {
                self.messages.remove(idx);
            }
            // Parse and dispatch the intent
            let parts: Vec<&str> = final_content.splitn(3, ':').collect();
            if parts.len() >= 2 {
                let tool = parts[1];
                let args_json = parts.get(2).unwrap_or(&"{}");
                let args: ToolArgs = serde_json::from_str(args_json).unwrap_or_default();
                let intent = ParsedIntent {
                    tool: tool.to_string(),
                    args,
                    confidence: 0.8,
                };
                // We need to get the original input from history
                let original_input = self.input_history.last().cloned().unwrap_or_default();
                dispatch_intent(self, &intent, &original_input);
            }
            return;
        }

        self.upsert_message(idx, Role::Assistant, final_content.clone());
        self.session.record(Message {
            role: Role::Assistant,
            content: final_content,
        });
        self.pending_idxs.retain(|&i| i != idx);
    }

    fn fail_assistant(&mut self, idx: usize, error: String) {
        let content = format!("Ollama error: {error}");
        self.upsert_message(idx, Role::System, content.clone());
        self.session.record(Message {
            role: Role::System,
            content,
        });
        self.pending_idxs.retain(|&i| i != idx);
    }

    fn upsert_message(&mut self, idx: usize, role: Role, content: String) {
        if let Some(msg) = self.messages.get_mut(idx) {
            msg.role = role;
            msg.content = content;
        } else {
            self.messages.push(Message { role, content });
        }
    }
}

// Implement IntentDispatcher for App
impl IntentDispatcher for App {
    fn reply(&mut self, content: impl Into<String>) {
        self.reply(content);
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

    fn set_session_cwd(&mut self, new_cwd: PathBuf) {
        self.session.set_cwd(new_cwd);
    }
}

// Implement WorkflowResponder for App
impl WorkflowResponder for App {
    fn reply(&mut self, content: impl Into<String>) {
        self.reply(content);
    }
}

fn handle_key_event(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true
        }
        KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            // Toggle input mode
            app.input_mode = match app.input_mode {
                InputMode::Chat => InputMode::Shell,
                InputMode::Shell => InputMode::Chat,
            };
        }
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Esc => app.should_quit = true,
        KeyCode::Enter => submit_input(app),
        KeyCode::Up => {
            recall_history_prev(app);
        }
        KeyCode::Down => {
            recall_history_next(app);
        }
        KeyCode::PageUp => {
            app.scroll = app.scroll.saturating_add(10);
        }
        KeyCode::PageDown => {
            app.scroll = app.scroll.saturating_sub(10);
        }
        KeyCode::Backspace => {
            app.input.pop();
        }
        KeyCode::Char(ch) => {
            app.input.push(ch);
        }
        _ => {}
    }
}

fn submit_input(app: &mut App) {
    if app.input.trim().is_empty() {
        return;
    }

    let raw_input = app.input.trim().to_string();
    app.history_idx = None;
    app.input.clear();

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
        crate::handlers::handle_shell_dispatch(app, cmd);
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
    if app
        .input_history
        .last()
        .map_or(true, |s| s != &history_entry)
    {
        app.input_history.push(history_entry);
    }

    // Handle pending workflow confirmations first
    if let Some(workflow) = app.pending_workflow.take() {
        // Need to handle special case for SaveWorkPlan
        if matches!(workflow.kind, crate::workflow::WorkflowKind::SaveWorkPlan) {
            let confirmed = matches!(prompt.trim().to_lowercase().as_str(), "y" | "yes");
            if confirmed {
                // Run git add -A first
                match crate::commands::run_command(&workflow.repo_root, "git", &["add", "-A"]) {
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

                // Generate commit message and move to next step
                let suggested = generate_commit_message(&app.config, &workflow.repo_root)
                    .unwrap_or_else(|| "chore: save work".to_string());
                app.pending_workflow = Some(WorkflowState {
                    kind: crate::workflow::WorkflowKind::SaveWorkCommit {
                        suggested: suggested.clone(),
                    },
                    repo_root: workflow.repo_root.clone(),
                });
                app.reply(format!(
                    "Suggested commit message:\n{}\nReply 'yes' to accept, or type a custom message. (Type 'cancel' to abort.)",
                    suggested
                ));
            } else {
                app.reply("Workflow cancelled.");
            }
        } else {
            handle_workflow_response(app, workflow, &prompt);
        }
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

    // Use embedding-based intent matching in background
    let tx = app.assistant_tx.clone();
    let model = app.config.model.clone();
    let system_prompt = app.config.system_prompt.clone();
    let timeout_secs = app.config.request_timeout_secs;
    let prompt_for_task = prompt.clone();
    let embedding_cache = Arc::clone(&app.embedding_cache);
    let use_embeddings = app.embedding_cache.is_initialized();

    // Insert placeholder for response
    let placeholder_idx = app.messages.len();
    app.messages.push(Message {
        role: Role::Assistant,
        content: String::new(),
    });
    app.pending_idxs.push(placeholder_idx);

    tokio::spawn(async move {
        // Try embedding-based intent matching if available
        if use_embeddings {
            if let Ok(parsed) =
                intent::parse_intent_with_embeddings(&embedding_cache, &prompt_for_task).await
            {
                // Non-chat intents get dispatched via signal to main thread
                if parsed.tool != "chat" && parsed.confidence >= 0.5 {
                    let _ = tx.send(AssistantEvent::Completed {
                        idx: placeholder_idx,
                        content: Some(format!(
                            "__INTENT__:{}:{}",
                            parsed.tool,
                            serde_json::to_string(&parsed.args).unwrap_or_default()
                        )),
                    });
                    return;
                }
            }
        }

        // Fall through to regular LLM chat
        let composed_prompt = format!(
            "{}\n\nUser: {}\nAssistant:",
            system_prompt, prompt_for_task
        );

        let mut child = match TokioCommand::new("ollama")
            .arg("run")
            .arg(&model)
            .arg(&composed_prompt)
            .stdout(std::process::Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(err) => {
                let _ = tx.send(AssistantEvent::Failed {
                    idx: placeholder_idx,
                    error: format!("{err}"),
                });
                return;
            }
        };

        let timeout = Duration::from_secs(timeout_secs);
        let start = Instant::now();

        if let Some(mut stdout) = child.stdout.take() {
            let mut buf = [0u8; 1024];
            loop {
                if start.elapsed() > timeout {
                    let _ = tx.send(AssistantEvent::Failed {
                        idx: placeholder_idx,
                        error: format!("ollama run timed out after {timeout_secs}s"),
                    });
                    let _ = child.kill().await;
                    return;
                }

                match stdout.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                        let _ = tx.send(AssistantEvent::Token {
                            idx: placeholder_idx,
                            chunk,
                        });
                    }
                    Err(err) => {
                        let _ = tx.send(AssistantEvent::Failed {
                            idx: placeholder_idx,
                            error: format!("{err}"),
                        });
                        let _ = child.kill().await;
                        return;
                    }
                }
            }
        }

        match child.wait().await {
            Ok(status) if status.success() => {
                let _ = tx.send(AssistantEvent::Completed {
                    idx: placeholder_idx,
                    content: None,
                });
            }
            Ok(status) => {
                let _ = tx.send(AssistantEvent::Failed {
                    idx: placeholder_idx,
                    error: format!("ollama exited with status {status}"),
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
