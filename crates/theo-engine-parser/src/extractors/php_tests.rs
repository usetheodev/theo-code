//! Sibling test body of `php.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `php.rs` via `#[path = "php_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.

    use std::path::PathBuf;

    use super::*;
    

    fn extract_php(source: &str) -> FileExtraction {
        let path = PathBuf::from("routes.php");
        let parsed =
            crate::tree_sitter::parse_source(&path, source, SupportedLanguage::Php, None).unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::Php)
    }

    #[test]
    fn extracts_laravel_get_route() {
        let ext = extract_php(
            r#"<?php
Route::get('/users', [UserController::class, 'index']);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/users");
    }

    #[test]
    fn extracts_laravel_post_route() {
        let ext = extract_php(
            r#"<?php
Route::post('/api/orders', [OrderController::class, 'store']);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
        assert_eq!(ext.interfaces[0].path, "/api/orders");
    }

    #[test]
    fn detects_middleware_auth() {
        let ext = extract_php(
            r#"<?php
Route::post('/api/orders', [OrderController::class, 'store'])->middleware('auth');
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_some());
    }

    #[test]
    fn no_auth_when_no_middleware() {
        let ext = extract_php(
            r#"<?php
Route::get('/health', function () { return 'ok'; });
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_none());
    }

    #[test]
    fn extracts_http_facade_call() {
        let ext = extract_php(
            r#"<?php
$response = Http::get('https://api.example.com/data');
?>"#,
        );
        assert_eq!(ext.dependencies.len(), 1);
        assert_eq!(
            ext.dependencies[0].dependency_type,
            DependencyType::HttpCall
        );
    }

    #[test]
    fn detects_pii_in_log() {
        let ext = extract_php(
            r#"<?php
Log::info("User email: " . $user->email);
?>"#,
        );
        assert!(ext.sinks.iter().any(|s| s.contains_pii));
    }

    #[test]
    fn extracts_multiple_routes() {
        let ext = extract_php(
            r#"<?php
Route::get('/users', [UserController::class, 'index']);
Route::post('/users', [UserController::class, 'store']);
Route::delete('/users/{id}', [UserController::class, 'destroy']);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 3);
    }

    // --- Resource routes ---

    #[test]
    fn extracts_laravel_resource_routes() {
        let ext = extract_php(
            r#"<?php
Route::resource('/photos', PhotoController::class);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 7, "resource() expands to 7 routes");
    }

    #[test]
    fn extracts_laravel_api_resource_routes() {
        let ext = extract_php(
            r#"<?php
Route::apiResource('/posts', PostController::class);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 5, "apiResource() expands to 5 routes");
    }

    #[test]
    fn extracts_laravel_any_route() {
        let ext = extract_php(
            r#"<?php
Route::any('/webhook', WebhookController::class);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::All);
        assert_eq!(ext.interfaces[0].path, "/webhook");
    }

    #[test]
    fn resource_routes_with_middleware() {
        let ext = extract_php(
            r#"<?php
Route::resource('/photos', PhotoController::class)->middleware('auth');
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 7, "resource() expands to 7 routes");
        assert!(
            ext.interfaces.iter().all(|i| i.auth.is_some()),
            "all resource routes inherit middleware auth"
        );
    }

    #[test]
    fn resource_route_paths_are_correct() {
        let ext = extract_php(
            r#"<?php
Route::resource('/photos', PhotoController::class);
?>"#,
        );
        let paths: Vec<&str> = ext.interfaces.iter().map(|i| i.path.as_str()).collect();
        assert!(paths.contains(&"/photos"), "index");
        assert!(paths.contains(&"/photos/create"), "create");
        assert!(paths.contains(&"/photos/{photo}"), "show (singular param)");
        assert!(paths.contains(&"/photos/edit"), "edit");
    }

    #[test]
    fn realistic_laravel_routes() {
        let ext = extract_php(
            r#"<?php
use Illuminate\Support\Facades\Route;

Route::get('/health', function () {
    return response()->json(['status' => 'ok']);
});

Route::post('/api/payments', [PaymentController::class, 'charge'])->middleware('auth');

Route::get('/api/products', [ProductController::class, 'index']);

$response = Http::post('https://stripe.api/charge', $data);
Log::info("Processing payment for: " . $request->email);
?>"#,
        );
        assert_eq!(ext.interfaces.len(), 3);
        assert!(ext.interfaces[0].auth.is_none()); // /health
        assert!(ext.interfaces[1].auth.is_some()); // /api/payments
        assert_eq!(ext.dependencies.len(), 1); // Http::post
        assert!(!ext.sinks.is_empty()); // Log::info
    }

    // --- Route group context propagation ---

    #[test]
    fn group_middleware_propagates_auth() {
        // Arrange
        let ext = extract_php(
            r#"<?php
Route::middleware('auth:api')->group(function () {
    Route::get('/users', [UserController::class, 'index']);
});
?>"#,
        );

        // Assert
        assert_eq!(ext.interfaces.len(), 1);
        assert!(
            ext.interfaces[0].auth.is_some(),
            "route inside middleware group should inherit auth"
        );
        assert_eq!(ext.interfaces[0].path, "/users");
    }

    #[test]
    fn group_prefix_prepends_path() {
        // Arrange
        let ext = extract_php(
            r#"<?php
Route::prefix('/api/v1')->group(function () {
    Route::get('/users', [UserController::class, 'index']);
});
?>"#,
        );

        // Assert
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(
            ext.interfaces[0].path, "/api/v1/users",
            "route path should be prefixed with group prefix"
        );
    }

    #[test]
    fn group_chained_prefix_and_middleware() {
        // Arrange
        let ext = extract_php(
            r#"<?php
Route::prefix('/api')->middleware('auth:api')->group(function () {
    Route::get('/users', [UserController::class, 'index']);
    Route::post('/users', [UserController::class, 'store']);
});
?>"#,
        );

        // Assert
        assert_eq!(ext.interfaces.len(), 2);
        for iface in &ext.interfaces {
            assert!(
                iface.path.starts_with("/api/"),
                "path '{}' should start with /api/",
                iface.path
            );
            assert!(
                iface.auth.is_some(),
                "route at '{}' should inherit auth from group",
                iface.path
            );
        }
        assert_eq!(ext.interfaces[0].path, "/api/users");
        assert_eq!(ext.interfaces[1].path, "/api/users");
    }

    #[test]
    fn group_nested() {
        // Arrange
        let ext = extract_php(
            r#"<?php
Route::prefix('/api')->group(function () {
    Route::prefix('/v1')->group(function () {
        Route::get('/items', [ItemController::class, 'index']);
    });
});
?>"#,
        );

        // Assert
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(
            ext.interfaces[0].path, "/api/v1/items",
            "nested groups should accumulate prefixes"
        );
    }

    #[test]
    fn group_does_not_leak() {
        // Arrange
        let ext = extract_php(
            r#"<?php
Route::prefix('/api')->middleware('auth')->group(function () {
    Route::get('/users', [UserController::class, 'index']);
});

Route::get('/health', function () { return 'ok'; });
?>"#,
        );

        // Assert
        assert_eq!(ext.interfaces.len(), 2);

        let api_route = ext.interfaces.iter().find(|i| i.path == "/api/users");
        assert!(api_route.is_some(), "should find /api/users route");
        assert!(
            api_route.unwrap().auth.is_some(),
            "/api/users should have auth"
        );

        let health_route = ext.interfaces.iter().find(|i| i.path == "/health");
        assert!(health_route.is_some(), "should find /health route");
        assert!(
            health_route.unwrap().auth.is_none(),
            "/health should NOT have auth — group context must not leak"
        );
    }

    #[test]
    fn group_middleware_only_no_prefix() {
        // Arrange
        let ext = extract_php(
            r#"<?php
Route::middleware('auth')->group(function () {
    Route::get('/dashboard', [DashboardController::class, 'index']);
    Route::post('/settings', [SettingsController::class, 'update']);
});
?>"#,
        );

        // Assert
        assert_eq!(ext.interfaces.len(), 2);
        assert_eq!(
            ext.interfaces[0].path, "/dashboard",
            "path should be unchanged when group has no prefix"
        );
        assert_eq!(ext.interfaces[1].path, "/settings");
        assert!(
            ext.interfaces.iter().all(|i| i.auth.is_some()),
            "all routes should inherit auth from middleware-only group"
        );
    }
