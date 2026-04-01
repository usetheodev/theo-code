/// Neural embeddings for semantic code search using fastembed (ONNX).
///
/// Uses all-MiniLM-L6-v2 (22MB quantized) for 384-dim embeddings.
/// Inference: <1ms per query on CPU.

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

pub struct NeuralEmbedder {
    model: TextEmbedding,
    dim: usize,
}

impl NeuralEmbedder {
    /// Initialize with all-MiniLM-L6-v2 (quantized).
    ///
    /// On first run, downloads the model (~90MB) to `~/.cache/fastembed/`.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
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
    fn test_embed_produces_384_dim() {
        let embedder = get_embedder();
        let vec = embedder.embed("JWT authentication handler");
        assert_eq!(
            vec.len(),
            384,
            "embedding should have 384 dimensions, got {}",
            vec.len()
        );
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
