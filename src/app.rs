use std::{
    io::{Read, stdout},
    process::Stdio,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
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
    assistant_tx: mpsc::Sender<AssistantEvent>,
    assistant_rx: mpsc::Receiver<AssistantEvent>,
    should_quit: bool,
}

impl App {
    fn new(config: Config) -> Self {
        let mut session = SessionState::new();
        let mut messages = Vec::new();
        let (assistant_tx, assistant_rx) = mpsc::channel();

        let system_msg = Message {
            role: Role::System,
            content: format!(
                "LLM CLI ready. Model: {} (streaming: {}). Type to chat; Enter to submit; Esc/q/Ctrl+C to exit.",
                config.model, config.streaming
            ),
        };
        messages.push(system_msg.clone());
        session.record(system_msg);

        Self {
            config,
            session,
            input: String::new(),
            messages,
            scroll: 0,
            pending_idxs: Vec::new(),
            input_history: Vec::new(),
            history_idx: None,
            assistant_tx,
            assistant_rx,
            should_quit: false,
        }
    }

    fn poll_assistant(&mut self) {
        while let Ok(event) = self.assistant_rx.try_recv() {
            match event {
                AssistantEvent::Token { idx, chunk } => {
                    if let Some(msg) = self.messages.get_mut(idx) {
                        msg.role = Role::Assistant;
                        msg.content.push_str(&chunk);
                    }
                }
                AssistantEvent::Completed { idx, content } => {
                    let final_content = if let Some(msg) = self.messages.get(idx) {
                        msg.content.clone()
                    } else {
                        String::new()
                    };

                    if let Some(msg) = self.messages.get_mut(idx) {
                        msg.role = Role::Assistant;
                        if let Some(provided) = content.clone() {
                            msg.content = provided;
                        }
                    } else {
                        self.messages.push(Message {
                            role: Role::Assistant,
                            content: content.clone().unwrap_or_else(String::new),
                        });
                    }
                    self.session.record(Message {
                        role: Role::Assistant,
                        content: content.unwrap_or(final_content),
                    });
                    self.pending_idxs.retain(|&i| i != idx);
                }
                AssistantEvent::Failed { idx, error } => {
                    let content = format!("Ollama error: {error}");
                    if let Some(msg) = self.messages.get_mut(idx) {
                        msg.role = Role::System;
                        msg.content = content.clone();
                    } else {
                        self.messages.push(Message {
                            role: Role::System,
                            content: content.clone(),
                        });
                    }
                    self.session.record(Message {
                        role: Role::System,
                        content,
                    });
                    self.pending_idxs.retain(|&i| i != idx);
                }
            }
            self.scroll = 0;
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
    let user_msg = Message {
        role: Role::User,
        content: prompt.clone(),
    };
    app.session.record(user_msg.clone());
    app.messages.push(user_msg);
    if !prompt.is_empty()
        && app
            .input_history
            .last()
            .map(|s| s != &prompt)
            .unwrap_or(true)
    {
        app.input_history.push(prompt.clone());
    }
    app.history_idx = None;
    app.scroll = 0;
    app.input.clear();

    // Insert placeholder assistant message and spawn background generation.
    let placeholder_idx = app.messages.len();
    app.messages.push(Message {
        role: Role::Assistant,
        content: String::new(),
    });
    app.pending_idxs.push(placeholder_idx);

    let tx = app.assistant_tx.clone();
    let model = app.config.model.clone();
    let prompt_for_thread = prompt.clone();
    thread::spawn(move || {
        let mut child = match std::process::Command::new("ollama")
            .arg("run")
            .arg(&model)
            .arg(&prompt_for_thread)
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

        if let Some(mut stdout) = child.stdout.take() {
            let mut buf = [0u8; 1024];
            loop {
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
        Span::raw(" | cwd: "),
        Span::raw(cwd_display),
        Span::raw(" | repo: "),
        Span::raw(repo_display),
        Span::raw(" | pending: "),
        Span::raw(app.pending_idxs.len().to_string()).bold(),
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
    let normalized = content.replace('\r', "");
    let mut lines_out = Vec::new();

    for line in normalized.split('\n') {
        let trimmed = line.trim_end();
        if trimmed.contains('•') {
            for chunk in trimmed.split('•') {
                let part = chunk.trim();
                if part.is_empty() {
                    continue;
                }
                lines_out.push(format!("• {part}"));
            }
        } else if trimmed.starts_with("- ") {
            lines_out.push(trimmed.to_string());
        } else {
            lines_out.push(trimmed.to_string());
        }
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
