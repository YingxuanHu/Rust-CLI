//! Learned aliases management.
//!
//! Handles loading, saving, and matching user-learned command aliases.
//! Stores learned aliases in .llm-cli/learned.toml in the project directory.
//!
//! Custom workflows (user-defined shell command sequences) are stored separately in custom_workflows.toml.

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
pub struct CustomWorkflow {
    pub phrase: String,
    pub command: String,
    pub timestamp: String,
    pub source: String, // "user_custom_generated", "user_custom_edited", "user_custom"
}

#[derive(Debug, Clone, Default)]
pub struct LearnedAliases {
    aliases: HashMap<String, LearnedAlias>,
    custom_workflows: HashMap<String, CustomWorkflow>,
}

impl LearnedAliases {
    /// Load from global and project-local learned.toml and custom_workflows.toml files.
    /// Project aliases/workflows override global ones for the same phrase.
    pub fn load(global_path: &Path, project_path: Option<&Path>) -> Result<Self> {
        let mut aliases = HashMap::new();
        let mut custom_workflows = HashMap::new();
        
        // Load global aliases
        if global_path.exists() {
            let data = Self::load_from_file(global_path)?;
            for alias in data.aliases {
                aliases.insert(alias.phrase.to_lowercase(), alias);
            }
        }
        
        // Load global custom workflows (also check legacy files for backward compatibility)
        let global_workflows_path = global_path.parent()
            .map(|p| p.join("custom_workflows.toml"))
            .unwrap_or_else(|| global_path.with_file_name("custom_workflows.toml"));
        let global_legacy_commands_path = global_path.parent()
            .map(|p| p.join("custom_commands.toml"))
            .unwrap_or_else(|| global_path.with_file_name("custom_commands.toml"));
        let global_legacy_path = global_path.parent()
            .map(|p| p.join("macros.toml"))
            .unwrap_or_else(|| global_path.with_file_name("macros.toml"));
        
        if global_workflows_path.exists() {
            let data = Self::load_custom_workflows_from_file(&global_workflows_path)?;
            for wf in data.custom_workflows {
                custom_workflows.insert(wf.phrase.to_lowercase(), wf);
            }
        } else if global_legacy_commands_path.exists() {
            let data = Self::load_custom_workflows_from_file(&global_legacy_commands_path)?;
            for wf in data.custom_workflows {
                custom_workflows.insert(wf.phrase.to_lowercase(), wf);
            }
        } else if global_legacy_path.exists() {
            // Load legacy macros.toml
            let data = Self::load_custom_workflows_from_file(&global_legacy_path)?;
            for wf in data.custom_workflows {
                custom_workflows.insert(wf.phrase.to_lowercase(), wf);
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
            
            // Load project-local custom workflows (overrides global)
            let proj_workflows_path = proj_path.parent()
                .map(|p| p.join("custom_workflows.toml"))
                .unwrap_or_else(|| proj_path.with_file_name("custom_workflows.toml"));
            let proj_legacy_commands_path = proj_path.parent()
                .map(|p| p.join("custom_commands.toml"))
                .unwrap_or_else(|| proj_path.with_file_name("custom_commands.toml"));
            let proj_legacy_path = proj_path.parent()
                .map(|p| p.join("macros.toml"))
                .unwrap_or_else(|| proj_path.with_file_name("macros.toml"));
            
            if proj_workflows_path.exists() {
                let data = Self::load_custom_workflows_from_file(&proj_workflows_path)?;
                for wf in data.custom_workflows {
                    custom_workflows.insert(wf.phrase.to_lowercase(), wf);
                }
            } else if proj_legacy_commands_path.exists() {
                let data = Self::load_custom_workflows_from_file(&proj_legacy_commands_path)?;
                for wf in data.custom_workflows {
                    custom_workflows.insert(wf.phrase.to_lowercase(), wf);
                }
            } else if proj_legacy_path.exists() {
                // Load legacy macros.toml
                let data = Self::load_custom_workflows_from_file(&proj_legacy_path)?;
                for wf in data.custom_workflows {
                    custom_workflows.insert(wf.phrase.to_lowercase(), wf);
                }
            }
        }
        
        Ok(Self { aliases, custom_workflows })
    }
    
    fn load_from_file(path: &Path) -> Result<LearnedData> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("reading learned aliases from {}", path.display()))?;
        toml::from_str(&contents).context("parsing learned.toml")
    }
    
    fn load_custom_workflows_from_file(path: &Path) -> Result<CustomWorkflowsData> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("reading custom workflows from {}", path.display()))?;
        
        // Try new format first, fall back to legacy "macros" format
        if let Ok(data) = toml::from_str::<CustomWorkflowsData>(&contents) {
            return Ok(data);
        }
        
        // Legacy format with "macros" key
        #[derive(Deserialize)]
        struct LegacyData {
            #[serde(default)]
            macros: Vec<CustomWorkflow>,
        }
        
        let legacy: LegacyData = toml::from_str(&contents)
            .context("parsing custom_workflows.toml or macros.toml")?;
        Ok(CustomWorkflowsData {
            custom_workflows: legacy.macros,
        })
    }
    
    /// Save a new learned alias to the specified file.
    #[allow(dead_code)]
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
    
    /// Save a new custom workflow to custom_workflows.toml.
    pub fn save_custom_workflow(
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
        
        let custom_wf = CustomWorkflow {
            phrase: phrase.to_string(),
            command: command.to_string(),
            timestamp: format!("{}", now),
            source: source.to_string(),
        };
        
        self.custom_workflows.insert(phrase.to_lowercase(), custom_wf.clone());
        
        // Determine custom_workflows.toml path
        let workflows_path = if base_path.file_name().map_or(false, |n| n == "learned.toml") {
            base_path.parent()
                .map(|p| p.join("custom_workflows.toml"))
                .unwrap_or_else(|| base_path.with_file_name("custom_workflows.toml"))
        } else {
            base_path.join("custom_workflows.toml")
        };
        
        let legacy_commands_path = workflows_path.with_file_name("custom_commands.toml");
        
        // Load existing file or create new
        let mut data = if workflows_path.exists() {
            Self::load_custom_workflows_from_file(&workflows_path).unwrap_or_else(|_| CustomWorkflowsData { custom_workflows: vec![] })
        } else if legacy_commands_path.exists() {
            Self::load_custom_workflows_from_file(&legacy_commands_path).unwrap_or_else(|_| CustomWorkflowsData { custom_workflows: vec![] })
        } else {
            CustomWorkflowsData { custom_workflows: vec![] }
        };
        
        // Check if phrase already exists, replace if so
        data.custom_workflows.retain(|m| m.phrase.to_lowercase() != phrase.to_lowercase());
        data.custom_workflows.push(custom_wf);
        
        // Ensure parent directory exists
        if let Some(parent) = workflows_path.parent() {
            fs::create_dir_all(parent)?;
        }
        
        // Save
        let contents = toml::to_string_pretty(&data)?;
        fs::write(workflows_path, contents)?;
        
        Ok(())
    }
    
    /// Try to match a phrase against learned aliases and custom workflows.
    /// Custom workflows are checked first (higher priority).
    pub fn match_phrase(&self, phrase: &str) -> Option<ParsedIntent> {
        let phrase_lower = phrase.to_lowercase();
        
        // Check custom workflows first
        if let Some(custom_wf) = self.custom_workflows.get(&phrase_lower) {
            let mut intent = ParsedIntent::new("shell", 1.0);
            intent.args.command = Some(custom_wf.command.clone());
            return Some(intent);
        }
        
        // Check aliases
        self.aliases.get(&phrase_lower).map(|alias| {
            ParsedIntent::new(&alias.tool, 1.0)
        })
    }
    
    /// Get all learned phrases for autocompletion.
    pub fn get_all_phrases(&self) -> Vec<String> {
        let mut phrases = Vec::new();
        
        for alias in self.aliases.values() {
            phrases.push(alias.phrase.clone());
        }
        
        for custom_wf in self.custom_workflows.values() {
            phrases.push(custom_wf.phrase.clone());
        }
        
        phrases
    }
    
}

#[derive(Debug, Serialize, Deserialize)]
struct LearnedData {
    #[serde(default)]
    aliases: Vec<LearnedAlias>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CustomWorkflowsData {
    #[serde(default, alias = "custom_commands")]
    custom_workflows: Vec<CustomWorkflow>,
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

    #[test]
    fn test_custom_workflow_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let learned_path = temp_dir.path().join("learned.toml");
        
        let mut learned = LearnedAliases::default();
        learned
            .save_custom_workflow(
                "discard unstaged changes",
                "git reset --hard HEAD",
                &learned_path,
                "test",
            )
            .unwrap();
        
        let loaded = LearnedAliases::load(&learned_path, Some(&learned_path)).unwrap();
        let intent = loaded
            .match_phrase("discard unstaged changes")
            .expect("custom workflow should be matched");
        assert_eq!(intent.tool, "shell");
        assert_eq!(intent.args.command.as_deref(), Some("git reset --hard HEAD"));
    }
}
