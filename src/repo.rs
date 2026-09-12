//! Repository detection and project type identification.

use std::fs;
use std::path::{Path, PathBuf};

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
        // Check all supported manifests at each level before moving upwards.
        // A nested frontend must not inherit a more distant Rust workspace.
        // When several manifests share one directory, retain the established
        // Rust, Node, Python, Go preference as a deterministic tie-breaker.
        for root in start.ancestors() {
            if root.join("Cargo.toml").is_file() {
                return Some(Self::detect_rust(root));
            }
            if root.join("package.json").is_file() {
                return Some(Self::detect_node(root));
            }
            if root.join("pyproject.toml").is_file() || root.join("setup.py").is_file() {
                return Some(Self::detect_python(root));
            }
            if root.join("go.mod").is_file() {
                return Some(Self::detect_go(root));
            }
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

    /// Get a supported test task, or None when no command is configured.
    pub fn test_task(&self) -> Option<(&str, Vec<&str>)> {
        match self.project_type {
            ProjectType::Rust => Some(("cargo", vec!["test"])),
            ProjectType::Node => self.node_task("test"),
            ProjectType::Python => Some(("pytest", vec![])),
            ProjectType::Go => Some(("go", vec!["test", "./..."])),
            ProjectType::Unknown => None,
        }
    }

    /// Get a supported build task, or None when no command is configured.
    pub fn build_task(&self) -> Option<(&str, Vec<&str>)> {
        match self.project_type {
            ProjectType::Rust => Some(("cargo", vec!["build"])),
            ProjectType::Node => self.node_task("build"),
            ProjectType::Go => Some(("go", vec!["build"])),
            ProjectType::Python | ProjectType::Unknown => None,
        }
    }

    fn node_task(&self, task: &'static str) -> Option<(&'static str, Vec<&'static str>)> {
        // Read on demand so edits to scripts and lockfiles take effect without
        // requiring a directory change or restart of the assistant.
        let package = read_package_json(&self.root)?;
        let script = package.get("scripts")?.get(task)?.as_str()?;
        if script.trim().is_empty() {
            return None;
        }
        let manager = node_package_manager(&self.root, &package)?;
        // `run` consistently invokes the declared script, including for Bun
        // where bare `bun test` invokes its built-in test runner instead.
        Some((manager, vec!["run", task]))
    }
}

/// Parse project name from Cargo.toml.
fn parse_cargo_name(root: &Path) -> Option<String> {
    let contents = fs::read_to_string(root.join("Cargo.toml")).ok()?;
    let manifest: toml::Value = toml::from_str(&contents).ok()?;
    let name = manifest.get("package")?.get("name")?.as_str()?;
    nonempty_name(name)
}

/// Parse project name from package.json.
fn parse_package_json_name(root: &Path) -> Option<String> {
    let package = read_package_json(root)?;
    nonempty_name(package.get("name")?.as_str()?)
}

fn nonempty_name(name: &str) -> Option<String> {
    (!name.trim().is_empty()).then(|| name.to_owned())
}

fn read_package_json(root: &Path) -> Option<serde_json::Value> {
    let contents = fs::read_to_string(root.join("package.json")).ok()?;
    serde_json::from_str(&contents).ok()
}

