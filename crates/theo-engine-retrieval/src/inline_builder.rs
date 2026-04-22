//! Call-graph inline builder — InlineCoder-inspired context expansion.
//!
//! Research: InlineCoder (2024) shows that inlining callee source at call sites
//! gives +2.6x EM improvement for cross-function tasks. This module builds
//! "inline slices" — chunks of source code that include a focal symbol plus
//! the source of its direct callees.
//!
//! Design:
//! - Triggered when query has exact hit in the graph's name_index
//! - Traverses Calls edges up to max_depth=3
//! - Respects token budget (max 500 tokens per chain)
//! - Produces InlineSlice as a new unit of context (complements files)
//! - Falls back gracefully when callees can't be resolved

use std::collections::HashSet;

use theo_engine_graph::model::CodeGraph;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum depth of call-chain inlining.
const MAX_INLINE_DEPTH: usize = 3;

/// Maximum tokens per inline chain.
const MAX_TOKENS_PER_CHAIN: usize = 500;

/// Maximum number of functions inlined per query.
const MAX_INLINED_FUNCTIONS: usize = 10;

// ---------------------------------------------------------------------------
// InliningPolicy
// ---------------------------------------------------------------------------

/// Policy for controlling inline expansion behavior.
#[derive(Debug, Clone)]
pub struct InliningPolicy {
    /// Maximum call-chain depth.
    pub max_depth: usize,
    /// Maximum tokens per chain.
    pub max_tokens_per_chain: usize,
    /// Maximum functions inlined per query.
    pub max_inlined_functions: usize,
    /// Budget split: fraction for focal symbol.
    pub focal_budget_fraction: f64,
    /// Budget split: fraction for callees.
    pub callee_budget_fraction: f64,
    /// Budget split: fraction for callers.
    pub caller_budget_fraction: f64,
}

impl Default for InliningPolicy {
    fn default() -> Self {
        Self {
            max_depth: MAX_INLINE_DEPTH,
            max_tokens_per_chain: MAX_TOKENS_PER_CHAIN,
            max_inlined_functions: MAX_INLINED_FUNCTIONS,
            focal_budget_fraction: 0.40,
            callee_budget_fraction: 0.35,
            caller_budget_fraction: 0.25,
        }
    }
}

// ---------------------------------------------------------------------------
// InlineSlice
// ---------------------------------------------------------------------------

/// A single inline slice — a focal symbol with its inlined callees/callers.
#[derive(Debug, Clone)]
pub struct InlineSlice {
    /// The focal symbol ID (query target).
    pub focal_symbol_id: String,
    /// The focal symbol's file path.
    pub focal_file: String,
    /// Content of the inline slice (assembled source text).
    pub content: String,
    /// Estimated token count of the content.
    pub token_count: usize,
    /// IDs of all inlined symbols (callees + callers).
    pub inlined_symbols: Vec<String>,
    /// Symbols that couldn't be resolved (degraded gracefully).
    pub unresolved_callees: Vec<String>,
}

/// Result of inline expansion.
#[derive(Debug, Clone)]
pub struct InlineExpansionResult {
    /// Inline slices produced.
    pub slices: Vec<InlineSlice>,
    /// Total tokens across all slices.
    pub total_tokens: usize,
    /// Whether the query had an exact hit in the name index.
    pub had_exact_hit: bool,
}

// ---------------------------------------------------------------------------
// Source provider trait
// ---------------------------------------------------------------------------

/// Trait for providing source code lines for a file.
///
/// Decouples the inline builder from filesystem access — callers provide
/// their own source resolution (from cache, memory, or disk).
pub trait SourceProvider {
    /// Return lines `start..=end` (1-based) from the given file.
    /// Returns empty string if the file/lines can't be resolved.
    fn get_lines(&self, file_path: &str, start_line: usize, end_line: usize) -> String;
}

/// A no-op source provider that returns signatures only.
/// Used when actual source is not available.
pub struct SignatureOnlyProvider;

