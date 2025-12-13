//! Repository detection and project type identification.

use std::path::{Path, PathBuf};
use std::fs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectType {
    Rust,
    Node,
    Python,
    Go,
    #[allow(dead_code)]
    Unknown,
}

#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub project_type: ProjectType,
    pub root: PathBuf,
    pub name: Option<String>,
    pub source_dirs: Vec<PathBuf>,
}

impl RepoInfo {
    /// Detect repository information starting from the given directory.
    pub fn detect(start: &Path) -> Option<Self> {
        // Look for Cargo.toml (Rust)
        if let Some(root) = find_manifest_root(start, "Cargo.toml") {
            return Some(Self::detect_rust(&root));
        }
        
        // Look for package.json (Node)
        if let Some(root) = find_manifest_root(start, "package.json") {
            return Some(Self::detect_node(&root));
        }
        
        // Look for pyproject.toml or setup.py (Python)
        if let Some(root) = find_manifest_root(start, "pyproject.toml") {
            return Some(Self::detect_python(&root));
        }
        if let Some(root) = find_manifest_root(start, "setup.py") {
            return Some(Self::detect_python(&root));
        }
        
        // Look for go.mod (Go)
        if let Some(root) = find_manifest_root(start, "go.mod") {
            return Some(Self::detect_go(&root));
        }
        
        None
    }
    
    fn detect_rust(root: &Path) -> Self {
        let name = parse_cargo_name(root);
        Self {
            project_type: ProjectType::Rust,
            root: root.to_path_buf(),
            name,
            source_dirs: vec![root.join("src")],
        }
    }
    
    fn detect_node(root: &Path) -> Self {
        let name = parse_package_json_name(root);
        Self {
            project_type: ProjectType::Node,
            root: root.to_path_buf(),
            name,
            source_dirs: vec![root.join("src"), root.join("lib")],
        }
    }
    
    fn detect_python(root: &Path) -> Self {
        Self {
            project_type: ProjectType::Python,
            root: root.to_path_buf(),
            name: None,
            source_dirs: vec![root.to_path_buf()],
        }
    }
    
    fn detect_go(root: &Path) -> Self {
        Self {
            project_type: ProjectType::Go,
            root: root.to_path_buf(),
            name: None,
            source_dirs: vec![root.to_path_buf()],
        }
    }
    
    /// Get the appropriate test command for this project type.
    pub fn test_command(&self) -> (&str, Vec<&str>) {
        match self.project_type {
            ProjectType::Rust => ("cargo", vec!["test"]),
            ProjectType::Node => ("npm", vec!["test"]),
            ProjectType::Python => ("pytest", vec![]),
            ProjectType::Go => ("go", vec!["test", "./..."]),
            ProjectType::Unknown => ("echo", vec!["No test command for unknown project type"]),
        }
    }
    
    /// Get the appropriate build command for this project type.
    pub fn build_command(&self) -> (&str, Vec<&str>) {
        match self.project_type {
            ProjectType::Rust => ("cargo", vec!["build"]),
            ProjectType::Node => ("npm", vec!["run", "build"]),
            ProjectType::Python => ("echo", vec!["No standard build command for Python"]),
            ProjectType::Go => ("go", vec!["build"]),
            ProjectType::Unknown => ("echo", vec!["No build command for unknown project type"]),
        }
    }
}

/// Find a manifest file by walking up the directory tree.
fn find_manifest_root(start: &Path, manifest: &str) -> Option<PathBuf> {
    let mut current = start;
    loop {
        let candidate = current.join(manifest);
        if candidate.exists() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

/// Parse project name from Cargo.toml.
fn parse_cargo_name(root: &Path) -> Option<String> {
    let cargo_toml = root.join("Cargo.toml");
    let contents = fs::read_to_string(cargo_toml).ok()?;
    
    // Simple parsing: look for name = "..." line
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("name") {
            if let Some(eq_pos) = trimmed.find('=') {
                let value = trimmed[eq_pos + 1..].trim();
                let name = value.trim_matches('"').trim_matches('\'');
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Parse project name from package.json.
fn parse_package_json_name(root: &Path) -> Option<String> {
    let package_json = root.join("package.json");
    let contents = fs::read_to_string(package_json).ok()?;
    
    // Simple JSON parsing for "name" field
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("\"name\"") {
            if let Some(colon_pos) = trimmed.find(':') {
                let value = trimmed[colon_pos + 1..].trim();
                let name = value.trim_end_matches(',').trim_matches('"');
                return Some(name.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_project_type_commands() {
        let rust_info = RepoInfo {
            project_type: ProjectType::Rust,
            root: PathBuf::from("/test"),
            name: Some("test".into()),
            source_dirs: vec![],
        };
        assert_eq!(rust_info.test_command(), ("cargo", vec!["test"]));
        assert_eq!(rust_info.build_command(), ("cargo", vec!["build"]));
    }
}

