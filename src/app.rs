use std::{
    error::Error,
    fs,
    io::{self, Read, stdout},
    path::PathBuf,
    process::Stdio,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::{
    config::Config,
    ollama,
    session::{Message, Role, SessionState},
};

enum AssistantEvent {
    Token { idx: usize, chunk: String },
    Completed { idx: usize, content: Option<String> },
    Failed { idx: usize, error: String },
}

pub fn run(config: Config) -> Result<()> {
    ollama::ensure_available(&config.model)?;

    let mut terminal = TerminalGuard::new().context("setting up terminal")?;
    let mut app = App::new(config);
    let tick_rate = Duration::from_millis(100);
    let mut last_tick = Instant::now();

    loop {
        terminal
            .terminal
            .draw(|f| ui(f, &app))
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

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
}

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode().context("enable raw mode")?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).context("create terminal")?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
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
    assistant_tx: mpsc::Sender<AssistantEvent>,
    assistant_rx: mpsc::Receiver<AssistantEvent>,
    should_quit: bool,
}

impl App {
    fn new(config: Config) -> Self {
        let (assistant_tx, assistant_rx) = mpsc::channel();
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
            should_quit: false,
        };

        let system_msg = format!(
            "LLM CLI ready. Model: {} (streaming: {}). Type to chat; Enter to submit; Esc/q/Ctrl+C to exit.",
            app.config.model, app.config.streaming
        );
        app.push_recorded(Role::System, system_msg);
        app
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
        self.push_recorded(Role::Assistant, content);
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

fn handle_key_event(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true
        }
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Esc => app.should_quit = true,
        KeyCode::Enter => submit_input(app),
        KeyCode::Up => {
            app.scroll = app.scroll.saturating_add(1);
        }
        KeyCode::Down => {
            app.scroll = app.scroll.saturating_sub(1);
        }
        KeyCode::PageUp => {
            app.scroll = app.scroll.saturating_add(10);
        }
        KeyCode::PageDown => {
            app.scroll = app.scroll.saturating_sub(10);
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            recall_history_prev(app);
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            recall_history_next(app);
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

    let prompt = app.input.trim().to_string();
    app.push_recorded(Role::User, prompt.clone());
    if app.input_history.last().map_or(true, |s| s != &prompt) {
        app.input_history.push(prompt.clone());
    }
    app.history_idx = None;
    app.input.clear();

    if let Some(workflow) = app.pending_workflow.take() {
        handle_workflow_response(app, workflow, &prompt);
        return;
    }

    if maybe_handle_intent(app, &prompt) {
        return;
    }

    // Insert placeholder assistant message and spawn background generation.
    let placeholder_idx = app.messages.len();
    app.messages.push(Message {
        role: Role::Assistant,
        content: String::new(),
    });
    app.pending_idxs.push(placeholder_idx);

    let tx = app.assistant_tx.clone();
    let model = app.config.model.clone();
    let system_prompt = app.config.system_prompt.clone();
    let timeout_secs = app.config.request_timeout_secs;
    let prompt_for_thread = prompt.clone();
    thread::spawn(move || {
        let composed_prompt = format!(
            "{}\n\nUser: {}\nAssistant:",
            system_prompt, prompt_for_thread
        );

        let mut child = match std::process::Command::new("ollama")
            .arg("run")
            .arg(&model)
            .arg(&composed_prompt)
            .stdout(Stdio::piped())
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
                    let _ = child.kill();
                    return;
                }

                match stdout.read(&mut buf) {
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
                        let _ = child.kill();
                        return;
                    }
                }
            }
        }

        match child.wait() {
            Ok(status) if status.success() => {
                // Finalize content (already accumulated in tokens)
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

fn ui(f: &mut ratatui::Frame, app: &App) {
    let cwd_display = app.session.cwd.to_string_lossy();
    let repo_display = app
        .session
        .repo_root
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "-".to_string());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.size());

    let message_lines = render_messages(&app.messages);
    let visible = clip_lines_from_bottom(&message_lines, app.scroll, chunks[0].height as usize);
    let log = Paragraph::new(visible)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Conversation (↑/↓/PgUp/PgDn scroll)"),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(log, chunks[0]);

    let input = Paragraph::new(app.input.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Input (Enter to submit)"),
    );
    f.render_widget(input, chunks[1]);

    let status_text = Line::from(vec![
        Span::raw("model: "),
        Span::raw(&app.config.model).bold(),
        Span::raw(" | streaming: "),
        Span::raw(if app.config.streaming { "on" } else { "off" }).bold(),
        Span::raw(" | style: bullets "),
        Span::raw(" | cwd: "),
        Span::raw(cwd_display),
        Span::raw(" | repo: "),
        Span::raw(repo_display),
        Span::raw(" | pending: "),
        Span::raw(app.pending_idxs.len().to_string()).bold(),
        Span::raw(" | workflow: "),
        Span::raw(if app.pending_workflow.is_some() {
            "confirm"
        } else {
            "-"
        })
        .bold(),
        Span::raw(" | history: Ctrl+P/Ctrl+N | quit: Esc/q/Ctrl+C"),
    ]);
    let status = Paragraph::new(status_text);
    f.render_widget(status, chunks[2]);
}

