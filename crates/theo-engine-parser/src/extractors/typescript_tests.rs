//! Sibling test body of `typescript.rs`.
//! Extracted by `scripts/extract-tests-to-sibling.py` (T0.2 of docs/plans/god-files-2026-07-23-plan.md).
//! Included from `typescript.rs` via `#[path = "typescript_tests.rs"] mod tests;`.
//!
//! Do not edit the path attribute — it is what keeps this file linked.


#![cfg(test)]

    use std::path::PathBuf;

    use super::*;
    

    fn extract_ts(source: &str) -> FileExtraction {
        let path = PathBuf::from("test.ts");
        let parsed =
            crate::tree_sitter::parse_source(&path, source, SupportedLanguage::TypeScript, None)
                .unwrap();
        extract(&path, source, &parsed.tree, SupportedLanguage::TypeScript)
    }

    #[test]
    fn extracts_express_get_route() {
        let ext = extract_ts(
            r#"
import express from 'express';
const app = express();
app.get('/api/users', (req, res) => res.json([]));
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/api/users");
    }

    #[test]
    fn extracts_express_post_route() {
        let ext = extract_ts(
            r#"
const router = require('express').Router();
router.post('/api/items', (req, res) => res.status(201).json({}));
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
    }

    #[test]
    fn detects_auth_middleware_on_route() {
        let ext = extract_ts(
            r#"
app.post('/api/orders', authMiddleware, (req, res) => {
    res.json({ ok: true });
});
"#,
        );
        assert_eq!(
            ext.interfaces[0].auth,
            Some(AuthKind::Middleware("authMiddleware".into()))
        );
    }

    #[test]
    fn detects_jwt_middleware() {
        let ext = extract_ts(
            r#"
app.delete('/api/users/:id', verifyJwt, validateAdmin, (req, res) => {
    res.status(204).send();
});
"#,
        );
        assert!(ext.interfaces[0].auth.is_some());
    }

    #[test]
    fn extracts_fetch_http_call() {
        let ext = extract_ts(
            r#"
const resp = await fetch("https://api.example.com/users");
"#,
        );
        assert_eq!(ext.dependencies.len(), 1);
        assert_eq!(
            ext.dependencies[0].dependency_type,
            DependencyType::HttpCall
        );
    }

    #[test]
    fn extracts_axios_http_call() {
        let ext = extract_ts(r#"const data = await axios.get("https://payment.service/charge");"#);
        assert_eq!(ext.dependencies.len(), 1);
    }

    #[test]
    fn extracts_console_log_sinks() {
        let ext = extract_ts(
            r#"
console.log("Server started");
console.error("Something failed");
"#,
        );
        assert_eq!(ext.sinks.len(), 2);
        assert!(!ext.sinks[0].contains_pii);
    }

    #[test]
    fn extracts_logger_sinks() {
        let ext = extract_ts(
            r#"
logger.info("Request processed");
logger.warn("Slow query");
"#,
        );
        assert_eq!(ext.sinks.len(), 2);
    }

    #[test]
    fn detects_pii_in_log() {
        let ext = extract_ts(r#"console.log("User email:", user.email);"#);
        assert!(ext.sinks[0].contains_pii);
    }

    #[test]
    fn detects_password_pii() {
        let ext = extract_ts(r#"console.log("Login with password:", password);"#);
        assert!(ext.sinks[0].contains_pii);
    }

    #[test]
    fn extracts_imports() {
        let ext = extract_ts(
            r#"
import express from 'express';
import { Router, Request } from 'express';
"#,
        );
        assert_eq!(ext.imports.len(), 2);
        assert_eq!(ext.imports[0].source, "express");
    }

    #[test]
    fn works_with_javascript_grammar() {
        let path = PathBuf::from("app.js");
        let source = r#"
const express = require('express');
const app = express();
app.get('/api/data', (req, res) => {
    console.log("request received");
    res.json({ ok: true });
});
"#;
        let parsed =
            crate::tree_sitter::parse_source(&path, source, SupportedLanguage::JavaScript, None)
                .unwrap();
        let ext = extract(&path, source, &parsed.tree, SupportedLanguage::JavaScript);

        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.sinks.len(), 1);
        assert_eq!(ext.language, SupportedLanguage::JavaScript);
    }

    // --- NestJS ---

    #[test]
    fn extracts_nestjs_get_route() {
        let ext = extract_ts(
            r#"
@Controller('articles')
class ArticlesController {
    @Get()
    findAll() {
        return [];
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/articles");
    }

    #[test]
    fn extracts_nestjs_get_with_subpath() {
        let ext = extract_ts(
            r#"
@Controller('articles')
class ArticlesController {
    @Get(':slug')
    findOne() {
        return {};
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Get);
        assert_eq!(ext.interfaces[0].path, "/articles/:slug");
    }

    #[test]
    fn extracts_nestjs_post_route() {
        let ext = extract_ts(
            r#"
@Controller('users')
class UsersController {
    @Post()
    create() {
        return {};
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert_eq!(ext.interfaces[0].method, HttpMethod::Post);
        assert_eq!(ext.interfaces[0].path, "/users");
    }

    #[test]
    fn detects_nestjs_useguards_on_method() {
        let ext = extract_ts(
            r#"
@Controller('items')
class ItemsController {
    @Post()
    @UseGuards(AuthGuard('jwt'))
    create() {
        return {};
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        assert!(ext.interfaces[0].auth.is_some());
        match &ext.interfaces[0].auth {
            Some(AuthKind::Decorator(s)) => assert!(s.contains("UseGuards")),
            other => panic!("expected Decorator auth, got {:?}", other),
        }
    }

    #[test]
    fn detects_nestjs_useguards_on_class() {
        let ext = extract_ts(
            r#"
@Controller('admin')
@UseGuards(AuthGuard('jwt'))
class AdminController {
    @Get()
    dashboard() {
        return {};
    }

    @Delete(':id')
    remove() {
        return {};
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 2);
        // Class-level UseGuards applies to all methods
        assert!(ext.interfaces.iter().all(|i| i.auth.is_some()));
    }

    #[test]
    fn nestjs_method_auth_overrides_class() {
        let ext = extract_ts(
            r#"
@Controller('mixed')
@UseGuards(AuthGuard('basic'))
class MixedController {
    @Get()
    @UseGuards(AuthGuard('jwt'))
    secured() {
        return {};
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 1);
        match &ext.interfaces[0].auth {
            Some(AuthKind::Decorator(s)) => assert!(s.contains("jwt")),
            other => panic!("expected jwt auth, got {:?}", other),
        }
    }

    #[test]
    fn realistic_nestjs_controller() {
        let ext = extract_ts(
            r#"
import { Controller, Get, Post, Delete, UseGuards } from '@nestjs/common';

@Controller('api/articles')
@UseGuards(AuthGuard('jwt'))
class ArticlesController {
    @Get()
    findAll() {
        console.log("Listing articles");
        return [];
    }

    @Get(':slug')
    findOne() {
        return {};
    }

    @Post()
    create() {
        console.log("Creating article for:", user.email);
        return {};
    }

    @Delete(':slug')
    remove() {
        return {};
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 4, "4 NestJS routes");
        assert!(
            ext.interfaces.iter().all(|i| i.auth.is_some()),
            "all should inherit class auth"
        );
        assert_eq!(ext.interfaces[0].path, "/api/articles");
        assert_eq!(ext.interfaces[1].path, "/api/articles/:slug");
        assert!(!ext.sinks.is_empty(), "should detect console.log sinks");
        assert!(!ext.imports.is_empty(), "should detect imports");
    }

    #[test]
    fn nestjs_coexists_with_express() {
        let ext = extract_ts(
            r#"
import express from 'express';

const app = express();
app.get('/health', (req, res) => res.json({ ok: true }));

@Controller('api/users')
class UsersController {
    @Get()
    findAll() {
        return [];
    }
}
"#,
        );
        assert_eq!(ext.interfaces.len(), 2, "1 Express + 1 NestJS");
        assert!(
            ext.interfaces.iter().any(|i| i.path == "/health"),
            "Express route"
        );
        assert!(
            ext.interfaces.iter().any(|i| i.path == "/api/users"),
            "NestJS route"
        );
    }

    #[test]
    fn realistic_multi_feature_file() {
        let ext = extract_ts(
            r#"
import express from 'express';
import { authMiddleware } from './auth';

const app = express();

app.get('/health', (req, res) => {
    res.json({ status: 'ok' });
});

app.post('/api/payments', authMiddleware, async (req, res) => {
    console.log("Processing payment for:", req.body.email);
    const result = await fetch("https://payment.gateway/charge", {
        method: 'POST',
        body: JSON.stringify(req.body),
    });
    logger.info("Payment processed");
    res.json(await result.json());
});

app.get('/api/users', (req, res) => {
    console.log("Fetching users");
    res.json([]);
});
"#,
        );
        assert_eq!(ext.interfaces.len(), 3);
        assert!(
            ext.interfaces
                .iter()
                .find(|i| i.path == "/health")
                .unwrap()
                .auth
                .is_none()
        );
        assert!(
            ext.interfaces
                .iter()
                .find(|i| i.path == "/api/payments")
                .unwrap()
                .auth
                .is_some()
        );
        assert_eq!(ext.dependencies.len(), 1);
        assert_eq!(ext.sinks.len(), 3);
        assert!(ext.sinks.iter().any(|s| s.contains_pii));
        assert_eq!(ext.imports.len(), 2);
    }
