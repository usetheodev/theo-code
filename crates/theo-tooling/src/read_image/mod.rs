//! T1.2 — `read_image` tool: load PNG/JPEG/WebP/GIF from disk and emit
//! a base64 vision block in the tool output metadata.
//!
//! Designed to feed `theo_infra_llm::types::ContentBlock::ImageBase64`
//! (T0.1) so vision-capable providers (Anthropic Claude, OpenAI gpt-4o)
//! can see images supplied by the user / agent.
//!
//! Constraints:
//! - File size capped at [`MAX_IMAGE_BYTES`] (20 MiB) — Anthropic's
//!   real-world cap before requests fail.
//! - MIME detected by magic bytes, NOT by file extension. This avoids
//!   "rename .jpg to .png" footguns and makes the tool resistant to
//!   typos.
//! - Path safety: relative paths resolved against `ctx.project_dir`;
//!   absolute paths land where the user asked. Path-traversal hardening
//!   uses `path::absolutize` from this crate.
//!
//! Output:
//! - `metadata.image_block` carries the JSON-shaped `ContentBlock::
//!   ImageBase64` (`{type, source: {type, media_type, data}}`). The
//!   agent runtime's `tool_bridge` propagates it to the next user
//!   message so the LLM sees it on the next turn.
//!
//! See `docs/plans/sota-tier1-tier2-plan.md` §T1.2 + ADR D1.

use std::path::Path;

use async_trait::async_trait;
use base64::Engine as _;
use serde_json::{Value, json};

use theo_domain::error::ToolError;
use theo_domain::tool::{
    PermissionCollector, Tool, ToolCategory, ToolContext, ToolOutput, ToolParam, ToolSchema,
};

/// Max image size accepted (bytes). Anthropic rejects payloads above
/// this threshold; we bail early with an actionable error.
pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

/// Tool that reads an image file and emits a base64 vision block.
pub struct ReadImageTool;

impl Default for ReadImageTool {
    fn default() -> Self {
        Self
    }
}

impl ReadImageTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReadImageTool {
    fn id(&self) -> &str {
        "read_image"
    }

    fn description(&self) -> &str {
        "T1.2 — Load a PNG/JPEG/WebP/GIF image from disk and emit a \
         base64 vision block. The block is attached to `metadata.image_block` \
         and the agent runtime propagates it as a vision content block on \
         the next LLM turn (Anthropic / gpt-4o). MIME is detected from \
         magic bytes (not file extension). Files larger than 20 MiB are \
         rejected upfront. Use to inspect screenshots, design mockups, or \
         diagram exports before reasoning about them. Example: \
         read_image({path: 'docs/design/dashboard.png'})."
    }

