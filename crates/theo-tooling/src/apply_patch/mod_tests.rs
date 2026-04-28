//! Sibling test body of `mod.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `mod.rs` via `#[path = "mod_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

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
