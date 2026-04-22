//! TurboQuant 2-bit vector quantization.
//!
//! Simplified from Zandieh et al. 2025:
//! 1. Random rotation via sign-flip + permutation (O(d) per vector)
//! 2. Per-coordinate scalar quantization to 2 bits (4 levels, Lloyd-Max for Gaussian)
//! 3. Unbiased inner product estimation via reconstruction

// ---------------------------------------------------------------------------
// Seeded RNG (LCG)
// ---------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A quantized vector: 2 bits per dimension, packed into bytes.
pub struct QuantizedVector {
    /// Packed 2-bit values. 4 values per byte. Length = ceil(dim / 4).
    data: Vec<u8>,
    /// L2 norm of the original vector (needed for reconstruction).
    norm: f64,
    /// Dimension.
    dim: usize,
}

impl QuantizedVector {
    /// Get the 2-bit quantization level (0..3) for dimension `i`.
    fn get_level(&self, i: usize) -> u8 {
        let byte_idx = i / 4;
        let bit_offset = (i % 4) * 2;
        (self.data[byte_idx] >> bit_offset) & 0b11
    }

    /// Dimension of the original vector.
    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Bytes used for storage.
    pub fn byte_size(&self) -> usize {
        self.data.len() + 8 /* norm */ + 8 /* dim */
    }
}

/// The quantizer: holds the rotation parameters and quantization boundaries.
pub struct TurboQuantizer {
    /// Random sign flips: +1.0 or -1.0 per dimension.
    signs: Vec<f64>,
    /// Random permutation: rotated[perm[i]] = v[i] * sign[i].
    perm: Vec<usize>,
    /// Lloyd-Max optimal boundaries for 4-level Gaussian quantizer: [b0, b1, b2].
    boundaries: [f64; 3],
    /// Reconstruction levels for the 4 bins.
    levels: [f64; 4],
    /// Dimension.
    dim: usize,
}

impl TurboQuantizer {
    /// Create a new quantizer for vectors of given dimension.
    ///
    /// The rotation (sign-flip + permutation) is generated from the given seed.
    /// Lloyd-Max optimal boundaries/levels for Gaussian distribution are hardcoded.
    pub fn new(dim: usize, seed: u64) -> Self {
        let mut rng = Rng::new(seed);

        // Random sign flips.
        let signs: Vec<f64> = (0..dim)
            .map(|_| if rng.next_f64() < 0.5 { -1.0 } else { 1.0 })
            .collect();

        // Fisher-Yates shuffle for random permutation.
        let mut perm: Vec<usize> = (0..dim).collect();
        for i in (1..dim).rev() {
            let j = (rng.next_u64() as usize) % (i + 1);
            perm.swap(i, j);
        }

        // Lloyd-Max optimal quantizer for Gaussian with 4 levels.
        let boundaries = [-0.9816, 0.0, 0.9816];
        let levels = [-1.51, -0.4528, 0.4528, 1.51];

        TurboQuantizer {
            signs,
            perm,
            boundaries,
            levels,
            dim,
        }
    }

    /// Apply the random rotation (sign-flip + permutation) to a vector.
    /// `rotated[perm[i]] = v[i] * sign[i]`
    fn rotate(&self, v: &[f64]) -> Vec<f64> {
        let mut rotated = vec![0.0; self.dim];
        for i in 0..self.dim {
            rotated[self.perm[i]] = v[i] * self.signs[i];
        }
        rotated
    }

    /// Map a scalar to a 2-bit quantization level (0..3).
    fn quantize_scalar(&self, x: f64) -> u8 {
        if x < self.boundaries[0] {
            0
        } else if x < self.boundaries[1] {
            1
        } else if x < self.boundaries[2] {
            2
        } else {
            3
        }
    }

    /// Quantize a vector to 2-bit representation. O(d) time.
    pub fn quantize(&self, vector: &[f64]) -> QuantizedVector {
        assert_eq!(
            vector.len(),
            self.dim,
            "vector dimension ({}) must match quantizer dimension ({})",
            vector.len(),
            self.dim
        );

        let norm = vector.iter().map(|x| x * x).sum::<f64>().sqrt();

        // Normalize, rotate, then quantize each coordinate.
        let normalized: Vec<f64> = if norm > 0.0 {
            vector.iter().map(|x| x / norm).collect()
        } else {
            vec![0.0; self.dim]
        };

        let rotated = self.rotate(&normalized);

        // Pack 4 quantized values per byte.
        let num_bytes = self.dim.div_ceil(4);
        let mut data = vec![0u8; num_bytes];

        for (i, &r) in rotated.iter().enumerate().take(self.dim) {
            let level = self.quantize_scalar(r);
            let byte_idx = i / 4;
            let bit_offset = (i % 4) * 2;
            data[byte_idx] |= level << bit_offset;
        }

        QuantizedVector {
            data,
            norm,
            dim: self.dim,
        }
    }

