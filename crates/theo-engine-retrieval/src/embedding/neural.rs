/// Neural embeddings for semantic code search using fastembed (ONNX).
///
/// Default: AllMiniLM-L6-v2 (384-dim, ~200MB RAM, fast).
/// Opt-in: Jina Code v2 (768-dim, ~2.5GB RAM) via THEO_JINA_CODE=1.
///
/// AllMiniLM is the production default — works on 8GB laptops.
/// Jina Code gives +5-10% quality but costs 12x more RAM.

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

pub struct NeuralEmbedder {
    model: TextEmbedding,
    dim: usize,
}

impl NeuralEmbedder {
    /// Initialize with AllMiniLM-L6-v2 (384-dim, ~200MB).
    ///
    /// Production default: lean, fast, works on 8GB laptops.
    /// Set THEO_JINA_CODE=1 for Jina Code v2 (768-dim, +5-10% quality, ~2.5GB).
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        // THEO_JINA_CODE=1 opts into Jina Code (heavy but higher quality)
        if std::env::var("THEO_JINA_CODE").is_ok() {
            return Self::new_jina_code();
        }

        // Default: AllMiniLM (lean, ~200MB)
        Self::new_fast()
    }

    /// Initialize with AllMiniLM-L6-v2 (384-dim, ~200MB RAM).
    pub fn new_fast() -> Result<Self, Box<dyn std::error::Error>> {
        let mut options = InitOptions::default();
        options.model_name = EmbeddingModel::AllMiniLML6V2;
        options.show_download_progress = false;
        let model = TextEmbedding::try_new(options)?;
        Ok(NeuralEmbedder { model, dim: 384 })
    }

    /// Initialize with Jina Code v2 (768-dim, ~2.5GB RAM, code-trained).
    ///
    /// Higher quality than AllMiniLM for code search (+5-10% on benchmarks)
    /// but 12x more RAM. Falls back to AllMiniLM if Jina fails.
    pub fn new_jina_code() -> Result<Self, Box<dyn std::error::Error>> {
        let mut opts = InitOptions::default();
        opts.model_name = EmbeddingModel::JinaEmbeddingsV2BaseCode;
        opts.show_download_progress = true;
        match TextEmbedding::try_new(opts) {
            Ok(model) => Ok(NeuralEmbedder { model, dim: 768 }),
            Err(e) => {
                eprintln!("[neural] Jina Code failed ({e}), falling back to AllMiniLM");
                Self::new_fast()
            }
        }
    }

    /// Generate embedding for a single text.
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
