use crate::ui::InputMode;

/// Check if a history entry is a shell command.
pub fn is_shell_command(entry: &str) -> bool {
    let trimmed = entry.trim();
    trimmed.starts_with('$') || trimmed.starts_with('!')
}

/// Expand bash-style bang shortcuts.
/// Returns Some(expanded_command) if a shortcut was matched, None otherwise.
pub fn expand_bang_shortcut(history: &[String], input: &str) -> Option<String> {
    let trimmed = input.trim();

    // !! - repeat last shell command
    if trimmed == "!!" {
        return history
            .iter()
            .rev()
            .find(|e| is_shell_command(e))
            .cloned();
    }

    // !prefix - find last command starting with prefix (after the !)
    if trimmed.starts_with('!') && trimmed.len() > 1 && !trimmed.starts_with("! ") {
        let prefix = &trimmed[1..];
        // Look for shell commands whose command part starts with prefix
        for entry in history.iter().rev() {
            if is_shell_command(entry) {
                // Extract the command part after $ or !
                let cmd_part = entry
                    .trim()
                    .strip_prefix('$')
                    .or_else(|| entry.trim().strip_prefix('!'))
                    .map(|s| s.trim())
                    .unwrap_or("");
                if cmd_part.starts_with(prefix) {
                    return Some(entry.clone());
                }
            }
        }
    }

    None
}

pub struct HistoryNavigation<'a> {
    pub history: &'a [String],
    pub input_mode: InputMode,
}

impl<'a> HistoryNavigation<'a> {
    pub fn new(history: &'a [String], input_mode: InputMode) -> Self {
        Self {
            history,
            input_mode,
        }
    }

    /// Get the previous history entry matching the current mode.
    pub fn get_prev(&self, current_idx: Option<usize>) -> Option<(usize, String)> {
        if self.history.is_empty() {
            return None;
        }

        let filter_by_mode = self.input_mode == InputMode::Shell;
        let start = current_idx
            .map(|i| i.saturating_sub(1))
            .unwrap_or(self.history.len().saturating_sub(1));

        // Search backwards for a matching entry
        for i in (0..=start).rev() {
            if let Some(entry) = self.history.get(i) {
                let is_shell = is_shell_command(entry);
                // In Shell mode, show only shell commands; in Chat mode, show only non-shell
                let matches_mode = if filter_by_mode { is_shell } else { !is_shell };
                if matches_mode {
                    // Strip $ prefix in Shell mode for cleaner display
                    let display = if filter_by_mode {
                        entry
                            .trim()
                            .strip_prefix('$')
                            .or_else(|| entry.trim().strip_prefix('!'))
                            .map(|s| s.trim().to_string())
                            .unwrap_or_else(|| entry.clone())
                    } else {
                        entry.clone()
                    };
                    return Some((i, display));
                }
            }
        }
        None
    }

    /// Get the next history entry matching the current mode.
    pub fn get_next(&self, current_idx: Option<usize>) -> Option<(usize, String)> {
        if self.history.is_empty() {
            return None;
        }

        let current_idx = current_idx?;
        let filter_by_mode = self.input_mode == InputMode::Shell;
        let start = current_idx + 1;

        // Search forwards for a matching entry
        for i in start..self.history.len() {
            if let Some(entry) = self.history.get(i) {
                let is_shell = is_shell_command(entry);
                // In Shell mode, show only shell commands; in Chat mode, show only non-shell
                let matches_mode = if filter_by_mode { is_shell } else { !is_shell };
                if matches_mode {
                    // Strip $ prefix in Shell mode for cleaner display
                    let display = if filter_by_mode {
                        entry
                            .trim()
                            .strip_prefix('$')
                            .or_else(|| entry.trim().strip_prefix('!'))
                            .map(|s| s.trim().to_string())
                            .unwrap_or_else(|| entry.clone())
                    } else {
                        entry.clone()
                    };
                    return Some((i, display));
                }
            }
        }

        None
    }
}

