//! Code-aware tokenizer for search indexing.
//!
//! Handles code-specific patterns that standard NL tokenizers miss:
//! - camelCase splitting: `getUserById` → `[get, user, by, id]`
//! - snake_case splitting: `get_user_by_id` → `[get, user, by, id]`
//! - Path splitting: `src/auth/oauth.rs` → `[src, auth, oauth, rs]`
//! - Dotted access: `request.headers.get` → `[request, headers, get]`
//! - Rust stop word filtering: `fn`, `pub`, `struct`, `impl`, etc.
//! - Unsplit preservation: `getUserById` also indexes as `getuserbyid`
//!
//! Used by both BM25 custom and Tantivy backends.

use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Stop words — language keywords that add noise to code search
// ---------------------------------------------------------------------------

/// Rust keywords and common type names that appear in virtually every file.
/// Filtering these improves BM25 signal-to-noise ratio.
const STOP_WORDS: &[&str] = &[
    // Rust keywords
    "fn",
    "pub",
    "let",
    "mut",
    "use",
    "mod",
    "impl",
    "struct",
    "enum",
    "trait",
    "type",
    "const",
    "static",
    "self",
    "super",
    "crate",
    "where",
    "for",
    "in",
    "if",
    "else",
    "match",
    "return",
    "async",
    "await",
    "move",
    "ref",
    "as",
    "loop",
    "while",
    "break",
    "continue",
    "unsafe",
    "extern",
    "dyn",
    // Common Rust types (appear everywhere)
    "str",
    "string",
    "bool",
    "i32",
    "i64",
    "u8",
    "u16",
    "u32",
    "u64",
    "usize",
    "isize",
    "f32",
    "f64",
    // Common Rust constructs
    "option",
    "result",
    "ok",
    "err",
    "some",
    "none",
    "vec",
    "box",
    "arc",
    "true",
    "false",
    "new",
    "default",
    "clone",
    "into",
    "from",
    // TypeScript/JavaScript keywords
    "var",
    "function",
    "class",
    "interface",
    "export",
    "import",
    "require",
    "this",
    "void",
    "null",
    "undefined",
    "typeof",
    "instanceof",
    // Python keywords
    "def",
    "class",
    "import",
    "from",
    "self",
    "none",
    "true",
    "false",
    "return",
    "yield",
    "lambda",
    "pass",
    // Go keywords
    "func",
    "package",
    "import",
    "var",
    "type",
    "struct",
    "interface",
    "chan",
    "map",
    "range",
    "defer",
    "go",
    // Java keywords
    "public",
    "private",
    "protected",
    "class",
    "interface",
    "extends",
    "implements",
    "abstract",
    "final",
    "void",
    "static",
    "throws",
    // Common across languages
    "the",
    "and",
    "for",
    "with",
    "not",
    "this",
    "that",
    // File extensions (when split from paths)
    "rs",
    "py",
    "ts",
    "tsx",
    "js",
    "jsx",
    "go",
    "java",
    "kt",
    "rb",
    "php",
    "cpp",
    "hpp",
    "css",
    "html",
    "json",
    "yaml",
    "toml",
    "md",
];

/// Pre-computed stop word set for O(1) lookup.
fn stop_word_set() -> HashSet<&'static str> {
    STOP_WORDS.iter().copied().collect()
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// Tokenize code text into searchable terms.
///
/// Produces lowercase tokens with code-specific splitting.
/// Each input word produces:
/// 1. The unsplit lowercased form (if >= 3 chars) — for substring-like matching
/// 2. camelCase/PascalCase sub-tokens
/// 3. All tokens filtered through stop words
pub fn tokenize_code(text: &str) -> Vec<String> {
    let stops = stop_word_set();
    let mut tokens = Vec::new();

    // Split on non-alphanumeric (handles snake_case, paths, spaces, punctuation)
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }

        let lower = word.to_lowercase();

        // 1. Unsplit form (for "oauth" matching "openaiauth")
        if lower.len() >= 3 && !stops.contains(lower.as_str()) {
            tokens.push(lower.clone());
        }

        // 2. camelCase / PascalCase split
        let sub_tokens = split_camel_case(word);
        for sub in sub_tokens {
            let sub_lower = sub.to_lowercase();
            if sub_lower.len() >= 2 && !stops.contains(sub_lower.as_str()) {
                // Stem basic suffixes
                let stemmed = basic_stem(&sub_lower);
                if !stops.contains(stemmed.as_str()) {
                    tokens.push(stemmed);
                }
            }
        }
    }

    tokens
}

/// Tokenize with field-specific boost weights.
///
/// Returns (token, weight) pairs for BM25F indexing.
/// Filename tokens get `filename_boost`, symbol names get `symbol_boost`, etc.
pub fn tokenize_with_boost(text: &str, boost: f64) -> Vec<(String, f64)> {
    tokenize_code(text)
        .into_iter()
        .map(|t| (t, boost))
        .collect()
}

// ---------------------------------------------------------------------------
// camelCase splitter
// ---------------------------------------------------------------------------