fn node_package_manager(root: &Path, package: &serde_json::Value) -> Option<&'static str> {
    if let Some(declared) = package.get("packageManager") {
        let (name, version) = declared.as_str()?.split_once('@')?;
        if version.is_empty() || version.chars().any(char::is_whitespace) {
            return None;
        }
        return match name {
            "npm" => Some("npm"),
            "pnpm" => Some("pnpm"),
            "yarn" => Some("yarn"),
            "bun" => Some("bun"),
            // An explicit unsupported/malformed declaration must not silently
            // select a different manager just because a stale lockfile exists.
            _ => None,
        };
    }

    let mut selected = None;
    for (manager, lockfiles) in [
        ("pnpm", &["pnpm-lock.yaml"][..]),
        ("yarn", &["yarn.lock"][..]),
        ("bun", &["bun.lock", "bun.lockb"][..]),
        ("npm", &["package-lock.json", "npm-shrinkwrap.json"][..]),
    ] {
        if lockfiles.iter().any(|name| root.join(name).is_file()) {
            if selected.is_some() {
                // Do not guess among conflicting package managers.
                return None;
            }
            selected = Some(manager);
        }
    }
    // npm remains the conventional fallback when no manager is declared and
    // no lockfile exists. Script presence has already been checked.
    Some(selected.unwrap_or("npm"))
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
        assert_eq!(rust_info.test_task(), Some(("cargo", vec!["test"])));
        assert_eq!(rust_info.build_task(), Some(("cargo", vec!["build"])));
    }

    #[test]
    fn nearest_manifest_wins_across_project_types() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[package]\nname = 'outer'").unwrap();
        let frontend = temp.path().join("frontend");
        fs::create_dir_all(frontend.join("src/components")).unwrap();
        fs::write(frontend.join("package.json"), r#"{"name":"frontend"}"#).unwrap();

        let repo = RepoInfo::detect(&frontend.join("src/components")).unwrap();
        assert_eq!(repo.project_type, ProjectType::Node);
        assert_eq!(repo.root, frontend);
        assert_eq!(repo.name.as_deref(), Some("frontend"));

        let python = repo.root.join("worker");
        fs::create_dir_all(&python).unwrap();
        fs::write(python.join("pyproject.toml"), "[project]\nname = 'worker'").unwrap();
        let repo = RepoInfo::detect(&python).unwrap();
        assert_eq!(repo.project_type, ProjectType::Python);
        assert_eq!(repo.root, python);
    }

    #[test]
    fn same_directory_manifest_preference_is_deterministic() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("Cargo.toml"), "[workspace]").unwrap();
        fs::write(temp.path().join("package.json"), "{}").unwrap();
        let repo = RepoInfo::detect(temp.path()).unwrap();
        assert_eq!(repo.project_type, ProjectType::Rust);
        assert_eq!(repo.name, None);
    }

    #[test]
    fn manifest_named_directory_is_not_a_manifest() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir(temp.path().join("Cargo.toml")).unwrap();
        fs::write(temp.path().join("package.json"), "{}").unwrap();
        assert_eq!(
            RepoInfo::detect(temp.path()).unwrap().project_type,
            ProjectType::Node
        );
    }

    #[test]
    fn cargo_name_comes_from_package_table() {
        let temp = tempfile::tempdir().unwrap();
        for (manifest, expected) in [
            (
                "[example]\nname = 'wrong'\n[package]\nname = 'right' # comment",
                Some("right"),
            ),
            (
                "package = { name = 'inline-package' }",
                Some("inline-package"),
            ),
            (
                "[workspace]\n[workspace.package]\nname = 'not-a-package'",
                None,
            ),
            ("[package]\nname = 42", None),
            ("[package]\nname = '   '", None),
            ("[package\nname = 'broken'", None),
        ] {
            fs::write(temp.path().join("Cargo.toml"), manifest).unwrap();
            assert_eq!(
                parse_cargo_name(temp.path()).as_deref(),
                expected,
                "{manifest}"
            );
        }
    }

    #[test]
    fn package_name_requires_valid_json_and_top_level_string() {
        let temp = tempfile::tempdir().unwrap();
        for (manifest, expected) in [
            (
                r#"{"name":"@scope/frontend","scripts":{"test":"vitest"}}"#,
                Some("@scope/frontend"),
            ),
            (r#"{"name":"unicode-\u263a"}"#, Some("unicode-☺")),
            (r#"{"nested":{"name":"wrong"}}"#, None),
            (r#"{"name":42}"#, None),
            (r#"{"name":""}"#, None),
            ("\"name\": \"broken\",", None),
        ] {
            fs::write(temp.path().join("package.json"), manifest).unwrap();
            assert_eq!(
                parse_package_json_name(temp.path()).as_deref(),
                expected,
                "{manifest}"
            );
        }
    }

    fn node_fixture(package: &str) -> (tempfile::TempDir, RepoInfo) {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("package.json"), package).unwrap();
        let repo = RepoInfo::detect(temp.path()).unwrap();
        (temp, repo)
    }

    #[test]
    fn explicit_package_manager_precedes_lockfiles() {
        for manager in ["npm", "pnpm", "yarn", "bun"] {
            let package = serde_json::json!({
                "packageManager": format!("{manager}@1.2.3"),
                "scripts": {"test": "test-tool", "build": "build-tool"},
            });
            let (temp, repo) = node_fixture(&package.to_string());
            fs::write(temp.path().join("pnpm-lock.yaml"), "").unwrap();
            fs::write(temp.path().join("yarn.lock"), "").unwrap();
            assert_eq!(repo.test_task(), Some((manager, vec!["run", "test"])));
            assert_eq!(repo.build_task(), Some((manager, vec!["run", "build"])));
        }
    }

    #[test]
    fn package_manager_lockfile_fallback() {
        for (lockfile, manager) in [
            ("pnpm-lock.yaml", "pnpm"),
            ("yarn.lock", "yarn"),
            ("bun.lock", "bun"),
            ("bun.lockb", "bun"),
            ("package-lock.json", "npm"),
            ("npm-shrinkwrap.json", "npm"),
        ] {
            let (temp, repo) =
                node_fixture(r#"{"scripts":{"test":"test-tool","build":"build-tool"}}"#);
            fs::write(temp.path().join(lockfile), "").unwrap();
            assert_eq!(
                repo.test_task(),
                Some((manager, vec!["run", "test"])),
                "{lockfile}"
            );
            assert_eq!(
                repo.build_task(),
                Some((manager, vec!["run", "build"])),
                "{lockfile}"
            );
        }
    }

    #[test]
    fn scripts_without_manager_or_lockfile_default_to_npm() {
        let (_temp, repo) =
            node_fixture(r#"{"scripts":{"test":"test-tool","build":"build-tool"}}"#);
        assert_eq!(repo.test_task(), Some(("npm", vec!["run", "test"])));
        assert_eq!(repo.build_task(), Some(("npm", vec!["run", "build"])));
    }

    #[test]
    fn conflicting_package_manager_lockfiles_are_not_guessed() {
        let (temp, repo) = node_fixture(r#"{"scripts":{"test":"test-tool","build":"build-tool"}}"#);
        fs::write(temp.path().join("pnpm-lock.yaml"), "").unwrap();
        fs::write(temp.path().join("package-lock.json"), "").unwrap();
        assert_eq!(repo.test_task(), None);
        assert_eq!(repo.build_task(), None);
    }

    #[test]
    fn multiple_lockfiles_for_the_same_manager_are_not_ambiguous() {
        let (temp, repo) = node_fixture(r#"{"scripts":{"test":"test-tool"}}"#);
        fs::write(temp.path().join("bun.lock"), "").unwrap();
        fs::write(temp.path().join("bun.lockb"), "").unwrap();
        assert_eq!(repo.test_task(), Some(("bun", vec!["run", "test"])));
    }

    #[test]
    fn invalid_or_unsupported_package_manager_does_not_fall_back() {
        for declared in [
            serde_json::json!("deno@2.0"),
            serde_json::json!("pnpm"),
            serde_json::json!("pnpm@"),
            serde_json::json!("npm@1 2"),
            serde_json::Value::Null,
        ] {
            let package =
                serde_json::json!({"packageManager": declared, "scripts": {"test":"test-tool"}});
            let (temp, repo) = node_fixture(&package.to_string());
            fs::write(temp.path().join("package-lock.json"), "").unwrap();
            assert_eq!(repo.test_task(), None, "{declared}");
        }
    }

    #[test]
    fn missing_invalid_and_empty_node_scripts_are_unsupported() {
        for package in [
            "{}",
            "not json",
            r#"{"scripts":{}}"#,
            r#"{"scripts":{"test":false,"build":"   "}}"#,
        ] {
            let (_temp, repo) = node_fixture(package);
            assert_eq!(repo.test_task(), None, "{package}");
            assert_eq!(repo.build_task(), None, "{package}");
        }
        let (_temp, repo) = node_fixture(r#"{"scripts":{"test":"test-tool"}}"#);
        assert!(repo.test_task().is_some());
        assert_eq!(repo.build_task(), None);
    }

    #[test]
    fn node_tasks_refresh_when_manifest_changes() {
        let (temp, repo) = node_fixture(r#"{"scripts":{"test":"test-tool"}}"#);
        assert!(repo.test_task().is_some());
        fs::write(temp.path().join("package.json"), "{}").unwrap();
        assert_eq!(repo.test_task(), None);
    }

    #[test]
    fn python_go_and_unknown_tasks_are_explicit() {
        for (project_type, test, build) in [
            (ProjectType::Python, Some(("pytest", vec![])), None),
            (
                ProjectType::Go,
                Some(("go", vec!["test", "./..."])),
                Some(("go", vec!["build"])),
            ),
            (ProjectType::Unknown, None, None),
        ] {
            let repo = RepoInfo {
                project_type,
                root: PathBuf::from("/test"),
                name: None,
                source_dirs: vec![],
            };
            assert_eq!(repo.test_task(), test);
            assert_eq!(repo.build_task(), build);
        }
    }
}
