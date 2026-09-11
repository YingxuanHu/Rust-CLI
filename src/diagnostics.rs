//! Startup and installation diagnostics for `llm_cli doctor`.

use std::{
    env,
    path::{Path, PathBuf},
};

use crate::{
    audit,
    config::Config,
    ollama::{self, OllamaStatus},
    repo::{ProjectType, RepoInfo},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Ok,
    Info,
    Warning,
    Problem,
}

impl DiagnosticLevel {
    fn symbol(self) -> &'static str {
        match self {
            Self::Ok => "✓",
            Self::Info => "•",
            Self::Warning => "!",
            Self::Problem => "✗",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub level: DiagnosticLevel,
    pub label: String,
    pub detail: String,
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DoctorReport {
    checks: Vec<Diagnostic>,
}

impl DoctorReport {
    pub fn has_blockers(&self) -> bool {
        self.checks
            .iter()
            .any(|check| matches!(check.level, DiagnosticLevel::Problem))
    }

    pub fn checks(&self) -> &[Diagnostic] {
        &self.checks
    }

    pub fn render(&self) -> String {
        let mut lines = vec!["llm_cli doctor".to_string()];
        for check in &self.checks {
            lines.push(format!(
                "{} {}: {}",
                check.level.symbol(),
                check.label,
                check.detail
            ));
            if let Some(remediation) = &check.remediation {
                lines.push(format!("  Fix: {remediation}"));
            }
        }
        if self.has_blockers() {
            lines.push("\nResult: setup needs attention before llm_cli can chat.".to_string());
        } else {
            lines.push("\nResult: core runtime checks passed.".to_string());
        }
        lines.join("\n")
    }

    fn push(
        &mut self,
        level: DiagnosticLevel,
        label: impl Into<String>,
        detail: impl Into<String>,
        remediation: Option<impl Into<String>>,
    ) {
        self.checks.push(Diagnostic {
            level,
            label: label.into(),
            detail: detail.into(),
            remediation: remediation.map(Into::into),
        });
    }
}

/// Abstraction for deterministic diagnostic tests. The system implementation
/// only reads process state; the doctor command never installs, starts, or
/// changes anything.
trait DoctorProbe {
    fn current_dir(&self) -> Result<PathBuf, String>;
    fn command_on_path(&self, program: &str) -> bool;
    fn ollama_status(&self, model: &str, host: &str) -> Result<OllamaStatus, String>;
}

struct SystemProbe;

impl DoctorProbe for SystemProbe {
    fn current_dir(&self) -> Result<PathBuf, String> {
        env::current_dir().map_err(|error| error.to_string())
    }

    fn command_on_path(&self, program: &str) -> bool {
        command_on_path(program)
    }

    fn ollama_status(&self, model: &str, host: &str) -> Result<OllamaStatus, String> {
        ollama::check_status(model, host).map_err(|error| error.to_string())
    }
}

/// Collect local prerequisite and project-tooling checks for the active
/// directory. Only the Ollama command, daemon, and configured chat model block
/// use of the assistant; project integrations remain useful warnings.
pub fn run_doctor(config: &Config, full: bool) -> DoctorReport {
    run_doctor_with_probe(config, full, &SystemProbe)
}

fn run_doctor_with_probe(config: &Config, full: bool, probe: &impl DoctorProbe) -> DoctorReport {
    let mut report = DoctorReport::default();
    let cwd = match probe.current_dir() {
        Ok(cwd) => {
            report.push(
                DiagnosticLevel::Ok,
                "working directory",
                cwd.display().to_string(),
                None::<String>,
            );
            cwd
        }
        Err(error) => {
            report.push(
                DiagnosticLevel::Problem,
                "working directory",
                error,
                Some("start llm_cli from an accessible directory"),
            );
            return report;
        }
    };

    let audit_path = audit::resolve_audit_path(&config.audit_path, &cwd);
    report.push(
        DiagnosticLevel::Info,
        "shell audit log",
        audit_path.display().to_string(),
        None::<String>,
    );

    if !probe.command_on_path("ollama") {
        report.push(
            DiagnosticLevel::Problem,
            "Ollama command",
            "not found on PATH",
            Some("install Ollama from https://ollama.com and restart the terminal"),
        );
    } else {
        check_ollama_models(&mut report, config, full, probe);
    }

    check_optional_command(
        &mut report,
        probe,
        "git",
        "Git workflows",
        "available",
        "install Git to enable status, staging, commits, and reviewed edit checks",
    );
    check_optional_command(
        &mut report,
        probe,
        "rg",
        "ripgrep",
        "available",
        "install ripgrep to enable the `find todos` command",
    );
    check_project_tooling(&mut report, probe, &cwd);
    report
}

fn check_ollama_models(
    report: &mut DoctorReport,
    config: &Config,
    full: bool,
    probe: &impl DoctorProbe,
) {
    let chat_status = match probe.ollama_status(&config.model, &config.ollama_host) {
        Ok(status) if status.reachable => status,
        Ok(status) => {
            let detail = if status.raw_output.trim().is_empty() {
                format!("daemon at {} did not respond", config.ollama_host)
            } else {
                format!("daemon at {} did not respond: {}", config.ollama_host, status.raw_output.trim())
            };
            report.push(
                DiagnosticLevel::Problem,
                "Ollama daemon",
                detail,
                Some("start it with `ollama serve` and verify LLM_CLI_OLLAMA_HOST"),
            );
            return;
        }
        Err(error) => {
            report.push(
                DiagnosticLevel::Problem,
                "Ollama daemon",
                error,
                Some("run `ollama serve` and verify the configured host"),
            );
            return;
        }
    };

    report.push(
        DiagnosticLevel::Ok,
        "Ollama daemon",
        format!("reachable at {}", config.ollama_host),
        None::<String>,
    );
    report_model(report, "chat model", &config.model, &chat_status, true);

    if full {
        for (label, model) in [
            ("embedding model", config.embedding_model.as_str()),
            ("intent classifier", config.classifier_model.as_str()),
        ] {
            match probe.ollama_status(model, &config.ollama_host) {
                Ok(status) if status.reachable => report_model(report, label, model, &status, false),
                Ok(_) => report.push(
                    DiagnosticLevel::Warning,
                    label,
                    "daemon became unreachable during the check",
                    Some("retry `llm_cli doctor --full` after Ollama is running"),
                ),
                Err(error) => report.push(
                    DiagnosticLevel::Warning,
                    label,
                    error,
                    Some("retry after restoring the Ollama connection"),
                ),
            }
        }
    } else {
        report.push(
            DiagnosticLevel::Info,
            "optional models",
            "run `llm_cli doctor --full` to check embedding and intent-classifier models",
            None::<String>,
        );
    }
}

fn report_model(
    report: &mut DoctorReport,
    label: &str,
    model: &str,
    status: &OllamaStatus,
    required: bool,
) {
    if status.has_model {
        report.push(
            DiagnosticLevel::Ok,
            label,
            format!("'{model}' is installed"),
            None::<String>,
        );
        return;
    }

    report.push(
        if required {
            DiagnosticLevel::Problem
        } else {
            DiagnosticLevel::Warning
        },
        label,
        format!("'{model}' is not installed"),
        Some(if required {
            "run `llm_cli init` to download it".to_string()
        } else {
            "run `llm_cli init --full` to download it".to_string()
        }),
    );
}

fn check_optional_command(
    report: &mut DoctorReport,
    probe: &impl DoctorProbe,
    command: &str,
    label: &str,
    available_detail: &str,
    remediation: &str,
) {
    if probe.command_on_path(command) {
        report.push(
            DiagnosticLevel::Ok,
            label,
            available_detail,
            None::<String>,
        );
    } else {
        report.push(
            DiagnosticLevel::Warning,
            label,
            "not found on PATH",
            Some(remediation),
        );
    }
}

fn check_project_tooling(report: &mut DoctorReport, probe: &impl DoctorProbe, cwd: &Path) {
    let Some(project) = RepoInfo::detect(cwd) else {
        report.push(
            DiagnosticLevel::Info,
            "project",
            "no supported project manifest found in this directory",
            None::<String>,
        );
        return;
    };

    let (tool, label) = match project.project_type {
        ProjectType::Rust => ("cargo", "Rust/Cargo"),
        ProjectType::Node => ("npm", "Node/npm"),
        ProjectType::Python => ("python3", "Python"),
        ProjectType::Go => ("go", "Go"),
        ProjectType::Unknown => return,
    };
    let name = project.name.as_deref().unwrap_or("unnamed project");
    if probe.command_on_path(tool) {
        report.push(
            DiagnosticLevel::Ok,
            format!("project: {name}"),
            format!("{} tooling is available", label),
            None::<String>,
        );
    } else {
        report.push(
            DiagnosticLevel::Warning,
            format!("project: {name}"),
            format!("{} manifest found, but `{tool}` is not on PATH", label),
            Some(format!("install {label} to enable build and test commands")),
        );
    }
}

fn command_on_path(program: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|directory| executable_candidate_exists(&directory, program))
}

fn executable_candidate_exists(directory: &Path, program: &str) -> bool {
    let candidate = directory.join(program);
    if candidate.is_file() {
        return true;
    }
    #[cfg(windows)]
    {
        return directory.join(format!("{program}.exe")).is_file();
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::{HashMap, HashSet}, path::PathBuf};

    use super::{run_doctor_with_probe, DiagnosticLevel, DoctorProbe};
    use crate::{config::Config, ollama::OllamaStatus};

    struct MockProbe {
        cwd: PathBuf,
        commands: HashSet<String>,
        statuses: HashMap<String, Result<OllamaStatus, String>>,
    }

    impl MockProbe {
        fn with_healthy_ollama(cwd: PathBuf) -> Self {
            let mut statuses = HashMap::new();
            for model in ["llama3", "nomic-embed-text", "qwen2:1.5b"] {
                statuses.insert(
                    model.to_string(),
                    Ok(OllamaStatus {
                        reachable: true,
                        has_model: true,
                        raw_output: String::new(),
                    }),
                );
            }
            Self {
                cwd,
                commands: ["ollama", "git", "rg", "cargo"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
                statuses,
            }
        }
    }

    impl DoctorProbe for MockProbe {
        fn current_dir(&self) -> Result<PathBuf, String> {
            Ok(self.cwd.clone())
        }

        fn command_on_path(&self, program: &str) -> bool {
            self.commands.contains(program)
        }

        fn ollama_status(&self, model: &str, _host: &str) -> Result<OllamaStatus, String> {
            self.statuses
                .get(model)
                .cloned()
                .unwrap_or_else(|| Err(format!("no status for {model}")))
        }
    }

    #[test]
    fn healthy_full_report_has_no_blockers() {
        let directory = tempfile::tempdir().unwrap();
        let probe = MockProbe::with_healthy_ollama(directory.path().to_path_buf());
        let report = run_doctor_with_probe(&Config::default(), true, &probe);

        assert!(!report.has_blockers());
        assert!(report.render().contains("core runtime checks passed"));
        assert!(report
            .checks()
            .iter()
            .any(|check| check.label == "embedding model" && check.level == DiagnosticLevel::Ok));
    }

    #[test]
    fn missing_chat_model_is_a_blocker_with_a_pull_command() {
        let directory = tempfile::tempdir().unwrap();
        let mut probe = MockProbe::with_healthy_ollama(directory.path().to_path_buf());
        probe.statuses.insert(
            "llama3".to_string(),
            Ok(OllamaStatus {
                reachable: true,
                has_model: false,
                raw_output: String::new(),
            }),
        );

        let report = run_doctor_with_probe(&Config::default(), false, &probe);
        assert!(report.has_blockers());
        assert!(report.render().contains("llm_cli init"));
    }

    #[test]
    fn missing_auxiliary_model_is_a_warning_in_full_mode() {
        let directory = tempfile::tempdir().unwrap();
        let mut probe = MockProbe::with_healthy_ollama(directory.path().to_path_buf());
        probe.statuses.insert(
            "nomic-embed-text".to_string(),
            Ok(OllamaStatus {
                reachable: true,
                has_model: false,
                raw_output: String::new(),
            }),
        );

        let report = run_doctor_with_probe(&Config::default(), true, &probe);
        assert!(!report.has_blockers());
        assert!(report.checks().iter().any(|check| {
            check.label == "embedding model" && check.level == DiagnosticLevel::Warning
        }));
    }

    #[test]
    fn missing_ollama_binary_is_reported_without_querying_the_daemon() {
        let directory = tempfile::tempdir().unwrap();
        let mut probe = MockProbe::with_healthy_ollama(directory.path().to_path_buf());
        probe.commands.remove("ollama");

        let report = run_doctor_with_probe(&Config::default(), false, &probe);
        assert!(report.has_blockers());
        assert!(report.render().contains("Ollama command: not found on PATH"));
    }
}
