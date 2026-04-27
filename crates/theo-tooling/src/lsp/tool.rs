//! T3.1 — Agent-callable LSP tool family.
//!
//! Wraps `LspSessionManager` so the agent can invoke `lsp_definition`,
//! `lsp_references`, `lsp_hover`, `lsp_rename` against the project's
//! native language servers (rust-analyzer, pyright, gopls, etc.).
//!
//! The tools share one `Arc<LspSessionManager>` constructed by the
//! registry on session start. Each tool reads the file's extension to
//! select a server, lazily spawns + initialises it on first use, and
//! caches the client across subsequent calls.
//!
//! Failure modes the agent will see:
//!   - "no LSP server installed for `.xyz`" → install a server and
//!     re-run, OR use grep/glob/codesearch as a fallback.
//!   - "file `<x>` has no extension" → not routable to LSP.
//!   - LSP server returned an error → surfaced verbatim with the
//!     server name so the agent can decide whether to retry or fall
//!     back to a different tool.

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

/// Common args every LSP positional tool accepts.
struct PositionArgs {
    file_path: PathBuf,
    line: u32,
    character: u32,
}

impl PositionArgs {
    fn parse(args: &Value) -> Result<Self, ToolError> {
        let file_path = args
            .get("file_path")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ToolError::InvalidArgs("missing string `file_path`".into())
            })?;
        let line = args
            .get("line")
            .and_then(Value::as_u64)
            .ok_or_else(|| ToolError::InvalidArgs("missing integer `line`".into()))?;
        let character = args
            .get("character")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                ToolError::InvalidArgs("missing integer `character`".into())
            })?;
        Ok(Self {
            file_path: PathBuf::from(file_path),
            line: line as u32,
            character: character as u32,
        })
    }
}

fn position_schema(extra: Vec<ToolParam>) -> ToolSchema {
    let mut params = vec![
        ToolParam {
            name: "file_path".into(),
            param_type: "string".into(),
            description:
                "Absolute path to the source file. The extension drives \
                 server selection (`.rs` → rust-analyzer, `.py` → pyright, etc.)."
                    .into(),
            required: true,
        },
        ToolParam {
            name: "line".into(),
            param_type: "integer".into(),
            description: "Zero-based line number of the symbol of interest.".into(),
            required: true,
        },
        ToolParam {
            name: "character".into(),
            param_type: "integer".into(),
            description: "Zero-based UTF-16 character offset within the line.".into(),
            required: true,
        },
    ];
    params.extend(extra);
    ToolSchema {
        params,
        input_examples: vec![json!({
            "file_path": "/abs/path/src/lib.rs",
            "line": 42,
            "character": 12,
        })],
    }
}

/// Resolve the file's extension OR surface a typed `ToolError`.
fn extension_or_error(path: &Path) -> Result<&str, ToolError> {
    path.extension()
        .and_then(|e| e.to_str())
        .ok_or_else(|| {
            ToolError::InvalidArgs(format!(
                "file `{}` has no extension; LSP routing requires one",
                path.display()
            ))
        })
}

/// Map a `LspSessionError` to a `ToolError` with an actionable message.
fn map_session_error(err: LspSessionError) -> ToolError {
    match err {
        LspSessionError::NoServerForExtension { ext } => ToolError::Execution(format!(
            "no LSP server installed for `.{ext}` files. Install one (e.g. \
             rust-analyzer for `.rs`, pyright for `.py`) or fall back to \
             grep/codesearch."
        )),
        LspSessionError::MissingExtension { path } => {
            ToolError::InvalidArgs(format!("file `{path}` has no extension"))
        }
        LspSessionError::InitializeFailed(msg) => ToolError::Execution(format!(
            "LSP `initialize` failed: {msg}"
        )),
        LspSessionError::Client(e) => ToolError::Execution(format!("LSP client error: {e}")),
    }
}

/// Send the open notification + the request, return the typed response.
/// T14.1 — emits a partial-progress envelope tagged with `method` so
/// each LSP call (definition / references / hover / rename) shows
/// up as a distinct progress line in the streaming UI. Cold first
/// calls hit the rust-analyzer initialize handshake and can take
/// several seconds; without progress the agent appears frozen.
async fn open_and_request(
    ctx: &ToolContext,
    client: &LspClient,
    file_path: &Path,
    method: &'static str,
    params: Value,
) -> Result<JsonRpcResponse, ToolError> {
    crate::partial::emit_progress(
        ctx,
        method,
        format!("Querying LSP server for {}", file_path.display()),
    );

    // Read the file so we can send `textDocument/didOpen` with the
    // current contents. LSP servers refuse to answer position queries
    // on documents they haven't seen.
    let text = std::fs::read_to_string(file_path).map_err(|e| {
        ToolError::Execution(format!(
            "could not read `{}`: {e}",
            file_path.display()
        ))
    })?;
    let uri = operations::path_to_uri(&file_path.to_string_lossy());
    let language_id = lang_id_for_extension(file_path);

    let did_open_params = json!({
        "textDocument": {
            "uri": uri,
            "languageId": language_id,
            "version": 1,
            "text": text,
        }
    });
    client
        .notify("textDocument/didOpen", Some(did_open_params))
        .await
        .map_err(|e| ToolError::Execution(format!("didOpen failed: {e}")))?;

    let resp = client
        .request(method, Some(params))
        .await
        .map_err(|e| ToolError::Execution(format!("{method} failed: {e}")))?;
    if let Some(err) = resp.error.as_ref() {
        return Err(ToolError::Execution(format!(
            "LSP server returned error: code={}, message={}",
            err.code, err.message
        )));
    }
    Ok(resp)
}

