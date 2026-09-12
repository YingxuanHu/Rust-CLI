//! Autocompletion provider for the CLI.
//!
//! Provides ghost-text completion with smart path detection.

use std::path::Path;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use crate::{frecency::FrecencyTracker, learned::LearnedAliases, tools::TOOLS};

pub struct CompletionProvider {
    tool_examples: Vec<String>,
    matcher: SkimMatcherV2,
}

impl CompletionProvider {
    pub fn new() -> Self {
        // Collect all tool examples
        let mut tool_examples = Vec::new();
        for tool in TOOLS {
            for example in tool.examples {
                tool_examples.push(example.to_string());
            }
        }
        
        Self {
            tool_examples,
            matcher: SkimMatcherV2::default(),
        }
    }
    
    /// Get the best completion for ghost text display.
    /// Returns the suffix to append after the current input.
    pub fn get_ghost_completion(
        &self,
        input: &str,
        cwd: &Path,
        history: &[String],
        learned: &LearnedAliases,
        frecency: &FrecencyTracker,
    ) -> Option<String> {
        if input.is_empty() {
            return None;
        }
        
        // Extract last word
        let last_word = input.split_whitespace().last().unwrap_or(input);
        
        // Check if last word is path-like
        if is_path_like(last_word, cwd) {
            if let Some(path_completion) = complete_path(last_word, cwd, frecency) {
                // Return only the suffix beyond the last word
                if let Some(suffix) = path_completion.strip_prefix(last_word) {
                    if !suffix.is_empty() {
                        return Some(suffix.to_string());
                    }
                }
            }
        }
        
        // Full input fuzzy matching
        let best_match = self.get_best_match(input, history, learned, frecency)?;
        
        // Return only the suffix
        if best_match.len() > input.len() && best_match.starts_with(input) {
            Some(best_match[input.len()..].to_string())
        } else {
            None
        }
    }
    
    fn get_best_match(
        &self,
        prefix: &str,
        history: &[String],
        learned: &LearnedAliases,
        frecency: &FrecencyTracker,
    ) -> Option<String> {
        let mut candidates = Vec::new();
        
        // Check tool examples
        for example in &self.tool_examples {
            if let Some(fuzzy_score) = self.matcher.fuzzy_match(example, prefix) {
                let frecency_score = frecency.get_command_score(example);
                let combined = fuzzy_score as f64 + (frecency_score * 10.0); // Boost frecency
                candidates.push((combined, example.clone()));
            }
        }
        
        // Check history
        for entry in history.iter().rev().take(50) {
            let display = entry
                .trim()
                .strip_prefix('$')
                .or_else(|| entry.trim().strip_prefix('!'))
                .map(|s| s.trim())
                .unwrap_or(entry);
            
            if let Some(fuzzy_score) = self.matcher.fuzzy_match(display, prefix) {
                let frecency_score = frecency.get_command_score(display);
                let combined = fuzzy_score as f64 + (frecency_score * 10.0);
                candidates.push((combined, display.to_string()));
            }
        }
        
        // Check learned aliases
        for alias in learned.get_all_phrases() {
            if let Some(fuzzy_score) = self.matcher.fuzzy_match(&alias, prefix) {
                let frecency_score = frecency.get_command_score(&alias);
                let combined = fuzzy_score as f64 + (frecency_score * 10.0);
                candidates.push((combined, alias));
            }
        }
        
        // Sort by combined score and return best
        candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        candidates.first().map(|(_, cmd)| cmd.clone())
    }
}

/// Check if a word looks like a path.
fn is_path_like(word: &str, cwd: &Path) -> bool {
    // Contains path separator
    if word.contains('/') || word.contains('\\') {
        return true;
    }
    
    // Starts with special path prefixes
    if word.starts_with('.') || word.starts_with('~') {
        return true;
    }
    
    // Matches existing file or directory in cwd
    if cwd.join(word).exists() {
        return true;
    }
    
    false
}

/// Complete a path by listing matching files/folders.
/// Uses frecency scoring to prioritize frequently/recently accessed files.
fn complete_path(partial: &str, cwd: &Path, frecency: &FrecencyTracker) -> Option<String> {
    complete_path_with_home(partial, cwd, frecency, dirs::home_dir().as_deref())
}

