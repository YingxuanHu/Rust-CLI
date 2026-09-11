use std::{path::PathBuf, time::Duration};

use crate::{
    command_policy::CommandAssessment,
    commands::{
        format_error, run_command_with_timeout, run_command_with_timeout_with_env,
        split_commit_message,
    },
    config::Config,
    file_ops,
    learned::LearnedAliases,
    patch::{self, AppliedPatch, PatchReview},
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
    SaveWorkMessagePending,
    SaveWorkCommit { suggested: String },
    CommitMessagePending,
    CommitOnlyConfirm { suggested: String },
    EditPatchPending { file: String },
    StagePlan { args: Vec<String> },
    DiffPreview { file: Option<String> },
    WriteFileConfirm {
        path: String,
        content: String,
        overwrite: bool,
    },
    CustomWorkflowConfirm {
        original_input: String,
        generated_cmd: String,
        save_path: PathBuf,
    },
    ChatCommandsConfirm {
        original_query: String,
        commands: Vec<String>,
        combined_command: String,
    },
    /// A direct shell command matched the high-impact policy. The command is
    /// already expanded, so confirmation applies to exactly what will run.
    ShellCommandConfirm {
        command: String,
        assessment: CommandAssessment,
    },
    ApplyDiff { review: PatchReview },
}

pub trait WorkflowResponder {
    fn reply(&mut self, content: impl Into<String>);
    /// Execute a command through the normal policy path. Existing workflows
    /// use this so generated commands still receive a risk review.
    fn execute_shell_command(&mut self, cmd: &str);
    /// Execute the exact command a user just approved after an elevated-risk
    /// prompt. This avoids asking the same confirmation twice.
    fn execute_approved_shell_command(&mut self, cmd: &str);
    fn command_timeout_secs(&self) -> u64;
    fn set_last_applied_patch(&mut self, patch: AppliedPatch);
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
        WorkflowKind::SaveWorkMessagePending => {
            responder.reply("Commit-message generation is still in progress. Please wait.");
        }
        WorkflowKind::CommitMessagePending => {
            responder.reply("Commit-message generation is still in progress. Please wait.");
        }
        WorkflowKind::EditPatchPending { file } => {
            responder.reply(format!("Generating a patch for {file} is still in progress. Please wait."));
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
        WorkflowKind::CustomWorkflowConfirm {
            original_input,
            generated_cmd,
            save_path,
        } => {
            handle_custom_workflow_confirm(responder, prompt, original_input, generated_cmd, save_path);
        }
        WorkflowKind::ChatCommandsConfirm {
            original_query,
            commands,
            combined_command,
        } => {
            handle_chat_commands_confirm(responder, &workflow.repo_root, prompt, original_query, commands, combined_command);
        }
        WorkflowKind::ShellCommandConfirm {
            command,
            assessment,
        } => {
            handle_shell_command_confirm(responder, prompt, command, assessment);
        }
        WorkflowKind::ApplyDiff { review } => {
            handle_apply_diff(responder, &workflow.repo_root, prompt, review);
        }
    }
}

fn handle_shell_command_confirm<R: WorkflowResponder>(
    responder: &mut R,
    prompt: &str,
    command: String,
    assessment: CommandAssessment,
) {
    if matches!(prompt.trim().to_ascii_lowercase().as_str(), "" | "y" | "yes") {
        responder.reply(format!(
            "Executing approved {} command.",
            assessment.risk
        ));
        responder.execute_approved_shell_command(&command);
    } else {
        responder.reply("High-impact command cancelled.");
    }
}

