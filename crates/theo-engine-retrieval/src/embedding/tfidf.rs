/// TF-IDF vector space model for community documents.
///
/// Generates sparse TF-IDF vectors, then converts to dense fixed-dimension
/// vectors via random projection (Johnson-Lindenstrauss) for TurboQuant.
use std::collections::HashMap;

use crate::search::tokenise;

// ---------------------------------------------------------------------------
// Seeded RNG (LCG + Box-Muller)
// ---------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn next_gaussian(&mut self) -> f64 {
        let u1 = self.next_f64().max(1e-10);
        let u2 = self.next_f64();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Configuration for the TF-IDF vector space.
pub struct TfidfConfig {
    /// Target dimension for dense vectors (after random projection). Default: 128
    pub target_dim: usize,
    /// Minimum document frequency to include a term. Default: 1
    pub min_df: usize,
}

impl Default for TfidfConfig {
    fn default() -> Self {
        TfidfConfig {
            target_dim: 128,
            min_df: 1,
        }
    }
}

/// A built TF-IDF model with vocabulary and IDF weights.
pub struct TfidfModel {
    /// term -> index in vocabulary
    vocab: HashMap<String, usize>,
    /// IDF weight per term (log scale)
    idf: Vec<f64>,
    /// Random projection matrix (vocab_size x target_dim) stored row-major as flat vec
    projection: Vec<f64>,
    /// Target dimensionality
    target_dim: usize,
}

impl TfidfModel {
    /// Build from a set of documents (strings).
    ///
    /// 1. Tokenises each document and builds vocabulary.
    /// 2. Computes IDF weights: `log(N / df(t))`.
    /// 3. Generates a random projection matrix (vocab_size x target_dim) seeded
    ///    with 42 for reproducibility.
    pub fn build(documents: &[String], config: &TfidfConfig) -> Self {
        let n = documents.len() as f64;

        // Tokenise all documents and count document frequency per term.
        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        let tokenised: Vec<Vec<String>> = documents
            .iter()
            .map(|doc| {
                let tokens = tokenise(doc);
                // Count unique terms in this document for DF.
                let mut seen = std::collections::HashSet::new();
                for t in &tokens {
                    if seen.insert(t.clone()) {
                        *doc_freq.entry(t.clone()).or_insert(0) += 1;
                    }
                }
                tokens
            })
            .collect();

        let _ = tokenised; // used only for side-effect on doc_freq

        // Build vocabulary: only terms with df >= min_df, sorted for determinism.
        let mut terms: Vec<(String, usize)> = doc_freq
            .into_iter()
            .filter(|(_, df)| *df >= config.min_df)
            .collect();
        terms.sort_by(|a, b| a.0.cmp(&b.0));

        let mut vocab = HashMap::with_capacity(terms.len());
        let mut idf = Vec::with_capacity(terms.len());
        for (idx, (term, df)) in terms.iter().enumerate() {
            vocab.insert(term.clone(), idx);
            idf.push((n / *df as f64).ln());
        }

        let vocab_size = vocab.len();

        // Random projection matrix: vocab_size rows x target_dim cols, N(0,1).
        let mut rng = Rng::new(42);
        let total = vocab_size * config.target_dim;
        let mut projection = Vec::with_capacity(total);
        for _ in 0..total {
            projection.push(rng.next_gaussian());
        }

        TfidfModel {
            vocab,
            idf,
            projection,
            target_dim: config.target_dim,
        }
    }

