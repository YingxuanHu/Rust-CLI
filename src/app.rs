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
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};

use std::sync::Arc;

use crate::{
    config::Config,
    context,
    custom_command_generator,
    embedding::EmbeddingCache,
    handlers::{dispatch_intent, AssistantEvent, IntentDispatcher},
    input::{expand_bang_shortcut, HistoryNavigation},
    intent::{self, ParsedIntent},
    learned::LearnedAliases,
    ollama,
    repo::ProjectType,
    session::{Message, Role, SessionState},
    tools::{ToolArgs, TOOLS},
    ui::{render_ui, AppView, InputMode, TerminalGuard},
    user_feedback,
    workflow::{generate_commit_message, handle_workflow_response, WorkflowResponder, WorkflowState},
};

pub async fn run(config: Config) -> Result<()> {
    ollama::ensure_available(&config.model)?;

    // Initialize embedding cache for semantic intent matching
    let mut embedding_cache = EmbeddingCache::new(Some(&config.embedding_model));
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
                Event::Mouse(mouse) => {
                    handle_mouse_event(&mut app, mouse);
                }
                _ => {}
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
    pending_user_feedback: Option<String>,
    assistant_tx: mpsc::UnboundedSender<AssistantEvent>,
    assistant_rx: mpsc::UnboundedReceiver<AssistantEvent>,
    embedding_cache: Arc<EmbeddingCache>,
    input_mode: InputMode,
    should_quit: bool,
    viewing_history: bool,
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
            pending_user_feedback: None,
            assistant_tx,
            assistant_rx,
            embedding_cache,
            input_mode: InputMode::Chat,
            should_quit: false,
            viewing_history: false,
        };

        let status = if embeddings_ready {
            "embeddings: ready"
        } else {
            "embeddings: disabled"
        };
        let system_msg = format!(
            "LLM CLI ready. Model: {} ({}). Modes: Chat/Shell (Ctrl+S). History: ↑/↓. Scroll: mouse/PgUp/PgDn. Enter to submit; Esc/q to exit.",
            app.config.model, status
        );
        app.push_recorded(Role::System, system_msg);
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
            self.viewing_history = false;
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

        // Check for commit message generation signal
        if final_content.starts_with("__COMMIT_MSG__:") {
            if let Some(msg_end) = final_content.find('\n') {
                let msg = &final_content[14..msg_end]; // Skip "__COMMIT_MSG__:"
                self.session.record_output("commit_msg", "Generated commit message", msg);
            }
            // Remove the signal prefix and continue with normal display
            let display_content = final_content.split_once('\n')
                .map(|(_, rest)| rest)
                .unwrap_or(&final_content);
            self.upsert_message(idx, Role::Assistant, display_content.to_string());
            self.session.record(Message {
                role: Role::Assistant,
                content: display_content.to_string(),
            });
            self.pending_idxs.retain(|&i| i != idx);
            return;
        }
        
        // Check for custom command generation signal
        if final_content.starts_with("__CUSTOM_COMMAND_GENERATED__:") {
            self.pending_idxs.retain(|&i| i != idx);
            // Parse the signal: __CUSTOM_COMMAND_GENERATED__:original_input:generated_cmd:save_path
            let parts: Vec<&str> = final_content.splitn(4, ':').collect();
            if parts.len() >= 4 {
                let original_input = parts[1];
                let generated_cmd = parts[2];
                let save_path_str = parts[3];
                let save_path = std::path::PathBuf::from(save_path_str);
                
                // Show the generated command and ask for confirmation
                self.reply(format!(
                    "💡 Generated command:\n  {}\n\n\
                     This will be saved as: \"{}\" → custom shell command\n\n\
                     Options:\n\
                     • Press Enter (or type 'yes') to confirm and execute\n\
                     • Type 'edit: <new command>' to modify\n\
                     • Type 'no' to cancel",
                    generated_cmd,
                    original_input
                ));
                
                // Store for confirmation
                self.pending_workflow = Some(WorkflowState {
                    kind: crate::workflow::WorkflowKind::CustomCommandConfirm {
                        original_input: original_input.to_string(),
                        generated_cmd: generated_cmd.to_string(),
                        save_path,
                    },
                    repo_root: self.session.repo_root.clone().unwrap_or_else(|| std::path::PathBuf::from(".")),
                });
                
                // Clear pending feedback since we're now in confirmation workflow
                self.pending_user_feedback = None;
            }
            return;
        }

        // Check for ask user signal from background task
        if final_content.starts_with("__ASK_USER__:") {
            self.pending_idxs.retain(|&i| i != idx);
            // Remove the placeholder message
            if idx < self.messages.len() {
                self.messages.remove(idx);
            }
            // Extract original input
            let original_input = final_content.strip_prefix("__ASK_USER__:")
                .unwrap_or("")
                .to_string();
            
            // Try to generate a suggested command first
            let tx = self.assistant_tx.clone();
            let model = self.config.classifier_model.clone();
            let original_input_clone = original_input.clone();
            let repo_context = self.session.repo_info.as_ref().map(|info| {
                let type_str = match info.project_type {
                    crate::repo::ProjectType::Rust => "Rust",
                    crate::repo::ProjectType::Node => "Node.js",
                    crate::repo::ProjectType::Python => "Python",
                    crate::repo::ProjectType::Go => "Go",
                    crate::repo::ProjectType::Unknown => "Unknown",
                };
                format!("{} project", type_str)
            });
            
            // Show a message that we're generating
            self.reply(format!("💭 Generating suggested command for \"{}\"...", original_input));
            
            tokio::spawn(async move {
                match custom_command_generator::generate_custom_command(
                    &original_input_clone,
                    &model,
                    repo_context.as_deref(),
                ).await {
                    Ok(suggested_cmd) => {
                        let _ = tx.send(AssistantEvent::Completed {
                            idx: 0,
                            content: Some(format!("__SUGGEST_COMMAND__:{}:{}", original_input_clone, suggested_cmd)),
                        });
                    }
                    Err(_) => {
                        // Fall back to user feedback prompt
                        let _ = tx.send(AssistantEvent::Completed {
                            idx: 0,
                            content: Some(format!("__FALLBACK_ASK_USER__:{}", original_input_clone)),
                        });
                    }
                }
            });
            return;
        }
        
        // Handle command suggestion response
        if final_content.starts_with("__SUGGEST_COMMAND__:") {
            let parts: Vec<&str> = final_content.splitn(3, ':').collect();
            if parts.len() >= 3 {
                let original_input = parts[1];
                let suggested_cmd = parts[2];
                
                self.reply(format!(
                    "💡 Suggested command: `{}`\n\n\
                     Options:\n\
                     • [y]es - Execute it\n\
                     • [s]ave - Save as custom command\n\
                     • [e]dit - Provide a different command (cmd: ...)\n\
                     • [n]o - Show tool selection menu instead",
                    suggested_cmd
                ));
                
                self.pending_workflow = Some(WorkflowState {
                    kind: crate::workflow::WorkflowKind::CustomCommandConfirm {
                        original_input: original_input.to_string(),
                        generated_cmd: suggested_cmd.to_string(),
                        save_path: self.session.repo_root.clone()
                            .unwrap_or_else(|| self.session.cwd.clone())
                            .join(".llm-cli/learned.toml"),
                    },
                    repo_root: self.session.repo_root.clone().unwrap_or_else(|| self.session.cwd.clone()),
                });
                
                // Store original input for potential fallback
                self.pending_user_feedback = Some(original_input.to_string());
            }
            return;
        }
        
        // Handle fallback to ask user
        if final_content.starts_with("__FALLBACK_ASK_USER__:") {
            let original_input = final_content.strip_prefix("__FALLBACK_ASK_USER__:")
                .unwrap_or("")
                .to_string();
            self.pending_user_feedback = Some(original_input.clone());
            let feedback = user_feedback::generate_feedback_prompt(&original_input);
            self.reply(feedback);
            return;
        }

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
            content: final_content.clone(),
        });
        self.pending_idxs.retain(|&i| i != idx);
        
        // Check if the response contains shell commands
        let commands = crate::workflow::extract_commands_from_text(&final_content);
        if !commands.is_empty() {
            let combined = commands.join(" && ");
            let original_query = self.input_history.last().cloned().unwrap_or_default();
            
            let msg = if commands.len() == 1 {
                format!(
                    "\n💡 Found command: `{}`\n\n\
                     Options:\n\
                     • [y]es - Execute it\n\
                     • [s]ave - Save as custom command for \"{}\" \n\
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
                     • [s]ave - Save as custom command for \"{}\"\n\
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

    fn set_session_cwd(&mut self, new_cwd: PathBuf) {
        self.session.set_cwd(new_cwd);
    }

    fn record_output(&mut self, kind: &'static str, summary: &str, content: &str) {
        self.session.record_output(kind, summary, content);
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
            scroll_session_history_up(app);
        }
        KeyCode::PageDown => {
            scroll_session_history_down(app);
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
    
    // Allow empty input only if we have a pending workflow or feedback
    if raw_input.is_empty() && app.pending_workflow.is_none() && app.pending_user_feedback.is_none() {
        return;
    }

    app.history_idx = None;
    app.input.clear();

    // Handle user feedback response if we're waiting for one
    if app.pending_user_feedback.is_some() {
        handle_user_feedback_response(app, &raw_input);
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
    let system_prompt = app.config.system_prompt.clone();
    let timeout_secs = app.config.request_timeout_secs;
    let prompt_for_task = prompt.clone();
    let embedding_cache = Arc::clone(&app.embedding_cache);
    
    // Load learned aliases
    let learned_global = app.config.learned_path.clone();
    let learned_project = app.session.repo_root.as_ref().map(|r| r.join(".llm-cli/learned.toml"));
    
    // Capture repo context before spawning
    let repo_context = if let Some(info) = &app.session.repo_info {
        let type_str = match info.project_type {
            crate::repo::ProjectType::Rust => "Rust",
            crate::repo::ProjectType::Node => "Node.js",
            crate::repo::ProjectType::Python => "Python",
            crate::repo::ProjectType::Go => "Go",
            crate::repo::ProjectType::Unknown => "Unknown",
        };
        let name_str = info.name.as_deref().unwrap_or("unnamed");
        format!("\n\nCurrent project: {} ({})", name_str, type_str)
    } else {
        String::new()
    };
    
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
        ).await {
            // Check if we need to ask the user
            if parsed.tool == "ask_user" {
                let _ = tx.send(AssistantEvent::Completed {
                    idx: placeholder_idx,
                    content: Some(format!("__ASK_USER__:{}", prompt_for_task)),
                });
                return;
            }
            
            // Non-chat intents get dispatched via signal to main thread
            if parsed.tool != "chat" && parsed.confidence >= 0.3 {
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

        // Fall through to regular LLM chat
        let composed_prompt = format!(
            "{}{}{}\n\nUser: {}\nAssistant:",
            system_prompt, repo_context, context_injection, prompt_for_task
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

fn handle_pending_workflow(app: &mut App, prompt: &str) {
    if let Some(workflow) = app.pending_workflow.take() {
        // Need to handle special case for SaveWorkPlan
        if matches!(workflow.kind, crate::workflow::WorkflowKind::SaveWorkPlan) {
            let confirmed = matches!(prompt.trim().to_lowercase().as_str(), "" | "y" | "yes");
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
                    "Suggested commit message:\n{}\nPress Enter (or type 'yes') to accept, or type a custom message. (Type 'cancel' to abort.)",
                    suggested
                ));
            } else {
                app.reply("Workflow cancelled.");
            }
        } else {
            handle_workflow_response(app, workflow, prompt);
            
            // Check if the last message is the special feedback signal
            if let Some(last_msg) = app.messages.last() {
                if last_msg.content == "__SHOW_FEEDBACK_PROMPT__" {
                    // Remove the signal message
                    app.messages.pop();
                    // Show feedback prompt if we have the original input
                    if let Some(original_input) = &app.pending_user_feedback {
                        let feedback = user_feedback::generate_feedback_prompt(original_input);
                        app.reply(feedback);
                    }
                }
            }
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
}

fn handle_user_feedback_response(app: &mut App, response: &str) {
    let original_input = app.pending_user_feedback.take().unwrap();
    
    // Determine save path (always use .llm-cli in current directory)
    let save_path = PathBuf::from(".llm-cli/learned.toml");
    
    // Load current learned aliases
    let learned_global = app.config.learned_path.clone();
    let learned_project = app.session.repo_root.as_ref().map(|r| r.join(".llm-cli/learned.toml"));
    let mut learned = LearnedAliases::load(&learned_global, learned_project.as_deref())
        .unwrap_or_default();
    
    match user_feedback::parse_feedback_response(response) {
        user_feedback::FeedbackResponse::None => {
            app.reply("Okay, I won't learn this.");
        }
        
        user_feedback::FeedbackResponse::ToolSelection(idx) => {
            let tool = &TOOLS[idx];
            
            // Save the new alias
            if let Err(e) = learned.save_alias(&original_input, tool.name, &save_path, "user_feedback") {
                app.reply(format!("Failed to save learned alias: {}", e));
            } else {
                app.reply(format!("✓ Learned: \"{}\" → {}", original_input, tool.name));
                
                // Now execute the tool
                let intent = ParsedIntent::new(tool.name, 1.0);
                dispatch_intent(app, &intent, &original_input);
            }
        }
        
        user_feedback::FeedbackResponse::ExplicitCommand(custom_cmd) => {
            // Save as a custom shell command
            if let Err(e) = learned.save_custom_command(
                &original_input,
                &custom_cmd,
                &save_path,
                "user_custom",
            ) {
                app.reply(format!("Failed to save custom command: {}", e));
            } else {
                app.reply(format!(
                    "✓ Learned custom command: \"{}\" → {}\nExecuting now...",
                    original_input,
                    custom_cmd
                ));
                
                // Execute the custom command
                crate::handlers::handle_shell_dispatch(app, &custom_cmd);
            }
        }
        
        user_feedback::FeedbackResponse::NaturalLanguageDescription(description) => {
            // Use LLM to generate the command
            app.reply(format!("🤔 Generating command for: \"{}\"...", description));
            
            // Spawn background task to generate macro
            let tx = app.assistant_tx.clone();
            let model = app.config.model.clone();
            let original_input_clone = original_input.clone();
            let save_path_clone = save_path.clone();
            
            // Get repo context
            let repo_context = if let Some(info) = &app.session.repo_info {
                let type_str = match info.project_type {
                    ProjectType::Rust => "Rust (Cargo)",
                    ProjectType::Node => "Node.js (npm)",
                    ProjectType::Python => "Python",
                    ProjectType::Go => "Go",
                    ProjectType::Unknown => "Unknown",
                };
                Some(format!("{} project", type_str))
            } else {
                None
            };
            
            tokio::spawn(async move {
                match custom_command_generator::generate_custom_command(
                    &description,
                    &model,
                    repo_context.as_deref(),
                ).await {
                    Ok(generated_cmd) => {
                        // Signal back with the generated command
                        let _ = tx.send(AssistantEvent::Completed {
                            idx: 0, // Dummy index
                            content: Some(format!(
                                "__CUSTOM_COMMAND_GENERATED__:{}:{}:{}",
                                original_input_clone,
                                generated_cmd,
                                save_path_clone.display()
                            )),
                        });
                    }
                    Err(e) => {
                        let _ = tx.send(AssistantEvent::Failed {
                            idx: 0,
                            error: format!("Failed to generate command: {}", e),
                        });
                    }
                }
            });
            
            // Put the original input back so we can handle it later
            app.pending_user_feedback = Some(original_input);
        }
        
        user_feedback::FeedbackResponse::Invalid => {
            app.reply("Invalid selection. Please type:\n  • A number (1-8) to select a tool\n  • Natural language description (e.g., 'stage and commit only')\n  • 'cmd: <command>' for explicit shell command\n  • 'none' to skip");
            app.pending_user_feedback = Some(original_input);
        }
    }
}