/// Split a single identifier on camelCase/PascalCase boundaries.
///
/// `getUserById` → `["get", "User", "By", "Id"]`
/// `HTMLParser` → `["HTML", "Parser"]`
/// `getHTTPResponse` → `["get", "HTTP", "Response"]`
fn split_camel_case(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    if chars.is_empty() {
        return vec![];
    }

    let mut parts = Vec::new();
    let mut start = 0;

    for i in 1..chars.len() {
        let prev_upper = chars[i - 1].is_uppercase();
        let curr_upper = chars[i].is_uppercase();
        let next_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();

        // Split before: lowercase→uppercase (getUserById: get|User|By|Id)
        if !prev_upper && curr_upper {
            let part: String = chars[start..i].iter().collect();
            if !part.is_empty() {
                parts.push(part);
            }
            start = i;
        }
        // Split before: uppercase→uppercase+lowercase (HTMLParser: HTML|Parser)
        else if prev_upper && curr_upper && next_lower {
            let part: String = chars[start..i].iter().collect();
            if !part.is_empty() {
                parts.push(part);
            }
            start = i;
        }
    }

    // Remaining
    let part: String = chars[start..].iter().collect();
    if !part.is_empty() {
        parts.push(part);
    }

    parts
}

/// Basic stemming — remove common suffixes.
///
/// Not a full Porter stemmer — just the most impactful suffixes for code search.
fn basic_stem(word: &str) -> String {
    let w = word.to_lowercase();
    if w.len() <= 4 {
        return w;
    }

    // -ing (processing → process)
    if w.ends_with("ing") && w.len() > 5 {
        return w[..w.len() - 3].to_string();
    }
    // -tion (authentication → authenticat) — keeps enough for matching
    if w.ends_with("tion") && w.len() > 6 {
        return w[..w.len() - 3].to_string();
    }
    // -ment (management → manage)
    if w.ends_with("ment") && w.len() > 6 {
        return w[..w.len() - 4].to_string();
    }
    // -able/-ible (searchable → search)
    if (w.ends_with("able") || w.ends_with("ible")) && w.len() > 6 {
        return w[..w.len() - 4].to_string();
    }
    // -er (parser → pars)
    if w.ends_with("er") && w.len() > 4 {
        return w[..w.len() - 2].to_string();
    }
    // -ed (resolved → resolv)
    if w.ends_with("ed") && w.len() > 4 {
        return w[..w.len() - 2].to_string();
    }

    w
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_split() {
        assert_eq!(
            split_camel_case("getUserById"),
            vec!["get", "User", "By", "Id"]
        );
    }

    #[test]
    fn pascal_case_split() {
        assert_eq!(
            split_camel_case("AgentRunEngine"),
            vec!["Agent", "Run", "Engine"]
        );
    }

    #[test]
    fn acronym_split() {
        assert_eq!(split_camel_case("HTMLParser"), vec!["HTML", "Parser"]);
    }

    #[test]
    fn snake_case_tokenize() {
        let tokens = tokenize_code("get_user_by_id");
        assert!(tokens.contains(&"get".to_string()));
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"id".to_string()));
    }

    #[test]
    fn path_tokenize() {
        let tokens = tokenize_code("src/auth/oauth.rs");
        assert!(tokens.contains(&"auth".to_string()));
        assert!(tokens.contains(&"oauth".to_string()));
        // "rs" is a stop word — should be filtered
        assert!(!tokens.contains(&"rs".to_string()));
    }

    #[test]
    fn stop_words_filtered() {
        let tokens = tokenize_code("pub fn verify_token");
        // "pub" and "fn" are stop words
        assert!(!tokens.contains(&"pub".to_string()));
        assert!(!tokens.contains(&"fn".to_string()));
        // "verify" and "token" should remain
        assert!(tokens.contains(&"verify".to_string()));
        assert!(tokens.contains(&"token".to_string()));
    }

    #[test]
    fn unsplit_form_preserved() {
        let tokens = tokenize_code("OpenAIAuth");
        // Should contain both split and unsplit forms
        assert!(tokens.contains(&"openaiauth".to_string())); // unsplit
        assert!(tokens.contains(&"open".to_string())); // split
        assert!(tokens.contains(&"auth".to_string())); // split
    }

    #[test]
    fn dotted_access_tokenize() {
        let tokens = tokenize_code("request.headers.get");
        assert!(tokens.contains(&"request".to_string()));
        assert!(tokens.contains(&"headers".to_string()));
        assert!(tokens.contains(&"get".to_string()));
    }

    #[test]
    fn empty_input() {
        assert!(tokenize_code("").is_empty());
    }

    #[test]
    fn single_word() {
        let tokens = tokenize_code("propagate");
        // "propagate" doesn't match any stem rule (-ing, -tion, etc.) — stays as-is
        assert!(tokens.contains(&"propagate".to_string()));
    }

    #[test]
    fn rust_signature() {
        let tokens = tokenize_code("pub fn verify_token(token: &str) -> Result<Claims>");
        assert!(tokens.contains(&"verify".to_string()));
        assert!(tokens.contains(&"token".to_string()));
        assert!(tokens.contains(&"claims".to_string()));
        // Stop words filtered
        assert!(!tokens.contains(&"pub".to_string()));
        assert!(!tokens.contains(&"fn".to_string()));
        assert!(!tokens.contains(&"str".to_string()));
        assert!(!tokens.contains(&"result".to_string()));
    }

    #[test]
    fn boost_tokenize() {
        let boosted = tokenize_with_boost("verify_token", 5.0);
        assert!(boosted.iter().any(|(t, w)| t == "verify" && *w == 5.0));
        assert!(boosted.iter().any(|(t, w)| t == "token" && *w == 5.0));
    }

    #[test]
    fn basic_stem_works() {
        assert_eq!(basic_stem("processing"), "process");
        assert_eq!(basic_stem("authentication"), "authenticat");
        assert_eq!(basic_stem("management"), "manage");
        assert_eq!(basic_stem("searchable"), "search");
        assert_eq!(basic_stem("parser"), "pars");
        assert_eq!(basic_stem("resolved"), "resolv");
        // Short words untouched
        assert_eq!(basic_stem("get"), "get");
        assert_eq!(basic_stem("id"), "id");
    }
}
