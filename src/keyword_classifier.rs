//! Tier 2: Keyword + Embedding hybrid classifier.
//!
//! This tier combines deterministic keyword matching with semantic embedding
//! similarity for accurate intent classification without ML training.

use std::collections::HashSet;

use anyhow::Result;

use crate::{
    embedding::{cosine_similarity, EmbeddingCache},
    intent::ParsedIntent,
    tools::TOOLS,
};

const CONFIDENCE_THRESHOLD: f32 = 0.7;

/// Hybrid classifier combining keyword matching with embedding similarity.
pub struct KeywordClassifier;

impl KeywordClassifier {
    /// Classify user input using keyword + embedding scoring.
    /// Returns Some(intent) if confidence >= threshold, None otherwise.
    pub async fn classify(
        input: &str,
        cache: &EmbeddingCache,
    ) -> Result<Option<ParsedIntent>> {
        if !cache.is_initialized() {
            return Ok(None);
        }
        
        // Get embedding for input
        let input_embedding = cache.get_embedding(input).await?;
        
        let input_lower = input.to_lowercase();
        let words: HashSet<_> = input_lower
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|w| !w.is_empty())
            .collect();
        
        let mut scores: Vec<(String, f32)> = Vec::new();
        
        for tool in TOOLS {
            // 1. Keyword scoring
            let keyword_score = Self::score_keywords(&words, tool);
            
            // 2. Embedding similarity (best match from tool examples)
            let embedding_score = Self::score_embeddings(&input_embedding, tool, cache).await?;
            
            // 3. Combine scores (60% keywords, 40% embeddings)
            let combined_score = 0.6 * keyword_score + 0.4 * embedding_score;
            
            scores.push((tool.name.to_string(), combined_score));
        }
        
        // Find best match
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        
        // Debug: Show top 3 scores
        eprintln!("[Keyword Classifier] Input: '{}'", input);
        for (i, (tool, score)) in scores.iter().take(3).enumerate() {
            eprintln!("  {}. {} (score: {:.3})", i + 1, tool, score);
        }
        
        if let Some((tool, score)) = scores.first() {
            if *score >= CONFIDENCE_THRESHOLD {
                eprintln!("[Keyword Classifier] ✓ Matched '{}' with confidence {:.3} (threshold: {})", tool, score, CONFIDENCE_THRESHOLD);
                return Ok(Some(ParsedIntent::new(tool, *score)));
            } else {
                eprintln!("[Keyword Classifier] ✗ Best score {:.3} below threshold {}", score, CONFIDENCE_THRESHOLD);
            }
        }
        
        Ok(None)
    }
    
    fn score_keywords(words: &HashSet<&str>, tool: &crate::tools::Tool) -> f32 {
        let mut score = 0.0;
        
        // Check if tool name appears in input
        let tool_words: HashSet<_> = tool.name.split('_').collect();
        let overlap = words.intersection(&tool_words).count();
        if overlap > 0 {
            score += 1.5 * (overlap as f32 / tool_words.len() as f32);
        }
        
        // Check keyword overlap with examples
        for example in tool.examples {
            let example_words: HashSet<_> = example
                .split_whitespace()
                .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
                .filter(|w| !w.is_empty())
                .collect();
            
            let overlap = words.intersection(&example_words).count();
            let union_size = words.len().max(example_words.len());
            
            if union_size > 0 {
                let similarity = overlap as f32 / union_size as f32;
                score = score.max(similarity);
            }
        }
        
        score.min(1.0)
    }
    
    async fn score_embeddings(
        input_embedding: &[f32],
        tool: &crate::tools::Tool,
        cache: &EmbeddingCache,
    ) -> Result<f32> {
        let mut best_similarity: f32 = 0.0;
        
        // Compare against all examples for this tool
        for example in tool.examples {
            let example_embedding = cache.get_embedding(example).await?;
            let similarity = cosine_similarity(input_embedding, &example_embedding);
            best_similarity = best_similarity.max(similarity);
        }
        
        Ok(best_similarity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_scoring() {
        let words: HashSet<_> = "save my work".split_whitespace().collect();
        let save_work_tool = TOOLS.iter().find(|t| t.name == "save_work").unwrap();
        let score = KeywordClassifier::score_keywords(&words, save_work_tool);
        assert!(score > 0.5);
    }
}