fn recall_history_prev(app: &mut App) {
    if app.input_history.is_empty() {
        return;
    }
    let next_idx = match app.history_idx {
        None => app.input_history.len().saturating_sub(1),
        Some(0) => 0,
        Some(i) => i.saturating_sub(1),
    };
    app.history_idx = Some(next_idx);
    if let Some(val) = app.input_history.get(next_idx) {
        app.input = val.clone();
    }
}

fn recall_history_next(app: &mut App) {
    if app.input_history.is_empty() {
        return;
    }
    let next_idx = match app.history_idx {
        None => return,
        Some(i) if i + 1 >= app.input_history.len() => {
            app.history_idx = None;
            app.input.clear();
            return;
        }
        Some(i) => i + 1,
    };
    app.history_idx = Some(next_idx);
    if let Some(val) = app.input_history.get(next_idx) {
        app.input = val.clone();
    }
}

fn handle_workflow_response(app: &mut App, workflow: WorkflowState, prompt: &str) {
    let confirmed = matches!(prompt.trim().to_lowercase().as_str(), "y" | "yes");
    if !confirmed {
        app.reply("Workflow cancelled.".to_string());
        return;
    }

    match workflow.kind {
        WorkflowKind::SaveWork => {
            run_save_work(app, &workflow.repo_root);
        }
    }
}

fn run_save_work(app: &mut App, repo_root: &std::path::Path) {
    let mut logs = vec!["Running save-work workflow…".to_string()];

    let commit_msg =
        generate_commit_message(app, repo_root).unwrap_or_else(|| "chore: save work".to_string());
    logs.push(format!("Using commit message: {}", commit_msg));

    if !run_workflow_step(&mut logs, repo_root, "git add -A", "git", &["add", "-A"]) {
        app.reply(logs.join("\n"));
        return;
    }

    if !run_workflow_step(
        &mut logs,
        repo_root,
        "git commit",
        "git",
        &["commit", "-m", &commit_msg],
    ) {
        app.reply(logs.join("\n"));
        return;
    }

    if !run_workflow_step(&mut logs, repo_root, "git push", "git", &["push"]) {
        app.reply(logs.join("\n"));
        return;
    }

    logs.push("Workflow completed successfully.".to_string());
    app.reply(logs.join("\n"));
}

fn run_workflow_step(
    logs: &mut Vec<String>,
    repo_root: &std::path::Path,
    label: &str,
    program: &str,
    args: &[&str],
) -> bool {
    match run_command(repo_root, program, args) {
        Ok(out) => {
            logs.push(format!("{label} OK\n{out}"));
            true
        }
        Err(err) => {
            logs.push(format!("{label} failed: {err}"));
            false
        }
    }
}