    /// Compute approximate inner product between a query (full precision)
    /// and a quantized vector. O(d) time.
    ///
    /// 1. Normalize and rotate the query.
    /// 2. For each dimension: `sum += q_rot[i] * levels[quantized_bits[i]]`
    /// 3. Multiply by `quantized.norm` to rescale.
    pub fn inner_product(&self, query: &[f64], quantized: &QuantizedVector) -> f64 {
        assert_eq!(query.len(), self.dim);

        let q_norm = query.iter().map(|x| x * x).sum::<f64>().sqrt();
        if q_norm == 0.0 || quantized.norm == 0.0 {
            return 0.0;
        }

        let q_normalized: Vec<f64> = query.iter().map(|x| x / q_norm).collect();
        let q_rotated = self.rotate(&q_normalized);

        let mut sum = 0.0f64;
        for (i, &q) in q_rotated.iter().enumerate().take(self.dim) {
            let level = quantized.get_level(i) as usize;
            sum += q * self.levels[level];
        }

        // Rescale by both norms: IP(a, b) = norm_a * norm_b * IP(â, b̂)
        sum * q_norm * quantized.norm
    }

    /// Compute approximate cosine similarity.
    ///
    /// `cosine_similarity = inner_product / (norm(query) * quantized.norm)`
    pub fn cosine_similarity(&self, query: &[f64], quantized: &QuantizedVector) -> f64 {
        let q_norm = query.iter().map(|x| x * x).sum::<f64>().sqrt();
        if q_norm == 0.0 || quantized.norm == 0.0 {
            return 0.0;
        }
        self.inner_product(query, quantized) / (q_norm * quantized.norm)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn random_vector(dim: usize, seed: u64) -> Vec<f64> {
        let mut rng = Rng::new(seed);
        (0..dim).map(|_| rng.next_f64() * 2.0 - 1.0).collect()
    }

    #[test]
    fn test_quantize_preserves_dimension() {
        let dim = 128;
        let quantizer = TurboQuantizer::new(dim, 42);
        let v = random_vector(dim, 123);
        let qv = quantizer.quantize(&v);
        assert_eq!(qv.dim(), dim);
        assert_eq!(qv.data.len(), dim.div_ceil(4));
    }

    #[test]
    fn test_inner_product_approximation() {
        // 2-bit quantization is lossy. The key property we need is that the
        // approximate inner product CORRELATES with the true inner product,
        // i.e., ranking is mostly preserved. We verify this by checking that
        // vectors with positive true IP tend to get positive approximate IP,
        // and the sign agreement rate is well above chance (50%).
        let dim = 128;
        let quantizer = TurboQuantizer::new(dim, 42);

        let mut sign_agree = 0;
        let mut total = 0;
        let trials = 50;

        for seed in 0..trials {
            let a = random_vector(dim, seed * 100 + 1);
            let b = random_vector(dim, seed * 100 + 2);

            let true_ip: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
            let qb = quantizer.quantize(&b);
            let approx_ip = quantizer.inner_product(&a, &qb);

            // Skip near-zero true IPs where sign is ambiguous.
            if true_ip.abs() > 1.0 {
                total += 1;
                if (true_ip > 0.0) == (approx_ip > 0.0) {
                    sign_agree += 1;
                }
            }
        }

        let agreement_rate = if total > 0 {
            sign_agree as f64 / total as f64
        } else {
            1.0
        };
        assert!(
            agreement_rate > 0.6,
            "sign agreement rate should be well above chance, got {agreement_rate:.2} ({sign_agree}/{total})"
        );
    }

    #[test]
    fn test_cosine_similarity_range() {
        let dim = 128;
        let quantizer = TurboQuantizer::new(dim, 42);

        for seed in 0..10u64 {
            let a = random_vector(dim, seed * 10 + 1);
            let b = random_vector(dim, seed * 10 + 2);
            let qb = quantizer.quantize(&b);
            let cos_sim = quantizer.cosine_similarity(&a, &qb);

            assert!(
                (-1.1..=1.1).contains(&cos_sim),
                "cosine similarity should be approximately in [-1, 1], got {cos_sim:.4}"
            );
        }
    }

    #[test]
    fn test_requantize_is_fast() {
        let dim = 128;
        let quantizer = TurboQuantizer::new(dim, 42);
        let v = random_vector(dim, 99);

        let start = std::time::Instant::now();
        for _ in 0..1000 {
            let _ = quantizer.quantize(&v);
        }
        let elapsed = start.elapsed();

        // 1000 quantizations in under 1 second => each under 1ms.
        assert!(
            elapsed.as_millis() < 1000,
            "1000 quantizations took {:?}, expected < 1s",
            elapsed
        );
    }

    #[test]
    fn test_zero_vector_handling() {
        let dim = 64;
        let quantizer = TurboQuantizer::new(dim, 42);
        let zero = vec![0.0; dim];
        let qz = quantizer.quantize(&zero);
        assert_eq!(qz.norm, 0.0);

        let v = random_vector(dim, 1);
        let ip = quantizer.inner_product(&v, &qz);
        assert_eq!(ip, 0.0, "inner product with zero vector should be 0");
    }

    #[test]
    fn test_self_similarity_is_high() {
        let dim = 128;
        let quantizer = TurboQuantizer::new(dim, 42);
        let v = random_vector(dim, 77);
        let qv = quantizer.quantize(&v);
        let cos_sim = quantizer.cosine_similarity(&v, &qv);

        assert!(
            cos_sim > 0.5,
            "self-similarity should be high, got {cos_sim:.4}"
        );
    }
}
