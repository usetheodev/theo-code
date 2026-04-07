/// Neural embeddings for semantic code search using fastembed (ONNX).
///
/// Default: Jina Embeddings v2 Base Code (768-dim, trained on code).
/// Fallback: AllMiniLM-L6-v2 (384-dim, generic NLP) if code model fails.
///
/// Jina Code understands code semantics: "error handling" ≈ "FailureTracker",
/// "database connection" ≈ "create_pool". This is a major quality upgrade
/// over generic NLP models for code retrieval.

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

pub struct NeuralEmbedder {
    model: TextEmbedding,
    dim: usize,
}

impl NeuralEmbedder {
    /// Initialize with Jina Code embeddings (768-dim, code-trained).
    ///
    /// Falls back to AllMiniLM-L6-v2 (384-dim) if Jina fails to load.
    /// On first run, downloads the model to `~/.cache/fastembed/`.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // Try Jina Code first (code-specific, 768-dim)
        {
            let mut jina_opts = InitOptions::default();
            jina_opts.model_name = EmbeddingModel::JinaEmbeddingsV2BaseCode;
            jina_opts.show_download_progress = true;
            if let Ok(model) = TextEmbedding::try_new(jina_opts) {
                return Ok(NeuralEmbedder { model, dim: 768 });
            }
        }

        // Fallback to AllMiniLM (generic NLP, 384-dim)
        eprintln!("[neural] Jina Code model failed, falling back to AllMiniLM-L6-v2");
        let mut options = InitOptions::default();
        options.model_name = EmbeddingModel::AllMiniLML6V2;
        options.show_download_progress = false;
        let model = TextEmbedding::try_new(options)?;

        Ok(NeuralEmbedder { model, dim: 384 })
    }

    /// Generate embedding for a single text. Returns 384-dim vector.
    pub fn embed(&self, text: &str) -> Vec<f64> {
        match self.model.embed(vec![text], None) {
            Ok(embeddings) => {
                if let Some(emb) = embeddings.into_iter().next() {
                    emb.into_iter().map(|x| x as f64).collect()
                } else {
                    vec![0.0; self.dim]
                }
            }
            Err(_) => vec![0.0; self.dim],
        }
    }

    /// Generate embeddings for multiple texts (batched, faster).
    pub fn embed_batch(&self, texts: &[&str]) -> Vec<Vec<f64>> {
        let texts_owned: Vec<String> = texts.iter().map(|t| t.to_string()).collect();
        match self.model.embed(texts_owned, None) {
            Ok(embeddings) => embeddings
                .into_iter()
                .map(|emb| emb.into_iter().map(|x| x as f64).collect())
                .collect(),
            Err(_) => texts.iter().map(|_| vec![0.0; self.dim]).collect(),
        }
    }

    /// Cosine similarity between two vectors.
    pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
        let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
        let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm_a == 0.0 || norm_b == 0.0 {
            0.0
        } else {
            dot / (norm_a * norm_b)
        }
    }

    pub fn dim(&self) -> usize {
        self.dim
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn get_embedder() -> NeuralEmbedder {
        NeuralEmbedder::new().expect("failed to initialize NeuralEmbedder")
    }

    #[test]
    fn test_embed_produces_correct_dim() {
        let embedder = get_embedder();
        let vec = embedder.embed("JWT authentication handler");
        // Jina Code: 768-dim, AllMiniLM fallback: 384-dim
        assert!(
            vec.len() == 768 || vec.len() == 384,
            "embedding should have 768 (Jina) or 384 (MiniLM) dimensions, got {}",
            vec.len()
        );
        assert_eq!(vec.len(), embedder.dim());
    }

    #[test]
    fn test_similar_texts_high_similarity() {
        let embedder = get_embedder();
        let v1 = embedder.embed("JWT authentication");
        let v2 = embedder.embed("token validation");
        let sim = NeuralEmbedder::cosine_similarity(&v1, &v2);
        assert!(
            sim > 0.3,
            "similar texts should have cosine similarity > 0.3, got {sim:.4}"
        );
    }

    #[test]
    fn test_different_texts_low_similarity() {
        let embedder = get_embedder();
        let v1 = embedder.embed("JWT authentication");
        let v2 = embedder.embed("database migration");
        let sim = NeuralEmbedder::cosine_similarity(&v1, &v2);
        assert!(
            sim < 0.3,
            "different texts should have cosine similarity < 0.3, got {sim:.4}"
        );
    }
}
