//! Tier 3: Small LLM classifier fallback.
//!
//! Uses a small, fast local LLM (like qwen2:0.5b) to classify intent when
//! deterministic methods fail. This provides flexibility for novel phrasing.

use anyhow::Result;

use crate::{intent::ParsedIntent, tools::TOOLS};

const LLM_CONFIDENCE_THRESHOLD: f32 = 0.5;

/// Use a small LLM to classify intent when other methods fail.
/// Returns Some(intent) if LLM provides a valid tool match, None otherwise.
/// Returns "chat" intent if user is just having a conversation.
pub async fn classify_with_llm(input: &str, model: &str) -> Result<Option<ParsedIntent>> {
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
    tracing::debug!("[LLM Classifier] Calling Ollama API at http://localhost:11434/api/generate");
    let client = reqwest::Client::new();
    let response = match client
        .post("http://localhost:11434/api/generate")
        .json(&serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "temperature": 0.1,
                "num_predict": 20,
            }
        }))
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            tracing::debug!("[LLM Classifier] ✗ ERROR: Failed to call Ollama API: {}", e);
            tracing::debug!("[LLM Classifier] ✗ Is Ollama running? Try: curl http://localhost:11434/api/version");
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
    
    let llm_response = llm_response.trim().to_lowercase();
    tracing::debug!("[LLM Classifier] ✓ Raw LLM response: '{}'", llm_response);
    
    // Check for "chat" response
    if llm_response == "chat" || llm_response.contains("chat") {
        tracing::debug!("[LLM Classifier] ✓ Classified as CHAT");
        return Ok(Some(ParsedIntent::new("chat", LLM_CONFIDENCE_THRESHOLD)));
    }
    
    // Check if response matches a valid tool
    for tool in TOOLS {
        if llm_response == tool.name || llm_response.contains(tool.name) {
            tracing::debug!("[LLM Classifier] ✓ Classified as TOOL: {}", tool.name);
            return Ok(Some(ParsedIntent::new(tool.name, LLM_CONFIDENCE_THRESHOLD)));
        }
    }
    
    // Also check if tool name is contained in response (handles "the tool is: status")
    for tool in TOOLS {
        if tool.name.contains(&llm_response) && llm_response.len() > 3 {
            tracing::debug!("[LLM Classifier] ✓ Classified as TOOL (fuzzy): {}", tool.name);
            return Ok(Some(ParsedIntent::new(tool.name, LLM_CONFIDENCE_THRESHOLD * 0.9)));
        }
    }
    
    // If LLM couldn't classify, return None (will fall through to ask_user)
    tracing::debug!("[LLM Classifier] ✗ Could not match response '{}' to any tool or 'chat'", llm_response);
    Ok(None)
}

