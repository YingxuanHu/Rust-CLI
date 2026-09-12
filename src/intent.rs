//! Intent parsing using a tiered resolution system.
//!
//! Tier 1: Local fuzzy matching
//! Tier 2: Keyword + cached embedding hybrid
//! Tier 3: Small LLM classifier → fallback to "chat"

use anyhow::Result;

use crate::{
    embedding::EmbeddingCache,
    fuzzy,
    keyword_classifier::KeywordClassifier,
    learned::LearnedAliases,
    llm_classifier,
    tools::ToolArgs,
};

/// The result of parsing user intent.
#[derive(Debug, Clone)]
pub struct ParsedIntent {
    pub tool: String,
    pub args: ToolArgs,
    pub confidence: f32,
}

impl ParsedIntent {
    pub fn new(tool: &str, confidence: f32) -> Self {
        Self {
            tool: tool.to_string(),
            args: ToolArgs::default(),
            confidence,
        }
    }
}

/// Main entry point: resolve intent through all 3 tiers.
pub async fn resolve_intent(
    input: &str,
    cache: &EmbeddingCache,
    learned: &LearnedAliases,
    llm_model: &str,
    request_timeout_secs: u64,
    ollama_host: &str,
) -> Result<ParsedIntent> {
    tracing::debug!("[Intent Resolution] Input: '{}'", input);

    // Explaining an existing result is read-only, even if a learned alias or
    // classifier associates its build/test keywords with an executable tool.
    // This must precede every classification tier, not just quick_match: chat
    // quick matches still reach this entry point before prompt composition.
    if is_output_explanation(input) {
        return Ok(ParsedIntent::new("chat", 1.0));
    }
    
    // Tier 1: Fuzzy matching + learned aliases
    if let Some(intent) = fuzzy::fuzzy_match(input, learned) {
        tracing::debug!("[Intent Resolution] ✓ Tier 1 (Fuzzy): {}", intent.tool);
        return Ok(enrich_intent(intent, input));
    }
    
    // Tier 2: Keyword + embedding classifier
    match KeywordClassifier::classify(input, cache).await {
        Ok(Some(intent)) => {
            tracing::debug!("[Intent Resolution] ✓ Tier 2 (Keyword/Embedding): {}", intent.tool);
            return Ok(enrich_intent(intent, input));
        }
        Ok(None) => {}
        Err(error) => {
            tracing::debug!(%error, "Embedding classification unavailable; trying the LLM classifier");
        }
    }
    
    // Tier 3: Small LLM classifier
    // This can return "chat" if user is just chatting, or a tool name if they're trying to do something
    tracing::debug!("[Intent Resolution] Attempting Tier 3 (LLM) with model: '{}'", llm_model);
    if let Some(intent) = llm_classifier::classify_with_llm(
        input,
        llm_model,
        request_timeout_secs,
        ollama_host,
    )
    .await?
    {
        let intent = enrich_intent(intent, input);
        
        // If LLM classified as "chat", return it directly
        if intent.tool == "chat" {
            tracing::debug!("[Intent Resolution] ✓ Tier 3 (LLM): chat");
            return Ok(intent);
        }
        
        // If LLM found a tool match, return it
        tracing::debug!("[Intent Resolution] ✓ Tier 3 (LLM): {}", intent.tool);
        return Ok(intent);
    }
    
    // Fallback: Default to chat when LLM couldn't classify
    // This means the input is likely conversational rather than an action intent
    tracing::debug!("[Intent Resolution] → Fallback to chat");
    Ok(ParsedIntent {
        tool: "chat".to_string(),
        args: ToolArgs::default(),
        confidence: 0.5,
    })
}

fn is_output_explanation(input: &str) -> bool {
    let normalized = input.trim()
        .trim_end_matches(|ch| matches!(ch, '?' | '!' | '.'))
        .trim()
        .to_ascii_lowercase();
    let words: Vec<&str> = normalized.split_whitespace().collect();
    if matches!(words.first().copied(), Some("why" | "explain" | "summarize"))
        && crate::context::contains_reference(input)
    {
        return true;
    }

    // Bare references are also requests about prior results, not instructions
    // to run again. Match whole phrases so "run tests and explain" remains an
    // action request and filenames such as last-build-error.rs are unaffected.
    matches!(words.as_slice(),
        ["last", "build" | "test" | "tests" | "error" | "errors" | "failure" | "output" | "result"]
        | ["last", "build" | "test" | "tests", "error" | "errors" | "failure" | "failures" | "output" | "result" | "results"]
    )
}

