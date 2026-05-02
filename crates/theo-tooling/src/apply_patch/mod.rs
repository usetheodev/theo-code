use async_trait::async_trait;
use std::path::{Path, PathBuf};
use theo_domain::error::ToolError;
use theo_domain::permission::{PermissionRequest, PermissionType};
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
    require_string,
};

pub struct ApplyPatchTool;

#[derive(Debug, Clone)]
enum PatchOp {
    Add {
        path: String,
        content: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        hunks: Vec<Hunk>,
        move_to: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct Hunk {
    context_header: Option<String>,
    lines: Vec<HunkLine>,
    eof_anchor: bool,
}

#[derive(Debug, Clone)]
enum HunkLine {
    Context(String),
    Add(String),
    Remove(String),
}

impl Default for ApplyPatchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ApplyPatchTool {
    pub fn new() -> Self {
        Self
    }

    /// Canonicalize a patch-relative path against `project_dir`.
    ///
    /// Mirrors read/write/edit (T2.3): `..`, `.`, and symlinks are resolved
    /// via `crate::path::absolutize`. The permission gate is separate
    /// (`record_external_if_escapes`) so each PatchOp can decide its own
    /// response to an out-of-workspace target.
    fn resolve_path(file_path: &str, project_dir: &Path) -> PathBuf {
        match crate::path::absolutize(project_dir, file_path) {
            Ok(canonical) => canonical,
            Err(_) => project_dir.join(file_path),
        }
    }

    /// Record an `ExternalDirectory` permission request if `resolved`
    /// escapes `project_dir`. Returns true when the path is inside.
    fn record_external_if_escapes(
        resolved: &Path,
        project_dir: &Path,
        permissions: &mut PermissionCollector,
    ) -> bool {
        let inside = crate::path::is_contained(resolved, project_dir)
            .unwrap_or_else(|_| resolved.starts_with(project_dir));
        if !inside {
            let dir = resolved.parent().unwrap_or(resolved);
            let pattern = format!("{}/*", dir.display()).replace('\\', "/");
            permissions.record(PermissionRequest {
                permission: PermissionType::ExternalDirectory,
                patterns: vec![pattern.clone()],
                always: vec![pattern],
                metadata: serde_json::json!({}),
            });
        }
        inside
    }

    fn strip_heredoc(text: &str) -> &str {
        let trimmed = text.trim();
        // Strip cat <<'EOF' ... EOF wrapper
        if let Some(rest) = trimmed.strip_prefix("cat <<")
            && let Some(body_start) = rest.find('\n') {
                let body = &rest[body_start + 1..];
                if let Some(eof_pos) = body.rfind("\nEOF") {
                    return &body[..eof_pos];
                }
                if let Some(stripped) = body.strip_suffix("EOF") {
                    return stripped;
                }
            }
        if let Some(rest) = trimmed.strip_prefix("<<")
            && let Some(body_start) = rest.find('\n') {
                let body = &rest[body_start + 1..];
                if let Some(eof_pos) = body.rfind("\nEOF") {
                    return &body[..eof_pos];
                }
                if let Some(stripped) = body.strip_suffix("EOF") {
                    return stripped;
                }
            }
        trimmed
    }

    /// Locate the line index just AFTER `*** Begin Patch`. Returns
    /// `Validation` error if the marker is missing.
    fn skip_to_begin_patch(lines: &[&str]) -> Result<usize, ToolError> {
        let mut i = 0;
        while i < lines.len() && !lines[i].starts_with("*** Begin Patch") {
            i += 1;
        }
        if i >= lines.len() {
            return Err(ToolError::Validation(
                "apply_patch verification failed: no Begin Patch marker".to_string(),
            ));
        }
        Ok(i + 1)
    }

    /// Parse a `*** Add File: <path>` section starting at `start_idx`.
    /// Returns the resulting `PatchOp::Add` and the index just past the section.
    fn parse_add_section(path_ref: &str, lines: &[&str], start_idx: usize) -> (PatchOp, usize) {
        let path = path_ref.to_string();
        let mut i = start_idx;
        let mut content_lines = Vec::new();
        while i < lines.len() && !lines[i].starts_with("***") {
            if let Some(rest) = lines[i].strip_prefix('+') {
                content_lines.push(rest.to_string());
            }
            i += 1;
        }
        let mut content = content_lines.join("\n");
        if !content.is_empty() {
            content.push('\n');
        }
        (PatchOp::Add { path, content }, i)
    }

    /// Parse a `*** Update File: <path>` section (optional `*** Move to:`
    /// header + 1+ hunks). Returns the `PatchOp::Update` and next index.
    fn parse_update_section(
        path_ref: &str,
        lines: &[&str],
        start_idx: usize,
    ) -> Result<(PatchOp, usize), ToolError> {
        let path = path_ref.to_string();
        let mut i = start_idx;
        let mut move_to = None;
        if i < lines.len()
            && let Some(dest_ref) = lines[i].strip_prefix("*** Move to: ")
        {
            move_to = Some(dest_ref.to_string());
            i += 1;
        }
        let mut hunks = Vec::new();
        while i < lines.len()
            && (lines[i].starts_with("@@")
                || lines[i].starts_with(' ')
                || lines[i].starts_with('+')
                || lines[i].starts_with('-')
                || lines[i].starts_with("*** End of File"))
        {
            if lines[i].starts_with("@@") {
                let (hunk, next) = Self::parse_hunk(lines, i);
                hunks.push(hunk);
                i = next;
            } else {
                i += 1;
            }
        }
        if hunks.is_empty() {
            return Err(ToolError::Validation(
                "apply_patch verification failed: empty hunks".to_string(),
            ));
        }
        Ok((
            PatchOp::Update {
                path,
                hunks,
                move_to,
            },
            i,
        ))
    }

    /// Parse a single hunk starting at `@@`. Returns the `Hunk` and the
    /// index just past the hunk's last line (including any `*** End of File`).
    fn parse_hunk(lines: &[&str], start_idx: usize) -> (Hunk, usize) {
        let header_line = lines[start_idx];
        let ctx_header = if header_line.len() > 2 {
            Some(header_line[2..].trim().to_string())
        } else {
            None
        };
        let mut i = start_idx + 1;
        let mut hunk_lines = Vec::new();
        let mut eof_anchor = false;
        while i < lines.len() && !lines[i].starts_with("@@") && !lines[i].starts_with("***") {
            let l = lines[i];
            if let Some(rest) = l.strip_prefix('+') {
                hunk_lines.push(HunkLine::Add(rest.to_string()));
            } else if let Some(rest) = l.strip_prefix('-') {
                hunk_lines.push(HunkLine::Remove(rest.to_string()));
            } else if let Some(rest) = l.strip_prefix(' ') {
                hunk_lines.push(HunkLine::Context(rest.to_string()));
            }
            i += 1;
        }
        if i < lines.len() && lines[i].starts_with("*** End of File") {
            eof_anchor = true;
            i += 1;
        }
        (
            Hunk {
                context_header: ctx_header.filter(|s| !s.is_empty()),
                lines: hunk_lines,
                eof_anchor,
            },
            i,
        )
    }

    fn parse(text: &str) -> Result<Vec<PatchOp>, ToolError> {
        let text = Self::strip_heredoc(text);
        let lines: Vec<&str> = text.lines().collect();
        let mut ops = Vec::new();
        let mut i = Self::skip_to_begin_patch(&lines)?;

        while i < lines.len() {
            let line = lines[i];
            if line.starts_with("*** End Patch") {
                break;
            }
            if let Some(path_ref) = line.strip_prefix("*** Add File: ") {
                let (op, next) = Self::parse_add_section(path_ref, &lines, i + 1);
                ops.push(op);
                i = next;
            } else if let Some(path_ref) = line.strip_prefix("*** Delete File: ") {
                ops.push(PatchOp::Delete {
                    path: path_ref.to_string(),
                });
                i += 1;
            } else if let Some(path_ref) = line.strip_prefix("*** Update File: ") {
                let (op, next) = Self::parse_update_section(path_ref, &lines, i + 1)?;
                ops.push(op);
                i = next;
            } else {
                return Err(ToolError::Validation(format!(
                    "apply_patch verification failed: unexpected line: {line}"
                )));
            }
        }

        if ops.is_empty() {
            return Err(ToolError::Validation(
                "patch rejected: empty patch".to_string(),
            ));
        }

        Ok(ops)
    }

    fn apply_hunks(content: &str, hunks: &[Hunk]) -> Result<String, ToolError> {
        let mut file_lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

        for hunk in hunks {
            let context_lines: Vec<&str> = hunk
                .lines
                .iter()
                .filter_map(|l| match l {
                    HunkLine::Context(s) => Some(s.as_str()),
                    HunkLine::Remove(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect();

            if context_lines.is_empty() && hunk.lines.iter().all(|l| matches!(l, HunkLine::Add(_)))
            {
                // Insert-only hunk: find context from surrounding Context lines
                // For now, append at end if no context
                let add_lines: Vec<&str> = hunk
                    .lines
                    .iter()
                    .filter_map(|l| match l {
                        HunkLine::Add(s) => Some(s.as_str()),
                        _ => None,
                    })
                    .collect();
                file_lines.extend(add_lines.iter().map(|s| s.to_string()));
                continue;
            }

            // Find match position
            let match_pos = Self::find_match_position(
                &file_lines,
                &context_lines,
                hunk.eof_anchor,
                hunk.context_header.as_deref(),
            )?;

            // Apply changes at match position
            let mut new_lines = Vec::new();
            let mut fi = match_pos;

            // Copy lines before match
            new_lines.extend_from_slice(&file_lines[..match_pos]);

            for hunk_line in &hunk.lines {
                match hunk_line {
                    HunkLine::Context(_) => {
                        if fi < file_lines.len() {
                            new_lines.push(file_lines[fi].clone());
                            fi += 1;
                        }
                    }
                    HunkLine::Remove(_) => {
                        fi += 1; // skip the removed line
                    }
                    HunkLine::Add(s) => {
                        new_lines.push(s.clone());
                    }
                }
            }

            // Copy remaining lines
            new_lines.extend_from_slice(&file_lines[fi..]);
            file_lines = new_lines;
        }

        let mut result = file_lines.join("\n");
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        Ok(result)
    }

    fn find_match_position(
        file_lines: &[String],
        context: &[&str],
        eof_anchor: bool,
        header_ctx: Option<&str>,
    ) -> Result<usize, ToolError> {
        if context.is_empty() {
            return Ok(file_lines.len());
        }

        let first = context[0];
        let mut candidates: Vec<usize> = Vec::new();

        for (i, line) in file_lines.iter().enumerate() {
            let trimmed_line = line.trim_end();
            let trimmed_ctx = first.trim_end();
            if trimmed_line == trimmed_ctx || line == first {
                // Verify full context match
                let mut full_match = true;
                for (j, ctx_line) in context.iter().enumerate() {
                    if i + j >= file_lines.len() {
                        full_match = false;
                        break;
                    }
                    let fl = file_lines[i + j].trim_end();
                    let cl = ctx_line.trim_end();
                    if fl != cl {
                        full_match = false;
                        break;
                    }
                }
                if full_match {
                    candidates.push(i);
                }
            }
        }

        if candidates.is_empty() {
            return Err(ToolError::Validation(
                "apply_patch verification failed: context not found in file".to_string(),
            ));
        }

        // If header context, filter candidates
        if let Some(header) = header_ctx {
            for &pos in candidates.iter().rev() {
                // Look backwards for the header
                for j in (0..pos).rev() {
                    if file_lines[j].contains(header) {
                        return Ok(pos);
                    }
                }
            }
        }

        // If EOF anchor, prefer last match
        if eof_anchor {
            return Ok(*candidates.last().unwrap());
        }

        Ok(candidates[0])
    }

    /// Declare ExternalDirectory permissions for any op whose resolved path
    /// escapes the project directory (mirrors ReadTool/WriteTool/EditTool T2.3).
    fn declare_external_permissions(
        ops: &[PatchOp],
        project_dir: &std::path::Path,
        permissions: &mut PermissionCollector,
    ) {
        for op in ops {
            match op {
                PatchOp::Add { path, .. }
                | PatchOp::Delete { path }
                | PatchOp::Update { path, .. } => {
                    let resolved = Self::resolve_path(path, project_dir);
                    let _ = Self::record_external_if_escapes(&resolved, project_dir, permissions);
                }
            }
            if let PatchOp::Update {
                move_to: Some(dest),
                ..
            } = op
            {
                let resolved = Self::resolve_path(dest, project_dir);
                let _ = Self::record_external_if_escapes(&resolved, project_dir, permissions);
            }
        }
    }

    /// Fail-fast verification: every Update target must exist; every Delete
    /// target must exist and be a file; every Update hunk must dry-run apply.
    async fn verify_operations(
        ops: &[PatchOp],
        project_dir: &std::path::Path,
    ) -> Result<(), ToolError> {
        for op in ops {
            match op {
                PatchOp::Update { path, .. } => {
                    let full = Self::resolve_path(path, project_dir);
                    if !full.exists() {
                        return Err(ToolError::Validation(
                            "apply_patch verification failed: Failed to read file to update"
                                .to_string(),
                        ));
                    }
                }
                PatchOp::Delete { path } => {
                    let full = Self::resolve_path(path, project_dir);
                    if !full.exists() {
                        return Err(ToolError::Validation(format!(
                            "apply_patch verification failed: File not found for delete: {path}"
                        )));
                    }
                    if full.is_dir() {
                        return Err(ToolError::Validation(format!(
                            "apply_patch verification failed: Cannot delete directory: {path}"
                        )));
                    }
                }
                _ => {}
            }
        }
        for op in ops {
            if let PatchOp::Update { path, hunks, .. } = op {
                let full = Self::resolve_path(path, project_dir);
                let content = tokio::fs::read_to_string(&full)
                    .await
                    .map_err(|e| ToolError::Execution(format!("Failed to read file: {e}")))?;
                Self::apply_hunks(&content, hunks)?;
            }
        }
        Ok(())
    }

    async fn apply_add(
        path: &str,
        content: &str,
        project_dir: &std::path::Path,
        summary: &mut Vec<String>,
        files_info: &mut Vec<serde_json::Value>,
    ) -> Result<(), ToolError> {
        let full = Self::resolve_path(path, project_dir);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::Execution(format!("{e}")))?;
        }
        let before = if full.exists() {
            tokio::fs::read_to_string(&full).await.unwrap_or_default()
        } else {
            String::new()
        };
        tokio::fs::write(&full, content)
            .await
            .map_err(|e| ToolError::Execution(format!("{e}")))?;
        summary.push(format!("A {}", path.replace('\\', "/")));
        files_info.push(serde_json::json!({
            "filePath": full.display().to_string(),
            "relativePath": path.replace('\\', "/"),
            "type": "add",
            "before": before,
            "after": content,
        }));
        Ok(())
    }

    async fn apply_delete(
        path: &str,
        project_dir: &std::path::Path,
        summary: &mut Vec<String>,
        files_info: &mut Vec<serde_json::Value>,
    ) -> Result<(), ToolError> {
        let full = Self::resolve_path(path, project_dir);
        tokio::fs::remove_file(&full)
            .await
            .map_err(|e| ToolError::Execution(format!("{e}")))?;
        summary.push(format!("D {}", path.replace('\\', "/")));
        files_info.push(serde_json::json!({
            "filePath": full.display().to_string(),
            "relativePath": path.replace('\\', "/"),
            "type": "delete",
        }));
        Ok(())
    }

    async fn apply_update(
        path: &str,
        hunks: &[Hunk],
        move_to: Option<&str>,
        project_dir: &std::path::Path,
        summary: &mut Vec<String>,
        files_info: &mut Vec<serde_json::Value>,
    ) -> Result<(), ToolError> {
        let full = Self::resolve_path(path, project_dir);
        let content = tokio::fs::read_to_string(&full)
            .await
            .map_err(|e| ToolError::Execution(format!("{e}")))?;
        let before = content.clone();
        let new_content = Self::apply_hunks(&content, hunks)?;

        if let Some(dest) = move_to {
            let dest_full = Self::resolve_path(dest, project_dir);
            if let Some(parent) = dest_full.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| ToolError::Execution(format!("{e}")))?;
            }
            tokio::fs::write(&dest_full, &new_content)
                .await
                .map_err(|e| ToolError::Execution(format!("{e}")))?;
            tokio::fs::remove_file(&full)
                .await
                .map_err(|e| ToolError::Execution(format!("{e}")))?;
            summary.push(format!(
                "M {} -> {}",
                path.replace('\\', "/"),
                dest.replace('\\', "/")
            ));
            files_info.push(serde_json::json!({
                "filePath": full.display().to_string(),
                "relativePath": dest.replace('\\', "/"),
                "type": "move",
                "movePath": dest_full.display().to_string(),
                "before": before,
                "after": new_content,
            }));
        } else {
            tokio::fs::write(&full, &new_content)
                .await
                .map_err(|e| ToolError::Execution(format!("{e}")))?;
            summary.push(format!("M {}", path.replace('\\', "/")));
            files_info.push(serde_json::json!({
                "filePath": full.display().to_string(),
                "relativePath": path.replace('\\', "/"),
                "type": "update",
                "before": before,
                "after": new_content,
            }));
        }
        Ok(())
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn id(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Apply a unified patch to files"
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "patchText".to_string(),
                param_type: "string".to_string(),
                description: "Unified diff patch to apply".to_string(),
                required: true,
            }],
            input_examples: vec![
                serde_json::json!({
                    "patchText": "*** Begin Patch\n*** Add File: src/hello.rs\n+pub fn hello() {\n+    println!(\"hi\");\n+}\n*** End Patch\n"
                }),
                serde_json::json!({
                    "patchText": "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n pub mod foo;\n+pub mod bar;\n*** End Patch\n"
                }),
                serde_json::json!({
                    "patchText": "*** Begin Patch\n*** Delete File: old/obsolete.rs\n*** End Patch\n"
                }),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FileOps
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        ctx: &ToolContext,
        permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let patch_text = require_string(&args, "patchText")?;
        if patch_text.is_empty() {
            return Err(ToolError::Validation("patchText is required".to_string()));
        }
        let ops = Self::parse(&patch_text)?;
        Self::declare_external_permissions(&ops, &ctx.project_dir, permissions);
        Self::verify_operations(&ops, &ctx.project_dir).await?;

        let mut files_info = Vec::new();
        let mut summary = Vec::new();
        for op in &ops {
            match op {
                PatchOp::Add { path, content } => {
                    Self::apply_add(path, content, &ctx.project_dir, &mut summary, &mut files_info)
                        .await?;
                }
                PatchOp::Delete { path } => {
                    Self::apply_delete(path, &ctx.project_dir, &mut summary, &mut files_info)
                        .await?;
                }
                PatchOp::Update {
                    path,
                    hunks,
                    move_to,
                } => {
                    Self::apply_update(
                        path,
                        hunks,
                        move_to.as_deref(),
                        &ctx.project_dir,
                        &mut summary,
                        &mut files_info,
                    )
                    .await?;
                }
            }
        }

        let output = format!(
            "Success. Updated the following files:\n{}",
            summary.join("\n")
        );

        // Record permission
        permissions.record(PermissionRequest {
            permission: PermissionType::Edit,
            patterns: vec!["*".to_string()],
            always: vec![],
            metadata: serde_json::json!({
                "diff": "Index: patch",
                "files": files_info,
            }),
        });

        Ok(ToolOutput {
            title: format!(
                "Success. Updated the following files: {}",
                summary.join(", ")
            ),
            output,
            metadata: serde_json::json!({
                "diff": "Index: patch",
                "files": files_info,
            }),
            attachments: None,
            // Large patches often leave the build in a half-applied state.
            // Coach the model to verify before claiming done.
            llm_suffix: Some(
                "Patch applied across multiple files. Run the relevant test suite or build \
                 (`cargo test -p <crate>` / `cargo check`) before calling `done` to catch \
                 compile errors early."
                    .to_string(),
            ),
        })
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