    fn schema(&self) -> ToolSchema {
        ToolSchema {
            params: vec![ToolParam {
                name: "path".into(),
                param_type: "string".into(),
                description:
                    "Project-relative or absolute path to a PNG/JPEG/WebP/GIF file."
                        .into(),
                required: true,
            }],
            input_examples: vec![
                json!({"path": "docs/design/dashboard.png"}),
                json!({"path": "/tmp/screenshot.jpg"}),
            ],
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::FileOps
    }

    async fn execute(
        &self,
        args: Value,
        ctx: &ToolContext,
        _permissions: &mut PermissionCollector,
    ) -> Result<ToolOutput, ToolError> {
        let raw = args
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::InvalidArgs("missing string `path`".into()))?;

        let resolved = crate::path::absolutize(&ctx.project_dir, raw)
            .map_err(|e| ToolError::InvalidArgs(format!("invalid path: {e}")))?;

        // Size check before any allocation.
        let metadata_fs = std::fs::metadata(&resolved).map_err(ToolError::Io)?;
        if metadata_fs.len() > MAX_IMAGE_BYTES {
            return Err(ToolError::Execution(format!(
                "image too large: {bytes} bytes exceeds limit of {limit} bytes ({mb} MiB)",
                bytes = metadata_fs.len(),
                limit = MAX_IMAGE_BYTES,
                mb = MAX_IMAGE_BYTES / (1024 * 1024),
            )));
        }

        let bytes = std::fs::read(&resolved).map_err(ToolError::Io)?;
        let media_type = detect_image_mime(&bytes).ok_or_else(|| {
            ToolError::Execution(
                "unsupported image format — accepted: png, jpeg, webp, gif".into(),
            )
        })?;

        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let block = build_image_block(media_type, &data);

        let title = format!(
            "Loaded {} ({} bytes, {})",
            display_filename(&resolved),
            bytes.len(),
            media_type,
        );

        Ok(ToolOutput {
            title,
            output: format!(
                "Image loaded: {}\nMIME: {}\nBytes: {}\n(base64 attached as image_block in metadata)",
                resolved.display(),
                media_type,
                bytes.len(),
            ),
            metadata: json!({
                "type": "read_image",
                "path": resolved.display().to_string(),
                "media_type": media_type,
                "bytes": bytes.len(),
                "image_block": block,
            }),
            attachments: None,
            llm_suffix: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Pure helpers (testable without filesystem)
// ---------------------------------------------------------------------------

/// Detect image MIME type from magic bytes. Returns `None` for unknown
/// shapes. Magic-byte references (well-known):
/// - PNG: `89 50 4E 47 0D 0A 1A 0A`
/// - JPEG: `FF D8 FF`
/// - GIF: `GIF87a` or `GIF89a`
/// - WebP: `RIFF....WEBP`
pub fn detect_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() >= 8 && bytes[..8] == [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        return Some("image/png");
    }
    if bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF] {
        return Some("image/jpeg");
    }
    if bytes.len() >= 6 && (&bytes[..6] == b"GIF87a" || &bytes[..6] == b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

/// Build the JSON for `ContentBlock::ImageBase64` matching the wire
/// format produced by `theo_infra_llm::types::ContentBlock`. Kept in
/// theo-tooling so we don't add an `infra-llm` dependency just for this
/// shape.
fn build_image_block(media_type: &str, base64_data: &str) -> Value {
    json!({
        "type": "image_base64",
        "source": {
            "type": "base64",
            "media_type": media_type,
            "data": base64_data,
        }
    })
}

/// Best-effort display filename (last component) for log messages.
fn display_filename(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("<image>")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;
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

    // The smallest valid PNG ever — 1x1 transparent pixel.
    // <https://en.wikipedia.org/wiki/Portable_Network_Graphics> minimal
    // example used widely in tests.
    fn tiny_png_bytes() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
            0x00, 0x00, 0x00, 0x0D, // IHDR length
            0x49, 0x48, 0x44, 0x52, // IHDR
            0x00, 0x00, 0x00, 0x01, // width=1
            0x00, 0x00, 0x00, 0x01, // height=1
            0x08, 0x06, 0x00, 0x00, 0x00, // bit_depth=8, color=RGBA, ...
            0x1F, 0x15, 0xC4, 0x89, // CRC
            0x00, 0x00, 0x00, 0x0D, // IDAT length
            0x49, 0x44, 0x41, 0x54, // IDAT
            0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4,
            // IDAT data + CRC
            0x00, 0x00, 0x00, 0x00, // IEND length
            0x49, 0x45, 0x4E, 0x44, // IEND
            0xAE, 0x42, 0x60, 0x82, // CRC
        ]
    }

    fn tiny_jpeg_bytes() -> Vec<u8> {
        // Just the magic prefix is enough for the MIME-detection tests.
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46];
        v.resize(64, 0);
        v
    }

    fn tiny_gif_bytes() -> Vec<u8> {
        let mut v = b"GIF89a".to_vec();
        v.resize(20, 0);
        v
    }

    fn tiny_webp_bytes() -> Vec<u8> {
        // RIFF<size>WEBP<...>
        let mut v = b"RIFF".to_vec();
        v.extend_from_slice(&[0u8; 4]); // size placeholder
        v.extend_from_slice(b"WEBP");
        v.extend_from_slice(&[0u8; 16]);
        v
    }

    #[test]
    fn t12_detect_mime_png() {
        assert_eq!(detect_image_mime(&tiny_png_bytes()), Some("image/png"));
    }

    #[test]
    fn t12_detect_mime_jpeg() {
        assert_eq!(detect_image_mime(&tiny_jpeg_bytes()), Some("image/jpeg"));
    }

    #[test]
    fn t12_detect_mime_gif87a() {
        let mut v = b"GIF87a".to_vec();
        v.resize(20, 0);
        assert_eq!(detect_image_mime(&v), Some("image/gif"));
    }

    #[test]
    fn t12_detect_mime_gif89a() {
        assert_eq!(detect_image_mime(&tiny_gif_bytes()), Some("image/gif"));
    }

    #[test]
    fn t12_detect_mime_webp() {
        assert_eq!(detect_image_mime(&tiny_webp_bytes()), Some("image/webp"));
    }

    #[test]
    fn t12_detect_mime_unknown_returns_none() {
        assert!(detect_image_mime(b"hello world").is_none());
        assert!(detect_image_mime(&[]).is_none());
        assert!(detect_image_mime(&[0xFF]).is_none()); // too short
    }

    #[test]
    fn t12_detect_mime_misleading_extension_caught_by_magic_bytes() {
        // Bytes start as PNG even though caller might claim JPEG.
        let bytes = tiny_png_bytes();
        assert_eq!(detect_image_mime(&bytes), Some("image/png"));
    }

    #[test]
    fn t12_build_image_block_shape_matches_anthropic() {
        let block = build_image_block("image/png", "ZGF0YQ==");
        assert_eq!(block["type"], "image_base64");
        assert_eq!(block["source"]["type"], "base64");
        assert_eq!(block["source"]["media_type"], "image/png");
        assert_eq!(block["source"]["data"], "ZGF0YQ==");
    }

    #[tokio::test]
    async fn t12_tool_reads_png_and_emits_image_block() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tiny.png");
        std::fs::write(&path, tiny_png_bytes()).unwrap();

        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();
        let result = ReadImageTool::new()
            .execute(json!({"path": "tiny.png"}), &ctx, &mut perms)
            .await
            .unwrap();
        assert_eq!(result.metadata["media_type"], "image/png");
        assert!(result.metadata["bytes"].as_u64().unwrap() > 0);
        assert_eq!(result.metadata["image_block"]["type"], "image_base64");
        assert!(
            !result.metadata["image_block"]["source"]["data"]
                .as_str()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn t12_tool_rejects_oversized_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("big.png");
        // Header is correct but file is way too large.
        let mut huge = tiny_png_bytes();
        huge.resize((MAX_IMAGE_BYTES + 1) as usize, 0);
        std::fs::write(&path, &huge).unwrap();

        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();
        let err = ReadImageTool::new()
            .execute(json!({"path": "big.png"}), &ctx, &mut perms)
            .await
            .unwrap_err();
        assert!(
            matches!(err, ToolError::Execution(msg) if msg.contains("too large")),
            "expected 'too large' Execution error"
        );
    }

    #[tokio::test]
    async fn t12_tool_rejects_unknown_format() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("not_an_image.png");
        std::fs::write(&path, b"this is not an image").unwrap();

        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();
        let err = ReadImageTool::new()
            .execute(json!({"path": "not_an_image.png"}), &ctx, &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Execution(_)));
    }

    #[tokio::test]
    async fn t12_tool_missing_file_returns_io_error() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();
        let err = ReadImageTool::new()
            .execute(json!({"path": "ghost.png"}), &ctx, &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::Io(_)));
    }

    #[tokio::test]
    async fn t12_tool_missing_path_arg_returns_invalid_args() {
        let dir = tempdir().unwrap();
        let ctx = make_ctx(dir.path().to_path_buf());
        let mut perms = PermissionCollector::new();
        let err = ReadImageTool::new()
            .execute(json!({}), &ctx, &mut perms)
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::InvalidArgs(_)));
    }

    #[test]
    fn t12_tool_id_and_category() {
        let t = ReadImageTool::new();
        assert_eq!(t.id(), "read_image");
        assert_eq!(t.category(), ToolCategory::FileOps);
    }

    #[test]
    fn t12_tool_schema_validates() {
        ReadImageTool::new().schema().validate().unwrap();
    }

    #[test]
    fn t12_max_image_bytes_is_20_mib() {
        assert_eq!(MAX_IMAGE_BYTES, 20 * 1024 * 1024);
    }
}
