//! Complex-task keyword set used by the rule-based classifier.
//!
//! paraphrased-from: referencias/hermes-agent/agent/smart_model_routing.py
//!   (AGPL-3.0; list re-derived from scratch based on the algorithmic
//!   spirit, not copied verbatim).
//!
//! The list is intentionally conservative — each entry names a surface
//! form an engineer would type when asking the agent to do something
//! substantive (debug, refactor, design) rather than something trivial
//! (list, show, print). Every entry is matched case-insensitively as a
//! whole word so we don't flag words like "architecturally" inside a
//! casual sentence when only "architecture" was meant as a signal.

/// Keywords that, when present in a user prompt as a whole word, push
/// the classifier from `cheap` to `default` tier.
pub const COMPLEX_KEYWORDS: &[&str] = &[
    // reasoning / debugging
    "debug",
    "analyse",
    "analyze",
    "diagnose",
    "investigate",
    "traceback",
    "exception",
    "panic",
    "deadlock",
    "race",
    // engineering
    "implement",
    "refactor",
    "design",
    "architecture",
    "optimise",
    "optimize",
    "profile",
    "benchmark",
    // review / quality
    "review",
    "audit",
    "assess",
    // testing
    "pytest",
    "cargo test",
    "docker",
    "compose",
    // data modelling
    "schema",
    "migration",
    "index",
    "query plan",
    // higher-order
    "tradeoff",
    "trade-off",
    "propose",
    "plan",
    "roadmap",
];

/// Upper bounds that characterise a "simple" turn: prompts shorter than
/// `MAX_SIMPLE_CHARS` *and* fewer words than `MAX_SIMPLE_WORDS` *and*
/// without any `COMPLEX_KEYWORDS` hit are routed to the cheap tier.
pub const MAX_SIMPLE_CHARS: usize = 160;
pub const MAX_SIMPLE_WORDS: usize = 28;

/// Case-insensitive whole-word match of any `COMPLEX_KEYWORDS` entry.
pub fn matches_complex_keyword(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    for kw in COMPLEX_KEYWORDS {
        if whole_word_contains(&lower, kw) {
            return true;
        }
    }
    false
}

fn whole_word_contains(haystack: &str, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    if needle.is_empty() {
        return false;
    }
    let hb = haystack.as_bytes();
    let nb = needle.as_bytes();
    if hb.len() < nb.len() {
        return false;
    }
    'outer: for start in 0..=(hb.len() - nb.len()) {
        if &hb[start..start + nb.len()] != nb {
            continue;
        }
        // Check boundary on the left
        if start > 0 {
            let prev = hb[start - 1] as char;
            if prev.is_alphanumeric() || prev == '_' {
                continue 'outer;
            }
        }
        // Check boundary on the right
        let end = start + nb.len();
        if end < hb.len() {
            let next = hb[end] as char;
            if next.is_alphanumeric() || next == '_' {
                continue 'outer;
            }
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_word_at_boundary() {
        assert!(matches_complex_keyword("please debug this test"));
        assert!(matches_complex_keyword("Refactor the client."));
    }

    #[test]
    fn does_not_match_substring_inside_larger_word() {
        // "debugger" contains "debug" but we want the whole word only.
        assert!(!matches_complex_keyword("the debugger attached"));
        assert!(!matches_complex_keyword("architecturally speaking"));
    }

    #[test]
    fn does_not_match_on_simple_prompts() {
        assert!(!matches_complex_keyword("list files"));
        assert!(!matches_complex_keyword("show git status"));
        assert!(!matches_complex_keyword("print hello world"));
    }

    #[test]
    fn case_insensitive_match() {
        assert!(matches_complex_keyword("DEBUG THIS"));
        assert!(matches_complex_keyword("Optimise the loop"));
    }

    #[test]
    fn empty_prompt_does_not_match() {
        assert!(!matches_complex_keyword(""));
    }
}
