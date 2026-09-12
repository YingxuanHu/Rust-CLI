use std::{path::PathBuf, time::Duration};

use crate::{
    command_policy::CommandAssessment,
    commands::split_commit_message,
    config::Config,
    file_ops,
    learned::LearnedAliases,
    patch::{self, AppliedPatch, PatchReview},
    task_runner::{self, TaskId, TaskOutcome, TaskSpec},
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
    SaveWorkCommit {
        suggested: String,
    },
    CommitMessagePending,
    CommitOnlyConfirm {
        suggested: String,
    },
    /// A successful save-work commit is local until this separate review is
    /// explicitly accepted. Cancellation never implicitly publishes it.
    PushConfirm,
    EditPatchPending {
        file: String,
    },
    StagePlan {
        args: Vec<String>,
    },
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
    ApplyDiff {
        review: PatchReview,
    },
}

/// The UI owns task completion and must only advance a continuation after a
/// successful, uncancelled task in the request's original working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitContinuation {
    None,
    SaveWorkStaged,
    OfferPush,
    SavePreview,
    StatusSummary,
    CommitPreview { save_work: bool },
}

pub trait WorkflowResponder {
    fn reply(&mut self, content: impl Into<String>);
    /// Execute a command through the normal policy path. Existing workflows
    /// use this so generated commands still receive a risk review.
    fn execute_shell_command(&mut self, cmd: &str);
    /// Execute the exact command a user just approved after an elevated-risk
    /// prompt. This avoids asking the same confirmation twice.
    fn execute_approved_shell_command(&mut self, cmd: &str);
    fn start_git_task(
        &mut self,
        repo_root: PathBuf,
        label: &str,
        args: Vec<String>,
        continuation: GitContinuation,
    );
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
            responder.reply(format!(
                "Generating a patch for {file} is still in progress. Please wait."
            ));
        }
        WorkflowKind::SaveWorkCommit { suggested } => {
            handle_save_work_commit(responder, &workflow.repo_root, prompt, suggested);
        }
        WorkflowKind::StagePlan { args } => {
            handle_stage_plan(responder, &workflow.repo_root, prompt, args);
        }
        WorkflowKind::CommitOnlyConfirm { suggested } => {
            handle_commit_only_confirm(responder, &workflow.repo_root, prompt, suggested);
        }
        WorkflowKind::PushConfirm => {
            if matches!(prompt.trim().to_ascii_lowercase().as_str(), "yes" | "y") {
                responder.start_git_task(
                    workflow.repo_root,
                    "Git push",
                    vec!["push".into()],
                    GitContinuation::None,
                );
            } else {
                responder
                    .reply("Push cancelled. Your commit remains local; nothing was published.");
            }
        }
        WorkflowKind::WriteFileConfirm {
            path,
            content,
            overwrite,
        } => {
            handle_write_file_confirm(
                responder,
                &workflow.repo_root,
                prompt,
                path,
                content,
                overwrite,
            );
        }
        WorkflowKind::CustomWorkflowConfirm {
            original_input,
            generated_cmd,
            save_path,
        } => {
            handle_custom_workflow_confirm(
                responder,
                prompt,
                original_input,
                generated_cmd,
                save_path,
            );
        }
        WorkflowKind::ChatCommandsConfirm {
            original_query,
            commands,
            combined_command,
        } => {
            handle_chat_commands_confirm(
                responder,
                &workflow.repo_root,
                prompt,
                original_query,
                commands,
                combined_command,
            );
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
    if matches!(prompt.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        responder.reply(format!("Executing approved {} command.", assessment.risk));
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

    responder.start_git_task(
        repo_root.to_path_buf(),
        "Git stage all",
        vec!["add".into(), "-A".into()],
        GitContinuation::SaveWorkStaged,
    );
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
    responder.reply(format!("Using commit message:\n{commit_msg}\nA successful commit will be followed by a separate push confirmation."));
    responder.start_git_task(
        repo_root.to_path_buf(),
        "Git commit",
        commit_arguments(&commit_msg),
        GitContinuation::OfferPush,
    );
}

fn handle_stage_plan<R: WorkflowResponder>(
    responder: &mut R,
    repo_root: &std::path::Path,
    prompt: &str,
    args: Vec<String>,
) {
    if matches!(prompt.trim().to_lowercase().as_str(), "" | "yes" | "y") {
        responder.start_git_task(
            repo_root.to_path_buf(),
            "Git stage",
            args,
            GitContinuation::None,
        );
    } else {
        responder.reply("Staging cancelled.");
    }
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
    responder.reply(format!(
        "Using commit message:\n{commit_msg}\nThis creates a local commit only; it will not push."
    ));
    responder.start_git_task(
        repo_root.to_path_buf(),
        "Git commit (local only)",
        commit_arguments(&commit_msg),
        GitContinuation::None,
    );
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

fn commit_arguments(commit_msg: &str) -> Vec<String> {
    let (subject, body_lines) = split_commit_message(commit_msg);
    let mut commit_args: Vec<String> = vec!["commit".into(), "-m".into(), subject];
    for line in body_lines {
        commit_args.push("-m".into());
        commit_args.push(line);
    }
    commit_args
}

pub async fn generate_commit_message_async(
    config: &Config,
    repo_root: &std::path::Path,
) -> Option<String> {
    if !config.generate_commit_message {
        return None;
    }
    let command_timeout = Duration::from_secs(config.cmd_timeout_secs);
    let stat = capture_git_output(
        repo_root,
        &[
            "diff",
            "--cached",
            "--no-ext-diff",
            "--no-textconv",
            "--stat",
        ],
        command_timeout,
    )
    .await?;
    if stat.trim().is_empty() {
        return None;
    }
    let patch = capture_git_output(
        repo_root,
        &[
            "diff",
            "--cached",
            "--no-ext-diff",
            "--no-textconv",
            "--unified=3",
        ],
        command_timeout,
    )
    .await?;
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
    let mut response_bytes = 0usize;
    let stdout = crate::model_stream::run(
        command,
        Duration::from_secs(config.llm_timeout_secs),
        |chunk| {
            response_bytes = response_bytes.saturating_add(chunk.len());
            anyhow::ensure!(
                response_bytes <= 8192,
                "generated commit message exceeded 8192 bytes"
            );
            Ok(())
        },
    )
    .await
    .ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Read-only background preparation uses the same bounded subprocess runner
/// as visible tasks. Do not treat a truncated tail as a complete staged diff.
async fn capture_git_output(
    repo_root: &std::path::Path,
    args: &[&str],
    timeout: Duration,
) -> Option<String> {
    let mut handle = task_runner::spawn(TaskSpec {
        id: TaskId(0),
        cwd: repo_root.to_path_buf(),
        program: "git".into(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
        environment: vec![
            ("GIT_TERMINAL_PROMPT".into(), "0".into()),
            ("GIT_PAGER".into(), "cat".into()),
        ],
        timeout,
    });
    loop {
        let snapshot = handle.updates.borrow_and_update().clone();
        if let Some(outcome) = snapshot.outcome {
            return (outcome == TaskOutcome::Succeeded
                && !snapshot.stdout_truncated
                && !snapshot.stderr_truncated)
                .then_some(snapshot.stdout);
        }
        if handle.updates.changed().await.is_err() {
            return None;
        }
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
    use std::{
        fs,
        path::{Path, PathBuf},
        time::Duration,
    };

    use tempfile::TempDir;

    use super::{
        GitContinuation, WorkflowKind, WorkflowResponder, WorkflowState, capture_git_output,
        handle_workflow_response,
    };
    use crate::{
        commands::run_command_with_timeout,
        patch::{AppliedPatch, PatchReview},
        task_runner::{self, TaskId, TaskOutcome, TaskSpec},
    };

    #[derive(Default)]
    struct TestResponder {
        replies: Vec<String>,
        executed_commands: Vec<String>,
        git_tasks: Vec<(PathBuf, String, Vec<String>, GitContinuation)>,
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

        fn start_git_task(
            &mut self,
            repo_root: PathBuf,
            label: &str,
            args: Vec<String>,
            continuation: GitContinuation,
        ) {
            self.git_tasks
                .push((repo_root, label.into(), args, continuation));
        }

        fn command_timeout_secs(&self) -> u64 {
            10
        }

        fn set_last_applied_patch(&mut self, patch: AppliedPatch) {
            self.last_applied_patch = Some(patch);
        }
    }

    impl TestResponder {
        async fn run_only_git_task(&mut self) -> GitContinuation {
            assert_eq!(
                self.git_tasks.len(),
                1,
                "one explicit task, no implicit shell chain"
            );
            let (cwd, _, args, continuation) = self.git_tasks.pop().unwrap();
            let mut handle = task_runner::spawn(TaskSpec {
                id: TaskId(1),
                cwd,
                program: "git".into(),
                args,
                environment: vec![("GIT_TERMINAL_PROMPT".into(), "0".into())],
                timeout: Duration::from_secs(10),
            });
            tokio::time::timeout(Duration::from_secs(15), async {
                loop {
                    let snapshot = handle.updates.borrow_and_update().clone();
                    if let Some(outcome) = snapshot.outcome {
                        assert_eq!(outcome, TaskOutcome::Succeeded, "{}", snapshot.stderr);
                        break;
                    }
                    handle.updates.changed().await.expect("task must complete");
                }
            })
            .await
            .expect("bounded task completion");
            continuation
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
        git(
            root,
            &["config", "user.email", "workflow-test@example.invalid"],
        );
        fs::write(root.join("README.md"), "# Test repository\n").expect("seed repository");
        git(root, &["add", "README.md"]);
        git(root, &["commit", "-qm", "Initialize repository"]);
        repo
    }

    #[tokio::test]
    async fn stage_workflow_stages_only_requested_file_in_a_real_repository() {
        let repo = initialized_repository();
        let root = repo.path();
        fs::write(root.join("staged.txt"), "stage this\n").expect("write staged file");
        fs::write(root.join("unstaged.txt"), "leave this alone\n").expect("write unstaged file");
        let mut responder = TestResponder::default();

        handle_workflow_response(
            &mut responder,
            WorkflowState {
                kind: WorkflowKind::StagePlan {
                    args: vec![
                        "add".to_string(),
                        "--".to_string(),
                        "staged.txt".to_string(),
                    ],
                },
                repo_root: root.to_path_buf(),
            },
            "yes",
        );

        assert!(
            git(root, &["diff", "--cached", "--name-only"])
                .trim()
                .is_empty(),
            "responding only schedules the subprocess"
        );
        assert_eq!(responder.run_only_git_task().await, GitContinuation::None);
        assert_eq!(
            git(root, &["diff", "--cached", "--name-only"]).trim(),
            "staged.txt"
        );
        assert!(git(root, &["status", "--short"]).contains("?? unstaged.txt"));
    }

    #[tokio::test]
    async fn commit_workflow_creates_a_local_commit_without_pushing() {
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

        assert_eq!(
            git(root, &["log", "-1", "--format=%s"]).trim(),
            "Initialize repository"
        );
        assert_eq!(responder.run_only_git_task().await, GitContinuation::None);
        assert_eq!(
            git(root, &["log", "-1", "--format=%s"]).trim(),
            "Add project note"
        );
        assert!(
            git(root, &["diff", "--cached", "--name-only"])
                .trim()
                .is_empty()
        );
        assert!(
            responder
                .replies
                .iter()
                .any(|reply| reply.contains("will not push"))
        );
    }

    #[test]
    fn cancelling_git_workflows_never_starts_a_task() {
        for kind in [
            WorkflowKind::SaveWorkPlan,
            WorkflowKind::StagePlan {
                args: vec!["add".into(), "-A".into()],
            },
            WorkflowKind::SaveWorkCommit {
                suggested: "Save notes".into(),
            },
            WorkflowKind::CommitOnlyConfirm {
                suggested: "Save notes".into(),
            },
            WorkflowKind::PushConfirm,
        ] {
            let mut responder = TestResponder::default();
            handle_workflow_response(
                &mut responder,
                WorkflowState {
                    kind,
                    repo_root: PathBuf::from("intended-repository"),
                },
                "no",
            );
            assert!(responder.git_tasks.is_empty());
        }
    }

    #[test]
    fn save_work_stages_before_review_and_only_offers_push_after_commit() {
        let mut responder = TestResponder::default();
        let root = PathBuf::from("intended-repository");
        handle_workflow_response(
            &mut responder,
            WorkflowState {
                kind: WorkflowKind::SaveWorkPlan,
                repo_root: root.clone(),
            },
            "yes",
        );
        assert_eq!(responder.git_tasks[0].0, root);
        assert_eq!(responder.git_tasks[0].2, vec!["add", "-A"]);
        assert_eq!(responder.git_tasks[0].3, GitContinuation::SaveWorkStaged);

        responder.git_tasks.clear();
        handle_workflow_response(
            &mut responder,
            WorkflowState {
                kind: WorkflowKind::SaveWorkCommit {
                    suggested: "Keep $(text) and 'quotes'\n- Preserve `data`".into(),
                },
                repo_root: root,
            },
            "yes",
        );
        assert_eq!(responder.git_tasks.len(), 1);
        assert_eq!(
            responder.git_tasks[0].2,
            vec![
                "commit",
                "-m",
                "Keep $(text) and 'quotes'",
                "-m",
                "- Preserve `data`"
            ]
        );
        assert_eq!(responder.git_tasks[0].3, GitContinuation::OfferPush);
    }

    #[test]
    fn push_requires_explicit_yes_and_is_a_separate_task() {
        let mut responder = TestResponder::default();
        let workflow = WorkflowState {
            kind: WorkflowKind::PushConfirm,
            repo_root: PathBuf::from("intended-repository"),
        };
        handle_workflow_response(&mut responder, workflow.clone(), "");
        assert!(
            responder.git_tasks.is_empty(),
            "Enter must not publish a commit"
        );
        handle_workflow_response(&mut responder, workflow, "yes");
        assert_eq!(responder.git_tasks.len(), 1);
        assert_eq!(responder.git_tasks[0].2, vec!["push"]);
        assert_eq!(responder.git_tasks[0].3, GitContinuation::None);
    }

    #[tokio::test]
    async fn background_git_reads_reject_failures_and_truncated_output() {
        let repo = initialized_repository();
        let root = repo.path();
        let output =
            capture_git_output(root, &["log", "-1", "--format=%s"], Duration::from_secs(10)).await;
        assert_eq!(
            output.as_deref().map(str::trim),
            Some("Initialize repository")
        );
        assert!(
            capture_git_output(root, &["not-a-real-git-command"], Duration::from_secs(10))
                .await
                .is_none()
        );
        fs::write(
            root.join("large.txt"),
            "x".repeat(task_runner::OUTPUT_LIMIT_BYTES * 2),
        )
        .expect("large fixture");
        git(root, &["add", "large.txt"]);
        assert!(
            capture_git_output(root, &["diff", "--cached"], Duration::from_secs(10))
                .await
                .is_none(),
            "a retained output tail is not a complete diff"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn background_git_reads_observe_the_task_deadline() {
        let repo = initialized_repository();
        let result = tokio::time::timeout(
            Duration::from_secs(3),
            capture_git_output(
                repo.path(),
                &["-c", "alias.task-read-test=!exec sleep 10", "task-read-test"],
                Duration::from_millis(50),
            ),
        ).await.expect("timed-out Git preparation must clean up promptly");
        assert!(result.is_none());
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
        assert!(
            responder
                .replies
                .iter()
                .any(|reply| reply.contains("Successfully created file"))
        );
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

        assert_eq!(
            fs::read_to_string(root.join("notes.txt")).unwrap(),
            "after\n"
        );
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
        assert!(
            responder
                .replies
                .iter()
                .any(|reply| reply.contains("cancelled"))
        );

        handle_workflow_response(&mut responder, workflow.clone(), "");
        assert!(responder.executed_commands.is_empty());

        handle_workflow_response(&mut responder, workflow, "yes");
        assert_eq!(
            responder.executed_commands,
            vec!["rm -rf generated".to_string()]
        );
    }
}
