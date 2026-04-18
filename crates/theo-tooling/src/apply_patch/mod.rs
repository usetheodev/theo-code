use async_trait::async_trait;
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

impl ApplyPatchTool {
    pub fn new() -> Self {
        Self
    }

    fn strip_heredoc(text: &str) -> &str {
        let trimmed = text.trim();
        // Strip cat <<'EOF' ... EOF wrapper
        if let Some(rest) = trimmed.strip_prefix("cat <<") {
            if let Some(body_start) = rest.find('\n') {
                let body = &rest[body_start + 1..];
                if let Some(eof_pos) = body.rfind("\nEOF") {
                    return &body[..eof_pos];
                }
                if body.ends_with("EOF") {
                    return &body[..body.len() - 3];
                }
            }
        }
        if let Some(rest) = trimmed.strip_prefix("<<") {
            if let Some(body_start) = rest.find('\n') {
                let body = &rest[body_start + 1..];
                if let Some(eof_pos) = body.rfind("\nEOF") {
                    return &body[..eof_pos];
                }
                if body.ends_with("EOF") {
                    return &body[..body.len() - 3];
                }
            }
        }
        trimmed
    }

    fn parse(text: &str) -> Result<Vec<PatchOp>, ToolError> {
        let text = Self::strip_heredoc(text);
        let lines: Vec<&str> = text.lines().collect();
        let mut ops = Vec::new();
        let mut i = 0;

        // Find *** Begin Patch
        while i < lines.len() && !lines[i].starts_with("*** Begin Patch") {
            i += 1;
        }
        if i >= lines.len() {
            return Err(ToolError::Validation(
                "apply_patch verification failed: no Begin Patch marker".to_string(),
            ));
        }
        i += 1;

        while i < lines.len() {
            let line = lines[i];
            if line.starts_with("*** End Patch") {
                break;
            }
            if line.starts_with("*** Add File: ") {
                let path = line.strip_prefix("*** Add File: ").unwrap().to_string();
                i += 1;
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
                ops.push(PatchOp::Add { path, content });
            } else if line.starts_with("*** Delete File: ") {
                let path = line.strip_prefix("*** Delete File: ").unwrap().to_string();
                ops.push(PatchOp::Delete { path });
                i += 1;
            } else if line.starts_with("*** Update File: ") {
                let path = line.strip_prefix("*** Update File: ").unwrap().to_string();
                i += 1;
                let mut move_to = None;
                if i < lines.len() && lines[i].starts_with("*** Move to: ") {
                    move_to = Some(lines[i].strip_prefix("*** Move to: ").unwrap().to_string());
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
                        let ctx_header = if lines[i].len() > 2 {
                            Some(lines[i][2..].trim().to_string())
                        } else {
                            None
                        };
                        i += 1;
                        let mut hunk_lines = Vec::new();
                        let mut eof_anchor = false;
                        while i < lines.len()
                            && !lines[i].starts_with("@@")
                            && !lines[i].starts_with("***")
                        {
                            let l = lines[i];
                            if l.starts_with('+') {
                                hunk_lines.push(HunkLine::Add(l[1..].to_string()));
                            } else if l.starts_with('-') {
                                hunk_lines.push(HunkLine::Remove(l[1..].to_string()));
                            } else if l.starts_with(' ') {
                                hunk_lines.push(HunkLine::Context(l[1..].to_string()));
                            }
                            i += 1;
                        }
                        if i < lines.len() && lines[i].starts_with("*** End of File") {
                            eof_anchor = true;
                            i += 1;
                        }
                        hunks.push(Hunk {
                            context_header: ctx_header.filter(|s| !s.is_empty()),
                            lines: hunk_lines,
                            eof_anchor,
                        });
                    } else {
                        i += 1;
                    }
                }
                if hunks.is_empty() {
                    return Err(ToolError::Validation(
                        "apply_patch verification failed: empty hunks".to_string(),
                    ));
                }
                ops.push(PatchOp::Update {
                    path,
                    hunks,
                    move_to,
                });
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

        // Verify all operations first (fail-fast)
        for op in &ops {
            match op {
                PatchOp::Update { path, .. } => {
                    let full = ctx.project_dir.join(path);
                    if !full.exists() {
                        return Err(ToolError::Validation(
                            "apply_patch verification failed: Failed to read file to update"
                                .to_string(),
                        ));
                    }
                }
                PatchOp::Delete { path } => {
                    let full = ctx.project_dir.join(path);
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

        // Verify update hunks match before applying anything
        for op in &ops {
            if let PatchOp::Update { path, hunks, .. } = op {
                let full = ctx.project_dir.join(path);
                let content = tokio::fs::read_to_string(&full)
                    .await
                    .map_err(|e| ToolError::Execution(format!("Failed to read file: {e}")))?;
                Self::apply_hunks(&content, hunks)?; // dry run
            }
        }

        // Apply all operations
        let mut files_info = Vec::new();
        let mut summary = Vec::new();

        for op in &ops {
            match op {
                PatchOp::Add { path, content } => {
                    let full = ctx.project_dir.join(path);
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
                }
                PatchOp::Delete { path } => {
                    let full = ctx.project_dir.join(path);
                    tokio::fs::remove_file(&full)
                        .await
                        .map_err(|e| ToolError::Execution(format!("{e}")))?;
                    summary.push(format!("D {}", path.replace('\\', "/")));
                    files_info.push(serde_json::json!({
                        "filePath": full.display().to_string(),
                        "relativePath": path.replace('\\', "/"),
                        "type": "delete",
                    }));
                }
                PatchOp::Update {
                    path,
                    hunks,
                    move_to,
                } => {
                    let full = ctx.project_dir.join(path);
                    let content = tokio::fs::read_to_string(&full)
                        .await
                        .map_err(|e| ToolError::Execution(format!("{e}")))?;
                    let before = content.clone();
                    let new_content = Self::apply_hunks(&content, hunks)?;

                    if let Some(dest) = move_to {
                        let dest_full = ctx.project_dir.join(dest);
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;

    fn patch() -> ApplyPatchTool {
        ApplyPatchTool::new()
    }

    #[tokio::test]
    async fn requires_patch_text() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();
        let result = patch()
            .execute(serde_json::json!({"patchText": ""}), &ctx, &mut perms)
            .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("patchText is required")
        );
    }

    #[tokio::test]
    async fn rejects_invalid_patch_format() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();
        let result = patch()
            .execute(
                serde_json::json!({"patchText": "invalid patch"}),
                &ctx,
                &mut perms,
            )
            .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("apply_patch verification failed")
        );
    }

    #[tokio::test]
    async fn rejects_empty_patch() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();
        let result = patch()
            .execute(
                serde_json::json!({"patchText": "*** Begin Patch\n*** End Patch"}),
                &ctx,
                &mut perms,
            )
            .await;
        assert!(result.unwrap_err().to_string().contains("empty patch"));
    }

    #[tokio::test]
    async fn applies_add_update_delete_in_one_patch() {
        let tmp = TestDir::with_git();
        tmp.write_file("modify.txt", "line1\nline2\n");
        tmp.write_file("delete.txt", "obsolete\n");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let patch_text = "*** Begin Patch\n*** Add File: nested/new.txt\n+created\n*** Delete File: delete.txt\n*** Update File: modify.txt\n@@\n-line2\n+changed\n*** End Patch";
        let result = patch()
            .execute(
                serde_json::json!({"patchText": patch_text}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(
            result
                .title
                .contains("Success. Updated the following files")
        );
        assert!(result.output.contains("A nested/new.txt"));
        assert!(result.output.contains("D delete.txt"));
        assert!(result.output.contains("M modify.txt"));

        assert_eq!(tmp.read_file("nested/new.txt"), "created\n");
        assert_eq!(tmp.read_file("modify.txt"), "line1\nchanged\n");
        assert!(!tmp.file_exists("delete.txt"));
    }

    #[tokio::test]
    async fn applies_multiple_hunks_to_one_file() {
        let tmp = TestDir::new();
        tmp.write_file("multi.txt", "line1\nline2\nline3\nline4\n");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let patch_text = "*** Begin Patch\n*** Update File: multi.txt\n@@\n-line2\n+changed2\n@@\n-line4\n+changed4\n*** End Patch";
        patch()
            .execute(
                serde_json::json!({"patchText": patch_text}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert_eq!(
            tmp.read_file("multi.txt"),
            "line1\nchanged2\nline3\nchanged4\n"
        );
    }

    #[tokio::test]
    async fn moves_file_to_new_directory() {
        let tmp = TestDir::new();
        tmp.write_file("old/name.txt", "old content\n");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let patch_text = "*** Begin Patch\n*** Update File: old/name.txt\n*** Move to: renamed/dir/name.txt\n@@\n-old content\n+new content\n*** End Patch";
        patch()
            .execute(
                serde_json::json!({"patchText": patch_text}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(!tmp.file_exists("old/name.txt"));
        assert_eq!(tmp.read_file("renamed/dir/name.txt"), "new content\n");
    }

    #[tokio::test]
    async fn rejects_update_when_target_file_missing() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let patch_text =
            "*** Begin Patch\n*** Update File: missing.txt\n@@\n-nope\n+better\n*** End Patch";
        let result = patch()
            .execute(
                serde_json::json!({"patchText": patch_text}),
                &ctx,
                &mut perms,
            )
            .await;
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to read file to update")
        );
    }

    #[tokio::test]
    async fn rejects_delete_when_file_missing() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let patch_text = "*** Begin Patch\n*** Delete File: missing.txt\n*** End Patch";
        let result = patch()
            .execute(
                serde_json::json!({"patchText": patch_text}),
                &ctx,
                &mut perms,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn rejects_delete_when_target_is_directory() {
        let tmp = TestDir::new();
        std::fs::create_dir(tmp.path().join("dir")).unwrap();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let patch_text = "*** Begin Patch\n*** Delete File: dir\n*** End Patch";
        let result = patch()
            .execute(
                serde_json::json!({"patchText": patch_text}),
                &ctx,
                &mut perms,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn verification_failure_leaves_no_side_effects() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        // Add file succeeds but Update fails - nothing should be written
        let patch_text = "*** Begin Patch\n*** Add File: created.txt\n+hello\n*** Update File: missing.txt\n@@\n-old\n+new\n*** End Patch";
        let result = patch()
            .execute(
                serde_json::json!({"patchText": patch_text}),
                &ctx,
                &mut perms,
            )
            .await;
        assert!(result.is_err());
        assert!(!tmp.file_exists("created.txt"));
    }

    #[tokio::test]
    async fn parses_heredoc_wrapped_patch() {
        let tmp = TestDir::new();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let patch_text = "cat <<'EOF'\n*** Begin Patch\n*** Add File: heredoc_test.txt\n+heredoc content\n*** End Patch\nEOF";
        patch()
            .execute(
                serde_json::json!({"patchText": patch_text}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
        assert_eq!(tmp.read_file("heredoc_test.txt"), "heredoc content\n");
    }

    #[tokio::test]
    async fn disambiguates_change_context_with_header() {
        let tmp = TestDir::new();
        tmp.write_file("multi_ctx.txt", "fn a\nx=10\ny=2\nfn b\nx=10\ny=20\n");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let patch_text =
            "*** Begin Patch\n*** Update File: multi_ctx.txt\n@@ fn b\n-x=10\n+x=11\n*** End Patch";
        patch()
            .execute(
                serde_json::json!({"patchText": patch_text}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
        assert_eq!(
            tmp.read_file("multi_ctx.txt"),
            "fn a\nx=10\ny=2\nfn b\nx=11\ny=20\n"
        );
    }

    #[tokio::test]
    async fn eof_anchor_matches_from_end_first() {
        let tmp = TestDir::new();
        tmp.write_file("eof_anchor.txt", "start\nmarker\nmiddle\nmarker\nend\n");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let patch_text = "*** Begin Patch\n*** Update File: eof_anchor.txt\n@@\n-marker\n-end\n+marker-changed\n+end\n*** End of File\n*** End Patch";
        patch()
            .execute(
                serde_json::json!({"patchText": patch_text}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();
        assert_eq!(
            tmp.read_file("eof_anchor.txt"),
            "start\nmarker\nmiddle\nmarker-changed\nend\n"
        );
    }
}
