//! BM25 sparse vector computation for Qdrant hybrid search.
//!
//! Uses a self-contained tokenizer with FNV-1a hashing to produce sparse
//! vectors without external NLP dependencies.

use std::collections::HashMap;

/// Sparse vector dimension. Large enough to minimize hash collisions.
const SPARSE_DIM: u32 = 131_072;

/// Common English stop words to filter from tokenization.
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "from", "has", "he", "in", "is",
    "it", "its", "of", "on", "or", "she", "that", "the", "to", "was", "were", "will", "with",
    "you", "your",
];

/// FNV-1a 32-bit hash.
fn fnv1a_32(data: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &byte in data {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Tokenizes text into lowercase terms, filtering stop words and
/// single-character tokens.
#[must_use]
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .filter(|t| !STOP_WORDS.contains(t))
        .map(String::from)
        .collect()
}

/// Computes a sparse vector of `(index, weight)` pairs suitable for
/// Qdrant sparse vector storage.
///
/// Terms are mapped to indices via FNV-1a hashing. Weights use simplified
/// BM25 term frequency normalization (`k1=1.5`), without IDF or document
/// length components. Hash collisions are handled by summing weights at
/// the same index.
///
/// The output is sorted by index in ascending order (Qdrant requirement).
#[must_use]
pub fn compute_sparse_vector(text: &str) -> Vec<(u32, f32)> {
    let tokens = tokenize(text);
    if tokens.is_empty() {
        return Vec::new();
    }

    // Count term frequencies
    let mut tf_map: HashMap<u32, f32> = HashMap::new();
    for token in &tokens {
        let index = fnv1a_32(token.as_bytes()) % SPARSE_DIM;
        *tf_map.entry(index).or_default() += 1.0;
    }

    // BM25+ TF normalization: (tf * (k1 + 1)) / (tf + k1)
    const K1: f32 = 1.5;
    let mut sparse: Vec<(u32, f32)> = tf_map
        .into_iter()
        .map(|(index, tf)| {
            let weight = (tf * (K1 + 1.0)) / (tf + K1);
            (index, weight)
        })
        .collect();

    sparse.sort_unstable_by_key(|(index, _)| *index);
    sparse
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_lowercases_and_splits() {
        let tokens = tokenize("Hello World");
        assert_eq!(tokens, vec!["hello", "world"]);
    }

    #[test]
    fn tokenize_strips_stop_words() {
        let tokens = tokenize("the quick brown fox and the lazy dog");
        assert!(!tokens.contains(&"the".to_string()));
        assert!(!tokens.contains(&"and".to_string()));
        assert!(tokens.contains(&"quick".to_string()));
    }

    #[test]
    fn tokenize_skips_single_char_tokens() {
        let tokens = tokenize("I am a test");
        assert!(!tokens.contains(&"i".to_string()));
        assert!(!tokens.contains(&"a".to_string()));
        assert!(tokens.contains(&"am".to_string()));
        assert!(tokens.contains(&"test".to_string()));
    }

    #[test]
    fn sparse_vector_returns_sorted_indices() {
        let sparse = compute_sparse_vector("memory retrieval system works");
        let indices: Vec<u32> = sparse.iter().map(|(i, _)| *i).collect();
        let mut sorted = indices.clone();
        sorted.sort_unstable();
        assert_eq!(indices, sorted);
    }

    #[test]
    fn sparse_vector_empty_text_returns_empty() {
        assert!(compute_sparse_vector("").is_empty());
    }

    #[test]
    fn sparse_vector_stop_words_only_returns_empty() {
        assert!(compute_sparse_vector("the and is").is_empty());
    }

    #[test]
    fn sparse_vector_no_duplicate_indices() {
        let sparse = compute_sparse_vector("test test test different words");
        let indices: Vec<u32> = sparse.iter().map(|(i, _)| *i).collect();
        let unique: std::collections::HashSet<u32> = indices.iter().copied().collect();
        assert_eq!(indices.len(), unique.len());
    }

    #[test]
    fn sparse_vector_weights_are_positive() {
        let sparse = compute_sparse_vector("hello world test data");
        for (_, weight) in &sparse {
            assert!(*weight > 0.0);
        }
    }

    #[test]
    fn repeated_term_has_higher_weight() {
        let single = compute_sparse_vector("memory");
        let repeated = compute_sparse_vector("memory memory memory");

        // With BM25+, repeated terms should produce higher (but diminishing) weight
        let single_w = single[0].1;
        let repeated_w = repeated[0].1;
        assert!(repeated_w > single_w);
    }
}
