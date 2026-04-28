//! Sibling test body of `python.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `python.rs` via `#[path = "python_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.


#![cfg(test)]

    use std::path::PathBuf;

    use super::*;
    

    fn extract_py(source: &str) -> FileExtraction {
        let path = PathBuf::from("test.py");
        let parsed =
            crate::tree_sitter::parse_source(&path, source, SupportedLanguage::Python, None)
                .unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::Python)
    }

    // ── Import extraction tests ───────────────────────────────────

    #[test]
    fn extracts_simple_import() {
        let ext = extract_py("import os\n");
        assert_eq!(ext.imports.len(), 1);
        assert_eq!(ext.imports[0].source, "os");
    }

    #[test]
    fn extracts_dotted_import() {
        let ext = extract_py("import torch.nn\n");
        assert_eq!(ext.imports.len(), 1);
        assert_eq!(ext.imports[0].source, "torch.nn");
    }

    #[test]
    fn extracts_aliased_import() {
        let ext = extract_py("import torch.nn as nn\n");
        assert_eq!(ext.imports.len(), 1);
        assert_eq!(ext.imports[0].source, "torch.nn");
        // Alias: "nn" → "torch.nn"
        assert_eq!(ext.imports[0].aliases.len(), 1);
        assert_eq!(ext.imports[0].aliases[0].0, "nn");
        assert_eq!(ext.imports[0].aliases[0].1, "torch.nn");
    }

    #[test]
    fn extracts_aliased_import_simple() {
        let ext = extract_py("import numpy as np\n");
        assert_eq!(ext.imports.len(), 1);
        assert_eq!(ext.imports[0].source, "numpy");
        assert_eq!(ext.imports[0].aliases.len(), 1);
        assert_eq!(ext.imports[0].aliases[0].0, "np");
        assert_eq!(ext.imports[0].aliases[0].1, "numpy");
    }

    #[test]
    fn non_aliased_import_has_empty_aliases() {
        let ext = extract_py("import os\n");
        assert!(ext.imports[0].aliases.is_empty());
    }

    #[test]
    fn extracts_from_import_with_specifiers() {
        let ext = extract_py("from fastapi import FastAPI, Depends\n");
        assert_eq!(ext.imports.len(), 1);
        assert_eq!(ext.imports[0].source, "fastapi");
        assert!(ext.imports[0].specifiers.contains(&"FastAPI".to_string()));
        assert!(ext.imports[0].specifiers.contains(&"Depends".to_string()));
    }

    #[test]
    fn extracts_relative_import_dot() {
        let ext = extract_py("from . import views\n");
        assert_eq!(ext.imports.len(), 1);
        assert_eq!(ext.imports[0].source, ".");
        assert!(ext.imports[0].specifiers.contains(&"views".to_string()));
    }

    #[test]
    fn extracts_relative_import_double_dot() {
        let ext = extract_py("from ..utils import helper\n");
        assert_eq!(ext.imports.len(), 1);
        assert_eq!(ext.imports[0].source, "..utils");
        assert!(ext.imports[0].specifiers.contains(&"helper".to_string()));
    }

    #[test]
    fn extracts_from_import_wildcard() {
        let ext = extract_py("from os.path import *\n");
        assert_eq!(ext.imports.len(), 1);
        assert_eq!(ext.imports[0].source, "os.path");
        assert!(ext.imports[0].specifiers.contains(&"*".to_string()));
    }

    #[test]
    fn extracts_multiple_imports_in_file() {
        let ext = extract_py(
            r#"
import os
import sys
from fastapi import FastAPI
from . import views
"#,
        );
        assert_eq!(ext.imports.len(), 4);
    }

    #[test]
    fn extracts_from_import_with_alias() {
        let ext = extract_py("from torch import Tensor as T\n");
        assert_eq!(ext.imports.len(), 1);
        assert_eq!(ext.imports[0].source, "torch");
        assert!(ext.imports[0].specifiers.contains(&"Tensor".to_string()));
        // Alias: "T" → "Tensor"
        assert_eq!(ext.imports[0].aliases.len(), 1);
        assert_eq!(ext.imports[0].aliases[0].0, "T");
        assert_eq!(ext.imports[0].aliases[0].1, "Tensor");
    }

    #[test]
    fn from_import_without_alias_has_empty_aliases() {
        let ext = extract_py("from fastapi import FastAPI, Depends\n");
        assert!(ext.imports[0].aliases.is_empty());
    }

    // ── Route extraction tests ──────────────────────────────────

    #[test]
    fn extracts_fastapi_get_route() {
        let ext = extract_py(
            r#"
from fastapi import FastAPI
app = FastAPI()

@app.get("/users")
def list_users():
    return []
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/users");
    }

    #[test]
    fn extracts_fastapi_post_route() {
        let ext = extract_py(
            r#"
@router.post("/api/orders")
def create_order(order: Order):
    return {"id": 1}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
        assert_eq!(ext.interfaces[0].path, "/api/orders");
    }

    #[test]
    fn extracts_flask_route() {
        let ext = extract_py(
            r#"
from flask import Flask
app = Flask(__name__)

@app.route("/items")
def list_items():
    return jsonify([])
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::All);
        assert_eq!(ext.interfaces[0].path, "/items");
    }

    #[test]
    fn extracts_django_path() {
        let ext = extract_py(
            r#"
from django.urls import path
from . import views

urlpatterns = [
    path('api/users/', views.list_users),
    path('api/orders/', views.create_order),
]
"#,
        );
        assert_eq!(ext.interfaces.len(), 2);
        assert_eq!(ext.interfaces[0].path, "/api/users/");
        assert_eq!(ext.interfaces[0].method, HttpMethod::All);
        assert_eq!(ext.interfaces[1].path, "/api/orders/");
    }

    #[test]
    fn detects_login_required_decorator() {
        let ext = extract_py(
            r#"
@app.get("/api/profile")
@login_required
def get_profile():
    return {"user": "me"}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(
            ext.interfaces[0].auth,
            Some(AuthKind::Decorator("login_required".into()))
        );
    }

    #[test]
    fn detects_jwt_required_decorator() {
        let ext = extract_py(
            r#"
@app.post("/api/orders")
@jwt_required()
def create_order():
    pass
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_some());
    }

    #[test]
    fn no_auth_when_missing() {
        let ext = extract_py(
            r#"
@app.get("/health")
def health_check():
    return {"status": "ok"}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_none());
    }

    #[test]
    fn extracts_requests_http_call() {
        let ext = extract_py(
            r#"
import requests
response = requests.get("https://api.example.com/data")
"#,
        );
        assert_eq!(ext.dependencies.len(), 1);
        assert_eq!(
            ext.dependencies[0].dependency_type,
            DependencyType::HttpCall
        );
    }

    #[test]
    fn extracts_httpx_http_call() {
        let ext = extract_py(
            r#"
import httpx
response = httpx.post("https://payment.service/charge", json=payload)
"#,
        );
        assert_eq!(ext.dependencies.len(), 1);
        assert_eq!(
            ext.dependencies[0].dependency_type,
            DependencyType::HttpCall
        );
    }

    #[test]
    fn detects_pii_in_log_sink() {
        let ext = extract_py(
            r#"
logging.info("User email: %s", user.email)
"#,
        );
        assert_eq!(ext.sinks.len(), 1);
        assert!(ext.sinks[0].contains_pii);
    }

    #[test]
    fn extracts_multiple_routes() {
        let ext = extract_py(
            r#"
@app.get("/users")
def list_users():
    return []

@app.post("/users")
@auth_required
def create_user(user: User):
    return user

@app.delete("/users/{id}")
@auth_required
def delete_user(id: int):
    pass
"#,
        );
        assert_eq!(ext.interfaces.len(), 3);
        assert!(ext.interfaces[0].auth.is_none());
        assert!(ext.interfaces[1].auth.is_some());
        assert!(ext.interfaces[2].auth.is_some());
    }

    #[test]
    fn unittest_mock_patch_not_detected_as_route() {
        let ext = extract_py(
            r#"
from unittest.mock import patch

@patch("torch._C._get_default_tensor_type")
def test_default_type():
    pass

@patch("module.config.use_fp64")
def test_fp64():
    pass
"#,
        );
        assert!(
            ext.interfaces.is_empty(),
            "unittest.mock.patch should not produce HTTP routes, got: {:?}",
            ext.interfaces.iter().map(|i| &i.path).collect::<Vec<_>>()
        );
    }

    #[test]
    fn bare_path_call_not_detected_as_django_route() {
        let ext = extract_py(
            r#"
script = path("bin/test_script.py")
config = path("config/settings.yaml")
"#,
        );
        assert!(
            ext.interfaces.is_empty(),
            "path() with single arg or file extension should not produce routes, got: {:?}",
            ext.interfaces.iter().map(|i| &i.path).collect::<Vec<_>>()
        );
    }

    #[test]
    fn non_router_object_decorator_not_detected_as_route() {
        let ext = extract_py(
            r#"
@config.patch("/some/setting")
def update_setting():
    pass

@mock.get("/fake/endpoint")
def test_something():
    pass
"#,
        );
        assert!(
            ext.interfaces.is_empty(),
            "decorators on unknown objects should not produce routes, got: {:?}",
            ext.interfaces.iter().map(|i| &i.path).collect::<Vec<_>>()
        );
    }

    #[test]
    fn django_path_with_view_handler_still_works() {
        let ext = extract_py(
            r#"
from django.urls import path
urlpatterns = [
    path('users/', views.user_list, name='user-list'),
    path('users/<int:pk>/', views.user_detail, name='user-detail'),
    path('health/', views.health_check),
]
"#,
        );
        assert_eq!(
            ext.interfaces.len(),
            3,
            "Django path() with view handler arg should still extract"
        );
    }

    #[test]
    fn realistic_fastapi_file() {
        let ext = extract_py(
            r#"
from fastapi import FastAPI, Depends
import requests

app = FastAPI()

@app.get("/health")
def health():
    return {"status": "ok"}

@app.post("/api/payments")
@jwt_required()
async def process_payment(payment: PaymentRequest):
    logging.info("Processing payment for: %s", payment.email)
    response = requests.post("https://stripe.api/charge", json=payment.dict())
    logger.info("Payment processed")
    return {"success": True}

@app.get("/api/users")
async def list_users():
    logging.info("Listing users")
    return []
"#,
        );
        assert_eq!(ext.interfaces.len(), 3);
        assert!(ext.interfaces[0].auth.is_none()); // /health
        assert!(ext.interfaces[1].auth.is_some()); // /api/payments
        assert!(ext.interfaces[2].auth.is_none()); // /api/users
        assert_eq!(ext.dependencies.len(), 1); // requests.post
        assert!(ext.sinks.len() >= 2); // logging calls
        assert!(ext.sinks.iter().any(|s| s.contains_pii));
    }
