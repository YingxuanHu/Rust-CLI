use std::{path::PathBuf, time::Duration};

use tokio::sync::mpsc;

use crate::{
    audit,
    command_policy::{self, CommandRisk},
    commands::{format_error, run_command_with_timeout, run_shell_command_with_timeout},
    config::Config,
    custom_command_generator::expand_command_handlers,
    file_ops,
    intent::ParsedIntent,
    input::is_shell_command,
    patch::{self, AppliedPatch},
    repo::{ProjectType, RepoInfo},
    session::Role,
    tools::ToolArgs,
    workflow::{generate_commit_message_async, WorkflowKind, WorkflowState},
};

pub enum AssistantEvent {
    Token { idx: usize, chunk: String },
    Completed { idx: usize, content: Option<String>, original_input: String, cwd: PathBuf },
    Failed { idx: usize, error: String },
    PatchFailed { idx: usize, error: String },
    PatchReady { idx: usize, review: patch::PatchReview },
    IntentResolved { idx: usize, intent: ParsedIntent, original_input: String, cwd: PathBuf },
    CommitPlanReady { idx: usize, suggested: String, repo_root: PathBuf, save_work: bool },
    CommitMessageReady { idx: usize, message: String },
}

pub trait IntentDispatcher {
    fn reply(&mut self, content: impl Into<String>);
    fn reply_scroll_to_top(&mut self, content: impl Into<String>);
    fn push_recorded(&mut self, role: Role, content: impl Into<String>) -> usize;
    fn set_pending_workflow(&mut self, workflow: WorkflowState);
    fn get_session_cwd(&self) -> PathBuf;
    fn get_session_repo_root(&self) -> Option<PathBuf>;
    fn get_session_repo_info(&self) -> Option<RepoInfo>;
    fn get_input_history(&self) -> &[String];
    fn get_config(&self) -> &Config;
    fn get_assistant_tx(&self) -> mpsc::UnboundedSender<AssistantEvent>;
    fn pending_placeholder(&mut self) -> usize;
    fn record_output(&mut self, kind: &'static str, summary: &str, content: &str);
    fn record_file_access(&mut self, file_path: &str);
    fn record_command_usage(&mut self, command: &str);
    fn get_last_applied_patch(&self) -> Option<AppliedPatch>;
    fn clear_last_applied_patch(&mut self);
}

fn run_tool_command<D: IntentDispatcher>(
    dispatcher: &D,
    cwd: &std::path::Path,
    program: &str,
    args: &[&str],
) -> anyhow::Result<String> {
    run_command_with_timeout(
        cwd,
        program,
        args,
        Duration::from_secs(dispatcher.get_config().cmd_timeout_secs),
    )
}

/// Dispatch a parsed intent to the appropriate handler.
/// Returns true if the intent was handled, false if it should fall through to chat.
pub fn dispatch_intent<D: IntentDispatcher>(
    dispatcher: &mut D,
    intent: &ParsedIntent,
    original_input: &str,
) -> bool {
    let handled = match intent.tool.as_str() {
        "shell" => {
            if let Some(cmd) = &intent.args.command {
                handle_shell_dispatch(dispatcher, cmd);
            } else {
                dispatcher.reply("No command specified for shell.");
            }
            true
        }
        "shell_repeat" => {
            handle_shell_repeat(dispatcher);
            true
        }
        "save_work" => {
            handle_save_work_intent(dispatcher);
            true
        }
        "stage" => {
            handle_stage_intent(dispatcher, &intent.args);
            true
        }
        "commit" => {
            handle_commit_intent(dispatcher);
            true
        }
        "status" => {
            handle_status_intent(dispatcher);
            true
        }
        "find_todos" => {
            handle_find_todos_intent(dispatcher);
            true
        }
        "run_tests" => {
            handle_run_tests_intent(dispatcher);
            true
        }
        "show_file" => {
            handle_show_file_intent(dispatcher, &intent.args, original_input);
            true
        }
        "draft_commit_message" => {
            handle_draft_commit_intent(dispatcher);
            true
        }
        "list_files" => {
            handle_list_files_intent(dispatcher, &intent.args, original_input);
            true
        }
        "write_file" => {
            handle_write_file_intent(dispatcher, &intent.args, original_input);
            true
        }
        "edit_file" => {
            handle_edit_file_intent(dispatcher, &intent.args, original_input);
            true
        }
        "rollback_edit" => {
            handle_rollback_edit_intent(dispatcher);
            true
        }
        "getting_started" => {
            handle_getting_started_intent(dispatcher);
            true
        }
        "build" => {
            handle_build_intent(dispatcher);
            true
        }
        "explain_project" => {
            handle_explain_project_intent(dispatcher);
            true
        }
        "help" => {
            handle_help_intent(dispatcher);
            true
        }
        "chat" => false, // Fall through to LLM chat
        _ => false,
    };
    
    // Record command usage for frecency tracking
    if handled {
        dispatcher.record_command_usage(original_input);
    }
    
    handled
}

/// A short, contextual first-use guide. It deliberately uses the same natural
/// language people can type, instead of requiring a command vocabulary.
pub fn getting_started_message(repo_info: Option<&RepoInfo>, has_git_repo: bool) -> String {
    let project = repo_info
        .map(|info| {
            let name = info.name.as_deref().unwrap_or("this project");
            let kind = match &info.project_type {
                ProjectType::Rust => "Rust",
                ProjectType::Node => "Node.js",
                ProjectType::Python => "Python",
                ProjectType::Go => "Go",
                ProjectType::Unknown => "project",
            };
            format!("You're in {name} ({kind}).")
        })
        .unwrap_or_else(|| "You're not in a recognized project yet.".to_string());

    let mut suggestions = vec![project, String::new(), "Try one of these:".to_string()];
    if repo_info.is_some() {
        suggestions.push("• `explain this project` — get a quick overview".to_string());
        suggestions.push("• `run tests` — check that it works".to_string());
    } else {
        suggestions.push("• `cd /path/to/project` — switch to your codebase".to_string());
        suggestions.push("• Ask a question in plain English".to_string());
    }
    if has_git_repo {
        suggestions.push("• `what changed` — see Git status and the diff summary".to_string());
    }
    suggestions.push("• `help` — see every available action".to_string());
    suggestions.push("\nYou can type naturally; I will ask before applying edits or high-impact commands.".to_string());
    suggestions.join("\n")
}

fn handle_getting_started_intent<D: IntentDispatcher>(dispatcher: &mut D) {
    let repo_info = dispatcher.get_session_repo_info();
    let guide = getting_started_message(repo_info.as_ref(), dispatcher.get_session_repo_root().is_some());
    dispatcher.reply_scroll_to_top(guide);
}

pub fn handle_shell_dispatch<D: IntentDispatcher>(dispatcher: &mut D, cmd: &str) {
    if cmd.is_empty() {
        dispatcher.reply("Usage: $ <command> or ! <command>");
        return;
    }

    // Expand composable handlers (like {{GEN_COMMIT_MSG}}) if present
    let expanded_cmd = if cmd.contains("{{") && cmd.contains("}}") {
        let repo_root = dispatcher.get_session_repo_root()
            .unwrap_or_else(|| dispatcher.get_session_cwd());
        match expand_command_handlers(cmd, dispatcher.get_config(), &repo_root) {
            Ok(expanded) => {
                // Show the expanded command to the user
                if expanded != cmd {
                    dispatcher.reply(format!("📝 Expanded command:\n  {}", expanded));
                }
                expanded
            }
            Err(e) => {
                dispatcher.reply(format!("Failed to expand command handlers: {}", e));
                return;
            }
        }
    } else {
        cmd.to_string()
    };

    let assessment = command_policy::assess_shell_command(&expanded_cmd);
    if assessment.requires_confirmation() {
        let reasons = assessment
            .reasons
            .iter()
            .map(|reason| format!("• {reason}"))
            .collect::<Vec<_>>()
            .join("\n");
        dispatcher.reply(format!(
            "Approval required before running this {} shell command.\n\n{}\n\n$ {}\n\nType 'yes' to run it, or anything else to cancel.",
            assessment.risk, reasons, expanded_cmd
        ));
        dispatcher.set_pending_workflow(WorkflowState {
            kind: WorkflowKind::ShellCommandConfirm {
                command: expanded_cmd,
                assessment,
            },
            repo_root: dispatcher
                .get_session_repo_root()
                .unwrap_or_else(|| dispatcher.get_session_cwd()),
        });
        return;
    }

    execute_shell_command(dispatcher, &expanded_cmd, assessment.risk);
}

/// Run a command that has already passed an explicit high-impact confirmation.
/// This is called only by the matching workflow response, and deliberately
/// skips reclassification so a single confirmation cannot loop forever.
pub fn handle_approved_shell_dispatch<D: IntentDispatcher>(dispatcher: &mut D, cmd: &str) {
    let assessment = command_policy::assess_shell_command(cmd);
    execute_shell_command(dispatcher, cmd, assessment.risk);
}

fn execute_shell_command<D: IntentDispatcher>(
    dispatcher: &mut D,
    command: &str,
    risk: CommandRisk,
) {
    let cwd = dispatcher.get_session_cwd();
    let timeout = Duration::from_secs(dispatcher.get_config().cmd_timeout_secs);
    match run_shell_command_with_timeout(&cwd, command, timeout) {
        Ok(output) => {
            record_shell_audit(dispatcher, &cwd, command, risk, "completed", output.len());
            // Record output for semantic reference resolution
            let summary = if output.trim().is_empty() {
                format!("Ran: {} (no output)", command)
            } else {
                format!("Ran: {} (success)", command)
            };
            dispatcher.record_output("command", &summary, &output);
            
            if output.trim().is_empty() {
                dispatcher.reply("(command completed with no output)");
            } else {
                dispatcher.reply(output);
            }
        }
        Err(err) => {
            record_shell_audit(dispatcher, &cwd, command, risk, "failed", 0);
            dispatcher.reply(format!("Error: {}", format_error(&err)));
        }
    }
}

fn record_shell_audit<D: IntentDispatcher>(
    dispatcher: &D,
    cwd: &std::path::Path,
    command: &str,
    risk: CommandRisk,
    outcome: &'static str,
    output_bytes: usize,
) {
    if let Err(error) = audit::append_shell_execution(
        &dispatcher.get_config().audit_path,
        cwd,
        command,
        risk,
        outcome,
        output_bytes,
    ) {
        tracing::warn!(%error, "could not append shell audit record");
    }
}

fn handle_shell_repeat<D: IntentDispatcher>(dispatcher: &mut D) {
    // Find last shell command in history
    let last_cmd = dispatcher
        .get_input_history()
        .iter()
        .rev()
        .find(|e| is_shell_command(e))
        .cloned();
    if let Some(last_cmd) = last_cmd {
        dispatcher.push_recorded(Role::User, format!("!! → {}", last_cmd));
        let cmd = last_cmd
            .trim()
            .strip_prefix('$')
            .or_else(|| last_cmd.trim().strip_prefix('!'))
            .map(|s| s.trim().to_string())
            .unwrap_or(last_cmd.clone());
        handle_shell_dispatch(dispatcher, &cmd);
    } else {
        dispatcher.reply("No previous shell command in history.");
    }
}

fn handle_save_work_intent<D: IntentDispatcher>(dispatcher: &mut D) {
    if let Some(repo_root) = dispatcher.get_session_repo_root() {
        let status_preview = match run_tool_command(dispatcher, &repo_root, "git", &["status", "--short"]) {
            Ok(out) => out,
            Err(err) => format!("(git status failed: {err})"),
        };
        let plan = [
            "Planned git workflow:",
            "• git status (preview)",
            "• git add -A",
            "• git commit -m \"<generated message>\"",
            "• git push",
            "",
            "Status preview:",
            &status_preview,
            "",
            "Press Enter (or type 'yes') to run, anything else to cancel.",
        ]
        .join("\n");
        dispatcher.reply(plan);
        dispatcher.set_pending_workflow(WorkflowState {
            kind: WorkflowKind::SaveWorkPlan,
            repo_root,
        });
    } else {
        dispatcher.reply("No git repository detected; cannot save work.");
    }
}

fn handle_stage_intent<D: IntentDispatcher>(dispatcher: &mut D, args: &ToolArgs) {
    let repo_root = dispatcher
        .get_session_repo_root()
        .unwrap_or_else(|| dispatcher.get_session_cwd());

    let stage_args = if let Some(path) = &args.path {
        vec!["add".into(), "--".into(), path.clone()]
    } else {
        vec!["add".into(), "-A".into()]
    };

    let display_args = stage_args.join(" ");
    dispatcher.set_pending_workflow(WorkflowState {
        kind: WorkflowKind::StagePlan { args: stage_args },
        repo_root,
    });
    dispatcher.reply(format!(
        "Plan: git {}\nPress Enter (or type 'yes') to run, anything else to cancel.",
        display_args
    ));
}

fn handle_commit_intent<D: IntentDispatcher>(dispatcher: &mut D) {
    if let Some(repo_root) = dispatcher.get_session_repo_root() {
        let idx = dispatcher.pending_placeholder();
        let tx = dispatcher.get_assistant_tx();
        let config = dispatcher.get_config().clone();
        dispatcher.set_pending_workflow(WorkflowState {
            kind: WorkflowKind::CommitMessagePending,
            repo_root: repo_root.clone(),
        });
        tokio::spawn(async move {
            let suggested = generate_commit_message_async(&config, &repo_root)
                .await
                .unwrap_or_else(|| "chore: update".to_string());
            let _ = tx.send(AssistantEvent::CommitPlanReady {
                idx,
                suggested,
                repo_root,
                save_work: false,
            });
        });
    } else {
        dispatcher.reply("No git repository detected; cannot commit.");
    }
}

fn handle_status_intent<D: IntentDispatcher>(dispatcher: &mut D) {
    let root = dispatcher
        .get_session_repo_root()
        .unwrap_or_else(|| dispatcher.get_session_cwd());
    let status = run_tool_command(dispatcher, &root, "git", &["status", "--short"])
        .unwrap_or_else(|e| format!("(git status failed: {})", format_error(&e)));
    let diffstat = run_tool_command(dispatcher, &root, "git", &["diff", "--stat"])
        .unwrap_or_else(|e| format!("(git diff --stat failed: {})", format_error(&e)));
    
    // Record output for semantic reference resolution
    let summary = if status.trim().is_empty() {
        "No changes".to_string()
    } else {
        let lines: Vec<&str> = status.lines().collect();
        format!("Git status: {} file(s) changed", lines.len())
    };
    let combined = format!("{}\n\n{}", status, diffstat);
    dispatcher.record_output("diff", &summary, &combined);
    
    dispatcher.set_pending_workflow(WorkflowState {
        kind: WorkflowKind::DiffPreview { file: None },
        repo_root: root,
    });
    dispatcher.reply(format!(
        "Status preview:\n{}\n\nDiff stat:\n{}\nReply with a file path to view its diff, press Enter (or type 'yes') to continue, or anything else to cancel.",
        status, diffstat
    ));
}

fn handle_find_todos_intent<D: IntentDispatcher>(dispatcher: &mut D) {
    let root = dispatcher
        .get_session_repo_root()
        .unwrap_or_else(|| dispatcher.get_session_cwd());
    let pattern = r"(?i)^\s*(?://|#|;|<!--|/\*+)\s*(TODO|FIXME)|^\s*(TODO|FIXME)";
    match run_tool_command(
        dispatcher,
        &root,
        "rg",
        &["--no-heading", "--line-number", "--pcre2", pattern],
    ) {
        Ok(out) if out.trim().is_empty() => dispatcher.reply("No TODO/FIXME found."),
        Ok(out) => {
            // Record output for semantic reference resolution
            let count = out.lines().count();
            let summary = format!("Found {} TODO/FIXME item(s)", count);
            dispatcher.record_output("todos", &summary, &out);
            
            dispatcher.reply(format!("TODO/FIXME:\n{out}"))
        }
        Err(err) => dispatcher.reply(format!("Search failed: {}", format_error(&err))),
    }
}

fn handle_run_tests_intent<D: IntentDispatcher>(dispatcher: &mut D) {
    if let Some(repo_info) = dispatcher.get_session_repo_info() {
        let (program, args) = repo_info.test_command();
        let args_refs: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();
        
        match run_tool_command(dispatcher, &repo_info.root, program, &args_refs) {
            Ok(out) => dispatcher.reply(format!("{} {} output:\n{}", program, args.join(" "), out)),
            Err(err) => dispatcher.reply(format!("{} {} failed: {}", program, args.join(" "), format_error(&err))),
        }
    } else {
        dispatcher.reply("No project detected; cannot run tests.");
    }
}

fn handle_show_file_intent<D: IntentDispatcher>(
    dispatcher: &mut D,
    args: &ToolArgs,
    original_input: &str,
) {
    // Try to get path from args, or parse from original input
    let path = args
        .path
        .as_ref()
        .map(PathBuf::from)
        .or_else(|| parse_show_file(original_input));

    if let Some(path) = path {
        let base = dispatcher
            .get_session_repo_root()
            .unwrap_or_else(|| dispatcher.get_session_cwd());
        let resolved = if path.is_absolute() {
            path.clone()
        } else {
            base.join(&path)
        };

        match file_ops::read_file(&resolved, &base) {
            Ok(contents) => {
                // Record output for semantic reference resolution
                let line_count = contents.lines().count();
                let summary = format!("{} ({} lines)", path.display(), line_count);
                dispatcher.record_output("file", &summary, &contents);
                
                // Record file access for frecency tracking
                dispatcher.record_file_access(&path.to_string_lossy());
                
                dispatcher.reply(format!(
                    "Contents of {}:\n{}",
                    resolved.display(),
                    contents
                ))
            }
            Err(err) => dispatcher.reply(format!("Could not read {}: {}", resolved.display(), err)),
        }
    } else {
        dispatcher.reply("Please specify a file path to show.");
    }
}

fn handle_draft_commit_intent<D: IntentDispatcher>(dispatcher: &mut D) {
    if let Some(repo_root) = dispatcher.get_session_repo_root() {
        let idx = dispatcher.pending_placeholder();
        let tx = dispatcher.get_assistant_tx();
        let config = dispatcher.get_config().clone();
        
        // Note: We'll record the output in App when the async result arrives
        // since we can't access the dispatcher from the spawned task
        tokio::spawn(async move {
            let event = match generate_commit_message_async(&config, &repo_root).await {
                Some(msg) => {
                    AssistantEvent::CommitMessageReady {
                        idx,
                        message: msg,
                    }
                }
                None => AssistantEvent::Failed {
                    idx,
                    error: "Could not generate commit message (is anything staged?).".into(),
                },
            };
            let _ = tx.send(event);
        });
    } else {
        dispatcher.reply("No git repository detected; cannot draft a commit message.");
    }
}

fn handle_list_files_intent<D: IntentDispatcher>(
    dispatcher: &mut D,
    args: &ToolArgs,
    original_input: &str,
) {
    // Try to extract path from args or input
    let path_str = args
        .path
        .clone()
        .or_else(|| extract_path_from_list_command(original_input));

    let base = dispatcher
        .get_session_repo_root()
        .unwrap_or_else(|| dispatcher.get_session_cwd());

    let target_path = if let Some(path) = path_str {
        file_ops::resolve_path(&path, &base)
    } else {
        dispatcher.get_session_cwd()
    };

    match file_ops::list_directory(&target_path, &base) {
        Ok(files) => {
            if files.is_empty() {
                dispatcher.reply(format!("Directory is empty: {}", target_path.display()));
            } else {
                let mut output = vec![format!("Files in {}:", target_path.display())];
                for file in files {
                    let size_str = if let Some(size) = file.size {
                        format!(" ({})", file_ops::format_size(size))
                    } else {
                        String::new()
                    };
                    let type_indicator = match file.file_type.as_str() {
                        "dir" => "/",
                        "link" => "@",
                        _ => "",
                    };
                    output.push(format!("  {}{}{}", file.name, type_indicator, size_str));
                }
                dispatcher.reply(output.join("\n"));
            }
        }
        Err(err) => {
            dispatcher.reply(format!(
                "Failed to list files in {}: {}",
                target_path.display(),
                err
            ));
        }
    }
}

fn handle_write_file_intent<D: IntentDispatcher>(
    dispatcher: &mut D,
    args: &ToolArgs,
    original_input: &str,
) {
    // Extract path and content
    let path_str = args.path.clone().or_else(|| {
        // Try to extract from input like "write file main.rs" or "write to main.rs"
        let lower = original_input.to_lowercase();
        for prefix in ["write file ", "write to ", "save to ", "create file ", "create "] {
            if let Some(rest) = lower.strip_prefix(prefix) {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if !parts.is_empty() {
                    return Some(parts[0].to_string());
                }
            }
        }
        None
    });

    if path_str.is_none() {
        dispatcher.reply("Please specify a file path to write to.");
        return;
    }

    let path_str = path_str.unwrap();

    // For now, we need the LLM to provide the content
    // In a real implementation, this would be part of a multi-turn conversation
    if args.content.is_none() {
        dispatcher.reply(
            "Please provide the content to write. For example:\n\
            'write file test.txt with content: Hello world'",
        );
        return;
    }

    let content = args.content.clone().unwrap();
    let base = dispatcher
        .get_session_repo_root()
        .unwrap_or_else(|| dispatcher.get_session_cwd());

    let target_path = file_ops::resolve_path(&path_str, &base);
    let overwrite = file_ops::file_exists(&target_path);

    // Show preview and ask for confirmation
    let preview = if content.chars().count() > 200 {
        format!("{}...\n[{} bytes total]", truncate_for_preview(&content, 200), content.len())
    } else {
        content.clone()
    };

    let action = if overwrite { "overwrite" } else { "create" };
    let message = format!(
        "Confirm {} file: {}\n\nContent preview:\n{}\n\nPress Enter (or type 'yes') to proceed, or 'cancel' to abort.",
        action,
        target_path.display(),
        preview
    );

    dispatcher.reply(message);
    dispatcher.set_pending_workflow(WorkflowState {
        kind: WorkflowKind::WriteFileConfirm {
            path: target_path.to_string_lossy().to_string(),
            content,
            overwrite,
        },
        repo_root: base,
    });
}

fn handle_edit_file_intent<D: IntentDispatcher>(
    dispatcher: &mut D,
    args: &ToolArgs,
    original_input: &str,
) {
    let parsed = crate::intent::parse_edit_request(original_input);
    let path = args.path.clone().or_else(|| parsed.as_ref().and_then(|edit| edit.path.clone()));
    let instruction = args
        .query
        .clone()
        .or_else(|| parsed.and_then(|edit| edit.query));
    let (Some(path), Some(instruction)) = (path, instruction) else {
        dispatcher.reply(
            "Use `edit path/to/file: describe the requested change`. The assistant will show a checked diff before changing anything.",
        );
        return;
    };
    if PathBuf::from(&path).is_absolute() {
        dispatcher.reply("Use a repository-relative path for edits.");
        return;
    }
    let Some(repo_root) = dispatcher.get_session_repo_root() else {
        dispatcher.reply("No git repository detected; code edits require a repository root.");
        return;
    };
    let target = file_ops::resolve_path(&path, &repo_root);
    let source = match file_ops::read_file(&target, &repo_root) {
        Ok(source) => source,
        Err(error) => {
            dispatcher.reply(format!("Cannot prepare an edit for {path}: {error}"));
            return;
        }
    };
    let canonical_target = match target.canonicalize() {
        Ok(target) => target,
        Err(error) => {
            dispatcher.reply(format!("Cannot resolve {path}: {error}"));
            return;
        }
    };
    let canonical_root = match repo_root.canonicalize() {
        Ok(root) => root,
        Err(error) => {
            dispatcher.reply(format!("Cannot resolve repository root: {error}"));
            return;
        }
    };
    let relative_file = match canonical_target.strip_prefix(&canonical_root) {
        Ok(path) => path.to_string_lossy().replace('\\', "/"),
        Err(_) => {
            dispatcher.reply("Refusing to edit a file outside the repository.");
            return;
        }
    };

    let idx = dispatcher.pending_placeholder();
    let tx = dispatcher.get_assistant_tx();
    let config = dispatcher.get_config().clone();
    dispatcher.set_pending_workflow(WorkflowState {
        kind: WorkflowKind::EditPatchPending {
            file: relative_file.clone(),
        },
        repo_root,
    });
    tokio::spawn(async move {
        let event = match patch::generate_edit_patch_async(
            &config,
            &relative_file,
            &source,
            &instruction,
        )
        .await
        {
            Ok(review) => AssistantEvent::PatchReady { idx, review },
            Err(error) => AssistantEvent::PatchFailed {
                idx,
                error: format!("could not prepare an edit: {error}"),
            },
        };
        let _ = tx.send(event);
    });
}

fn handle_rollback_edit_intent<D: IntentDispatcher>(dispatcher: &mut D) {
    let Some(applied) = dispatcher.get_last_applied_patch() else {
        dispatcher.reply("There is no reviewed edit available to roll back in this session.");
        return;
    };
    match patch::rollback_patch(
        &applied.repo_root,
        &applied.review.patch,
        Duration::from_secs(dispatcher.get_config().cmd_timeout_secs),
    ) {
        Ok(()) => {
            dispatcher.clear_last_applied_patch();
            dispatcher.reply(format!("Rolled back the last edit to {}.", applied.review.file));
        }
        Err(error) => dispatcher.reply(format!(
            "Could not roll back {} because the file may have changed since the edit: {error}",
            applied.review.file
        )),
    }
}

fn parse_show_file(prompt: &str) -> Option<PathBuf> {
    let lower = prompt.to_lowercase();
    let prefixes = ["show file ", "read file ", "open file ", "show "];
    for p in prefixes {
        if lower.starts_with(p) {
            let rest = prompt[p.len()..].trim();
            if !rest.is_empty() {
                return Some(PathBuf::from(rest));
            }
        }
    }
    None
}

fn truncate_for_preview(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn extract_path_from_list_command(input: &str) -> Option<String> {
    let lower = input.to_lowercase();
    
    // Try patterns with "in" keyword first
    for prefix in ["list files in ", "show files in "] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let path = rest.trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    
    // Try direct path patterns (list files <path>, list file <path>)
    for prefix in ["list files ", "list file ", "ls ", "dir "] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            let path = rest.trim();
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    
    None
}

fn handle_build_intent<D: IntentDispatcher>(dispatcher: &mut D) {
    if let Some(repo_info) = dispatcher.get_session_repo_info() {
        let (program, args) = repo_info.build_command();
        let args_refs: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();
        
        match run_tool_command(dispatcher, &repo_info.root, program, &args_refs) {
            Ok(out) => dispatcher.reply(format!("{} {} output:\n{}", program, args.join(" "), out)),
            Err(err) => dispatcher.reply(format!("{} {} failed: {}", program, args.join(" "), format_error(&err))),
        }
    } else {
        dispatcher.reply("No project detected; cannot build.");
    }
}

fn handle_explain_project_intent<D: IntentDispatcher>(dispatcher: &mut D) {
    if let Some(repo_info) = dispatcher.get_session_repo_info() {
        let type_str = match repo_info.project_type {
            ProjectType::Rust => "Rust (Cargo)",
            ProjectType::Node => "Node.js (npm)",
            ProjectType::Python => "Python",
            ProjectType::Go => "Go",
            ProjectType::Unknown => "Unknown",
        };
        
        let name_str = repo_info.name.as_deref().unwrap_or("(unnamed)");
        let root_str = repo_info.root.display();
        
        let source_dirs: Vec<String> = repo_info.source_dirs
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        
        let mut output = vec![
            format!("Project: {}", name_str),
            format!("Type: {}", type_str),
            format!("Root: {}", root_str),
        ];
        
        if !source_dirs.is_empty() {
            output.push(format!("Source dirs: {}", source_dirs.join(", ")));
        }
        
        dispatcher.reply(output.join("\n"));
    } else {
        dispatcher.reply("No project detected in current directory.");
    }
}

fn handle_help_intent<D: IntentDispatcher>(dispatcher: &mut D) {
    let help_text = r#"
═══════════════════════════════════════════════════════════════════════════════
                        LLM-POWERED CLI ASSISTANT                              
═══════════════════════════════════════════════════════════════════════════════

DESCRIPTION
    A lightweight, Rust-based CLI assistant powered by local LLM inference.
    Provides context-aware developer workflows, git automation, and natural
    language interaction with your development environment.

MODES
    Chat Mode (default)
        • Natural language conversations with AI
        • Repository awareness and semantic context
        • Intent classification for tool invocation
        
    Shell Mode (prefix: $ or ! or Ctrl+S to switch)
        • Execute shell commands directly
        • Macro expansion for composable workflows

KEY BINDINGS
    Esc / Ctrl+C        Exit the application
    Ctrl+S              Toggle between Chat and Shell mode
    Tab                 Autocomplete (context-aware)
    Enter               Submit current input
    PgUp / PgDn         Scroll through conversation history
    ↑ / ↓               Navigate input history

COMMON COMMANDS

  Git Workflows
    save work           Stage all, commit with AI message, and push
    push changes        Same as 'save work'
    status              Show git status and diff summary
    commit              Commit staged changes (without push)
    stage all           Stage all changes with git add
    draft commit        Generate commit message for staged changes

  File Operations
    show <file>         Display file contents
    list files <path>   List directory contents
    write file <path>   Create or overwrite a file (with confirmation)
    edit <file>: <task> Generate, check, and review a single-file patch
    rollback last edit   Reverse the most recently applied reviewed patch
    find todos          Search for TODO/FIXME comments

  Project Commands
    cd <directory>      Change the session directory and re-detect the project
    build               Build the project (cargo/npm/etc.)
    run tests           Run project test suite
    explain project     Show project type and structure

  Shell Commands
    $ <command>         Execute a shell command
    ! <command>         Execute a shell command (alias)
    !!                  Repeat last shell command
    High-impact commands are shown and require explicit confirmation.
    Direct shell executions are recorded in .llm-cli/audit.jsonl by default.

  Other
    show me around      Show contextual starter actions
    help                Show this help message
    ?                   Show this help message (alias)

SEMANTIC CONTEXT
    The CLI maintains semantic context from recent outputs. After running
    commands like 'status' or 'show file', you can refer to them naturally:
    
    User: status
    CLI:  [shows git status]
    User: commit it
    CLI:  [understands 'it' refers to the shown changes]

CONFIGURATION
    Config location: .llm-cli/config.toml (or pass --config <path>)
    
    Customize:
    • LLM model selection
    • Streaming preferences  
    • Output style (bullets/paragraph)
    • Macros and custom workflows

REQUIREMENTS
    • Ollama running locally (ollama.com)
    • Default model: llama3 (configurable)
    • Git (for repository commands)
    • ripgrep (for code search, optional)

MORE INFORMATION
    Documentation: docs/
    • SEMANTIC_CONTEXT.md - Context system details
    • REPO_AWARENESS.md - Project detection features
    • AUTOCOMPLETION.md - Completion system
    • INTENT_SYSTEM.md - How commands are classified

VERSION
    llm-cli v0.1.0 (Rust-based, locally-powered)

For specific questions, just ask naturally: "How do I stage specific files?"
"#;

    dispatcher.reply_scroll_to_top(help_text.to_string());
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tokio::sync::mpsc;

    use super::{handle_shell_dispatch, AssistantEvent, IntentDispatcher};
    use crate::{
        config::Config,
        patch::AppliedPatch,
        repo::{ProjectType, RepoInfo},
        session::Role,
        tools::ToolArgs,
        workflow::{WorkflowKind, WorkflowState},
    };

    #[test]
    fn getting_started_guide_is_contextual_and_uses_plain_language() {
        let guide = super::getting_started_message(None, false);
        assert!(guide.contains("cd /path/to/project"));
        assert!(guide.contains("Ask a question in plain English"));
        assert!(guide.contains("help"));
    }

    #[test]
    fn getting_started_guide_uses_project_specific_next_steps() {
        let repo = RepoInfo {
            project_type: ProjectType::Rust,
            root: PathBuf::from("/project"),
            name: Some("demo".to_string()),
            source_dirs: Vec::new(),
        };
        let guide = super::getting_started_message(Some(&repo), true);
        assert!(guide.contains("demo (Rust)"));
        assert!(guide.contains("run tests"));
        assert!(guide.contains("what changed"));
    }

    struct TestDispatcher {
        config: Config,
        cwd: PathBuf,
        replies: Vec<String>,
        pending: Option<WorkflowState>,
        tx: mpsc::UnboundedSender<AssistantEvent>,
        history: Vec<String>,
    }

    impl TestDispatcher {
        fn new(cwd: PathBuf) -> Self {
            let (tx, _rx) = mpsc::unbounded_channel();
            Self {
                config: Config::default(),
                cwd,
                replies: Vec::new(),
                pending: None,
                tx,
                history: Vec::new(),
            }
        }
    }

    impl IntentDispatcher for TestDispatcher {
        fn reply(&mut self, content: impl Into<String>) {
            self.replies.push(content.into());
        }

        fn reply_scroll_to_top(&mut self, content: impl Into<String>) {
            self.reply(content);
        }

        fn push_recorded(&mut self, _role: Role, _content: impl Into<String>) -> usize {
            0
        }

        fn set_pending_workflow(&mut self, workflow: WorkflowState) {
            self.pending = Some(workflow);
        }

        fn get_session_cwd(&self) -> PathBuf {
            self.cwd.clone()
        }

        fn get_session_repo_root(&self) -> Option<PathBuf> {
            None
        }

        fn get_session_repo_info(&self) -> Option<RepoInfo> {
            None
        }

        fn get_input_history(&self) -> &[String] {
            &self.history
        }

        fn get_config(&self) -> &Config {
            &self.config
        }

        fn get_assistant_tx(&self) -> mpsc::UnboundedSender<AssistantEvent> {
            self.tx.clone()
        }

        fn pending_placeholder(&mut self) -> usize {
            0
        }

        fn record_output(&mut self, _kind: &'static str, _summary: &str, _content: &str) {}

        fn record_file_access(&mut self, _file_path: &str) {}

        fn record_command_usage(&mut self, _command: &str) {}

        fn get_last_applied_patch(&self) -> Option<AppliedPatch> {
            None
        }

        fn clear_last_applied_patch(&mut self) {}
    }

    #[test]
    fn stage_path_is_not_interpreted_as_a_git_option() {
        let directory = tempfile::tempdir().unwrap();
        let mut dispatcher = TestDispatcher::new(directory.path().to_path_buf());
        super::handle_stage_intent(&mut dispatcher, &ToolArgs {
            path: Some("-A".to_string()),
            ..ToolArgs::default()
        });
        match dispatcher.pending.unwrap().kind {
            WorkflowKind::StagePlan { args } => assert_eq!(args, vec!["add", "--", "-A"]),
            kind => panic!("unexpected workflow: {kind:?}"),
        }
    }

    #[test]
    fn high_impact_shell_command_stops_for_a_review() {
        let directory = tempfile::tempdir().unwrap();
        let mut dispatcher = TestDispatcher::new(directory.path().to_path_buf());

        handle_shell_dispatch(&mut dispatcher, "git push origin main");

        assert!(dispatcher
            .replies
            .iter()
            .any(|reply| reply.contains("Approval required")));
        match dispatcher.pending.expect("confirmation workflow").kind {
            WorkflowKind::ShellCommandConfirm {
                command,
                assessment,
            } => {
                assert_eq!(command, "git push origin main");
                assert!(assessment.requires_confirmation());
            }
            kind => panic!("unexpected workflow: {kind:?}"),
        }
    }
}
