//! `docs_search` tool — LLM-callable wrapper over [`DocsIndex`].
//!
//! Holds an `Arc<DocsIndex>` so multiple registry instances can share
//! the same index without rebuilding it. The default registry-wired
//! tool starts with an empty index — callers populate it from an
//! external source via [`Self::with_index`] when constructing the
//! registry. Documented as the wiring point for crates.io / MDN / npm
//! corpora that future commits will fill.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

use crate::docs_search::index::DocsIndex;

/// LLM-callable docs search.
pub struct DocsSearchTool {
    index: Arc<DocsIndex>,
}

impl Default for DocsSearchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl DocsSearchTool {
    /// Build a tool with an empty index. Useful for tests and as a
    /// placeholder until a populated index is wired in.
    pub fn new() -> Self {
        Self {
            index: Arc::new(DocsIndex::new()),
        }
    }

    /// Build a tool over the provided index (typically populated from
    /// crates.io, MDN, npm, or other sources).
    pub fn with_index(index: Arc<DocsIndex>) -> Self {
        Self { index }
    }

    /// Total number of indexed documents.
    pub fn doc_count(&self) -> usize {
        self.index.len()
    }
}

const DEFAULT_TOP_K: usize = 5;
const MAX_TOP_K: usize = 25;

#[async_trait]
impl Tool for DocsSearchTool {
    fn id(&self) -> &str {
        "docs_search"
    }

