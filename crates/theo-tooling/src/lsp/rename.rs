//! Single-tool slice extracted from `lsp/tool.rs` (T1.3 of god-files-2026-07-23-plan.md, ADR D2).

#![allow(unused_imports, dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

use crate::lsp::client::LspClient;
use crate::lsp::operations;
use crate::lsp::protocol::JsonRpcResponse;
use crate::lsp::session_manager::{LspSessionError, LspSessionManager};

use crate::lsp::tool_common::*;

// ---------------------------------------------------------------------------
// `lsp_rename`
// ---------------------------------------------------------------------------

/// `lsp_rename` — preview every file/range that would change when the
/// symbol at a position is renamed. Preview-only by design: returns
/// the `WorkspaceEdit` so the agent can inspect + apply via
/// `edit`/`apply_patch` rather than silently rewriting files.
pub struct LspRenameTool {
    manager: Arc<LspSessionManager>,
}

impl LspRenameTool {
    pub fn new(manager: Arc<LspSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for LspRenameTool {
    fn id(&self) -> &str {
        "lsp_rename"
    }

    fn description(&self) -> &str {
        "T3.1 — Preview a rename of the symbol at file_path:line:character to \
         `new_name`. Returns the LSP WorkspaceEdit (file → range → newText) so \
         the agent can inspect what WOULD change. PREVIEW-ONLY: does NOT write \
         files. To actually rename, apply each edit with `edit` or `apply_patch` \
         after reviewing. The LSP server understands scope so it changes only \
         true references — beats global find-and-replace. \
         Example: lsp_rename({file_path: \"/abs/src/lib.rs\", line: 42, character: 12, new_name: \"foo_v2\"})."
    }

    fn schema(&self) -> ToolSchema {
        let mut s = position_schema(vec![ToolParam {
            name: "new_name".into(),
            param_type: "string".into(),
            description:
                "The new identifier. The LSP server validates that the name is \
                 syntactically valid for the language; if not, the result is \
                 empty and the server may include an error message."
                    .into(),
            required: true,
        }]);
        // Override the inherited position-only example so the LLM sees a
        // complete invocation including the rename-specific `new_name`
        // arg. Without this override the JSON Schema would advertise
        // `{file_path, line, character}` and the LLM would copy it,
        // omitting `new_name`, and get InvalidArgs back.
        s.input_examples = vec![json!({
            "file_path": "/abs/src/lib.rs",
            "line": 42,
            "character": 12,
            "new_name": "foo_v2",
        })];
        s
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let pos = PositionArgs::parse(&args)?;
        let _ = extension_or_error(&pos.file_path)?;
        let new_name = args
            .get("new_name")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidArgs("missing string `new_name`".into())
            })?;
        if new_name.trim().is_empty() {
            return Err(ToolError::InvalidArgs("`new_name` is empty".into()));
        }

        let client = self
            .manager
            .ensure_client_for(&pos.file_path, &ctx.project_dir)
            .await
            .map_err(map_session_error)?;

        let uri = operations::path_to_uri(&pos.file_path.to_string_lossy());
        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": pos.line, "character": pos.character},
            "newName": new_name,
        });
        let resp = open_and_request(
            ctx,
            client.as_ref(),
            &pos.file_path,
            "textDocument/rename",
            params,
        )
        .await?;
        Ok(format_rename_output(&resp, new_name))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RenameEditPreview {
    pub uri: String,
    pub line: u64,
    pub character: u64,
    pub end_line: u64,
    pub end_character: u64,
    pub new_text: String,
}