fn complete_path_with_home(
    partial: &str,
    cwd: &Path,
    frecency: &FrecencyTracker,
    home: Option<&Path>,
) -> Option<String> {
    // Handle relative paths
    let path_str = if partial.starts_with('~') {
        // Expand home directory
        if partial != "~" && !partial.starts_with("~/") && !partial.starts_with("~\\") {
            // Resolving another user's home is not supported by this completer.
            return None;
        }
        partial.replacen('~', &home?.to_string_lossy(), 1)
    } else {
        partial.to_string()
    };
    
    let path = Path::new(&path_str);
    
    // Check if the partial path itself is a complete directory
    let resolved_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    
    if resolved_path.is_dir() && !partial.ends_with(['/', '\\']) {
        // Complete directory by adding trailing slash
        return Some(format!("{}/", partial));
    }
    
    // Split into directory and filename prefix
    let (dir, prefix) = if path_str.ends_with('/') || path_str.ends_with('\\') {
        // Preserve filesystem roots such as `/` while listing the directory.
        (path, "")
    } else if let Some(parent) = path.parent() {
        (parent, path.file_name()?.to_str()?)
    } else {
        (Path::new("."), path_str.as_str())
    };
    
    // Resolve directory relative to cwd
    let search_dir = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        cwd.join(dir)
    };
    
    if !search_dir.exists() {
        return None;
    }
    
    // Collect matching entries and sort by frecency score
    if let Ok(entries) = std::fs::read_dir(&search_dir) {
        let mut candidates = Vec::new();
        
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(prefix) {
                    // Keep the user's spelling (`~/`, `./`, or an absolute
                    // path), expanding home only for filesystem lookup.
                    let completion = format!("{partial}{}", &name[prefix.len()..]);
                    
                    // Get frecency score (higher = more frequently/recently used)
                    let score = frecency.get_file_score(&completion);
                    let is_dir = entry.path().is_dir();
                    
                    candidates.push((score, completion, is_dir));
                }
            }
        }
        
        // Sort by frecency score (highest first), then alphabetically
        candidates.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap()
                .then_with(|| a.1.cmp(&b.1))
        });
        
        // Return best match
        if let Some((_, completion, is_dir)) = candidates.first() {
            if *is_dir {
                return Some(format!("{}/", completion));
            } else {
                return Some(completion.clone());
            }
        }
    }
    
    None
}

impl Default for CompletionProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use super::{complete_path_with_home, CompletionProvider};
    use crate::{frecency::FrecencyTracker, learned::LearnedAliases};

    #[test]
    fn home_completion_keeps_the_typed_tilde_prefix() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path().join("home");
        fs::create_dir_all(home.join("Documents")).unwrap();
        fs::write(home.join("résumé.txt"), "").unwrap();
        let frecency = FrecencyTracker::load(&directory.path().join("frecency.toml"));

        assert_eq!(
            complete_path_with_home("~/Doc", directory.path(), &frecency, Some(&home)),
            Some("~/Documents/".to_string())
        );
        assert_eq!(
            complete_path_with_home("~/ré", directory.path(), &frecency, Some(&home)),
            Some("~/résumé.txt".to_string())
        );
        assert_eq!(
            complete_path_with_home("~", directory.path(), &frecency, Some(&home)),
            Some("~/".to_string())
        );
        assert_eq!(
            complete_path_with_home("~someone/Doc", directory.path(), &frecency, Some(&home)),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn root_completion_looks_in_root_instead_of_the_working_directory() {
        let directory = tempfile::tempdir().unwrap();
        // An empty working directory makes the old root-to-cwd fallback fail.
        let frecency = FrecencyTracker::load(&directory.path().join("frecency.toml"));
        let completed = complete_path_with_home("/", directory.path(), &frecency, None)
            .expect("filesystem root should contain an entry");

        assert!(completed.starts_with('/'));
        // Root may contain a dangling symlink (for example .VolumeIcon.icns on
        // macOS), which is still a real completion candidate.
        assert!(fs::symlink_metadata(&completed).is_ok());
        assert_eq!(
            Path::new(completed.trim_end_matches('/')).parent(),
            Some(Path::new("/"))
        );
    }

    #[test]
    fn relative_unicode_paths_produce_only_the_remaining_suffix() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("src")).unwrap();
        fs::write(directory.path().join("src/résumé.rs"), "").unwrap();
        let frecency = FrecencyTracker::load(&directory.path().join("frecency.toml"));
        let provider = CompletionProvider::new();

        assert_eq!(
            provider.get_ghost_completion(
                "show ./src/ré",
                directory.path(),
                &[],
                &LearnedAliases::default(),
                &frecency,
            ),
            Some("sumé.rs".to_string())
        );
    }
}
