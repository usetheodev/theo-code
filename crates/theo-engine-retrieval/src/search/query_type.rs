//! Query-type classifier — splits a search query into one of three buckets
//! used by the retrieval router (cycle-4 analysis).
//!
//! - `Identifier` — single code-like token (snake_case, camelCase or
//!   PascalCase). BM25 / lexical retrieval handles these best.
//! - `NaturalLanguage` — multiple all-lowercase English-shaped words
//!   with no code-like tokens. Dense / embedding retrieval handles
//!   these best.
//! - `Mixed` — anything containing both, or single-token queries that
//!   don't look like identifiers (e.g. an acronym, a number).
//!
//! Pure function, no allocation beyond `split_whitespace` iteration. The
//! caller (a future router in `retrieve_files`) decides which ranker to
//! invoke based on the returned variant.
//!
//! Evidence-driven: thresholds were selected from the 30-query
//! `theo-code` ground truth so every existing benchmark query maps to
//! the variant that scored higher in cycle-4 (`gap-iteration-4.md`).

/// Classification of a retrieval query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueryType {
    /// Single code-like token (snake_case, camelCase, PascalCase).
    Identifier,
    /// Multiple plain English words, no code-like tokens.
    /// Default — used for empty queries and as the placeholder when
    /// a `FileRetrievalResult` is `Default::default()`-constructed.
    #[default]
    NaturalLanguage,
    /// Mixed shapes (e.g. `BM25 scoring tokenization`,
    /// `AgentRunEngine execute`) or atypical single tokens.
    Mixed,
}

/// Classify a search query.
pub fn classify(query: &str) -> QueryType {
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return QueryType::NaturalLanguage;
    }

    let mut ident_words = 0usize;
    let mut nl_words = 0usize;

    for w in &words {
        if is_identifier_like(w) {
            ident_words += 1;
        } else if is_plain_word(w) {
            nl_words += 1;
        }
    }

    match (words.len(), ident_words, nl_words) {
        (1, 1, _) => QueryType::Identifier,
        (n, i, _) if i == n => QueryType::Identifier,
        (n, 0, p) if p == n => QueryType::NaturalLanguage,
        _ => QueryType::Mixed,
    }
}

// ---------------------------------------------------------------------------
// Internal: shape predicates
// ---------------------------------------------------------------------------

/// True for `snake_case`, `camelCase`, or `PascalCase` tokens — the shapes
/// that BM25 / tokenize_code splits effectively.
fn is_identifier_like(word: &str) -> bool {
    if word.len() < 3 {
        return false;
    }
    if !word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    contains_snake_boundary(word) || contains_camel_boundary(word)
}

/// True for tokens with at least one `_` between two alphanumerics.
fn contains_snake_boundary(word: &str) -> bool {
    let chars: Vec<char> = word.chars().collect();
    for i in 1..chars.len().saturating_sub(1) {
        if chars[i] == '_' && chars[i - 1].is_ascii_alphanumeric() && chars[i + 1].is_ascii_alphanumeric() {
            return true;
        }
    }
    false
}

/// True for tokens with at least one camelCase / PascalCase boundary, i.e.
/// a lowercase letter followed by an uppercase one (`getUserById`,
/// `AgentRunEngine`).
fn contains_camel_boundary(word: &str) -> bool {
    let chars: Vec<char> = word.chars().collect();
    for i in 1..chars.len() {
        if chars[i].is_ascii_uppercase() && chars[i - 1].is_ascii_lowercase() {
            return true;
        }
    }
    false
}

/// True for plain English-shaped words: all-lowercase ASCII letters,
/// length >= 2. Excludes pure numbers, acronyms, and code tokens.
fn is_plain_word(word: &str) -> bool {
    word.len() >= 2 && word.chars().all(|c| c.is_ascii_lowercase())
}

// ---------------------------------------------------------------------------
// Tests — calibrated against the 30-query theo-code ground truth.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_empty_is_natural_language() {
        assert_eq!(classify(""), QueryType::NaturalLanguage);
        assert_eq!(classify("   \t \n"), QueryType::NaturalLanguage);
    }

    #[test]
    fn classify_snake_case_single_token_is_identifier() {
        // theo-sym-001
        assert_eq!(classify("assemble_greedy"), QueryType::Identifier);
        // theo-sym-002
        assert_eq!(classify("propagate_attention"), QueryType::Identifier);
        // theo-sym-003 — has digit, still snake_case
        assert_eq!(classify("louvain_phase1"), QueryType::Identifier);
    }

    #[test]
    fn classify_pascal_case_single_token_is_identifier() {
        // matches `AgentRunEngine` alone
        assert_eq!(classify("AgentRunEngine"), QueryType::Identifier);
        assert_eq!(classify("TurboQuantizer"), QueryType::Identifier);
    }

    #[test]
    fn classify_camel_case_single_token_is_identifier() {
        assert_eq!(classify("getUserById"), QueryType::Identifier);
    }

    #[test]
    fn classify_pascal_plus_lowercase_word_is_mixed() {
        // theo-sym-004 — PascalCase + plain word
        assert_eq!(classify("AgentRunEngine execute"), QueryType::Mixed);
        // theo-sym-005
        assert_eq!(classify("TurboQuantizer quantize"), QueryType::Mixed);
    }

    #[test]
    fn classify_all_lowercase_words_is_natural_language() {
        // theo-mod-005
        assert_eq!(
            classify("agent loop state machine transitions"),
            QueryType::NaturalLanguage
        );
        // theo-mod-002
        assert_eq!(
            classify("community detection clustering algorithm"),
            QueryType::NaturalLanguage
        );
    }

    #[test]
    fn classify_acronym_plus_words_is_mixed() {
        // theo-xcut-002 — acronym is neither identifier-shaped nor plain
        // word, so a query with an acronym + plain words falls into Mixed.
        assert_eq!(classify("BM25 scoring tokenization"), QueryType::Mixed);
    }

    #[test]
    fn classify_short_token_alone_is_natural_language() {
        // "id" is only 2 chars — not enough signal to classify as code.
        assert_eq!(classify("id"), QueryType::NaturalLanguage);
    }

    #[test]
    fn classify_pure_acronym_alone_is_mixed() {
        // No snake/camel boundary, no lowercase shape.
        assert_eq!(classify("HTML"), QueryType::Mixed);
        assert_eq!(classify("BM25"), QueryType::Mixed);
    }

    #[test]
    fn classify_two_identifiers_is_identifier() {
        // Both tokens are code-like → Identifier.
        assert_eq!(classify("assemble_greedy AgentRunEngine"), QueryType::Identifier);
    }
}