    /// Transform a document (query or community text) into a dense vector.
    ///
    /// 1. Tokenise and compute TF-IDF sparse vector.
    /// 2. Multiply by random projection matrix to get dense vector of `target_dim`.
    pub fn transform(&self, document: &str) -> Vec<f64> {
        let tokens = tokenise(document);

        // Count term frequencies.
        let mut tf_counts: HashMap<&str, f64> = HashMap::new();
        for t in &tokens {
            *tf_counts.entry(t.as_str()).or_insert(0.0) += 1.0;
        }

        // Build sparse TF-IDF: only non-zero entries.
        // TF = 1 + log(raw_tf), IDF from model.
        let mut sparse: Vec<(usize, f64)> = Vec::new();
        for (term, count) in &tf_counts {
            if let Some(&idx) = self.vocab.get(*term) {
                let tf = 1.0 + count.ln();
                let tfidf = tf * self.idf[idx];
                sparse.push((idx, tfidf));
            }
        }

        // Multiply sparse vector by projection matrix (vocab_size x target_dim).
        // dense[j] = Σ_i sparse[i] * projection[i * target_dim + j]
        let mut dense = vec![0.0f64; self.target_dim];
        for &(vocab_idx, value) in &sparse {
            let row_start = vocab_idx * self.target_dim;
            for (j, d) in dense.iter_mut().enumerate() {
                *d += value * self.projection[row_start + j];
            }
        }

        dense
    }

    /// Transform and L2-normalize the result.
    pub fn transform_normalized(&self, document: &str) -> Vec<f64> {
        let mut vec = self.transform(document);
        let norm = vec.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 0.0 {
            for x in &mut vec {
                *x /= norm;
            }
        }
        vec
    }

    /// Number of terms in the vocabulary.
    pub fn vocab_size(&self) -> usize {
        self.vocab.len()
    }

    /// Target dimensionality of output vectors.
    pub fn target_dim(&self) -> usize {
        self.target_dim
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_docs() -> Vec<String> {
        vec![
            "the quick brown fox jumps over the lazy dog".to_string(),
            "the fast brown fox leaps over the sleepy dog".to_string(),
            "a cat sits on a warm mat near the window".to_string(),
            "the dog chases the cat around the yard".to_string(),
        ]
    }

    #[test]
    fn test_build_model_creates_vocab() {
        let docs = sample_docs();
        let model = TfidfModel::build(&docs, &TfidfConfig::default());
        assert!(model.vocab_size() > 0, "vocabulary must have entries");
    }

    #[test]
    fn test_transform_produces_correct_dim() {
        let docs = sample_docs();
        let config = TfidfConfig {
            target_dim: 64,
            min_df: 1,
        };
        let model = TfidfModel::build(&docs, &config);
        let vec = model.transform("the quick fox");
        assert_eq!(vec.len(), 64);
    }

    #[test]
    fn test_similar_documents_have_high_similarity() {
        let docs = sample_docs();
        let model = TfidfModel::build(&docs, &TfidfConfig::default());

        let v1 = model.transform_normalized("quick brown fox jumps");
        let v2 = model.transform_normalized("fast brown fox leaps");
        let v3 = model.transform_normalized("cat sits warm mat window");

        let sim_related: f64 = v1.iter().zip(v2.iter()).map(|(a, b)| a * b).sum();
        let sim_unrelated: f64 = v1.iter().zip(v3.iter()).map(|(a, b)| a * b).sum();

        assert!(
            sim_related > sim_unrelated,
            "related docs ({sim_related:.4}) should score higher than unrelated ({sim_unrelated:.4})"
        );
    }

    #[test]
    fn test_orthogonal_documents_have_low_similarity() {
        let docs = vec![
            "alpha beta gamma delta".to_string(),
            "epsilon zeta eta theta".to_string(),
        ];
        let model = TfidfModel::build(&docs, &TfidfConfig::default());

        let v1 = model.transform_normalized("alpha beta gamma");
        let v2 = model.transform_normalized("epsilon zeta eta");

        let sim: f64 = v1.iter().zip(v2.iter()).map(|(a, b)| a * b).sum();

        assert!(
            sim.abs() < 0.3,
            "documents with no shared terms should have near-zero similarity, got {sim:.4}"
        );
    }

    #[test]
    fn test_normalized_vectors_have_unit_norm() {
        let docs = sample_docs();
        let model = TfidfModel::build(&docs, &TfidfConfig::default());
        let v = model.transform_normalized("quick brown fox");
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-10,
            "normalized vector should have unit norm, got {norm}"
        );
    }
}
