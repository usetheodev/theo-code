//! Sibling test body of `mod.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `mod.rs` via `#[path = "mod_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use super::*;
    use crate::test_helpers::*;

    fn read_tool() -> ReadTool {
        ReadTool::new()
    }

    // --- External directory permission tests ---

    #[tokio::test]
    async fn allows_reading_absolute_path_inside_project() {
        let tmp = TestDir::new();
        tmp.write_file("test.txt", "hello world");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("test.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("hello world"));
    }

    #[tokio::test]
    async fn allows_reading_file_in_subdirectory() {
        let tmp = TestDir::new();
        tmp.write_file("subdir/test.txt", "nested content");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("subdir/test.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("nested content"));
    }

    #[tokio::test]
    async fn asks_external_directory_permission_for_path_outside_project() {
        let outer = TestDir::new();
        outer.write_file("secret.txt", "secret data");

        let tmp = TestDir::with_git();
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let _ = read_tool()
            .execute(
                serde_json::json!({"filePath": outer.path().join("secret.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await;

        let ext_req = find_permission(&perms, &PermissionType::ExternalDirectory);
        assert!(ext_req.is_some());
    }

    #[tokio::test]
    async fn does_not_ask_external_directory_for_path_inside_project() {
        let tmp = TestDir::new();
        tmp.write_file("internal.txt", "internal content");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("internal.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let ext_req = find_permission(&perms, &PermissionType::ExternalDirectory);
        assert!(ext_req.is_none());
    }

    // --- Env file permission tests ---

    #[tokio::test]
    async fn env_file_asks_for_read_permission() {
        let tmp = TestDir::new();
        tmp.write_file(".env", "SECRET=value");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join(".env").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let read_req = perms
            .requests
            .iter()
            .find(|r| r.permission == PermissionType::Read);
        assert!(read_req.is_some());
    }

    #[tokio::test]
    async fn env_local_asks_for_read_permission() {
        let tmp = TestDir::new();
        tmp.write_file(".env.local", "SECRET=value");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join(".env.local").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let read_req = perms
            .requests
            .iter()
            .find(|r| r.permission == PermissionType::Read);
        assert!(read_req.is_some());
    }

    #[tokio::test]
    async fn env_example_does_not_ask_for_read_permission() {
        let tmp = TestDir::new();
        tmp.write_file(".env.example", "EXAMPLE=value");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join(".env.example").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        let read_req = perms
            .requests
            .iter()
            .find(|r| r.permission == PermissionType::Read);
        assert!(read_req.is_none());
    }

    // --- Truncation tests ---

    #[tokio::test]
    async fn truncates_by_line_count_when_limit_specified() {
        let tmp = TestDir::new();
        let lines: String = (0..100)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        tmp.write_file("many-lines.txt", &lines);
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({
                    "filePath": tmp.path().join("many-lines.txt").to_string_lossy().to_string(),
                    "limit": 10
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.metadata["truncated"].as_bool().unwrap());
        assert!(result.output.contains("Showing lines 1-10 of 100"));
        assert!(result.output.contains("Use offset=11"));
        assert!(result.output.contains("line0"));
        assert!(result.output.contains("line9"));
        assert!(!result.output.contains("10: line10"));
    }

    #[tokio::test]
    async fn does_not_truncate_small_file() {
        let tmp = TestDir::new();
        tmp.write_file("small.txt", "hello world");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("small.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(!result.metadata["truncated"].as_bool().unwrap());
        assert!(result.output.contains("End of file"));
    }

    #[tokio::test]
    async fn respects_offset_parameter() {
        let tmp = TestDir::new();
        let lines: String = (1..=20)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        tmp.write_file("offset.txt", &lines);
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({
                    "filePath": tmp.path().join("offset.txt").to_string_lossy().to_string(),
                    "offset": 10,
                    "limit": 5
                }),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("line10"));
        assert!(result.output.contains("line14"));
        assert!(!result.output.contains("line15"));
    }

    #[tokio::test]
    async fn throws_when_offset_beyond_end_of_file() {
        let tmp = TestDir::new();
        let lines: String = (1..=3)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        tmp.write_file("short.txt", &lines);
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({
                    "filePath": tmp.path().join("short.txt").to_string_lossy().to_string(),
                    "offset": 4,
                    "limit": 5
                }),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Offset 4 is out of range for this file (3 lines)"));
    }

    #[tokio::test]
    async fn allows_reading_empty_file() {
        let tmp = TestDir::new();
        tmp.write_file("empty.txt", "");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("empty.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(!result.metadata["truncated"].as_bool().unwrap());
        assert!(result.output.contains("End of file - total 0 lines"));
    }

    #[tokio::test]
    async fn throws_when_offset_gt_1_for_empty_file() {
        let tmp = TestDir::new();
        tmp.write_file("empty.txt", "");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({
                    "filePath": tmp.path().join("empty.txt").to_string_lossy().to_string(),
                    "offset": 2
                }),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Offset 2 is out of range")
        );
    }

    #[tokio::test]
    async fn truncates_long_lines() {
        let tmp = TestDir::new();
        let long_line = "x".repeat(3000);
        tmp.write_file("long-line.txt", &long_line);
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("long-line.txt").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.output.contains("(line truncated to 2000 chars)"));
        assert!(result.output.len() < 3000);
    }

    // --- Image file tests ---

    #[tokio::test]
    async fn image_files_set_truncated_to_false() {
        let tmp = TestDir::new();
        // 1x1 red PNG
        let png_bytes: &[u8] = &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 2, 0, 0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207,
            192, 0, 0, 0, 3, 0, 1, 24, 216, 141, 164, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ];
        let png_path = tmp.path().join("image.png");
        std::fs::write(&png_path, png_bytes).unwrap();

        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": png_path.to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(!result.metadata["truncated"].as_bool().unwrap());
        assert!(result.attachments.is_some());
        assert_eq!(result.attachments.as_ref().unwrap().len(), 1);
        assert_eq!(result.attachments.as_ref().unwrap()[0].file_type, "file");
    }

    // --- Binary detection tests ---

    #[tokio::test]
    async fn rejects_text_extension_files_with_null_bytes() {
        let tmp = TestDir::new();
        let bytes: &[u8] = &[
            0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x00, 0x77, 0x6f, 0x72, 0x6c, 0x64,
        ];
        let path = tmp.path().join("null-byte.txt");
        std::fs::write(&path, bytes).unwrap();

        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": path.to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Cannot read binary file")
        );
    }

    #[tokio::test]
    async fn rejects_known_binary_extensions() {
        let tmp = TestDir::new();
        tmp.write_file("module.wasm", "not really wasm");
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("module.wasm").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Cannot read binary file")
        );
    }

    #[tokio::test]
    async fn fbs_files_read_as_text_not_images() {
        let tmp = TestDir::new();
        let fbs_content = "namespace MyGame;\n\ntable Monster {\n  pos:Vec3;\n  name:string;\n}\n\nroot_type Monster;";
        tmp.write_file("schema.fbs", fbs_content);
        let ctx = test_context(tmp.path());
        let mut perms = PermissionCollector::new();

        let result = read_tool()
            .execute(
                serde_json::json!({"filePath": tmp.path().join("schema.fbs").to_string_lossy().to_string()}),
                &ctx,
                &mut perms,
            )
            .await
            .unwrap();

        assert!(result.attachments.is_none());
        assert!(result.output.contains("namespace MyGame"));
        assert!(result.output.contains("table Monster"));
    }
