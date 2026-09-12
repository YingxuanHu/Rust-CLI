//! Tier 3: Small LLM classifier fallback.
//!
//! Uses a small, fast local LLM (like qwen2:0.5b) to classify intent when
//! deterministic methods fail. This provides flexibility for novel phrasing.

use anyhow::Result;
use std::time::Duration;

use crate::{config::ollama_api_url, intent::ParsedIntent, tools::TOOLS};

const LLM_CONFIDENCE_THRESHOLD: f32 = 0.5;

/// Use a small LLM to classify intent when other methods fail.
/// Returns Some(intent) if LLM provides a valid tool match, None otherwise.
/// Returns "chat" intent if user is just having a conversation.
pub async fn classify_with_llm(
    input: &str,
    model: &str,
    request_timeout_secs: u64,
    ollama_host: &str,
) -> Result<Option<ParsedIntent>> {
    tracing::debug!("[LLM Classifier] Starting classification for input: '{}' with model: '{}'", input, model);
    
    // Build tool list for prompt
    let tool_names: Vec<_> = TOOLS.iter().map(|t| t.name).collect();
    let tools_str = tool_names.join(", ");
    
    let prompt = format!(
        "You are a command classifier. The user is either:\n\
         1. Trying to perform an ACTION with the codebase/git (use one of the tools)\n\
         2. Just CHATTING or asking questions about concepts (respond with 'chat')\n\
         \n\
         Available tools: {}\n\
         \n\
         Rules:\n\
         - If asking about concepts/explanations/how things work → 'chat'\n\
         - If asking to DO something (run, execute, show, save, commit) → tool name\n\
         - When in doubt, prefer 'chat'\n\
         \n\
         Examples:\n\
         Input: \"hello\" → chat\n\
         Input: \"how are you?\" → chat\n\
         Input: \"what is rust?\" → chat\n\
         Input: \"explain how this works\" → chat\n\
         Input: \"can you explain the code?\" → chat\n\
         Input: \"show me the git status\" → status\n\
         Input: \"save my work\" → save_work\n\
         Input: \"run the tests\" → run_tests\n\
         \n\
         Respond with ONLY the tool name or 'chat', nothing else.\n\
         \n\
         User input: \"{}\"\n\
         Response:",
        tools_str, input
    );
    
    // Call Ollama
    let endpoint = ollama_api_url(ollama_host, "api/generate");
    tracing::debug!("[LLM Classifier] Calling Ollama API at {}", endpoint);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(request_timeout_secs))
        .build()?;
    let mut request = client
        .post(&endpoint)
        .json(&serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "temperature": 0.1,
                "num_predict": 20,
            }
        }));
    if let Ok(api_key) = std::env::var("OLLAMA_API_KEY") {
        if !api_key.trim().is_empty() {
            request = request.bearer_auth(api_key);
        }
    }
    let response = match request
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            tracing::debug!("[LLM Classifier] ✗ ERROR: Failed to call Ollama API: {}", e);
            tracing::debug!(
                "[LLM Classifier] ✗ Is Ollama running? Try: curl {}",
                ollama_api_url(ollama_host, "api/version")
            );
            return Ok(None);
        }
    };
    
    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "unknown error".to_string());
        tracing::debug!("[LLM Classifier] ✗ ERROR: Ollama returned status {}: {}", status, error_text);
        tracing::debug!("[LLM Classifier] ✗ Model '{}' may not be installed. Try: ollama pull {}", model, model);
        return Ok(None);
    }
    
    tracing::debug!("[LLM Classifier] ✓ Got successful response from Ollama");
    
    let result: serde_json::Value = match response.json().await {
        Ok(json) => json,
        Err(e) => {
            tracing::debug!("[LLM Classifier] ✗ ERROR: Failed to parse JSON response: {}", e);
            return Ok(None);
        }
    };
    
    let llm_response = result["response"]
        .as_str()
        .unwrap_or("");
    
    if llm_response.is_empty() {
        tracing::debug!("[LLM Classifier] ✗ ERROR: Got empty response from Ollama");
        tracing::debug!("[LLM Classifier] ✗ Full JSON: {}", result);
        return Ok(None);
    }
    
    Ok(parse_classifier_response(llm_response))
}

/// Model output must name exactly one catalog entry. Substring matching can
/// turn `draft_commit_message` into `commit`, or explanatory prose into an
/// unintended action. Invalid output falls back to chat.
fn parse_classifier_response(response: &str) -> Option<ParsedIntent> {
    let response = response.trim();
    let response = if response.starts_with("```") {
        let (opening, body) = response.split_once('\n')?;
        if !matches!(opening, "```" | "```text" | "```plaintext") {
            return None;
        }
        body.strip_suffix("```")?.trim()
    } else {
        response
    };
    let name = response.to_ascii_lowercase();
    TOOLS.iter()
        .find(|tool| tool.name == name)
        .map(|tool| ParsedIntent::new(tool.name, LLM_CONFIDENCE_THRESHOLD))
}

#[cfg(test)]
mod tests {
    use super::{classify_with_llm, parse_classifier_response};

    #[test]
    fn every_catalog_tool_round_trips_without_substring_collisions() {
        for tool in crate::tools::TOOLS {
            assert_eq!(parse_classifier_response(tool.name).unwrap().tool, tool.name);
        }
        assert_eq!(parse_classifier_response("  DRAFT_COMMIT_MESSAGE\n").unwrap().tool, "draft_commit_message");
        assert_eq!(parse_classifier_response("```text\ndraft_commit_message\n```").unwrap().tool, "draft_commit_message");
    }

    #[test]
    fn ambiguous_or_explanatory_model_output_does_not_dispatch() {
        for response in ["not commit", "status or save_work", "the tool is: shell", "draft", "", "```\ncommit\nshell\n```"] {
            assert!(parse_classifier_response(response).is_none(), "{response}");
        }
    }

    #[tokio::test]
    async fn classifier_uses_configured_host_and_parses_a_tool_response() {
        let server = crate::test_support::MockHttpServer::respond_once(
            200,
            r#"{"response":"status"}"#,
        );

        let intent = classify_with_llm("show repository status", "test-classifier", 5, server.host())
            .await
            .unwrap()
            .expect("a recognized intent");

        assert_eq!(intent.tool, "status");
        let request = server.finish();
        assert!(request.starts_with("POST /api/generate HTTP/1.1"));
        assert!(request.contains("\"model\":\"test-classifier\""));
        assert!(request.contains("\"stream\":false"));
    }
}
