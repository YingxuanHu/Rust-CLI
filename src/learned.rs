//! Learned aliases management.
//!
//! Handles loading, saving, and matching user-learned command aliases.
//! Stores learned aliases in .llm-cli/learned.toml in the project directory.
//!
//! Custom commands (user-defined shell commands) are stored separately in custom_commands.toml.

use std::{collections::HashMap, fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::intent::ParsedIntent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearnedAlias {
    pub phrase: String,
    pub tool: String,
    pub timestamp: String,
    pub source: String, // "user_feedback" or "explicit"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomCommand {
    pub phrase: String,
    pub command: String,
    pub timestamp: String,
    pub source: String, // "user_custom_generated", "user_custom_edited", "user_custom"
}

#[derive(Debug, Clone, Default)]
pub struct LearnedAliases {
    aliases: HashMap<String, LearnedAlias>,
    custom_commands: HashMap<String, CustomCommand>,
}

impl LearnedAliases {
    /// Load from global and project-local learned.toml and custom_commands.toml files.
    /// Project aliases/commands override global ones for the same phrase.
    pub fn load(global_path: &Path, project_path: Option<&Path>) -> Result<Self> {
        let mut aliases = HashMap::new();
        let mut custom_commands = HashMap::new();
        
        // Load global aliases
        if global_path.exists() {
            let data = Self::load_from_file(global_path)?;
            for alias in data.aliases {
                aliases.insert(alias.phrase.to_lowercase(), alias);
            }
        }
        
        // Load global custom commands (also check legacy macros.toml for backward compatibility)
        let global_commands_path = global_path.parent()
            .map(|p| p.join("custom_commands.toml"))
            .unwrap_or_else(|| global_path.with_file_name("custom_commands.toml"));
        let global_legacy_path = global_path.parent()
            .map(|p| p.join("macros.toml"))
            .unwrap_or_else(|| global_path.with_file_name("macros.toml"));
        
        if global_commands_path.exists() {
            let data = Self::load_custom_commands_from_file(&global_commands_path)?;
            for cmd in data.custom_commands {
                custom_commands.insert(cmd.phrase.to_lowercase(), cmd);
            }
        } else if global_legacy_path.exists() {
            // Load legacy macros.toml
            let data = Self::load_custom_commands_from_file(&global_legacy_path)?;
            for cmd in data.custom_commands {
                custom_commands.insert(cmd.phrase.to_lowercase(), cmd);
            }
        }
        
        // Load project-local aliases (overrides global)
        if let Some(proj_path) = project_path {
            if proj_path.exists() {
                let data = Self::load_from_file(proj_path)?;
                for alias in data.aliases {
                    aliases.insert(alias.phrase.to_lowercase(), alias);
                }
            }
            
            // Load project-local custom commands (overrides global)
            let proj_commands_path = proj_path.parent()
                .map(|p| p.join("custom_commands.toml"))
                .unwrap_or_else(|| proj_path.with_file_name("custom_commands.toml"));
            let proj_legacy_path = proj_path.parent()
                .map(|p| p.join("macros.toml"))
                .unwrap_or_else(|| proj_path.with_file_name("macros.toml"));
            
            if proj_commands_path.exists() {
                let data = Self::load_custom_commands_from_file(&proj_commands_path)?;
                for cmd in data.custom_commands {
                    custom_commands.insert(cmd.phrase.to_lowercase(), cmd);
                }
            } else if proj_legacy_path.exists() {
                // Load legacy macros.toml
                let data = Self::load_custom_commands_from_file(&proj_legacy_path)?;
                for cmd in data.custom_commands {
                    custom_commands.insert(cmd.phrase.to_lowercase(), cmd);
                }
            }
        }
        
        Ok(Self { aliases, custom_commands })
    }
    
    fn load_from_file(path: &Path) -> Result<LearnedData> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("reading learned aliases from {}", path.display()))?;
        toml::from_str(&contents).context("parsing learned.toml")
    }
    
    fn load_custom_commands_from_file(path: &Path) -> Result<CustomCommandsData> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("reading custom commands from {}", path.display()))?;
        
        // Try new format first, fall back to legacy "macros" format
        if let Ok(data) = toml::from_str::<CustomCommandsData>(&contents) {
            return Ok(data);
        }
        
        // Legacy format with "macros" key
        #[derive(Deserialize)]
        struct LegacyData {
            #[serde(default)]
            macros: Vec<CustomCommand>,
        }
        
        let legacy: LegacyData = toml::from_str(&contents)
            .context("parsing custom_commands.toml or macros.toml")?;
        Ok(CustomCommandsData {
            custom_commands: legacy.macros,
        })
    }
    
    /// Save a new learned alias to the specified file.
    pub fn save_alias(
        &mut self,
        phrase: &str,
        tool: &str,
        path: &Path,
        source: &str,
    ) -> Result<()> {
        use std::time::SystemTime;
        
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let alias = LearnedAlias {
            phrase: phrase.to_string(),
            tool: tool.to_string(),
            timestamp: format!("{}", now),
            source: source.to_string(),
        };
        
        self.aliases.insert(phrase.to_lowercase(), alias.clone());
        
        // Load existing file or create new
        let mut data = if path.exists() {
            Self::load_from_file(path).unwrap_or_else(|_| LearnedData { aliases: vec![] })
        } else {
            LearnedData { aliases: vec![] }
        };
        
        // Check if phrase already exists, replace if so
        data.aliases.retain(|a| a.phrase.to_lowercase() != phrase.to_lowercase());
        data.aliases.push(alias);
        
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Save
        let contents = toml::to_string_pretty(&data)?;
        fs::write(path, contents)?;
        
        Ok(())
    }
    
    /// Save a new custom command to custom_commands.toml.
    pub fn save_custom_command(
        &mut self,
        phrase: &str,
        command: &str,
        base_path: &Path,
        source: &str,
    ) -> Result<()> {
        use std::time::SystemTime;
        
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let custom_cmd = CustomCommand {
            phrase: phrase.to_string(),
            command: command.to_string(),
            timestamp: format!("{}", now),
            source: source.to_string(),
        };
        
        self.custom_commands.insert(phrase.to_lowercase(), custom_cmd.clone());
        
        // Determine custom_commands.toml path
        let commands_path = if base_path.file_name().map_or(false, |n| n == "learned.toml") {
            base_path.parent()
                .map(|p| p.join("custom_commands.toml"))
                .unwrap_or_else(|| base_path.with_file_name("custom_commands.toml"))
        } else {
            base_path.join("custom_commands.toml")
        };
        
        // Load existing file or create new
        let mut data = if commands_path.exists() {
            Self::load_custom_commands_from_file(&commands_path).unwrap_or_else(|_| CustomCommandsData { custom_commands: vec![] })
        } else {
            CustomCommandsData { custom_commands: vec![] }
        };
        
        // Check if phrase already exists, replace if so
        data.custom_commands.retain(|m| m.phrase.to_lowercase() != phrase.to_lowercase());
        data.custom_commands.push(custom_cmd);
        
        // Ensure parent directory exists
        if let Some(parent) = commands_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Save
        let contents = toml::to_string_pretty(&data)?;
        fs::write(commands_path, contents)?;
        
        Ok(())
    }
    
    /// Try to match a phrase against learned aliases and custom commands.
    /// Custom commands are checked first (higher priority).
    pub fn match_phrase(&self, phrase: &str) -> Option<ParsedIntent> {
        let phrase_lower = phrase.to_lowercase();
        
        // Check custom commands first
        if let Some(custom_cmd) = self.custom_commands.get(&phrase_lower) {
            let mut intent = ParsedIntent::new("shell", 1.0);
            intent.args.command = Some(custom_cmd.command.clone());
            return Some(intent);
        }
        
        // Check aliases
        self.aliases.get(&phrase_lower).map(|alias| {
            ParsedIntent::new(&alias.tool, 1.0)
        })
    }
    
}

#[derive(Debug, Serialize, Deserialize)]
struct LearnedData {
    #[serde(default)]
    aliases: Vec<LearnedAlias>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CustomCommandsData {
    #[serde(default)]
    custom_commands: Vec<CustomCommand>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_save_and_load() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path();
        
        let mut learned = LearnedAliases::default();
        learned.save_alias("yeet", "save_work", path, "test").unwrap();
        
        let loaded = LearnedAliases::load(path, None).unwrap();
        assert!(loaded.match_phrase("yeet").is_some());
    }

    #[test]
    fn test_case_insensitive() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path();
        
        let mut learned = LearnedAliases::default();
        learned.save_alias("YeEt", "save_work", path, "test").unwrap();
        
        let loaded = LearnedAliases::load(path, None).unwrap();
        assert!(loaded.match_phrase("yeet").is_some());
        assert!(loaded.match_phrase("YEET").is_some());
    }
}