fn handle_save_work_plan<R: WorkflowResponder>(
    responder: &mut R,
    repo_root: &std::path::Path,
    prompt: &str,
) {
    let confirmed = matches!(prompt.trim().to_lowercase().as_str(), "" | "y" | "yes");
    if !confirmed {
        responder.reply("Workflow cancelled.");
        return;
    }

    match run_command_with_timeout(
        repo_root,
        "git",
        &["add", "-A"],
        Duration::from_secs(responder.command_timeout_secs()),
    ) {
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
    let commit_msg = if matches!(lower.as_str(), "" | "yes" | "y") {
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
    if matches!(prompt.trim().to_lowercase().as_str(), "" | "yes" | "y") {
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        match run_command_with_timeout(
            repo_root,
            "git",
            &arg_refs,
            Duration::from_secs(responder.command_timeout_secs()),
        ) {
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
    if input.is_empty() || input.eq_ignore_ascii_case("yes") || input.eq_ignore_ascii_case("y") {
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
    let diff = run_command_with_timeout(
        repo_root,
        "git",
        &["diff", "--", &target],
        Duration::from_secs(responder.command_timeout_secs()),
    )
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
    let commit_msg = if matches!(lower.as_str(), "" | "yes" | "y") {
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
        Duration::from_secs(responder.command_timeout_secs()),
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
    if !matches!(lower.as_str(), "" | "yes" | "y") {
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

fn handle_apply_diff<R: WorkflowResponder>(
    responder: &mut R,
    repo_root: &std::path::Path,
    prompt: &str,
    review: PatchReview,
) {
    let lower = prompt.trim().to_lowercase();
    
    if matches!(lower.as_str(), "cancel" | "no" | "n") {
        responder.reply("Diff application cancelled.");
        return;
    }
    
    if matches!(lower.as_str(), "" | "yes" | "y") {
        let timeout = Duration::from_secs(responder.command_timeout_secs());
        let result = patch::validate_patch_for_file(&review.patch, std::path::Path::new(&review.file))
            .and_then(|_| patch::check_patch(repo_root, &review.patch, timeout))
            .and_then(|_| patch::apply_patch(repo_root, &review.patch, timeout));
        match result {
            Ok(()) => {
                responder.set_last_applied_patch(AppliedPatch {
                    repo_root: repo_root.to_path_buf(),
                    review: review.clone(),
                });
                responder.reply(format!(
                    "Applied reviewed patch to {}. Run tests to verify it, or type `rollback last edit` to reverse this patch while the file is unchanged.",
                    review.file
                ));
            }
            Err(err) => responder.reply(format!("Patch was not applied: {err}")),
        }
        return;
    }
    
    responder.reply("Patch review is still open. Press Enter (or type 'yes') to apply, or 'no' to cancel.");
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

    if !run_workflow_step(
        &mut logs,
        repo_root,
        "git commit",
        "git",
        &commit_arg_refs,
        Duration::from_secs(responder.command_timeout_secs()),
    ) {
        responder.reply(logs.join("\n"));
        return;
    }

    if !run_workflow_step(
        &mut logs,
        repo_root,
        "git push",
        "git",
        &["push"],
        Duration::from_secs(responder.command_timeout_secs()),
    ) {
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
    timeout: Duration,
) -> bool {
    match run_command_with_timeout(repo_root, program, args, timeout) {
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
    let command_timeout = Duration::from_secs(config.cmd_timeout_secs);
    let stat = run_command_with_timeout(
        repo_root,
        "git",
        &["diff", "--cached", "--stat"],
        command_timeout,
    )
    .ok()?;
    if stat.trim().is_empty() {
        return None;
    }
    let patch = run_command_with_timeout(
        repo_root,
        "git",
        &["diff", "--cached", "--unified=3", "--max-count=1"],
        command_timeout,
    )
    .unwrap_or_default();
    let patch_snippet = truncate_for_display(&patch, 4000);
    let prompt = format!(
        "Generate a git commit message with:\n- Subject line in imperative mood, <=72 chars, include scope if obvious.\n- Then 1-2 bullet lines summarizing key changes (no line counts or LOC numbers; describe what changed).\nFormat exactly:\nSubject line\n- bullet\n- bullet\nAvoid filler. Staged changes (stat):\n{stat}\n\nPatch snippet:\n{patch_snippet}\n\nReturn only the formatted commit message."
    );
    let output = run_command_with_timeout_with_env(
        repo_root,
        "ollama",
        &["run", &config.model, &prompt],
        &[("OLLAMA_HOST", config.ollama_host.as_str())],
        Duration::from_secs(config.llm_timeout_secs),
    )
    .ok()?;
    let trimmed = output.trim();
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
    let command_timeout = Duration::from_secs(config.cmd_timeout_secs);
    let stat = run_command_with_timeout(
        repo_root,
        "git",
        &["diff", "--cached", "--stat"],
        command_timeout,
    )
    .ok()?;
    if stat.trim().is_empty() {
        return None;
    }
    let patch = run_command_with_timeout(
        repo_root,
        "git",
        &["diff", "--cached", "--unified=3", "--max-count=1"],
        command_timeout,
    )
    .unwrap_or_default();
    let patch_snippet = truncate_for_display(&patch, 4000);
    let prompt = format!(
        "Generate a git commit message with:\n- Subject line in imperative mood, <=72 chars, include scope if obvious.\n- Then 1-2 bullet lines summarizing key changes (no line counts or LOC numbers; describe what changed).\nFormat exactly:\nSubject line\n- bullet\n- bullet\nAvoid filler. Staged changes (stat):\n{stat}\n\nPatch snippet:\n{patch_snippet}\n\nReturn only the formatted commit message."
    );
    let mut command = TokioCommand::new("ollama");
    command
        .arg("run")
        .arg(&config.model)
        .arg(prompt)
        .env("OLLAMA_HOST", &config.ollama_host)
        .kill_on_drop(true);
    let result = tokio::time::timeout(
        Duration::from_secs(config.llm_timeout_secs),
        command.output(),
    )
    .await
    .ok()?
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

fn truncate_for_display(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let prefix: String = text.chars().take(max_chars).collect();
    format!("{prefix}...\n[truncated]")
}


fn handle_custom_workflow_confirm<R: WorkflowResponder>(
    responder: &mut R,
    prompt: &str,
    original_input: String,
    generated_cmd: String,
    save_path: PathBuf,
) {
    let prompt_lower = prompt.trim().to_lowercase();
    
    // Check for edit command
    if let Some(new_cmd) = prompt.trim().strip_prefix("edit:").or_else(|| prompt.trim().strip_prefix("edit ")) {
        let edited_cmd = new_cmd.trim();
        if !edited_cmd.is_empty() {
            let learned_global = save_path.parent().and_then(|p| p.parent()).map(|p| p.join("learned.toml"))
                .unwrap_or_else(|| save_path.clone());
            let learned_project = if save_path.to_string_lossy().contains(".llm-cli") {
                Some(save_path.as_path())
            } else {
                None
            };
            
            let mut learned = LearnedAliases::load(&learned_global, learned_project).unwrap_or_default();
            
            if let Err(e) = learned.save_custom_workflow(
                &original_input,
                edited_cmd,
                &save_path,
                "user_custom_edited",
            ) {
                responder.reply(format!("Failed to save custom workflow: {}", e));
            } else {
                responder.reply(format!(
                    "✓ Learned custom workflow: \"{}\" → {}\nExecuting now...",
                    original_input,
                    edited_cmd
                ));
                
                // Use execute_shell_command to properly expand handlers like {{GEN_COMMIT_MSG}}
                responder.execute_shell_command(edited_cmd);
            }
        } else {
            responder.reply("Empty command. Custom command not saved.");
        }
        return;
    }
    
    // Handle "save" option - save and execute
    if matches!(prompt_lower.as_str(), "" | "y" | "yes" | "s" | "save") {
        let learned_global = save_path.parent().and_then(|p| p.parent()).map(|p| p.join("learned.toml"))
            .unwrap_or_else(|| save_path.clone());
        let learned_project = if save_path.to_string_lossy().contains(".llm-cli") {
            Some(save_path.as_path())
        } else {
            None
        };
        
        let mut learned = LearnedAliases::load(&learned_global, learned_project).unwrap_or_default();
        
        if let Err(e) = learned.save_custom_workflow(
            &original_input,
            &generated_cmd,
            &save_path,
            "user_custom_generated",
        ) {
            responder.reply(format!("Failed to save custom workflow: {}", e));
        } else {
            responder.reply(format!(
                "✓ Learned custom workflow: \"{}\" → {}\nExecuting now...",
                original_input,
                generated_cmd
            ));
            
            // Use execute_shell_command to properly expand handlers like {{GEN_COMMIT_MSG}}
            responder.execute_shell_command(&generated_cmd);
        }
    } else if matches!(prompt_lower.as_str(), "n" | "no") {
        // Signal to show feedback prompt instead
        responder.reply("__SHOW_FEEDBACK_PROMPT__");
    } else if matches!(prompt_lower.as_str(), "cancel") {
        responder.reply("Cancelled.");
    } else {
        responder.reply("Please type [y]es to execute, [s]ave to save, [e]dit: <cmd> to edit, or [n]o to see other options.");
    }
}

fn handle_chat_commands_confirm<R: WorkflowResponder>(
    responder: &mut R,
    repo_root: &std::path::Path,
    prompt: &str,
    original_query: String,
    _commands: Vec<String>,
    combined_command: String,
) {
    let prompt_lower = prompt.trim().to_lowercase();
    let learned_global = repo_root.join(".llm-cli/learned.toml");
    let learned_project = Some(learned_global.clone());
    
    let save_custom_workflow =
        |source: &str| -> Result<(), String> {
            let mut learned = LearnedAliases::load(&learned_global, learned_project.as_deref())
                .map_err(|e| e.to_string())?;
            
            learned
                .save_custom_workflow(
                    &original_query,
                    &combined_command,
                    &learned_global,
                    source,
                )
                .map_err(|e| e.to_string())
        };
    
    // Handle "save" or "s" - save as custom workflow
    if matches!(prompt_lower.as_str(), "s" | "save") {
        if let Err(e) = save_custom_workflow("user_chat_extracted") {
            responder.reply(format!("Failed to save custom workflow: {}", e));
        } else {
            responder.reply(format!(
                "✓ Saved as custom workflow: \"{}\" → {}\nYou can now use \"{}\" directly.",
                original_query,
                combined_command,
                original_query
            ));
        }
        return;
    }
    
    // Handle "yes" or Enter - execute
    if matches!(prompt_lower.as_str(), "" | "y" | "yes") {
        match save_custom_workflow("user_chat_confirmed") {
            Ok(_) => responder.reply(format!(
                "✓ Saved and executing \"{}\" → {}\nRunning now...",
                original_query,
                combined_command
            )),
            Err(e) => responder.reply(format!(
                "Executing: {}\n(Warning: failed to save custom workflow: {})",
                combined_command,
                e
            )),
        }
        responder.execute_shell_command(&combined_command);
        return;
    }
    
    // Handle "no" or cancel
    if matches!(prompt_lower.as_str(), "n" | "no" | "cancel") {
        responder.reply("Commands not executed.");
        return;
    }
    
    // Invalid response
    responder.reply("Please type [y]es to execute, [s]ave to save as custom workflow, or [n]o to cancel.");
}

/// Extract shell commands from LLM chat response text.
/// Looks for code blocks (```bash, ```sh, ```) and inline commands after bullets.
pub fn extract_commands_from_text(text: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut in_code_block = false;
    let mut code_block_lang = None;
    
    for line in lines {
        let trimmed = line.trim();
        
        // Check for code block start
        if trimmed.starts_with("```") {
            if in_code_block {
                // End of code block
                in_code_block = false;
                code_block_lang = None;
            } else {
                // Start of code block
                in_code_block = true;
                let lang = trimmed.strip_prefix("```").unwrap_or("").trim();
                code_block_lang = if lang.is_empty() || lang == "bash" || lang == "sh" || lang == "shell" {
                    Some(lang)
                } else {
                    None
                };
            }
            continue;
        }
        
        // If we're in a relevant code block, extract the command
        if in_code_block && code_block_lang.is_some() {
            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                commands.push(trimmed.to_string());
            }
            continue;
        }
        
        // Look for commands after bullet points (common in chat responses)
        // Example: "• git checkout ." or "- git reset --hard HEAD"
        // Also handles: "• **Description**: `git command`"
        if let Some(rest) = trimmed.strip_prefix('•').or_else(|| trimmed.strip_prefix('-')) {
            let rest = rest.trim();
            
            // Check if there's a backtick-wrapped command
            if let Some(start_idx) = rest.find('`') {
                if let Some(end_idx) = rest[start_idx + 1..].find('`') {
                    let cmd = rest[start_idx + 1..start_idx + 1 + end_idx].trim();
                    if is_shell_command(cmd) {
                        commands.push(cmd.to_string());
                        continue;
                    }
                }
            }
            
            // Otherwise check if command directly follows bullet
            if is_shell_command(rest) {
                commands.push(rest.to_string());
            }
        }
    }
    
    commands
}

/// Check if a string looks like a shell command
fn is_shell_command(cmd: &str) -> bool {
    cmd.starts_with("git ")
        || cmd.starts_with("cargo ")
        || cmd.starts_with("npm ")
        || cmd.starts_with("docker ")
        || cmd.starts_with("cd ")
        || cmd.starts_with("ls ")
        || cmd.starts_with("rm ")
        || cmd.starts_with("cp ")
        || cmd.starts_with("mv ")
        || cmd.starts_with("mkdir ")
        || cmd.starts_with("chmod ")
        || cmd.starts_with("chown ")
}

#[cfg(test)]
mod tests {
    use std::{fs, path::{Path, PathBuf}, time::Duration};

    use tempfile::TempDir;

    use super::{
        handle_workflow_response, WorkflowKind, WorkflowResponder, WorkflowState,
    };
    use crate::{
        commands::run_command_with_timeout,
        patch::{AppliedPatch, PatchReview},
    };

    #[derive(Default)]
    struct TestResponder {
        replies: Vec<String>,
        executed_commands: Vec<String>,
        last_applied_patch: Option<AppliedPatch>,
    }

    impl WorkflowResponder for TestResponder {
        fn reply(&mut self, content: impl Into<String>) {
            self.replies.push(content.into());
        }

        fn execute_shell_command(&mut self, cmd: &str) {
            self.executed_commands.push(cmd.to_string());
        }

        fn execute_approved_shell_command(&mut self, cmd: &str) {
            self.executed_commands.push(cmd.to_string());
        }

        fn command_timeout_secs(&self) -> u64 {
            10
        }

        fn set_last_applied_patch(&mut self, patch: AppliedPatch) {
            self.last_applied_patch = Some(patch);
        }
    }

    fn git(repo_root: &Path, args: &[&str]) -> String {
        run_command_with_timeout(repo_root, "git", args, Duration::from_secs(10))
            .unwrap_or_else(|error| panic!("git {} failed: {error}", args.join(" ")))
    }

    fn initialized_repository() -> TempDir {
        let repo = tempfile::tempdir().expect("temporary repository");
        let root = repo.path();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.name", "Workflow Test"]);
        git(root, &["config", "user.email", "workflow-test@example.invalid"]);
        fs::write(root.join("README.md"), "# Test repository\n").expect("seed repository");
        git(root, &["add", "README.md"]);
        git(root, &["commit", "-qm", "Initialize repository"]);
        repo
    }

    #[test]
    fn stage_workflow_stages_only_requested_file_in_a_real_repository() {
        let repo = initialized_repository();
        let root = repo.path();
        fs::write(root.join("staged.txt"), "stage this\n").expect("write staged file");
        fs::write(root.join("unstaged.txt"), "leave this alone\n").expect("write unstaged file");
        let mut responder = TestResponder::default();

        handle_workflow_response(
            &mut responder,
            WorkflowState {
                kind: WorkflowKind::StagePlan {
                    args: vec!["add".to_string(), "staged.txt".to_string()],
                },
                repo_root: root.to_path_buf(),
            },
            "yes",
        );

        assert_eq!(git(root, &["diff", "--cached", "--name-only"]).trim(), "staged.txt");
        assert!(git(root, &["status", "--short"]).contains("?? unstaged.txt"));
        assert!(responder.replies.iter().any(|reply| reply.contains("git add staged.txt ok")));
    }

    #[test]
    fn commit_workflow_creates_a_local_commit_without_pushing() {
        let repo = initialized_repository();
        let root = repo.path();
        fs::write(root.join("notes.txt"), "A useful note.\n").expect("write note");
        git(root, &["add", "notes.txt"]);
        let mut responder = TestResponder::default();

        handle_workflow_response(
            &mut responder,
            WorkflowState {
                kind: WorkflowKind::CommitOnlyConfirm {
                    suggested: "Add project note\n- Record a useful note".to_string(),
                },
                repo_root: root.to_path_buf(),
            },
            "yes",
        );

        assert_eq!(git(root, &["log", "-1", "--format=%s"]).trim(), "Add project note");
        assert!(git(root, &["diff", "--cached", "--name-only"]).trim().is_empty());
        assert!(responder
            .replies
            .iter()
            .any(|reply| reply.contains("Commit completed (no push).")));
    }

    #[test]
    fn file_write_workflow_respects_confirmation() {
        let repo = initialized_repository();
        let root = repo.path();
        let mut responder = TestResponder::default();

        handle_workflow_response(
            &mut responder,
            WorkflowState {
                kind: WorkflowKind::WriteFileConfirm {
                    path: "generated.txt".to_string(),
                    content: "created by the workflow\n".to_string(),
                    overwrite: false,
                },
                repo_root: root.to_path_buf(),
            },
            "yes",
        );

        assert_eq!(
            fs::read_to_string(root.join("generated.txt")).expect("workflow output"),
            "created by the workflow\n"
        );
        assert!(responder
            .replies
            .iter()
            .any(|reply| reply.contains("Successfully created file")));
    }

    #[test]
    fn diff_workflow_applies_a_checked_patch_and_keeps_a_rollback_record() {
        let repo = initialized_repository();
        let root = repo.path();
        fs::write(root.join("notes.txt"), "before\n").expect("write source file");
        git(root, &["add", "notes.txt"]);
        git(root, &["commit", "-qm", "Add notes"]);
        let mut responder = TestResponder::default();
        let patch = "diff --git a/notes.txt b/notes.txt\n--- a/notes.txt\n+++ b/notes.txt\n@@ -1 +1 @@\n-before\n+after\n";

        handle_workflow_response(
            &mut responder,
            WorkflowState {
                kind: WorkflowKind::ApplyDiff {
                    review: PatchReview {
                        file: "notes.txt".to_string(),
                        patch: patch.to_string(),
                        description: "Change the note".to_string(),
                    },
                },
                repo_root: root.to_path_buf(),
            },
            "yes",
        );

        assert_eq!(fs::read_to_string(root.join("notes.txt")).unwrap(), "after\n");
        assert!(responder.last_applied_patch.is_some());
    }

    #[test]
    fn elevated_shell_workflow_executes_only_after_confirmation() {
        let mut responder = TestResponder::default();
        let workflow = WorkflowState {
            kind: WorkflowKind::ShellCommandConfirm {
                command: "rm -rf generated".to_string(),
                assessment: crate::command_policy::assess_shell_command("rm -rf generated"),
            },
            repo_root: PathBuf::from("."),
        };

        handle_workflow_response(&mut responder, workflow.clone(), "no");
        assert!(responder.executed_commands.is_empty());
        assert!(responder.replies.iter().any(|reply| reply.contains("cancelled")));

        handle_workflow_response(&mut responder, workflow, "yes");
        assert_eq!(
            responder.executed_commands,
            vec!["rm -rf generated".to_string()]
        );
    }
}
