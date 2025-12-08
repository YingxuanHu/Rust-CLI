//! Embedding-based intent matching using Ollama's embedding API.
//!
//! This module provides semantic matching of user input to tools using
//! vector embeddings and cosine similarity.

use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::tools::TOOLS;

/// Default embedding model to use with Ollama.
pub const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";

/// Minimum similarity threshold to consider a match.
pub const SIMILARITY_THRESHOLD: f32 = 0.5;

/// Request body for Ollama embedding API.
#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    prompt: &'a str,
}

/// Response from Ollama embedding API.
#[derive(Deserialize)]
struct EmbeddingResponse {
    embedding: Vec<f32>,
}

/// Cache of pre-computed embeddings for tool examples.
pub struct EmbeddingCache {
    /// Map from example phrase to (tool_name, embedding)
    examples: HashMap<String, (String, Vec<f32>)>,
    /// HTTP client for Ollama API
    client: reqwest::Client,
    /// Embedding model name
    model: String,
}

/// Result of embedding-based intent matching.
#[derive(Debug, Clone)]
pub struct EmbeddingMatch {
    pub tool: String,
    pub similarity: f32,
    #[allow(dead_code)] // Useful for debugging/logging
    pub matched_example: String,
}

impl EmbeddingCache {
    /// Create a new embedding cache (embeddings not yet loaded).
    pub fn new(model: Option<&str>) -> Self {
        Self {
            examples: HashMap::new(),
            client: reqwest::Client::new(),
            model: model.unwrap_or(DEFAULT_EMBEDDING_MODEL).to_string(),
        }
    }

    /// Initialize the cache by computing embeddings for all tool examples.
    /// This should be called once at startup.
    pub async fn initialize(&mut self) -> Result<()> {
        for tool in TOOLS {
            for &example in tool.examples {
                let embedding = self.get_embedding(example).await?;
                self.examples.insert(
                    example.to_string(),
                    (tool.name.to_string(), embedding),
                );
            }
        }
        Ok(())
    }

    /// Check if the cache is initialized (has embeddings).
    pub fn is_initialized(&self) -> bool {
        !self.examples.is_empty()
    }

    /// Get embedding for a text string from Ollama.
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let request = EmbeddingRequest {
            model: &self.model,
            prompt: text,
        };

        let response = self
            .client
            .post("http://localhost:11434/api/embeddings")
            .json(&request)
            .send()
            .await
            .context("sending embedding request to Ollama")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("Ollama embedding API returned {}: {}", status, body);
        }

        let embedding_response: EmbeddingResponse = response
            .json()
            .await
            .context("parsing embedding response")?;

        Ok(embedding_response.embedding)
    }

    /// Find the best matching tool for user input using cosine similarity.
    pub async fn find_match(&self, user_input: &str) -> Result<Option<EmbeddingMatch>> {
        if self.examples.is_empty() {
            return Ok(None);
        }

        let input_embedding = self.get_embedding(user_input).await?;

        let mut best_match: Option<EmbeddingMatch> = None;
        let mut best_similarity: f32 = SIMILARITY_THRESHOLD;

        for (example, (tool_name, example_embedding)) in &self.examples {
            let similarity = cosine_similarity(&input_embedding, example_embedding);

            if similarity > best_similarity {
                best_similarity = similarity;
                best_match = Some(EmbeddingMatch {
                    tool: tool_name.clone(),
                    similarity,
                    matched_example: example.clone(),
                });
            }
        }

        Ok(best_match)
    }
}

/// Compute cosine similarity between two vectors.
/// Returns a value between -1.0 and 1.0, where 1.0 means identical.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let mag_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let mag_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if mag_a == 0.0 || mag_b == 0.0 {
        return 0.0;
    }

    dot / (mag_a * mag_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![-1.0, -2.0, -3.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &b);
        assert_eq!(sim, 0.0);
    }
}

