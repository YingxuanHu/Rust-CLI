use std::path::PathBuf;

use crate::{
    commands::{format_error, run_command, split_commit_message},
    config::Config,
    file_ops,
};

use tokio::process::Command as TokioCommand;

#[derive(Debug, Clone)]
pub struct WorkflowState {
    pub kind: WorkflowKind,
    pub repo_root: PathBuf,
}

#[derive(Debug, Clone)]
pub enum WorkflowKind {
    SaveWorkPlan,
    SaveWorkCommit { suggested: String },
    CommitOnlyConfirm { suggested: String },
    StagePlan { args: Vec<String> },
    DiffPreview { file: Option<String> },
    WriteFileConfirm {
        path: String,
        content: String,
        overwrite: bool,
    },
}

pub trait WorkflowResponder {
    fn reply(&mut self, content: impl Into<String>);
}

pub fn handle_workflow_response<R: WorkflowResponder>(
    responder: &mut R,
    workflow: WorkflowState,
    prompt: &str,
) {
    match workflow.kind {
        WorkflowKind::SaveWorkPlan => {
            handle_save_work_plan(responder, &workflow.repo_root, prompt);
        }
        WorkflowKind::SaveWorkCommit { suggested } => {
            handle_save_work_commit(responder, &workflow.repo_root, prompt, suggested);
        }
        WorkflowKind::StagePlan { args } => {
            handle_stage_plan(responder, &workflow.repo_root, prompt, args);
        }
        WorkflowKind::DiffPreview { file } => {
            handle_diff_preview(responder, &workflow.repo_root, prompt, file);
        }
        WorkflowKind::CommitOnlyConfirm { suggested } => {
            handle_commit_only_confirm(responder, &workflow.repo_root, prompt, suggested);
        }
        WorkflowKind::WriteFileConfirm {
            path,
            content,
            overwrite,
        } => {
            handle_write_file_confirm(responder, &workflow.repo_root, prompt, path, content, overwrite);
        }
    }
}

fn handle_save_work_plan<R: WorkflowResponder>(
    responder: &mut R,
    repo_root: &std::path::Path,
    prompt: &str,
) {
    let confirmed = matches!(prompt.trim().to_lowercase().as_str(), "y" | "yes");
    if !confirmed {
        responder.reply("Workflow cancelled.");
        return;
    }

    match run_command(repo_root, "git", &["add", "-A"]) {
        Ok(out) => {
            if !out.trim().is_empty() {
                responder.reply(format!("git add -A output:\n{out}"));
            }
        }
        Err(err) => {
            responder.reply(format!("git add -A failed: {}", format_error(&err)));
            return;
        }
    }

    // Note: The suggested message generation and next workflow step
    // need to be handled by the caller
}

fn handle_save_work_commit<R: WorkflowResponder>(
    responder: &mut R,
    repo_root: &std::path::Path,
    prompt: &str,
    suggested: String,
) {
    let lower = prompt.trim().to_lowercase();
    if matches!(lower.as_str(), "cancel" | "no" | "n") {
        responder.reply("Workflow cancelled.");
        return;
    }
    let commit_msg = if matches!(lower.as_str(), "yes" | "y") {
        suggested
    } else {
        prompt.trim().to_string()
    };
    run_save_work_impl(responder, repo_root, &commit_msg);
}

fn handle_stage_plan<R: WorkflowResponder>(
    responder: &mut R,
    repo_root: &std::path::Path,
    prompt: &str,
    args: Vec<String>,
) {
    if matches!(prompt.trim().to_lowercase().as_str(), "yes" | "y") {
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        match run_command(repo_root, "git", &arg_refs) {
            Ok(out) => {
                let detail = if out.trim().is_empty() {
                    "ok".to_string()
                } else {
                    out
                };
                responder.reply(format!("git {} ok\n{}", args.join(" "), detail));
            }
            Err(err) => responder.reply(format!(
                "git {} failed: {}",
                args.join(" "),
                format_error(&err)
            )),
        }
    } else {
        responder.reply("Staging cancelled.");
    }
}

fn handle_diff_preview<R: WorkflowResponder>(
    responder: &mut R,
    repo_root: &std::path::Path,
    prompt: &str,
    file: Option<String>,
) {
    let input = prompt.trim();
    if matches!(input.to_lowercase().as_str(), "cancel" | "no" | "n") {
        responder.reply("Cancelled.");
        return;
    }
    if input.eq_ignore_ascii_case("yes") || input.eq_ignore_ascii_case("y") {
        responder.reply("Ok.");
        return;
    }
    let target = if input.is_empty() {
        file.unwrap_or_default()
    } else {
        input.to_string()
    };
    if target.is_empty() {
        responder.reply("No file specified.");
        return;
    }
    let diff = run_command(repo_root, "git", &["diff", "--", &target])
        .unwrap_or_else(|e| format!("(git diff failed: {})", format_error(&e)));
    responder.reply(format!("Diff for {}:\n{}", target, diff));
}

