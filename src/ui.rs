use std::io::stdout;

use anyhow::{Context, Result};
use crossterm::{
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
    session::{Message, Role},
};

pub struct TerminalGuard {
    pub terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
}

impl TerminalGuard {
    pub fn new() -> Result<Self> {
        enable_raw_mode().context("enable raw mode")?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).context("create terminal")?;
        terminal.show_cursor().context("show cursor")?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Chat,
    Shell,
}

pub struct AppView<'a> {
    pub messages: &'a [Message],
    pub scroll: usize,
    pub input: &'a str,
    pub input_mode: InputMode,
    pub model: &'a str,
    pub streaming: bool,
    pub cwd: String,
    pub repo_root: Option<String>,
    pub pending_count: usize,
    pub has_workflow: bool,
}

pub fn render_ui(f: &mut ratatui::Frame, view: AppView) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.size());

    let message_lines = render_messages(view.messages);
    let visible = clip_lines_from_bottom(&message_lines, view.scroll, chunks[0].height as usize);
    let log = Paragraph::new(visible)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Conversation (PgUp/PgDn scroll)"),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(log, chunks[0]);

    let mode_indicator = match view.input_mode {
        InputMode::Chat => "Chat Mode (Ctrl+S for Shell)",
        InputMode::Shell => "Shell Mode (Ctrl+S for Chat)",
    };
    let input = Paragraph::new(view.input).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Input: {} | Enter to submit", mode_indicator)),
    );
    f.render_widget(input, chunks[1]);

    // Position cursor at the end of input text
    let cursor_x = chunks[1].x + view.input.len() as u16 + 1;
    let cursor_y = chunks[1].y + 1;
    f.set_cursor(cursor_x, cursor_y);

    let status_text = Line::from(vec![
        Span::raw("model: "),
        Span::raw(view.model).bold(),
        Span::raw(" | streaming: "),
        Span::raw(if view.streaming { "on" } else { "off" }).bold(),
        Span::raw(" | style: bullets "),
        Span::raw(" | cwd: "),
        Span::raw(&view.cwd),
        Span::raw(" | repo: "),
        Span::raw(view.repo_root.as_deref().unwrap_or("-")),
        Span::raw(" | pending: "),
        Span::raw(view.pending_count.to_string()).bold(),
        Span::raw(" | workflow: "),
        Span::raw(if view.has_workflow {
            "confirm"
        } else {
            "-"
        })
        .bold(),
        Span::raw(" | mode: Ctrl+S | history: ↑/↓ | scroll: PgUp/PgDn | quit: Esc/q"),
    ]);
    let status = Paragraph::new(status_text);
    f.render_widget(status, chunks[2]);
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

