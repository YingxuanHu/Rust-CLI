use std::io::stdout;

use anyhow::{Context, Result};
use crossterm::{
    execute,
    event::{EnableMouseCapture, DisableMouseCapture},
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
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture).context("enter alternate screen")?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).context("create terminal")?;
        terminal.show_cursor().context("show cursor")?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen);
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
    pub ghost_text: Option<&'a str>,
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
                .title("Conversation"),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(log, chunks[0]);

    let mode_indicator = match view.input_mode {
        InputMode::Chat => "Chat Mode (Ctrl+S for Shell)",
        InputMode::Shell => "Shell Mode (Ctrl+S for Chat)",
    };
    
    // Build input text with ghost text
    let input_area_width = chunks[1].width.saturating_sub(2) as usize;
    let input_char_count = view.input.chars().count();
    
    // Create styled input with ghost text
    let input_line = if let Some(ghost) = view.ghost_text {
        Line::from(vec![
            Span::raw(view.input),
            Span::styled(ghost, Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(view.input)
    };
    
    let input = Paragraph::new(input_line).block(
        Block::default()
            .borders(Borders::ALL)
            .title(format!("Input: {}", mode_indicator)),
    );
    f.render_widget(input, chunks[1]);

    // Position cursor at the end of actual input (not ghost text)
    let cursor_x = chunks[1].x + 1 + input_char_count.min(input_area_width) as u16;
    let cursor_y = chunks[1].y + 1;
    f.set_cursor(cursor_x, cursor_y);

    let mut status_parts = vec![
        Span::raw(view.model).bold(),
        Span::raw(" | "),
        Span::raw(&view.cwd),
    ];
    
    // Only show pending count if there are pending responses
    if view.pending_count > 0 {
        status_parts.push(Span::raw(" | pending: "));
        status_parts.push(Span::raw(view.pending_count.to_string()).bold());
    }
    
    // Only show workflow when active
    if view.has_workflow {
        status_parts.push(Span::raw(" | "));
        status_parts.push(Span::raw("workflow: confirm").bold());
    }
    
    let status_text = Line::from(status_parts);
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
        // Only trim trailing whitespace, preserve leading indentation
        let trimmed = line.trim_end();
        
        // Check for bullet points after any leading whitespace
        if let Some(bullet_pos) = trimmed.find('•') {
            let indent = &trimmed[..bullet_pos];
            lines_out.extend(trimmed.split('•').filter_map(|chunk| {
                let part = chunk.trim();
                if part.is_empty() {
                    None
                } else {
                    Some(format!("{indent}• {part}"))
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
    
    // Detect and preserve leading whitespace
    let leading_spaces = text.len() - text.trim_start().len();
    let indent = &text[..leading_spaces];
    let content = &text[leading_spaces..];
    
    // If the text fits on one line, return it as-is
    if text.chars().count() <= max_width {
        return vec![text.to_string()];
    }
    
    let mut result = Vec::new();
    let mut current_line = String::from(indent);
    let mut current_width = leading_spaces;
    
    for word in content.split_whitespace() {
        let word_len = word.chars().count();
        
        // If this is the first word after indent
        if current_width == leading_spaces {
            current_line.push_str(word);
            current_width += word_len;
        } else if current_width + 1 + word_len <= max_width {
            // Add space and word
            current_line.push(' ');
            current_line.push_str(word);
            current_width += 1 + word_len;
        } else {
            // Start a new line with same indentation
            result.push(current_line);
            current_line = format!("{}{}", indent, word);
            current_width = leading_spaces + word_len;
        }
    }
    
    // Add the last line if not empty
    if !current_line.trim().is_empty() {
        result.push(current_line);
    }
    
    // If the input was empty or only whitespace, return at least one line
    if result.is_empty() {
        result.push(text.to_string());
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

