//! Embedding-based intent matching using Ollama's embedding API.
//!
//! This module provides semantic matching of user input to tools using
//! vector embeddings and cosine similarity.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{config::ollama_api_url, tools::TOOLS};

/// Default embedding model to use with Ollama.
pub const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";


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

/// Serializable cache structure for persistent storage.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct EmbeddingCacheData {
    /// Model name used to generate embeddings
    model: String,
    /// Endpoint that produced these vectors; equal model names need not mean
    /// equal model weights on different servers.
    ollama_host: String,
    /// Map from example phrase to (tool_name, embedding)
    examples: HashMap<String, (String, Vec<f32>)>,
    /// Version of the cache format (for future compatibility)
    version: u32,
}

/// Cache of pre-computed embeddings for tool examples.
pub struct EmbeddingCache {
    /// Map from example phrase to (tool_name, embedding)
    examples: HashMap<String, (String, Vec<f32>)>,
    /// HTTP client for Ollama API
    client: reqwest::Client,
    /// Embedding model name
    model: String,
    /// Ollama daemon address in `host:port` form.
    ollama_host: String,
}


impl EmbeddingCache {
    /// Create a new embedding cache (embeddings not yet loaded).
    pub fn new(model: Option<&str>, request_timeout_secs: u64, ollama_host: &str) -> Self {
        Self {
            examples: HashMap::new(),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(request_timeout_secs))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            model: model.unwrap_or(DEFAULT_EMBEDDING_MODEL).to_string(),
            ollama_host: ollama_host.to_string(),
        }
    }

    /// Initialize the cache by loading from disk or computing embeddings.
    /// This should be called once at startup.
    pub async fn initialize(&mut self, cache_path: Option<&Path>) -> Result<()> {
        // Try to load from cache first
        if let Some(path) = cache_path {
            if let Ok(()) = self.load_from_cache(path) {
                tracing::info!("Loaded embeddings from cache: {}", path.display());
                return Ok(());
            } else {
                tracing::info!("Cache not found or invalid, computing embeddings...");
            }
        }

        // Publish only a complete cache. A failed initialization must not leave
        // a partial set looking ready to the classifier.
        let mut examples = HashMap::new();
        for tool in TOOLS {
            for &example in tool.examples {
                let embedding = self.get_embedding(example).await?;
                examples.insert(
                    example.to_string(),
                    (tool.name.to_string(), embedding),
                );
            }
        }
        validate_examples(&examples)?;
        self.examples = examples;

        // Save to cache if path is provided
        if let Some(path) = cache_path {
            if let Err(e) = self.save_to_cache(path) {
                tracing::warn!("Failed to save embedding cache: {}", e);
            } else {
                tracing::info!("Saved embeddings to cache: {}", path.display());
            }
        }

        Ok(())
    }

    /// Load embeddings from a cache file.
    fn load_from_cache(&mut self, path: &Path) -> Result<()> {
        let contents = fs::read_to_string(path)
            .context("reading embedding cache file")?;
        
        let cache_data: EmbeddingCacheData = toml::from_str(&contents)
            .context("parsing embedding cache")?;

        // Verify the model matches
        if cache_data.model != self.model {
            bail!(
                "Cache model mismatch: cached '{}' vs current '{}'",
                cache_data.model,
                self.model
            );
        }

        if cache_data.version != 2 {
            bail!("Unsupported cache version: {}", cache_data.version);
        }
        if cache_data.ollama_host != self.ollama_host {
            bail!("Cache endpoint differs from the configured Ollama endpoint");
        }
        validate_examples(&cache_data.examples)?;

        self.examples = cache_data.examples;
        Ok(())
    }

    /// Save embeddings to a cache file.
    fn save_to_cache(&self, path: &Path) -> Result<()> {
        let cache_data = EmbeddingCacheData {
            model: self.model.clone(),
            ollama_host: self.ollama_host.clone(),
            examples: self.examples.clone(),
            version: 2,
        };

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .context("creating cache directory")?;
        }

        let contents = toml::to_string_pretty(&cache_data)
            .context("serializing embedding cache")?;
        
        fs::write(path, contents)
            .context("writing embedding cache file")?;

        Ok(())
    }

    /// Check if the cache is initialized (has embeddings).
    pub fn is_initialized(&self) -> bool {
        !self.examples.is_empty()
    }

    /// Retrieve a precomputed tool example without making a network request.
    pub fn example_embedding(&self, example: &str) -> Option<&[f32]> {
        self.examples.get(example).map(|(_, vector)| vector.as_slice())
    }

    #[cfg(test)]
    pub(crate) fn with_test_examples(host: &str) -> Self {
        let mut cache = Self::new(Some("test-embed"), 1, host);
        for tool in TOOLS {
            for &example in tool.examples {
                cache.examples.insert(example.to_string(), (tool.name.to_string(), vec![1.0, 0.0]));
            }
        }
        cache
    }

    /// Get embedding for a text string from Ollama.
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let request = EmbeddingRequest {
            model: &self.model,
            prompt: text,
        };

        let mut request_builder = self
            .client
            .post(ollama_api_url(&self.ollama_host, "api/embeddings"))
            .json(&request);
        if let Ok(api_key) = std::env::var("OLLAMA_API_KEY") {
            if !api_key.trim().is_empty() {
                request_builder = request_builder.bearer_auth(api_key);
            }
        }
        let response = request_builder
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

        validate_vector(&embedding_response.embedding)?;
        Ok(embedding_response.embedding)
    }
}