fn handle_commit_only_confirm<R: WorkflowResponder>(
    responder: &mut R,
    repo_root: &std::path::Path,
    prompt: &str,
    suggested: String,
) {
    let lower = prompt.trim().to_lowercase();
    if matches!(lower.as_str(), "cancel" | "no" | "n") {
        responder.reply("Commit cancelled.");
        return;
    }
    let commit_msg = if matches!(lower.as_str(), "yes" | "y") {
        suggested
    } else {
        prompt.trim().to_string()
    };
    let mut logs = vec![format!("Using commit message:\n{}", commit_msg)];
    let (subject, body_lines) = split_commit_message(&commit_msg);
    let mut commit_args: Vec<String> = vec!["commit".into(), "-m".into(), subject];
    for line in body_lines {
        commit_args.push("-m".into());
        commit_args.push(line);
    }
    let commit_arg_refs: Vec<&str> = commit_args.iter().map(|s| s.as_str()).collect();
    if !run_workflow_step(
        &mut logs,
        repo_root,
        "git commit",
        "git",
        &commit_arg_refs,
    ) {
        responder.reply(logs.join("\n"));
        return;
    }
    logs.push("Commit completed (no push).".to_string());
    responder.reply(logs.join("\n"));
}

fn handle_write_file_confirm<R: WorkflowResponder>(
    responder: &mut R,
    repo_root: &std::path::Path,
    prompt: &str,
    path: String,
    content: String,
    overwrite: bool,
) {
    let lower = prompt.trim().to_lowercase();
    if !matches!(lower.as_str(), "yes" | "y") {
        responder.reply("File write cancelled.");
        return;
    }

    let target_path = PathBuf::from(&path);
    match file_ops::write_file(&target_path, &content, repo_root) {
        Ok(()) => {
            let action = if overwrite { "overwrote" } else { "created" };
            responder.reply(format!("Successfully {} file: {}", action, path));
        }
        Err(err) => {
            responder.reply(format!("Failed to write file: {}", err));
        }
    }
}

fn run_save_work_impl<R: WorkflowResponder>(
    responder: &mut R,
    repo_root: &std::path::Path,
    commit_msg: &str,
) {
    let mut logs = vec!["Running save-work workflow…".to_string()];

    logs.push(format!("Using commit message: {}", commit_msg));

    let (subject, body_lines) = split_commit_message(commit_msg);
    let mut commit_args: Vec<String> = vec!["commit".into(), "-m".into(), subject];
    for line in body_lines {
        commit_args.push("-m".into());
        commit_args.push(line);
    }
    let commit_arg_refs: Vec<&str> = commit_args.iter().map(|s| s.as_str()).collect();

    if !run_workflow_step(&mut logs, repo_root, "git commit", "git", &commit_arg_refs) {
        responder.reply(logs.join("\n"));
        return;
    }

    if !run_workflow_step(&mut logs, repo_root, "git push", "git", &["push"]) {
        responder.reply(logs.join("\n"));
        return;
    }

    logs.push("Workflow completed successfully.".to_string());
    responder.reply(logs.join("\n"));
}

fn run_workflow_step(
    logs: &mut Vec<String>,
    repo_root: &std::path::Path,
    label: &str,
    program: &str,
    args: &[&str],
) -> bool {
    match run_command(repo_root, program, args) {
        Ok(out) => {
            logs.push(format!("{label} OK\n{out}"));
            true
        }
        Err(err) => {
            logs.push(format!("{label} failed: {err}"));
            false
        }
    }
}

pub fn generate_commit_message(config: &Config, repo_root: &std::path::Path) -> Option<String> {
    if !config.generate_commit_message {
        return None;
    }
    let stat = run_command(repo_root, "git", &["diff", "--cached", "--stat"]).ok()?;
    if stat.trim().is_empty() {
        return None;
    }
    let patch = run_command(
        repo_root,
        "git",
        &["diff", "--cached", "--unified=3", "--max-count=1"],
    )
    .unwrap_or_default();
    let patch_snippet = if patch.len() > 4000 {
        format!("{}...\n[truncated]", &patch[..4000])
    } else {
        patch
    };
    let prompt = format!(
        "Generate a git commit message with:\n- Subject line in imperative mood, <=72 chars, include scope if obvious.\n- Then 1-2 bullet lines summarizing key changes (no line counts or LOC numbers; describe what changed).\nFormat exactly:\nSubject line\n- bullet\n- bullet\nAvoid filler. Staged changes (stat):\n{stat}\n\nPatch snippet:\n{patch_snippet}\n\nReturn only the formatted commit message."
    );
    let result = std::process::Command::new("ollama")
        .arg("run")
        .arg(&config.model)
        .arg(prompt)
        .output()
        .ok()?;

    if !result.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub async fn generate_commit_message_async(
    config: &Config,
    repo_root: &std::path::Path,
) -> Option<String> {
    if !config.generate_commit_message {
        return None;
    }
    let stat = run_command(repo_root, "git", &["diff", "--cached", "--stat"]).ok()?;
    if stat.trim().is_empty() {
        return None;
    }
    let patch = run_command(
        repo_root,
        "git",
        &["diff", "--cached", "--unified=3", "--max-count=1"],
    )
    .unwrap_or_default();
    let patch_snippet = if patch.len() > 4000 {
        format!("{}...\n[truncated]", &patch[..4000])
    } else {
        patch
    };
    let prompt = format!(
        "Generate a git commit message with:\n- Subject line in imperative mood, <=72 chars, include scope if obvious.\n- Then 1-2 bullet lines summarizing key changes (no line counts or LOC numbers; describe what changed).\nFormat exactly:\nSubject line\n- bullet\n- bullet\nAvoid filler. Staged changes (stat):\n{stat}\n\nPatch snippet:\n{patch_snippet}\n\nReturn only the formatted commit message."
    );
    let result = TokioCommand::new("ollama")
        .arg("run")
        .arg(&config.model)
        .arg(prompt)
        .output()
        .await
        .ok()?;

    if !result.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