pub fn format_rename_output(resp: &JsonRpcResponse, new_name: &str) -> ToolOutput {
    let edits = collect_rename_edits(resp.result.as_ref());
    if edits.is_empty() {
        return ToolOutput::new(
            format!("lsp_rename: no edits proposed for `{new_name}`"),
            "The LSP server returned no rename edits — the position may not be \
             a renameable symbol, or `new_name` was rejected as invalid.",
        )
        .with_metadata(json!({
            "type": "lsp_rename",
            "new_name": new_name,
            "matched": 0,
            "files_affected": 0,
            "edits": [],
        }));
    }

    let mut files: std::collections::BTreeMap<&str, usize> =
        std::collections::BTreeMap::new();
    for e in &edits {
        *files.entry(e.uri.as_str()).or_insert(0) += 1;
    }
    let files_affected = files.len();

    let mut out = format!(
        "lsp_rename: PREVIEW — {} edit(s) across {} file(s) for `{new_name}`\n\
         (PREVIEW-ONLY — call `edit` or `apply_patch` to actually rewrite files)\n\n",
        edits.len(),
        files_affected,
    );
    for (uri, count) in &files {
        out.push_str(&format!("  {uri}: {count} edit(s)\n"));
    }
    out.push('\n');
    for (i, e) in edits.iter().enumerate() {
        out.push_str(&format!(
            "{rank}. {uri}:{line}:{character} → {endline}:{endchar}  newText={text:?}\n",
            rank = i + 1,
            uri = e.uri,
            line = e.line,
            character = e.character,
            endline = e.end_line,
            endchar = e.end_character,
            text = e.new_text,
        ));
    }

    let meta_edits: Vec<Value> = edits
        .iter()
        .map(|e| {
            json!({
                "uri": e.uri,
                "line": e.line,
                "character": e.character,
                "end_line": e.end_line,
                "end_character": e.end_character,
                "new_text": e.new_text,
            })
        })
        .collect();
    ToolOutput::new(
        format!(
            "lsp_rename: PREVIEW {} edit(s) across {} file(s)",
            edits.len(),
            files_affected
        ),
        out,
    )
    .with_metadata(json!({
        "type": "lsp_rename",
        "new_name": new_name,
        "preview_only": true,
        "matched": edits.len(),
        "files_affected": files_affected,
        "edits": meta_edits,
    }))
}

/// Pull every `(uri, range, new_text)` triple out of an LSP
/// `WorkspaceEdit`. Supports BOTH shapes: `changes: {uri: TextEdit[]}`
/// (legacy) AND `documentChanges: TextDocumentEdit[]` (LSP 3.16+).
pub fn collect_rename_edits(result: Option<&Value>) -> Vec<RenameEditPreview> {
    let Some(v) = result else { return Vec::new() };
    if v.is_null() {
        return Vec::new();
    }
    let mut out = Vec::new();
    // documentChanges: [{textDocument:{uri,version}, edits:[{range,newText}]}, ...]
    if let Some(doc_changes) = v.get("documentChanges").and_then(Value::as_array) {
        for entry in doc_changes {
            // Skip CreateFile/RenameFile/DeleteFile resource ops; we
            // only render text edits in the preview.
            let Some(uri) = entry
                .get("textDocument")
                .and_then(|t| t.get("uri"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let Some(edits) = entry.get("edits").and_then(Value::as_array) else {
                continue;
            };
            for e in edits {
                if let Some(p) = parse_text_edit(uri, e) {
                    out.push(p);
                }
            }
        }
    }
    // Legacy: changes: {uri: [TextEdit]}
    if let Some(changes) = v.get("changes").and_then(Value::as_object) {
        for (uri, edits) in changes {
            let Some(arr) = edits.as_array() else { continue };
            for e in arr {
                if let Some(p) = parse_text_edit(uri, e) {
                    out.push(p);
                }
            }
        }
    }
    out
}

pub fn parse_text_edit(uri: &str, e: &Value) -> Option<RenameEditPreview> {
    let new_text = e.get("newText").and_then(Value::as_str)?.to_string();
    let range = e.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end")?;
    Some(RenameEditPreview {
        uri: uri.to_string(),
        line: start.get("line")?.as_u64()?,
        character: start.get("character")?.as_u64()?,
        end_line: end.get("line")?.as_u64()?,
        end_character: end.get("character")?.as_u64()?,
        new_text,
    })
}