fn enrich_intent(mut intent: ParsedIntent, input: &str) -> ParsedIntent {
    let extracted = extract_args_from_input(input, &intent.tool);
    intent.args.merge_missing(extracted);
    // Missing a requested path must not silently widen a natural-language
    // staging request to the entire repository. Leave ambiguous wording in
    // chat until the user supplies an explicit scope.
    if intent.tool == "stage" && intent.args.path.is_none() && !is_explicit_stage_all(input) {
        return ParsedIntent::new("chat", intent.confidence);
    }
    intent
}

fn is_explicit_stage_all(input: &str) -> bool {
    let trimmed = input.trim();
    matches!(trimmed.to_ascii_lowercase().as_str(),
        "stage all" | "stage changes" | "stage all files"
            | "stage all changes" | "add all files" | "git add ."
    ) || matches!(trimmed, "git add -A" | "git add --all")
}

/// Extract arguments from user input based on the matched tool.
fn extract_args_from_input(input: &str, tool: &str) -> ToolArgs {
    let mut args = ToolArgs::default();
    
    match tool {
        "show_file" => {
            let lower = input.to_ascii_lowercase();
            for prefix in ["show file ", "read file ", "open file ", "show ", "read ", "open "] {
                if lower.starts_with(prefix) {
                    let path = input[prefix.len()..].trim();
                    if !path.is_empty() {
                        args.path = Some(path.to_string());
                        break;
                    }
                }
            }
        }
        "stage" => {
            let trimmed = input.trim();
            let lower = trimmed.to_ascii_lowercase();
            // Only explicit whole phrases mean "all". A filename such as
            // `all-important.txt` or `file-a.rs` must retain its scope.
            if !is_explicit_stage_all(trimmed) {
                let path = if lower.starts_with("stage ") {
                    Some(trimmed[6..].trim())
                } else if lower.starts_with("git add ") {
                    Some(trimmed[8..].trim())
                } else {
                    None
                };
                if let Some(path) = path.filter(|path| !path.is_empty()) {
                    args.path = Some(path.to_string());
                }
            }
        }
        "shell" => {
            let trimmed = input.trim();
            if let Some(cmd) = trimmed.strip_prefix('$').or_else(|| trimmed.strip_prefix('!')) {
                args.command = Some(cmd.trim().to_string());
            }
        }
        "list_files" => {
            let lower = input.to_ascii_lowercase();
            for prefix in ["list files in ", "show files in ", "ls ", "dir "] {
                if lower.starts_with(prefix) {
                    let path = input[prefix.len()..].trim();
                    if !path.is_empty() {
                        args.path = Some(path.to_string());
                        break;
                    }
                }
            }
        }
        "write_file" => {
            let lower = input.to_ascii_lowercase();
            
            for prefix in ["write to ", "save to ", "create file ", "create ", "write file "] {
                if lower.starts_with(prefix) {
                    let rest = input[prefix.len()..].trim();
                    if let Some(space_idx) = rest.find(char::is_whitespace) {
                        args.path = Some(rest[..space_idx].to_string());
                    } else {
                        args.path = Some(rest.to_string());
                    }
                    break;
                }
            }
            
            if let Some(content_idx) = lower.find("with content:") {
                let content = input[content_idx + 13..].trim();
                if !content.is_empty() {
                    args.content = Some(content.to_string());
                }
            }
        }
        "edit_file" => {
            if let Some(edit) = parse_edit_request(input) {
                args = edit;
            }
        }
        _ => {}
    }
    
    args
}

/// Parse the explicit edit syntax: `edit path/to/file: requested change`.
/// Requiring the colon makes the file boundary unambiguous and prevents a
/// conversational sentence from accidentally triggering a code edit.
pub fn parse_edit_request(input: &str) -> Option<ToolArgs> {
    let trimmed = input.trim();
    let (verb, rest) = trimmed.split_once(char::is_whitespace)?;
    if !verb.eq_ignore_ascii_case("edit") {
        return None;
    }
    let (path, instruction) = rest.split_once(':')?;
    let path = path.trim();
    let instruction = instruction.trim();
    if path.is_empty() || instruction.is_empty() {
        return None;
    }
    Some(ToolArgs {
        path: Some(path.to_string()),
        query: Some(instruction.to_string()),
        ..Default::default()
    })
}