fn validate_vector(vector: &[f32]) -> Result<()> {
    if vector.is_empty()
        || vector.iter().any(|value| !value.is_finite())
        || vector.iter().all(|value| *value == 0.0)
    {
        bail!("Embedding must be a nonempty finite vector with a nonzero magnitude");
    }
    Ok(())
}

fn validate_examples(examples: &HashMap<String, (String, Vec<f32>)>) -> Result<()> {
    let expected: HashMap<_, _> = TOOLS.iter().flat_map(|tool| {
        tool.examples.iter().map(move |example| (*example, tool.name))
    }).collect();
    if expected.len() != examples.len() {
        bail!("Cached examples differ from the current tool catalog");
    }
    let mut dimension = None;
    for (phrase, expected_tool) in expected {
        let (tool, vector) = examples.get(phrase)
            .with_context(|| format!("Missing cached example: {phrase}"))?;
        if tool != expected_tool {
            bail!("Cached example has changed tools: {phrase}");
        }
        validate_vector(vector)?;
        if dimension.is_some_and(|size| size != vector.len()) {
            bail!("Cached embedding dimensions differ");
        }
        dimension = Some(vector.len());
    }
    Ok(())
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

    fn populated_cache(host: &str) -> EmbeddingCache {
        EmbeddingCache::with_test_examples(host)
    }

    #[tokio::test]
    async fn warm_classification_only_requests_the_input_embedding() {
        // This server accepts exactly one request. Re-embedding any catalog
        // example fails, catching the previous O(number of examples) calls.
        let server = crate::test_support::MockHttpServer::respond_once(200, r#"{"embedding":[1.0,0.0]}"#);
        let cache = populated_cache(server.host());
        let intent = crate::keyword_classifier::KeywordClassifier::classify("run tests please", &cache)
            .await.unwrap().expect("test intent");
        assert_eq!(intent.tool, "run_tests");
        assert!(server.finish().contains("\"prompt\":\"run tests please\""));
    }

    #[test]
    fn disk_cache_requires_matching_host_catalog_and_vector_dimensions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("embeddings.toml");
        let mut original = populated_cache("127.0.0.1:11434");
        original.save_to_cache(&path).unwrap();
        let mut loaded = EmbeddingCache::new(Some("test-embed"), 1, "127.0.0.1:11434");
        loaded.load_from_cache(&path).unwrap();
        assert_eq!(loaded.example_embedding("run tests"), Some([1.0, 0.0].as_slice()));
        let mut different_host = EmbeddingCache::new(Some("test-embed"), 1, "127.0.0.1:11435");
        assert!(different_host.load_from_cache(&path).is_err());

        original.examples.get_mut("run tests").unwrap().0 = "commit".into();
        original.save_to_cache(&path).unwrap();
        assert!(loaded.load_from_cache(&path).is_err());

        original = populated_cache("127.0.0.1:11434");
        original.examples.get_mut("run tests").unwrap().1 = vec![1.0];
        original.save_to_cache(&path).unwrap();
        assert!(loaded.load_from_cache(&path).is_err());

        original.examples.remove("run tests");
        original.save_to_cache(&path).unwrap();
        assert!(loaded.load_from_cache(&path).is_err());
    }

    #[tokio::test]
    async fn empty_embedding_is_rejected() {
        let server = crate::test_support::MockHttpServer::respond_once(200, r#"{"embedding":[]}"#);
        let cache = EmbeddingCache::new(Some("test-embed"), 1, server.host());
        assert!(cache.get_embedding("question").await.is_err());
        server.finish();
    }

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

    #[tokio::test]
    async fn embedding_requests_use_the_configured_ollama_host() {
        let server = crate::test_support::MockHttpServer::respond_once(
            200,
            r#"{"embedding":[0.25,0.75]}"#,
        );
        let cache = EmbeddingCache::new(Some("test-embed"), 5, server.host());

        assert_eq!(cache.get_embedding("find a file").await.unwrap(), vec![0.25, 0.75]);

        let request = server.finish();
        assert!(request.starts_with("POST /api/embeddings HTTP/1.1"));
        assert!(request.contains("\"model\":\"test-embed\""));
        assert!(request.contains("\"prompt\":\"find a file\""));
    }
}