/// Map common file extensions to LSP `languageId` strings. The
/// language id MUST match the LSP spec (`rust`, `python`, `typescript`,
/// not `rs` / `py` / `ts`) — servers reject unknown ids on didOpen.
fn lang_id_for_extension(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "rs" => "rust",
        "py" | "pyi" => "python",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "go" => "go",
        "c" => "c",
        "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" | "hxx" => "cpp",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "rb" => "ruby",
        "lua" => "lua",
        _ => "plaintext",
    }
}

// ---------------------------------------------------------------------------
// `lsp_status`
// ---------------------------------------------------------------------------

/// `lsp_status` — report which LSP servers are discoverable on PATH.
/// Lets the agent decide between LSP-based navigation and a grep
/// fallback BEFORE issuing a doomed `lsp_definition` call against
/// a language with no installed server.
pub struct LspStatusTool {
    manager: Arc<LspSessionManager>,
}

impl LspStatusTool {
    pub fn new(manager: Arc<LspSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for LspStatusTool {
    fn id(&self) -> &str {
        "lsp_status"
    }

    fn description(&self) -> &str {
        "T3.1 — Report which LSP servers are discoverable on PATH and which \
         file extensions they support. Use this BEFORE `lsp_definition` / \
         `lsp_references` / `lsp_hover` / `lsp_rename` to know whether the \
         language even has a server installed; when none is available for \
         your file's extension, fall back to grep / codesearch. Empty \
         result list means no LSP server is installed at all (or PATH is \
         unusual). \
         Example: lsp_status({})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![],
            input_examples: vec![json!({})],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Search
    }

    async fn execute(
        &self,
        _args: Value,
        _ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let mut extensions = self.manager.supported_extensions().await;
        extensions.sort();

        let count = extensions.len();
        let metadata = json!({
            "type": "lsp_status",
            "supported_extension_count": count,
            "supported_extensions": extensions,
        });
        let output = if extensions.is_empty() {
            "No LSP servers discovered on PATH. Install one for the languages \
             you work with (rust-analyzer / pyright / typescript-language-server / \
             gopls / clangd / etc.) or fall back to grep / codesearch for \
             navigation."
                .to_string()
        } else {
            let listed = extensions.join(", ");
            format!(
                "{count} extension(s) routable to an installed LSP server: {listed}.\n\
                 Use lsp_definition / lsp_references / lsp_hover / lsp_rename \
                 against files matching these extensions."
            )
        };
        Ok(ToolOutput::new(
            format!("lsp_status: {count} extension(s) routable"),
            output,
        )
        .with_metadata(metadata))
    }
}

// ---------------------------------------------------------------------------
// `lsp_definition`
// ---------------------------------------------------------------------------

/// `lsp_definition` — find the source location where a symbol is defined.
pub struct LspDefinitionTool {
    manager: Arc<LspSessionManager>,
}

impl LspDefinitionTool {
    pub fn new(manager: Arc<LspSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for LspDefinitionTool {
    fn id(&self) -> &str {
        "lsp_definition"
    }

    fn description(&self) -> &str {
        "T3.1 — Jump to the definition of the symbol at file_path:line:character. \
         Uses the project's installed LSP server (rust-analyzer, pyright, gopls, \
         tsserver, clangd, etc.) — accuracy beats grep/codesearch for navigation. \
         Returns one or more {uri, range} entries the agent can read with `read`. \
         Falls back gracefully when no server is installed for the file's extension. \
         Example: lsp_definition({file_path: \"/abs/src/lib.rs\", line: 42, character: 12})."
    }

    fn schema(&self) -> ToolSchema {
        position_schema(Vec::new())
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

        let client = self
            .manager
            .ensure_client_for(&pos.file_path, &ctx.project_dir)
            .await
            .map_err(map_session_error)?;

        let uri = operations::path_to_uri(&pos.file_path.to_string_lossy());
        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": pos.line, "character": pos.character},
        });
        let resp = open_and_request(
            ctx,
            client.as_ref(),
            &pos.file_path,
            "textDocument/definition",
            params,
        )
        .await?;
        Ok(format_definition_output(&resp))
    }
}

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
        position_schema(vec![ToolParam {
            name: "new_name".into(),
            param_type: "string".into(),
            description:
                "The new identifier. The LSP server validates that the name is \
                 syntactically valid for the language; if not, the result is \
                 empty and the server may include an error message."
                    .into(),
            required: true,
        }])
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
struct RenameEditPreview {
    uri: String,
    line: u64,
    character: u64,
    end_line: u64,
    end_character: u64,
    new_text: String,
}

fn format_rename_output(resp: &JsonRpcResponse, new_name: &str) -> ToolOutput {
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
fn collect_rename_edits(result: Option<&Value>) -> Vec<RenameEditPreview> {
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

fn parse_text_edit(uri: &str, e: &Value) -> Option<RenameEditPreview> {
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

// ---------------------------------------------------------------------------
// `lsp_references`
// ---------------------------------------------------------------------------

/// `lsp_references` — find every reference to the symbol at a position.
pub struct LspReferencesTool {
    manager: Arc<LspSessionManager>,
}

impl LspReferencesTool {
    pub fn new(manager: Arc<LspSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for LspReferencesTool {
    fn id(&self) -> &str {
        "lsp_references"
    }

    fn description(&self) -> &str {
        "T3.1 — List every reference to the symbol at file_path:line:character. \
         Uses the project's installed LSP server. Pass `include_declaration: true` \
         to also include the declaration site (default: false — references only). \
         Beats grep when the symbol name is shared across modules — the LSP \
         server understands scope and returns only true references. Returns a \
         deduplicated list of {uri, line, character} entries. \
         Example: lsp_references({file_path: \"/abs/src/lib.rs\", line: 42, character: 12, include_declaration: true})."
    }

    fn schema(&self) -> ToolSchema {
        position_schema(vec![ToolParam {
            name: "include_declaration".into(),
            param_type: "boolean".into(),
            description:
                "When true, the result also includes the declaration site. Default: false."
                    .into(),
            required: false,
        }])
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
        let include_declaration = args
            .get("include_declaration")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let client = self
            .manager
            .ensure_client_for(&pos.file_path, &ctx.project_dir)
            .await
            .map_err(map_session_error)?;

        let uri = operations::path_to_uri(&pos.file_path.to_string_lossy());
        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": pos.line, "character": pos.character},
            "context": {"includeDeclaration": include_declaration},
        });
        let resp = open_and_request(
            ctx,
            client.as_ref(),
            &pos.file_path,
            "textDocument/references",
            params,
        )
        .await?;
        Ok(format_references_output(&resp, include_declaration))
    }
}

fn format_references_output(resp: &JsonRpcResponse, include_declaration: bool) -> ToolOutput {
    let entries = collect_locations(resp.result.as_ref());
    if entries.is_empty() {
        return ToolOutput::new(
            "lsp_references: no references found",
            "The LSP server returned no references for the requested position.",
        )
        .with_metadata(json!({
            "type": "lsp_references",
            "include_declaration": include_declaration,
            "matched": 0,
            "results": [],
        }));
    }
    // Dedup identical (uri, line, character) tuples — some servers
    // emit overlapping references when the call site spans multiple
    // ranges, and the user's view shouldn't be cluttered.
    let mut seen: std::collections::HashSet<(String, u64, u64)> =
        std::collections::HashSet::new();
    let entries: Vec<LocationEntry> = entries
        .into_iter()
        .filter(|e| seen.insert((e.uri.clone(), e.line, e.character)))
        .collect();

    let mut out = format!(
        "lsp_references: {} reference(s){}\n\n",
        entries.len(),
        if include_declaration {
            " (including declaration)"
        } else {
            ""
        }
    );
    for (i, e) in entries.iter().enumerate() {
        out.push_str(&format!(
            "{rank}. {uri}:{line}:{character}\n",
            rank = i + 1,
            uri = e.uri,
            line = e.line,
            character = e.character,
        ));
    }
    let meta_results: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "uri": e.uri,
                "line": e.line,
                "character": e.character,
            })
        })
        .collect();
    ToolOutput::new(format!("lsp_references: {} hit(s)", entries.len()), out).with_metadata(
        json!({
            "type": "lsp_references",
            "include_declaration": include_declaration,
            "matched": entries.len(),
            "results": meta_results,
        }),
    )
}

// ---------------------------------------------------------------------------
// `lsp_hover`
// ---------------------------------------------------------------------------

/// `lsp_hover` — return the LSP server's documentation for a symbol.
pub struct LspHoverTool {
    manager: Arc<LspSessionManager>,
}

impl LspHoverTool {
    pub fn new(manager: Arc<LspSessionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for LspHoverTool {
    fn id(&self) -> &str {
        "lsp_hover"
    }

    fn description(&self) -> &str {
        "T3.1 — Show the LSP server's hover documentation for the symbol at \
         file_path:line:character. Includes type signature, doc comments, and \
         (in some servers) examples. Cheaper than reading whole files when you \
         only need to know what a function takes / returns. \
         Example: lsp_hover({file_path: \"/abs/src/lib.rs\", line: 42, character: 12})."
    }

    fn schema(&self) -> ToolSchema {
        position_schema(Vec::new())
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

        let client = self
            .manager
            .ensure_client_for(&pos.file_path, &ctx.project_dir)
            .await
            .map_err(map_session_error)?;

        let uri = operations::path_to_uri(&pos.file_path.to_string_lossy());
        let params = json!({
            "textDocument": {"uri": uri},
            "position": {"line": pos.line, "character": pos.character},
        });
        let resp = open_and_request(
            ctx,
            client.as_ref(),
            &pos.file_path,
            "textDocument/hover",
            params,
        )
        .await?;
        Ok(format_hover_output(&resp))
    }
}

fn format_hover_output(resp: &JsonRpcResponse) -> ToolOutput {
    let result = resp.result.as_ref();
    let body = result.and_then(extract_hover_text).unwrap_or_default();
    if body.is_empty() {
        return ToolOutput::new(
            "lsp_hover: no documentation",
            "The LSP server returned no hover content for the requested position.",
        )
        .with_metadata(json!({
            "type": "lsp_hover",
            "matched": 0,
            "contents": "",
        }));
    }
    let preview = body.lines().next().unwrap_or("");
    ToolOutput::new(format!("lsp_hover: {preview}"), body.clone()).with_metadata(json!({
        "type": "lsp_hover",
        "matched": 1,
        "contents": body,
    }))
}

/// Pull the displayable text out of a hover result. LSP `Hover.contents`
/// is `MarkupContent | MarkedString | MarkedString[]`. We flatten to
/// a newline-joined string. Unknown shapes return None.
fn extract_hover_text(v: &Value) -> Option<String> {
    let contents = v.get("contents")?;
    if contents.is_null() {
        return None;
    }
    Some(flatten_contents(contents))
}

fn flatten_contents(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    // MarkedString { language, value }
    if let Some(value) = v.get("value").and_then(Value::as_str) {
        return value.to_string();
    }
    if let Some(arr) = v.as_array() {
        let parts: Vec<String> = arr.iter().map(flatten_contents).collect();
        return parts.join("\n\n");
    }
    String::new()
}

fn format_definition_output(resp: &JsonRpcResponse) -> ToolOutput {
    let result = resp.result.as_ref();
    // LSP `definition` returns Location | Location[] | LocationLink[] | null.
    // Normalise to a Vec<{uri, range}> for the metadata; render lines for the
    // text output.
    let entries = collect_locations(result);
    if entries.is_empty() {
        return ToolOutput::new(
            "lsp_definition: no definition found",
            "The LSP server returned no locations for the requested position.",
        )
        .with_metadata(json!({
            "type": "lsp_definition",
            "matched": 0,
            "results": [],
        }));
    }
    let mut out = format!("lsp_definition: {} location(s)\n\n", entries.len());
    for (i, e) in entries.iter().enumerate() {
        out.push_str(&format!(
            "{rank}. {uri}:{line}:{character}\n",
            rank = i + 1,
            uri = e.uri,
            line = e.line,
            character = e.character,
        ));
    }
    let meta_results: Vec<Value> = entries
        .iter()
        .map(|e| {
            json!({
                "uri": e.uri,
                "line": e.line,
                "character": e.character,
            })
        })
        .collect();
    ToolOutput::new(format!("lsp_definition: {} hit(s)", entries.len()), out)
        .with_metadata(json!({
            "type": "lsp_definition",
            "matched": entries.len(),
            "results": meta_results,
        }))
}

#[derive(Debug, Clone, PartialEq)]
struct LocationEntry {
    uri: String,
    line: u64,
    character: u64,
}

fn collect_locations(result: Option<&Value>) -> Vec<LocationEntry> {
    let Some(v) = result else { return Vec::new() };
    if v.is_null() {
        return Vec::new();
    }
    if v.is_array() {
        return v
            .as_array()
            .unwrap()
            .iter()
            .filter_map(extract_location)
            .collect();
    }
    extract_location(v).into_iter().collect()
}

/// Pull `(uri, line, character)` from either a `Location` or a
/// `LocationLink`. LocationLink uses `targetUri` + `targetRange`.
fn extract_location(v: &Value) -> Option<LocationEntry> {
    let (uri, range) = if let Some(uri) = v.get("uri").and_then(Value::as_str) {
        (uri.to_string(), v.get("range")?)
    } else if let Some(uri) = v.get("targetUri").and_then(Value::as_str) {
        (uri.to_string(), v.get("targetRange")?)
    } else {
        return None;
    };
    let start = range.get("start")?;
    let line = start.get("line")?.as_u64()?;
    let character = start.get("character")?.as_u64()?;
    Some(LocationEntry { uri, line, character })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use theo_domain::session::{MessageId, SessionId};

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

    fn empty_manager() -> Arc<LspSessionManager> {
        Arc::new(LspSessionManager::from_catalogue(HashMap::new()))
    }

    // ── lsp_status ────────────────────────────────────────────────

    #[test]
    fn t31lsptool_status_id_and_category() {
        let t = LspStatusTool::new(empty_manager());
        assert_eq!(t.id(), "lsp_status");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t31lsptool_status_schema_validates_with_no_args() {
        let t = LspStatusTool::new(empty_manager());
        let schema = t.schema();
        schema.validate().unwrap();
        assert!(schema.params.is_empty());
    }

    #[tokio::test]
    async fn t31lsptool_status_empty_catalogue_returns_zero_extensions() {
        // Default registry uses an empty-catalogue manager; status
        // should list zero extensions and point at grep fallback.
        let t = LspStatusTool::new(empty_manager());
        let mut perms = PermissionCollector::new();
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let out = t.execute(json!({}), &ctx, &mut perms).await.unwrap();
        assert_eq!(out.metadata["supported_extension_count"], 0);
        assert!(
            out.output.contains("No LSP servers"),
            "empty catalogue must surface the install/grep-fallback hint"
        );
        assert!(out.output.contains("grep") || out.output.contains("codesearch"));
    }

    #[tokio::test]
    async fn t31lsptool_status_lists_extensions_alphabetically_with_count() {
        // Inject a fake catalogue with two distinct extensions and
        // verify the response sorts them deterministically + counts
        // them. Extension order matters for status-line UIs that
        // diff frame-to-frame.
        use std::collections::HashMap;
        use std::path::PathBuf;

        let mut catalogue: HashMap<&'static str, crate::lsp::DiscoveredServer> =
            HashMap::new();
        catalogue.insert(
            "rs",
            crate::lsp::DiscoveredServer {
                name: "rust-analyzer",
                command: PathBuf::from("/usr/bin/rust-analyzer"),
                args: vec![],
                file_extensions: &["rs"],
                languages: &["rust"],
            },
        );
        catalogue.insert(
            "py",
            crate::lsp::DiscoveredServer {
                name: "pyright",
                command: PathBuf::from("/usr/bin/pyright-langserver"),
                args: vec!["--stdio"],
                file_extensions: &["py", "pyi"],
                languages: &["python"],
            },
        );
        let manager = Arc::new(LspSessionManager::from_catalogue(catalogue));

        let t = LspStatusTool::new(manager);
        let mut perms = PermissionCollector::new();
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let out = t.execute(json!({}), &ctx, &mut perms).await.unwrap();

        assert_eq!(out.metadata["supported_extension_count"], 2);
        let exts = out.metadata["supported_extensions"].as_array().unwrap();
        assert_eq!(exts.len(), 2);
        // Alphabetical ordering: py before rs.
        assert_eq!(exts[0], "py");
        assert_eq!(exts[1], "rs");
        // Output text mentions both and points the agent at the
        // navigation tools.
        assert!(out.output.contains("py"));
        assert!(out.output.contains("rs"));
        assert!(out.output.contains("lsp_definition"));
    }

    #[test]
    fn t31lsptool_definition_id_and_category() {
        let t = LspDefinitionTool::new(empty_manager());
        assert_eq!(t.id(), "lsp_definition");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t31lsptool_definition_schema_validates() {
        let t = LspDefinitionTool::new(empty_manager());
        t.schema().validate().unwrap();
    }

    #[test]
    fn t31lsptool_position_schema_lists_required_params() {
        let schema = position_schema(Vec::new());
        let names: Vec<_> = schema.params.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"file_path"));
        assert!(names.contains(&"line"));
        assert!(names.contains(&"character"));
        // All three are required.
        for p in &schema.params {
            assert!(p.required, "{} should be required", p.name);
        }
    }

    #[tokio::test]
    async fn t31lsptool_missing_file_path_returns_invalid_args() {
        let t = LspDefinitionTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(json!({"line": 1, "character": 1}), &ctx, &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t31lsptool_missing_line_returns_invalid_args() {
        let t = LspDefinitionTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"file_path": "/tmp/x.rs", "character": 1}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t31lsptool_missing_character_returns_invalid_args() {
        let t = LspDefinitionTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"file_path": "/tmp/x.rs", "line": 1}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t31lsptool_extensionless_file_returns_invalid_args() {
        let t = LspDefinitionTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"file_path": "/tmp/Makefile", "line": 0, "character": 0}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => assert!(msg.contains("no extension")),
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t31lsptool_unknown_extension_returns_actionable_execution_error() {
        // Empty manager has no servers — any known extension also
        // hits NoServerForExtension. Verify the user-facing message
        // is actionable (mentions installing a server or fallback).
        let t = LspDefinitionTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"file_path": "/tmp/x.rs", "line": 0, "character": 0}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => {
                assert!(msg.contains("no LSP server installed"));
                assert!(msg.contains("`.rs`"));
                // Actionable: tell the agent what to do instead.
                assert!(msg.contains("rust-analyzer") || msg.contains("grep"));
            }
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    #[test]
    fn t31lsptool_lang_id_for_known_extensions() {
        assert_eq!(lang_id_for_extension(Path::new("/x.rs")), "rust");
        assert_eq!(lang_id_for_extension(Path::new("/x.py")), "python");
        assert_eq!(lang_id_for_extension(Path::new("/x.ts")), "typescript");
        assert_eq!(lang_id_for_extension(Path::new("/x.tsx")), "typescript");
        assert_eq!(lang_id_for_extension(Path::new("/x.js")), "javascript");
        assert_eq!(lang_id_for_extension(Path::new("/x.go")), "go");
        assert_eq!(lang_id_for_extension(Path::new("/x.cpp")), "cpp");
        assert_eq!(lang_id_for_extension(Path::new("/x.java")), "java");
        assert_eq!(lang_id_for_extension(Path::new("/x.rb")), "ruby");
    }

    #[test]
    fn t31lsptool_lang_id_for_unknown_extension_falls_back_to_plaintext() {
        assert_eq!(lang_id_for_extension(Path::new("/x.xyz")), "plaintext");
        assert_eq!(lang_id_for_extension(Path::new("/no_ext")), "plaintext");
    }

    #[test]
    fn t31lsptool_extract_location_handles_location_shape() {
        let v = json!({
            "uri": "file:///abs/x.rs",
            "range": {
                "start": {"line": 10, "character": 4},
                "end":   {"line": 10, "character": 9},
            }
        });
        let loc = extract_location(&v).unwrap();
        assert_eq!(loc.uri, "file:///abs/x.rs");
        assert_eq!(loc.line, 10);
        assert_eq!(loc.character, 4);
    }

    #[test]
    fn t31lsptool_extract_location_handles_location_link_shape() {
        // LSP 3.14+: `LocationLink` uses `targetUri` + `targetRange`
        // instead of `uri` + `range`. Easy to miss.
        let v = json!({
            "originSelectionRange": {"start":{"line":1,"character":2},"end":{"line":1,"character":3}},
            "targetUri": "file:///abs/y.rs",
            "targetRange": {"start":{"line":20,"character":0},"end":{"line":25,"character":5}},
            "targetSelectionRange": {"start":{"line":20,"character":0},"end":{"line":20,"character":3}},
        });
        let loc = extract_location(&v).unwrap();
        assert_eq!(loc.uri, "file:///abs/y.rs");
        assert_eq!(loc.line, 20);
        assert_eq!(loc.character, 0);
    }

    #[test]
    fn t31lsptool_extract_location_returns_none_for_unknown_shape() {
        let v = json!({"random": "shape"});
        assert!(extract_location(&v).is_none());
    }

    #[test]
    fn t31lsptool_collect_locations_handles_array_response() {
        let v = json!([
            {"uri":"file:///a","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":3}}},
            {"uri":"file:///b","range":{"start":{"line":4,"character":5},"end":{"line":4,"character":6}}},
        ]);
        let locs = collect_locations(Some(&v));
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[0].uri, "file:///a");
        assert_eq!(locs[1].uri, "file:///b");
    }

    #[test]
    fn t31lsptool_collect_locations_handles_single_location_response() {
        let v = json!({
            "uri":"file:///a",
            "range":{"start":{"line":1,"character":2},"end":{"line":1,"character":3}}
        });
        let locs = collect_locations(Some(&v));
        assert_eq!(locs.len(), 1);
    }

    #[test]
    fn t31lsptool_collect_locations_handles_null_response() {
        let v = serde_json::Value::Null;
        let locs = collect_locations(Some(&v));
        assert!(locs.is_empty());
    }

    #[test]
    fn t31lsptool_collect_locations_handles_missing_result() {
        let locs = collect_locations(None);
        assert!(locs.is_empty());
    }

    #[test]
    fn t31lsptool_format_output_includes_count_and_uris() {
        // Build a fake response with two locations.
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(json!([
                {"uri":"file:///a","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":3}}},
                {"uri":"file:///b","range":{"start":{"line":4,"character":5},"end":{"line":4,"character":6}}},
            ])),
            error: None,
        };
        let out = format_definition_output(&resp);
        assert!(out.output.contains("2 location(s)"));
        assert!(out.output.contains("file:///a:1:2"));
        assert!(out.output.contains("file:///b:4:5"));
        assert_eq!(out.metadata["matched"], 2);
    }

    #[test]
    fn t31lsptool_format_output_handles_no_locations_gracefully() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(serde_json::Value::Null),
            error: None,
        };
        let out = format_definition_output(&resp);
        assert!(out.title.contains("no definition found"));
        assert_eq!(out.metadata["matched"], 0);
    }

    // ── lsp_references ────────────────────────────────────────────

    #[test]
    fn t31lsptool_references_id_and_category() {
        let t = LspReferencesTool::new(empty_manager());
        assert_eq!(t.id(), "lsp_references");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t31lsptool_references_schema_validates_and_includes_optional_flag() {
        let t = LspReferencesTool::new(empty_manager());
        let schema = t.schema();
        schema.validate().unwrap();
        let names: Vec<_> = schema.params.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"include_declaration"));
        let inc = schema
            .params
            .iter()
            .find(|p| p.name == "include_declaration")
            .unwrap();
        assert!(!inc.required, "include_declaration must be optional");
    }

    #[tokio::test]
    async fn t31lsptool_references_extensionless_file_returns_invalid_args() {
        let t = LspReferencesTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"file_path": "/tmp/Makefile", "line": 0, "character": 0}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t31lsptool_references_unknown_extension_returns_actionable_error() {
        let t = LspReferencesTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"file_path": "/tmp/x.rs", "line": 0, "character": 0}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => assert!(msg.contains("no LSP server installed")),
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    #[test]
    fn t31lsptool_format_references_includes_count_and_dedups() {
        // Same (uri, line, character) twice — must collapse to 1.
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(json!([
                {"uri":"file:///a","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":3}}},
                {"uri":"file:///a","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":4}}},
                {"uri":"file:///b","range":{"start":{"line":4,"character":5},"end":{"line":4,"character":6}}},
            ])),
            error: None,
        };
        let out = format_references_output(&resp, false);
        assert_eq!(out.metadata["matched"], 2, "duplicate (a,1,2) collapses");
        assert!(out.output.contains("file:///a:1:2"));
        assert!(out.output.contains("file:///b:4:5"));
    }

    #[test]
    fn t31lsptool_format_references_with_declaration_marks_metadata() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(json!([
                {"uri":"file:///a","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":3}}},
            ])),
            error: None,
        };
        let out = format_references_output(&resp, true);
        assert_eq!(out.metadata["include_declaration"], true);
        assert!(out.output.contains("(including declaration)"));
    }

    #[test]
    fn t31lsptool_format_references_handles_no_results_gracefully() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(serde_json::Value::Null),
            error: None,
        };
        let out = format_references_output(&resp, false);
        assert!(out.title.contains("no references found"));
        assert_eq!(out.metadata["matched"], 0);
    }

    // ── lsp_hover ─────────────────────────────────────────────────

    #[test]
    fn t31lsptool_hover_id_and_category() {
        let t = LspHoverTool::new(empty_manager());
        assert_eq!(t.id(), "lsp_hover");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t31lsptool_hover_schema_validates() {
        let t = LspHoverTool::new(empty_manager());
        t.schema().validate().unwrap();
    }

    #[tokio::test]
    async fn t31lsptool_hover_unknown_extension_returns_actionable_error() {
        let t = LspHoverTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"file_path": "/tmp/x.rs", "line": 0, "character": 0}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => assert!(msg.contains("no LSP server installed")),
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    #[test]
    fn t31lsptool_extract_hover_text_handles_markup_content() {
        // MarkupContent { kind: "markdown", value: "..." }
        let v = json!({
            "contents": {"kind": "markdown", "value": "fn foo(x: u32) -> bool"}
        });
        let text = extract_hover_text(&v).unwrap();
        assert_eq!(text, "fn foo(x: u32) -> bool");
    }

    #[test]
    fn t31lsptool_extract_hover_text_handles_marked_string_array() {
        // MarkedString[] — older LSP servers (deprecated but real).
        let v = json!({
            "contents": [
                {"language": "rust", "value": "fn foo(x: u32) -> bool"},
                "Returns true when even.",
            ]
        });
        let text = extract_hover_text(&v).unwrap();
        assert!(text.contains("fn foo(x: u32) -> bool"));
        assert!(text.contains("Returns true when even."));
    }

    #[test]
    fn t31lsptool_extract_hover_text_handles_plain_string() {
        // MarkedString as a bare string (legacy).
        let v = json!({"contents": "plain doc string"});
        let text = extract_hover_text(&v).unwrap();
        assert_eq!(text, "plain doc string");
    }

    #[test]
    fn t31lsptool_extract_hover_text_returns_none_for_null() {
        let v = json!({"contents": null});
        assert!(extract_hover_text(&v).is_none());
    }

    #[test]
    fn t31lsptool_extract_hover_text_returns_none_for_missing_contents() {
        let v = json!({"unrelated": "field"});
        assert!(extract_hover_text(&v).is_none());
    }

    #[test]
    fn t31lsptool_format_hover_uses_first_line_as_title() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(json!({
                "contents": {"kind":"markdown","value":"fn first_line\nbody continues"}
            })),
            error: None,
        };
        let out = format_hover_output(&resp);
        assert!(out.title.contains("fn first_line"));
        assert_eq!(out.metadata["matched"], 1);
        assert_eq!(out.metadata["contents"], "fn first_line\nbody continues");
    }

    #[test]
    fn t31lsptool_format_hover_handles_empty_response_gracefully() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(serde_json::Value::Null),
            error: None,
        };
        let out = format_hover_output(&resp);
        assert!(out.title.contains("no documentation"));
        assert_eq!(out.metadata["matched"], 0);
    }

    // ── lsp_rename ────────────────────────────────────────────────

    #[test]
    fn t31lsptool_rename_id_and_category() {
        let t = LspRenameTool::new(empty_manager());
        assert_eq!(t.id(), "lsp_rename");
        assert_eq!(t.category(), ToolCategory::Search);
    }

    #[test]
    fn t31lsptool_rename_schema_validates_and_includes_new_name() {
        let t = LspRenameTool::new(empty_manager());
        let schema = t.schema();
        schema.validate().unwrap();
        let names: Vec<_> = schema.params.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"new_name"));
        let nn = schema.params.iter().find(|p| p.name == "new_name").unwrap();
        assert!(nn.required, "new_name must be required");
    }

    #[tokio::test]
    async fn t31lsptool_rename_missing_new_name_returns_invalid_args() {
        let t = LspRenameTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({"file_path": "/tmp/x.rs", "line": 0, "character": 0}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[tokio::test]
    async fn t31lsptool_rename_empty_new_name_returns_invalid_args() {
        let t = LspRenameTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({
                    "file_path": "/tmp/x.rs",
                    "line": 0,
                    "character": 0,
                    "new_name": "   "
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::InvalidArgs(msg) => assert!(msg.contains("`new_name` is empty")),
            other => panic!("expected InvalidArgs, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn t31lsptool_rename_unknown_extension_returns_actionable_error() {
        let t = LspRenameTool::new(empty_manager());
        let ctx = make_ctx(PathBuf::from("/tmp"));
        let mut perms = PermissionCollector::new();
        let err = t
            .execute(
                json!({
                    "file_path": "/tmp/x.rs",
                    "line": 0,
                    "character": 0,
                    "new_name": "foo_v2"
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap_err();
        match err {
            ToolError::Execution(msg) => assert!(msg.contains("no LSP server installed")),
            other => panic!("expected Execution error, got {other:?}"),
        }
    }

    #[test]
    fn t31lsptool_collect_rename_edits_handles_legacy_changes_shape() {
        // LSP < 3.16: changes: {uri: TextEdit[]}
        let v = json!({
            "changes": {
                "file:///a": [
                    {"range":{"start":{"line":1,"character":2},"end":{"line":1,"character":7}},"newText":"foo"},
                    {"range":{"start":{"line":3,"character":0},"end":{"line":3,"character":3}},"newText":"foo"},
                ],
                "file:///b": [
                    {"range":{"start":{"line":5,"character":4},"end":{"line":5,"character":9}},"newText":"foo"},
                ],
            }
        });
        let edits = collect_rename_edits(Some(&v));
        assert_eq!(edits.len(), 3);
        // Edits with uri=file:///a are 2; uri=file:///b is 1.
        let a_count = edits.iter().filter(|e| e.uri == "file:///a").count();
        let b_count = edits.iter().filter(|e| e.uri == "file:///b").count();
        assert_eq!(a_count, 2);
        assert_eq!(b_count, 1);
    }

    #[test]
    fn t31lsptool_collect_rename_edits_handles_document_changes_shape() {
        // LSP 3.16+: documentChanges: TextDocumentEdit[]
        let v = json!({
            "documentChanges": [
                {
                    "textDocument": {"uri":"file:///c","version":1},
                    "edits": [
                        {"range":{"start":{"line":7,"character":0},"end":{"line":7,"character":5}},"newText":"bar"},
                    ]
                },
                {
                    "textDocument": {"uri":"file:///d","version":1},
                    "edits": [
                        {"range":{"start":{"line":9,"character":2},"end":{"line":9,"character":6}},"newText":"bar"},
                        {"range":{"start":{"line":12,"character":0},"end":{"line":12,"character":3}},"newText":"bar"},
                    ]
                }
            ]
        });
        let edits = collect_rename_edits(Some(&v));
        assert_eq!(edits.len(), 3);
        // Verify endLine/endChar were captured (we render them in the
        // preview output).
        let c_edit = edits.iter().find(|e| e.uri == "file:///c").unwrap();
        assert_eq!(c_edit.line, 7);
        assert_eq!(c_edit.end_character, 5);
        assert_eq!(c_edit.new_text, "bar");
    }

    #[test]
    fn t31lsptool_collect_rename_edits_skips_resource_ops_in_document_changes() {
        // documentChanges can contain CreateFile/RenameFile/DeleteFile
        // ops alongside text edits. We only render text edits.
        let v = json!({
            "documentChanges": [
                {"kind": "rename", "oldUri": "file:///old", "newUri": "file:///new"},
                {
                    "textDocument": {"uri":"file:///e","version":1},
                    "edits": [
                        {"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":3}},"newText":"new"},
                    ]
                }
            ]
        });
        let edits = collect_rename_edits(Some(&v));
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].uri, "file:///e");
    }

    #[test]
    fn t31lsptool_collect_rename_edits_handles_null_response() {
        let v = serde_json::Value::Null;
        let edits = collect_rename_edits(Some(&v));
        assert!(edits.is_empty());
    }

    #[test]
    fn t31lsptool_collect_rename_edits_handles_missing_response() {
        let edits = collect_rename_edits(None);
        assert!(edits.is_empty());
    }

    #[test]
    fn t31lsptool_format_rename_marks_preview_only_in_metadata() {
        // PREVIEW-ONLY safety invariant: metadata.preview_only must
        // be true so the agent (and any downstream auditor) knows
        // this tool didn't write files.
        let v = json!({
            "changes": {
                "file:///a": [
                    {"range":{"start":{"line":1,"character":2},"end":{"line":1,"character":7}},"newText":"foo_v2"},
                ]
            }
        });
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(v),
            error: None,
        };
        let out = format_rename_output(&resp, "foo_v2");
        assert_eq!(out.metadata["preview_only"], true);
        assert_eq!(out.metadata["new_name"], "foo_v2");
        assert_eq!(out.metadata["matched"], 1);
        assert_eq!(out.metadata["files_affected"], 1);
        assert!(out.output.contains("PREVIEW-ONLY"));
        assert!(out.output.contains("`edit` or `apply_patch`"));
    }

    #[test]
    fn t31lsptool_format_rename_groups_edits_by_file_in_summary() {
        let v = json!({
            "changes": {
                "file:///a": [
                    {"range":{"start":{"line":1,"character":2},"end":{"line":1,"character":7}},"newText":"X"},
                    {"range":{"start":{"line":3,"character":0},"end":{"line":3,"character":3}},"newText":"X"},
                ],
                "file:///b": [
                    {"range":{"start":{"line":5,"character":4},"end":{"line":5,"character":9}},"newText":"X"},
                ]
            }
        });
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(v),
            error: None,
        };
        let out = format_rename_output(&resp, "X");
        // Per-file summary lines: "  file:///a: 2 edit(s)"
        assert!(out.output.contains("file:///a: 2 edit(s)"));
        assert!(out.output.contains("file:///b: 1 edit(s)"));
        assert_eq!(out.metadata["files_affected"], 2);
        assert_eq!(out.metadata["matched"], 3);
    }

    #[test]
    fn t31lsptool_format_rename_no_edits_path_keeps_preview_only_flag() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: 1,
            result: Some(serde_json::Value::Null),
            error: None,
        };
        let out = format_rename_output(&resp, "anything");
        assert_eq!(out.metadata["matched"], 0);
        assert_eq!(out.metadata["files_affected"], 0);
        assert!(out.title.contains("no edits proposed"));
    }
}