/// Quick match for explicit shell, edit, and rollback syntax.
/// This bypasses tiered classification when the user has supplied structured
/// input that should never be interpreted as ordinary chat.
pub fn quick_match(input: &str) -> Option<ParsedIntent> {
    let trimmed = input.trim();

    match trimmed.to_ascii_lowercase().as_str() {
        "run tests" => return Some(ParsedIntent::new("run_tests", 1.0)),
        "build" => return Some(ParsedIntent::new("build", 1.0)),
        "tasks" | "project tasks" => return Some(ParsedIntent::new("project_tasks", 1.0)),
        "status" | "what changed" => return Some(ParsedIntent::new("status", 1.0)),
        "commit" | "save locally" => return Some(ParsedIntent::new("commit", 1.0)),
        "save work" => return Some(ParsedIntent::new("save_work", 1.0)),
        "help" | "?" => return Some(ParsedIntent::new("help", 1.0)),
        _ => {}
    }

    if let Some(path) = trimmed.get(..5).filter(|prefix| prefix.eq_ignore_ascii_case("diff "))
        .map(|_| trimmed[5..].trim()).filter(|path| !path.is_empty()) {
        return Some(ParsedIntent { tool: "show_diff".into(), confidence: 1.0,
            args: ToolArgs { path: Some(path.to_string()), ..ToolArgs::default() } });
    }

    if matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "start" | "getting started" | "show me around" | "what should i do first"
    ) {
        return Some(ParsedIntent::new("getting_started", 1.0));
    }

    if let Some(args) = parse_edit_request(trimmed) {
        return Some(ParsedIntent {
            tool: "edit_file".to_string(),
            args,
            confidence: 1.0,
        });
    }

    if matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "rollback last edit" | "undo last edit"
    ) {
        return Some(ParsedIntent::new("rollback_edit", 1.0));
    }

    // Shell command prefix ($ or !) - needs special syntax parsing
    if trimmed.starts_with('$') || (trimmed.starts_with('!') && trimmed != "!!") {
        let cmd = trimmed[1..].trim();
        return Some(ParsedIntent {
            tool: "shell".to_string(),
            args: ToolArgs {
                command: Some(cmd.to_string()),
                ..Default::default()
            },
            confidence: 1.0,
        });
    }

    // Bang shortcuts (!!)
    if trimmed == "!!" {
        return Some(ParsedIntent {
            tool: "shell_repeat".to_string(),
            args: ToolArgs::default(),
            confidence: 1.0,
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_test_and_help_commands_do_not_need_a_model() {
        assert_eq!(quick_match("  RUN TESTS ").unwrap().tool, "run_tests");
        assert_eq!(quick_match("help").unwrap().tool, "help");
        assert_eq!(quick_match("?").unwrap().tool, "help");
        assert!(quick_match("how do I run tests?").is_none());
    }

    #[test]
    fn common_actions_resolve_locally_without_matching_questions() {
        for (input, tool) in [
            ("BUILD", "build"), ("tasks", "project_tasks"),
            ("project tasks", "project_tasks"), ("status", "status"),
            ("what changed", "status"), ("commit", "commit"),
            ("save locally", "commit"), ("save work", "save_work"),
        ] {
            assert_eq!(quick_match(input).unwrap().tool, tool);
        }
        for question in ["why did the build fail?", "how do I commit?", "explain tasks"] {
            assert!(quick_match(question).is_none());
        }
    }

    #[test]
    fn direct_diff_preserves_path_text() {
        for path in ["--output=secret", "src/My File.rs", "中文/🦀.rs", ":(top)*"] {
            let parsed = quick_match(&format!("DIFF {path}")).unwrap();
            assert_eq!(parsed.tool, "show_diff");
            assert_eq!(parsed.args.path.as_deref(), Some(path));
        }
        assert!(quick_match("diff").is_none());
        assert!(quick_match("diff  ").is_none());
        assert!(quick_match("🦀🦀🦀").is_none());
    }

    #[test]
    fn test_quick_match_shell() {
        let intent = quick_match("$ ls -la").unwrap();
        assert_eq!(intent.tool, "shell");
        assert_eq!(intent.args.command, Some("ls -la".to_string()));
    }

    #[test]
    fn quick_match_recognizes_getting_started_phrases() {
        assert_eq!(quick_match("show me around").unwrap().tool, "getting_started");
    }

    #[test]
    fn test_quick_match_bang_repeat() {
        let intent = quick_match("!!").unwrap();
        assert_eq!(intent.tool, "shell_repeat");
    }

    #[test]
    fn test_extract_args_show_file() {
        let args = extract_args_from_input("show file src/main.rs", "show_file");
        assert_eq!(args.path, Some("src/main.rs".to_string()));
    }

    #[test]
    fn test_extract_args_stage() {
        let args = extract_args_from_input("stage src/lib.rs", "stage");
        assert_eq!(args.path, Some("src/lib.rs".to_string()));
    }

    #[test]
    fn staging_keeps_filenames_that_look_like_all_flags() {
        for (input, expected) in [
            ("stage all-important.txt", "all-important.txt"),
            ("git add file-a.rs", "file-a.rs"),
            ("git add src/Feature-A.rs", "src/Feature-A.rs"),
            ("stage All Changes.txt", "All Changes.txt"),
        ] {
            assert_eq!(extract_args_from_input(input, "stage").path.as_deref(), Some(expected), "{input}");
        }
    }

    #[test]
    fn staging_all_requires_an_explicit_whole_phrase() {
        for input in ["stage all", "stage changes", "stage all files", "git add -A", "git add --all", "git add ."] {
            assert!(extract_args_from_input(input, "stage").path.is_none(), "{input}");
        }
        assert_eq!(extract_args_from_input("git add -a", "stage").path.as_deref(), Some("-a"));
    }

    #[test]
    fn unresolved_staging_scope_does_not_default_to_all_files() {
        for input in ["stage", "git add", "please stage README.md", "add this file", "add only my Rust source"] {
            assert_eq!(enrich_intent(ParsedIntent::new("stage", 0.9), input).tool, "chat", "{input}");
        }
        assert_eq!(enrich_intent(ParsedIntent::new("stage", 0.9), "stage all").tool, "stage");
    }

    #[test]
    fn create_file_keeps_the_requested_filename() {
        let args = extract_args_from_input("create file Notes.txt with content: Hello", "write_file");
        assert_eq!(args.path.as_deref(), Some("Notes.txt"));
        assert_eq!(args.content.as_deref(), Some("Hello"));
        let unicode = extract_args_from_input("write file İ.txt with content: 😀", "write_file");
        assert_eq!(unicode.path.as_deref(), Some("İ.txt"));
        assert_eq!(unicode.content.as_deref(), Some("😀"));
    }

    #[test]
    fn local_save_phrases_never_select_the_push_workflow() {
        let learned = LearnedAliases::default();
        for input in ["save locally", "save local", "save changes locally"] {
            assert_eq!(fuzzy::fuzzy_match(input, &learned).unwrap().tool, "commit", "{input}");
        }
    }

    #[tokio::test]
    async fn output_explanations_bypass_action_aliases_and_unavailable_classifiers() {
        let fixture = tempfile::tempdir().expect("isolated aliases");
        let mut learned = LearnedAliases::default();
        learned.save_alias(
            "why did tests fail?",
            "build",
            &fixture.path().join("learned.toml"),
            "regression fixture",
        ).unwrap();
        let unavailable_host = "http://127.0.0.1:0";
        let cache = EmbeddingCache::with_test_examples(unavailable_host);
        for input in [
            "why did tests fail?",
            "Why did the build fail?",
            "explain the output",
            "explain that failure",
            "summarize the results",
            "last build error",
            "last test failure",
            "last tests",
            "last error?",
        ] {
            let parsed = resolve_intent(
                input, &cache, &learned, "unavailable-classifier", 1, unavailable_host,
            ).await.expect("output questions must not contact a classifier");
            assert_eq!(parsed.tool, "chat", "{input}");
            assert_eq!(parsed.confidence, 1.0, "{input} must resolve deterministically");
        }
    }

    #[test]
    fn output_explanation_guard_does_not_capture_actions_or_unrelated_questions() {
        for input in [
            "run tests", "build", "save work", "stage all", "save locally",
            "run tests and explain why they failed", "build and summarize the output",
            "$ explain the output", "show file last-build-error.rs", "why build?",
            "how do I commit?", "explain tasks", "summarizer the output",
        ] {
            assert!(!is_output_explanation(input), "{input}");
        }
    }

    #[tokio::test]
    async fn embedding_failure_still_tries_the_classifier() {
        let embeddings = crate::test_support::MockHttpServer::respond_once(500, "{}");
        let classifier = crate::test_support::MockHttpServer::respond_once(200, r#"{"response":"draft_commit_message"}"#);
        let cache = EmbeddingCache::with_test_examples(embeddings.host());
        let intent = resolve_intent(
            "prepare a thoughtful summary of the changes I have staged for later review",
            &cache,
            &LearnedAliases::default(),
            "test-classifier",
            1,
            classifier.host(),
        )
        .await
        .unwrap();
        assert_eq!(intent.tool, "draft_commit_message");
        assert!(embeddings.finish().starts_with("POST /api/embeddings HTTP/1.1"));
        assert!(classifier.finish().starts_with("POST /api/generate HTTP/1.1"));
    }

    #[test]
    fn quick_match_parses_a_structured_edit_request() {
        let intent = quick_match("edit src/main.rs: add a version flag").unwrap();
        assert_eq!(intent.tool, "edit_file");
        assert_eq!(intent.args.path.as_deref(), Some("src/main.rs"));
        assert_eq!(intent.args.query.as_deref(), Some("add a version flag"));
    }

    #[test]
    fn quick_match_recognizes_edit_rollback() {
        assert_eq!(quick_match("rollback last edit").unwrap().tool, "rollback_edit");
    }
}