fn run_command(cwd: &std::path::Path, program: &str, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                anyhow!("command '{program}' not found in PATH")
            } else {
                e.into()
            }
        })
        .with_context(|| format!("running {program} {:?}", args))?;

    if !output.status.success() {
        // Allow ripgrep exit code 1 (no matches) as a soft success.
        if program == "rg" && output.status.code() == Some(1) {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            return Ok(stdout.trim().to_string());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        bail!(
            "{program} {:?} exited with {}.\nstdout:\n{}\nstderr:\n{}",
            args,
            output.status,
            stdout,
            stderr
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout.trim().to_string())
}

fn format_error(err: &anyhow::Error) -> String {
    let mut parts = vec![err.to_string()];
    let mut current: Option<&(dyn Error + 'static)> = err.source();
    while let Some(src) = current {
        parts.push(src.to_string());
        current = src.source();
    }
    parts.join(": ")
}

#[derive(Debug, Clone)]
struct WorkflowState {
    kind: WorkflowKind,
    repo_root: std::path::PathBuf,
}

#[derive(Debug, Clone)]
enum WorkflowKind {
    SaveWork,
}

fn maybe_handle_intent(app: &mut App, prompt: &str) -> bool {
    let normalized = prompt.to_lowercase();
    if normalized.contains("save work") {
        if let Some(repo_root) = app.session.repo_root.clone() {
            let status_preview = match run_command(&repo_root, "git", &["status", "--short"]) {
                Ok(out) => out,
                Err(err) => format!("(git status failed: {err})"),
            };
            let plan = [
                "Planned git workflow:",
                "• git status (preview)",
                "• git add -A",
                "• git commit -m \"chore: save work\"",
                "• git push",
                "",
                "Status preview:",
                &status_preview,
                "",
                "Type 'yes' to run, anything else to cancel.",
            ]
            .join("\n");
            app.reply(plan);
            app.pending_workflow = Some(WorkflowState {
                kind: WorkflowKind::SaveWork,
                repo_root,
            });
        } else {
            app.reply("No git repository detected; cannot save work.".to_string());
        }
        return true;
    }

    if normalized.contains("git status") || normalized == "status" {
        if let Some(repo_root) = app.session.repo_root.clone() {
            match run_command(&repo_root, "git", &["status"]) {
                Ok(out) => app.reply(out),
                Err(err) => app.reply(format!("git status failed: {}", format_error(&err))),
            }
        } else {
            app.reply("No git repository detected.".to_string());
        }
        return true;
    }

    if normalized.contains("find todos")
        || normalized.contains("find todo")
        || normalized == "todos"
        || normalized.contains("todo")
    {
        let root = app
            .session
            .repo_root
            .clone()
            .unwrap_or_else(|| app.session.cwd.clone());
        let pattern = r"(?i)^\s*(?://|#|;|<!--|/\*+)\s*(TODO|FIXME)|^\s*(TODO|FIXME)";
        match run_command(
            &root,
            "rg",
            &["--no-heading", "--line-number", "--pcre2", pattern],
        ) {
            Ok(out) if out.trim().is_empty() => app.reply("No TODO/FIXME found.".to_string()),
            Ok(out) => app.reply(format!("TODO/FIXME:\n{out}")),
            Err(err) => app.reply(format!("Search failed: {}", format_error(&err))),
        }
        return true;
    }

    if normalized.contains("run tests") || normalized == "tests" {
        if let Some(repo_root) = app.session.repo_root.clone() {
            match run_command(&repo_root, "cargo", &["test"]) {
                Ok(out) => app.reply(format!("cargo test output:\n{out}")),
                Err(err) => app.reply(format!("cargo test failed: {}", format_error(&err))),
            }
        } else {
            app.reply("Not in a cargo project; cannot run tests.".to_string());
        }
        return true;
    }

    if normalized.contains("draft commit") || normalized.contains("commit message") {
        if let Some(repo_root) = app.session.repo_root.clone() {
            match generate_commit_message(app, &repo_root) {
                Some(msg) => app.reply(format!("Suggested commit message:\n{msg}")),
                None => app
                    .reply("Could not generate commit message (is anything staged?).".to_string()),
            }
        } else {
            app.reply("No git repository detected; cannot draft a commit message.".to_string());
        }
        return true;
    }

    if let Some(path) = parse_show_file(prompt) {
        let resolved = if path.is_absolute() {
            path
        } else if let Some(repo) = app.session.repo_root.clone() {
            repo.join(path)
        } else {
            app.session.cwd.join(path)
        };

        match fs::read_to_string(&resolved) {
            Ok(contents) => app.reply(format!("Contents of {}:\n{}", resolved.display(), contents)),
            Err(err) => app.reply(format!("Could not read {}: {}", resolved.display(), err)),
        }
        return true;
    }

    false
}

fn parse_show_file(prompt: &str) -> Option<PathBuf> {
    let lower = prompt.to_lowercase();
    let prefixes = ["show file ", "read file ", "open file ", "show "];
    for p in prefixes {
        if lower.starts_with(p) {
            let rest = prompt[p.len()..].trim();
            if !rest.is_empty() {
                return Some(PathBuf::from(rest));
            }
        }
    }
    None
}

fn render_messages(messages: &[Message]) -> Vec<Line<'static>> {
    let mut rendered = Vec::new();
    for m in messages {
        let (label, color) = match m.role {
            Role::User => ("user", Color::Cyan),
            Role::Assistant => ("assistant", Color::Green),
            Role::System => ("system", Color::Yellow),
        };
        let label_text = format!("[{label}] ");
        let body_lines = format_message_body(&m.content);

        for (i, body) in body_lines.into_iter().enumerate() {
            if i == 0 {
                rendered.push(Line::from(vec![
                    Span::styled(label_text.clone(), Style::default().fg(color)),
                    Span::raw(body),
                ]));
            } else {
                rendered.push(Line::from(vec![
                    Span::raw(" ".repeat(label_text.len())),
                    Span::raw(body),
                ]));
            }
        }
    }
    rendered
}

fn format_message_body(content: &str) -> Vec<String> {
    let mut lines_out = Vec::new();

    for line in content.replace('\r', "").lines() {
        let trimmed = line.trim_end();
        if trimmed.contains('•') {
            lines_out.extend(trimmed.split('•').filter_map(|chunk| {
                let part = chunk.trim();
                if part.is_empty() {
                    None
                } else {
                    Some(format!("• {part}"))
                }
            }));
            continue;
        }
        lines_out.push(trimmed.to_string());
    }

    if lines_out.is_empty() {
        lines_out.push(String::new());
    }

    lines_out
}

fn clip_lines_from_bottom<'a>(
    lines: &'a [Line<'a>],
    scroll_from_bottom: usize,
    height: usize,
) -> Vec<Line<'a>> {
    if height == 0 {
        return Vec::new();
    }
    let total = lines.len();
    let max_scroll = total.saturating_sub(height);
    let offset = scroll_from_bottom.min(max_scroll);
    let end = total.saturating_sub(offset);
    let start = end.saturating_sub(height);
    lines[start..end].to_vec()
}
fn generate_commit_message(app: &App, repo_root: &std::path::Path) -> Option<String> {
    if !app.config.generate_commit_message {
        return None;
    }
    let stat = run_command(repo_root, "git", &["diff", "--cached", "--stat"]).ok()?;
    let patch = run_command(
        repo_root,
        "git",
        &["diff", "--cached", "--unified=3", "--max-count=1"],
    )
    .unwrap_or_default();
    let prompt = format!(
        "Generate a concise git commit message in imperative mood, <=72 chars.\nInclude scope if obvious. Avoid filler.\nStaged changes (stat):\n{stat}\n\nPatch snippet:\n{patch}\n\nReturn only the commit message."
    );

    let result = std::process::Command::new("ollama")
        .arg("run")
        .arg(&app.config.model)
        .arg(prompt)
        .output()
        .ok()?;

    if !result.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let first_line = stdout.lines().next().unwrap_or("").trim();
    if first_line.is_empty() {
        None
    } else {
        Some(first_line.to_string())
    }
}
