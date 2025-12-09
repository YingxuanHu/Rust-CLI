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

    // Account for borders when calculating available width for text
    let available_width = chunks[0].width.saturating_sub(2) as usize;
    let message_lines = render_messages(view.messages, available_width);
    let available_height = chunks[0].height.saturating_sub(2) as usize; // Account for borders
    let visible = clip_lines_from_bottom(&message_lines, view.scroll, available_height);
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
    
    // Calculate how much space we have for input text (account for borders)
    let input_area_width = chunks[1].width.saturating_sub(2) as usize;
    let input_char_count = view.input.chars().count();
    
    // If input is longer than field, scroll to show the end
    let display_text = if input_char_count > input_area_width {
        // Show the last N characters that fit
        let start_char = input_char_count.saturating_sub(input_area_width);
        view.input.chars().skip(start_char).collect::<String>()
    } else {
        view.input.to_string()
    };
    
    let input = Paragraph::new(display_text.as_str()).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Input: {} | Enter to submit", mode_indicator)),
    );
    f.render_widget(input, chunks[1]);

    // Position cursor at the end of visible text
    let visible_chars = display_text.chars().count().min(input_area_width);
    let cursor_x = chunks[1].x + 1 + visible_chars as u16;
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

fn render_messages(messages: &[Message], available_width: usize) -> Vec<Line<'static>> {
    let mut rendered = Vec::new();
    for m in messages {
        let (label, color) = match m.role {
            Role::User => ("user", Color::Cyan),
            Role::Assistant => ("assistant", Color::Green),
            Role::System => ("system", Color::Yellow),
        };
        let label_text = format!("[{label}] ");
        let label_len = label_text.len();
        let body_lines = format_message_body(&m.content);

        for (i, body) in body_lines.into_iter().enumerate() {
            let prefix_len = label_len;
            let content_width = available_width.saturating_sub(prefix_len);
            
            // Wrap the body text if it's too long
            let wrapped_lines = wrap_text(&body, content_width);
            
            for (j, wrapped_line) in wrapped_lines.into_iter().enumerate() {
                if i == 0 && j == 0 {
                    // First line of first body line: show label
                    rendered.push(Line::from(vec![
                        Span::styled(label_text.clone(), Style::default().fg(color)),
                        Span::raw(wrapped_line),
                    ]));
                } else {
                    // Continuation lines: indent to match label
                    rendered.push(Line::from(vec![
                        Span::raw(" ".repeat(label_len)),
                        Span::raw(wrapped_line),
                    ]));
                }
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

fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }
    
    let mut result = Vec::new();
    let mut current_line = String::new();
    let mut current_width = 0;
    
    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        
        // If this is the first word in the line, add it regardless of length
        if current_width == 0 {
            current_line.push_str(word);
            current_width = word_len;
        } else if current_width + 1 + word_len <= max_width {
            // Add space and word
            current_line.push(' ');
            current_line.push_str(word);
            current_width += 1 + word_len;
        } else {
            // Start a new line
            result.push(current_line);
            current_line = word.to_string();
            current_width = word_len;
        }
    }
    
    // Add the last line if not empty
    if !current_line.is_empty() {
        result.push(current_line);
    }
    
    // If the input was empty or only whitespace, return at least one empty line
    if result.is_empty() {
        result.push(String::new());
    }
    
    result
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