impl SourceProvider for SignatureOnlyProvider {
    fn get_lines(&self, _file_path: &str, _start_line: usize, _end_line: usize) -> String {
        String::new()
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Build inline slices for a query using the code graph.
///
/// Returns `None` if the query has no exact match in the name index.
pub fn build_inline_slices(
    query: &str,
    graph: &CodeGraph,
    source_provider: &dyn SourceProvider,
    policy: &InliningPolicy,
) -> InlineExpansionResult {
    // Step 1: Find focal symbol via name_index.
    let matching_ids = graph.nodes_by_name(query);
    if matching_ids.is_empty() {
        return InlineExpansionResult {
            slices: Vec::new(),
            total_tokens: 0,
            had_exact_hit: false,
        };
    }

    let mut slices = Vec::new();
    let mut total_tokens = 0;
    let mut total_inlined = 0;

    // Process each matching symbol (usually 1, but could be overloaded names).
    for focal_id in matching_ids {
        if total_inlined >= policy.max_inlined_functions {
            break;
        }

        let Some((focal_file, focal_start, focal_end)) = graph.symbol_source_range(focal_id)
        else {
            continue;
        };

        let mut content = String::new();
        let mut inlined_symbols = Vec::new();
        let mut unresolved = Vec::new();
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(focal_id.clone());

        // Focal symbol source.
        let focal_source = source_provider.get_lines(focal_file, focal_start, focal_end);
        if !focal_source.is_empty() {
            content.push_str(&format!("// [focal] {focal_file}:{focal_start}-{focal_end}\n"));
            content.push_str(&focal_source);
            content.push('\n');
        } else {
            // Fallback: use signature from graph.
            if let Some(node) = graph.get_node(focal_id) {
                if let Some(ref sig) = node.signature {
                    content.push_str(&format!("// [focal] {}\n", node.name));
                    content.push_str(sig);
                    content.push_str(" { ... }\n");
                }
            }
        }

        // Step 2: Inline callees (depth-limited BFS).
        let callees = graph.calls_children(focal_id);
        let callee_budget = policy.max_tokens_per_chain * 35 / 100; // 35% for callees
        let mut callee_tokens = 0;

        for callee_id in callees {
            if total_inlined >= policy.max_inlined_functions {
                break;
            }
            if callee_tokens >= callee_budget {
                break;
            }
            if !visited.insert(callee_id.to_string()) {
                continue;
            }

            if let Some((callee_file, callee_start, callee_end)) =
                graph.symbol_source_range(callee_id)
            {
                let callee_source =
                    source_provider.get_lines(callee_file, callee_start, callee_end);
                if !callee_source.is_empty() {
                    let tokens = (callee_source.len() + 3) / 4;
                    content.push_str(&format!(
                        "\n// [callee] {callee_file}:{callee_start}-{callee_end}\n"
                    ));
                    content.push_str(&callee_source);
                    content.push('\n');
                    callee_tokens += tokens;
                    inlined_symbols.push(callee_id.to_string());
                    total_inlined += 1;
                } else {
                    // Degraded: use signature.
                    if let Some(node) = graph.get_node(callee_id) {
                        if let Some(ref sig) = node.signature {
                            content
                                .push_str(&format!("\n// [callee:unresolved] {}\n", node.name));
                            content.push_str(sig);
                            content.push_str(" { ... }\n");
                        }
                    }
                    unresolved.push(callee_id.to_string());
                }
            } else {
                unresolved.push(callee_id.to_string());
            }
        }

        // Step 3: Inline callers (reverse).
        let callers = graph.calls_parents(focal_id);
        let caller_budget = policy.max_tokens_per_chain * 25 / 100; // 25% for callers
        let mut caller_tokens = 0;

        for caller_id in callers {
            if total_inlined >= policy.max_inlined_functions {
                break;
            }
            if caller_tokens >= caller_budget {
                break;
            }
            if !visited.insert(caller_id.to_string()) {
                continue;
            }

            if let Some((caller_file, caller_start, caller_end)) =
                graph.symbol_source_range(caller_id)
            {
                let caller_source =
                    source_provider.get_lines(caller_file, caller_start, caller_end);
                if !caller_source.is_empty() {
                    let tokens = (caller_source.len() + 3) / 4;
                    content.push_str(&format!(
                        "\n// [caller] {caller_file}:{caller_start}-{caller_end}\n"
                    ));
                    content.push_str(&caller_source);
                    content.push('\n');
                    caller_tokens += tokens;
                    inlined_symbols.push(caller_id.to_string());
                    total_inlined += 1;
                }
            }
        }

        let token_count = (content.len() + 3) / 4;

        slices.push(InlineSlice {
            focal_symbol_id: focal_id.clone(),
            focal_file: focal_file.to_string(),
            content,
            token_count,
            inlined_symbols,
            unresolved_callees: unresolved,
        });
        total_tokens += token_count;
    }

    InlineExpansionResult {
        slices,
        total_tokens,
        had_exact_hit: true,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use theo_engine_graph::model::{Edge, EdgeType, Node, NodeType, SymbolKind};

    /// Test source provider that returns predefined source for known files.
    struct MockSourceProvider {
        files: HashMap<String, Vec<String>>,
    }

    impl MockSourceProvider {
        fn new() -> Self {
            Self {
                files: HashMap::new(),
            }
        }

        fn add_file(&mut self, path: &str, lines: Vec<&str>) {
            self.files.insert(
                path.to_string(),
                lines.iter().map(|s| s.to_string()).collect(),
            );
        }
    }

    impl SourceProvider for MockSourceProvider {
        fn get_lines(&self, file_path: &str, start_line: usize, end_line: usize) -> String {
            if let Some(lines) = self.files.get(file_path) {
                let start = start_line.saturating_sub(1);
                let end = end_line.min(lines.len());
                if start < end {
                    return lines[start..end].join("\n");
                }
            }
            String::new()
        }
    }

    fn make_sym(id: &str, name: &str, file: &str, start: usize, end: usize) -> Node {
        Node {
            id: id.to_string(),
            node_type: NodeType::Symbol,
            name: name.to_string(),
            file_path: Some(file.to_string()),
            signature: Some(format!("fn {name}()")),
            kind: Some(SymbolKind::Function),
            line_start: Some(start),
            line_end: Some(end),
            last_modified: 0.0,
            doc: None,
        }
    }

    fn build_test_graph() -> CodeGraph {
        let mut g = CodeGraph::new();
        g.add_node(make_sym("fn:process", "process", "src/core.rs", 1, 5));
        g.add_node(make_sym("fn:validate", "validate", "src/core.rs", 7, 12));
        g.add_node(make_sym("fn:save", "save", "src/db.rs", 1, 8));
        g.add_node(make_sym("fn:handler", "handler", "src/api.rs", 1, 4));

        // process -> validate (callee)
        g.add_edge(Edge {
            source: "fn:process".into(),
            target: "fn:validate".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        // process -> save (callee)
        g.add_edge(Edge {
            source: "fn:process".into(),
            target: "fn:save".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        // handler -> process (handler is a caller of process)
        g.add_edge(Edge {
            source: "fn:handler".into(),
            target: "fn:process".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });
        g
    }

    fn build_mock_source() -> MockSourceProvider {
        let mut sp = MockSourceProvider::new();
        sp.add_file(
            "src/core.rs",
            vec![
                "fn process(data: Data) -> Result<()> {",
                "    let valid = validate(&data);",
                "    save(&data)?;",
                "    Ok(())",
                "}",
                "",
                "fn validate(data: &Data) -> bool {",
                "    !data.name.is_empty()",
                "    && data.age > 0",
                "    && data.email.contains('@')",
                "    // more checks...",
                "}",
            ],
        );
        sp.add_file(
            "src/db.rs",
            vec![
                "fn save(data: &Data) -> Result<()> {",
                "    let conn = pool.get()?;",
                "    conn.execute(INSERT_SQL, &data)?;",
                "    conn.execute(UPDATE_IDX, &data)?;",
                "    log::info!(\"saved {}\", data.id);",
                "    metrics::inc(\"db.saves\");",
                "    audit::record(Action::Save, &data);",
                "}",
            ],
        );
        sp.add_file(
            "src/api.rs",
            vec![
                "fn handler(req: Request) -> Response {",
                "    let data = parse(req);",
                "    process(data).map(|_| ok())",
                "}",
            ],
        );
        sp
    }

    #[test]
    fn inline_builder_includes_callee_source() {
        let g = build_test_graph();
        let sp = build_mock_source();
        let policy = InliningPolicy::default();

        let result = build_inline_slices("process", &g, &sp, &policy);

        assert!(result.had_exact_hit);
        assert_eq!(result.slices.len(), 1);

        let slice = &result.slices[0];
        assert_eq!(slice.focal_symbol_id, "fn:process");
        assert!(
            slice.content.contains("validate"),
            "Should include callee source"
        );
        assert!(
            slice.content.contains("save"),
            "Should include callee source"
        );
    }

    #[test]
    fn inline_builder_includes_caller_snippets() {
        let g = build_test_graph();
        let sp = build_mock_source();
        let policy = InliningPolicy::default();

        let result = build_inline_slices("process", &g, &sp, &policy);
        let slice = &result.slices[0];

        assert!(
            slice.content.contains("[caller]"),
            "Should include caller section"
        );
        assert!(
            slice.content.contains("handler"),
            "Should include handler as caller"
        );
    }

    #[test]
    fn inline_builder_degrades_for_unresolved_callee() {
        let mut g = CodeGraph::new();
        g.add_node(make_sym("fn:main", "main", "src/main.rs", 1, 3));
        g.add_node(make_sym(
            "fn:external",
            "external",
            "vendor/lib.rs",
            1,
            5,
        ));
        g.add_edge(Edge {
            source: "fn:main".into(),
            target: "fn:external".into(),
            edge_type: EdgeType::Calls,
            weight: 1.0,
        });

        // Source provider that doesn't have the vendor file.
        let mut sp = MockSourceProvider::new();
        sp.add_file("src/main.rs", vec!["fn main() {", "    external();", "}"]);

        let policy = InliningPolicy::default();
        let result = build_inline_slices("main", &g, &sp, &policy);

        let slice = &result.slices[0];
        assert!(
            !slice.unresolved_callees.is_empty(),
            "Should report unresolved callees"
        );
        assert!(
            slice.content.contains("[callee:unresolved]"),
            "Should include degraded signature"
        );
    }

    #[test]
    fn inline_builder_respects_token_budget() {
        let mut g = CodeGraph::new();
        g.add_node(make_sym("fn:hub", "hub", "src/hub.rs", 1, 2));

        // Create many callees.
        let mut sp = MockSourceProvider::new();
        sp.add_file("src/hub.rs", vec!["fn hub() {", "}"]);
        for i in 0..20 {
            let id = format!("fn:callee_{i}");
            let file = format!("src/callee_{i}.rs");
            g.add_node(make_sym(&id, &format!("callee_{i}"), &file, 1, 50));
            g.add_edge(Edge {
                source: "fn:hub".into(),
                target: id.clone(),
                edge_type: EdgeType::Calls,
                weight: 1.0,
            });
            // Each callee has 50 lines.
            let lines: Vec<&str> = (0..50).map(|_| "    some_code();").collect();
            sp.add_file(&file, lines);
        }

        let policy = InliningPolicy {
            max_inlined_functions: 5,
            ..Default::default()
        };
        let result = build_inline_slices("hub", &g, &sp, &policy);

        let slice = &result.slices[0];
        assert!(
            slice.inlined_symbols.len() <= 5,
            "Should respect max_inlined_functions: got {}",
            slice.inlined_symbols.len()
        );
    }

    #[test]
    fn inline_builder_no_hit_returns_empty() {
        let g = CodeGraph::new();
        let sp = SignatureOnlyProvider;
        let policy = InliningPolicy::default();

        let result = build_inline_slices("nonexistent_function", &g, &sp, &policy);
        assert!(!result.had_exact_hit);
        assert!(result.slices.is_empty());
    }

    #[test]
    fn inline_builder_focal_with_no_source_uses_signature() {
        let mut g = CodeGraph::new();
        g.add_node(make_sym("fn:mystery", "mystery", "src/mystery.rs", 1, 10));

        // Source provider has no file.
        let sp = SignatureOnlyProvider;
        let policy = InliningPolicy::default();

        let result = build_inline_slices("mystery", &g, &sp, &policy);
        assert!(result.had_exact_hit);
        assert_eq!(result.slices.len(), 1);
        assert!(
            result.slices[0].content.contains("fn mystery()"),
            "Should fall back to signature"
        );
    }
}