    fn description(&self) -> &str {
        "T15.1 — Search local index of external API docs (crates.io / \
         MDN / npm / ReadTheDocs / etc.). Returns top results with \
         title, URL, snippet for the LLM to cite. Pass `query` (free \
         text), optional `source` filter (e.g., 'crates.io', 'mdn'), \
         optional `top_k` (default 5, max 25). Use to look up signatures, \
         options, deprecation notes BEFORE writing code. Example: \
         docs_search({query: 'std::env::var_os', source: 'crates.io', top_k: 3})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![
                ToolParam {
                    name: "query".into(),
                    param_type: "string".into(),
                    description: "Free-text query — supports identifiers, paths, and natural language.".into(),
                    required: true,
                },
                ToolParam {
                    name: "source".into(),
                    param_type: "string".into(),
                    description:
                        "Optional source filter: 'crates.io', 'mdn', 'npm', 'rust-std', etc."
                            .into(),
                    required: false,
                },
                ToolParam {
                    name: "top_k".into(),
                    param_type: "integer".into(),
                    description:
                        "Max results to return (default 5, max 25)."
                            .into(),
                    required: false,
                },
            ],
            input_examples: vec![
                json!({"query": "Vec::push"}),
                json!({"query": "Promise.all", "source": "mdn", "top_k": 3}),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing string `query`".into()))?
            .trim();
        if query.is_empty() {
            return Err(ToolError::InvalidArgs("`query` is empty".into()));
        }

        let source = args.get("source").and_then(Value::as_str);
        let top_k = args
            .get("top_k")
            .and_then(Value::as_u64)
            .map(|n| (n as usize).clamp(1, MAX_TOP_K))
            .unwrap_or(DEFAULT_TOP_K);

        let results = self.index.search(query, source, top_k);

        if results.is_empty() {
            return Ok(ToolOutput {
                title: format!("docs_search: no results for `{query}`"),
                output: format!(
                    "No documents matched `{query}`{filter}. \
                     Index size: {n} document(s).",
                    filter = source.map(|s| format!(" with source=`{s}`")).unwrap_or_default(),
                    n = self.index.len()
                ),
                metadata: json!({
                    "type": "docs_search",
                    "query": query,
                    "source": source,
                    "top_k": top_k,
                    "matched": 0,
                    "results": [],
                }),
                attachments: None,
                llm_suffix: None,
            });
        }

        let mut output = format!(
            "docs_search: {n} result(s) for `{query}`{filter}\n\n",
            n = results.len(),
            filter = source.map(|s| format!(" (source=`{s}`)")).unwrap_or_default(),
        );
        for (i, scored) in results.iter().enumerate() {
            output.push_str(&format!(
                "{rank}. [{src}] {title} (score {score:.2})\n   {url}\n",
                rank = i + 1,
                src = scored.doc.source,
                title = scored.doc.title,
                score = scored.score,
                url = scored.doc.url,
            ));
            // Snippet: first 200 chars of body, single line.
            let snippet: String = scored.doc.body.chars().take(200).collect();
            let snippet = snippet.replace('\n', " ");
            output.push_str(&format!("   {snippet}\n\n"));
        }

        let results_meta: Vec<Value> = results
            .iter()
            .map(|s| {
                json!({
                    "id": s.doc.id,
                    "source": s.doc.source,
                    "title": s.doc.title,
                    "url": s.doc.url,
                    "score": s.score,
                })
            })
            .collect();

        Ok(ToolOutput {
            title: format!("docs_search: {} hit(s)", results.len()),
            output,
            metadata: json!({
                "type": "docs_search",
                "query": query,
                "source": source,
                "top_k": top_k,
                "matched": results.len(),
                "results": results_meta,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use theo_domain::session::{MessageId, SessionId};

    use crate::docs_search::index::DocEntry;

    fn make_ctx(project_dir: PathBuf) -> ToolContext {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolContext {
            session_id: SessionId::new("ses_test"),
            message_id: MessageId::new(""),
            call_id: "call_test".into(),
            agent: "build".into(),
            abort: rx,
            project_dir,
            graph_context: None,
            stdout_tx: None,
        }
    }

    fn populated_index() -> Arc<DocsIndex> {
        let mut idx = DocsIndex::new();
        idx.insert(DocEntry {
            id: "rust-std::Vec".into(),
            source: "rust-std".into(),
            language: "rust".into(),
            title: "Vec".into(),
            url: "https://doc.rust-lang.org/std/vec/struct.Vec.html".into(),
            body: "A contiguous growable array type. push appends to the end.".into(),
        });
        idx.insert(DocEntry {
            id: "mdn::Array.push".into(),
            source: "mdn".into(),
            language: "javascript".into(),
            title: "Array.prototype.push".into(),
            url: "https://developer.mozilla.org/Array/push".into(),
            body: "Adds elements to the end of an array.".into(),
        });
        Arc::new(idx)
    }

    #[tokio::test]
    async fn t151_tool_search_returns_results_with_metadata() {
        let tool = DocsSearchTool::with_index(populated_index());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let result = tool
            .execute(json!({"query": "push"}), &ctx, &mut perms)
            .await
            .unwrap();
        let matched = result.metadata["matched"].as_u64().unwrap();
        assert!(matched >= 2);
        let arr = result.metadata["results"].as_array().unwrap();
        assert_eq!(arr.len() as u64, matched);
    }

    #[tokio::test]
    async fn t151_tool_search_source_filter_restricts() {
        let tool = DocsSearchTool::with_index(populated_index());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let result = tool
            .execute(json!({"query": "push", "source": "mdn"}), &ctx, &mut perms)
            .await
            .unwrap();
        let arr = result.metadata["results"].as_array().unwrap();
        for r in arr {
            assert_eq!(r["source"], "mdn");
        }
    }

    #[tokio::test]
    async fn t151_tool_search_top_k_caps_at_max() {
        let tool = DocsSearchTool::with_index(populated_index());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let result = tool
            .execute(
                json!({"query": "push", "top_k": 1000}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
        // top_k clamped to MAX_TOP_K (25) but matched limited by index size.
        let arr = result.metadata["results"].as_array().unwrap();
        assert!(arr.len() <= MAX_TOP_K);
    }

    #[tokio::test]
    async fn t151_tool_search_zero_results_graceful() {
        let tool = DocsSearchTool::with_index(populated_index());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let result = tool
            .execute(json!({"query": "no_such_term_xyz_123"}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(result.metadata["matched"], 0);
        assert!(result.output.contains("No documents matched"));
    }

    #[tokio::test]
    async fn t151_tool_missing_query_returns_invalid_args() {
        let tool = DocsSearchTool::new();
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = tool
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t151_tool_empty_query_returns_invalid_args() {
        let tool = DocsSearchTool::new();
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = tool
            .execute(json!({"query": "   "}), &ctx, &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t151_tool_output_text_contains_url() {
        let tool = DocsSearchTool::with_index(populated_index());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let result = tool
            .execute(json!({"query": "push"}), &ctx, &mut perms)
            .await
            .unwrap();
        // Output text should always include URLs so the LLM can cite.
        assert!(result.output.contains("https://"));
    }

    #[test]
    fn t151_tool_id_and_category() {
        let t = DocsSearchTool::new();
        assert_eq!(t.id(), "docs_search");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t151_tool_schema_validates() {
        DocsSearchTool::new().schema().validate().unwrap();
    }

    #[test]
    fn t151_doc_count_reflects_index_size() {
        let t = DocsSearchTool::with_index(populated_index());
        assert_eq!(t.doc_count(), 2);
    }

    #[test]
    fn t151_default_top_k_constants() {
        assert_eq!(DEFAULT_TOP_K, 5);
        assert_eq!(MAX_TOP_K, 25);
    }
}
