//! Single-purpose slice extracted from `types.rs` (T2.4 of god-files-2026-07-23-plan.md, ADR D5).

#![allow(unused_imports, dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tree_sitter::SupportedLanguage;

// ---------------------------------------------------------------------------

use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBudget {
    /// Maximum total bytes across all selected files.
    pub max_bytes: Option<u64>,
    /// Maximum total estimated tokens across all selected files.
    pub max_tokens: Option<u64>,
}

/// Estimate token count from byte size using the `bytes / 4` heuristic.
///
/// This is the standard rough approximation used by LLM tooling.
/// Actual token counts vary by tokenizer, but this is sufficient
/// for budget enforcement and cost estimation.
pub fn estimate_tokens(byte_size: u64) -> u64 {
    byte_size / 4
}

/// The CodeModel: a semantic snapshot of the entire codebase.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodeModel {
    pub version: String,
    pub project_name: String,
    pub components: Vec<Component>,
    pub stats: CodeModelStats,
    // TODO: integrate with pipeline — FileTree from file_tree module
    // pub file_tree: Option<FileTree>,
}

impl CodeModel {
    /// Return a filtered copy containing only references at or above `min_confidence`.
    ///
    /// Filters:
    /// - `Component.references` — removes entries below threshold
    /// - `FileTree.directory_dependencies` — removes entries with `avg_confidence < min_confidence`
    /// - `CodeModelStats` — recalculates `total_references`, `resolved_references`,
    ///   `avg_resolution_confidence`
    pub fn filtered(&self, min_confidence: f64) -> Self {
        let mut model = self.clone();

        for component in &mut model.components {
            component
                .references
                .retain(|r| r.confidence >= min_confidence);
        }

        // Recalculate reference stats
        let total_refs: usize = model.components.iter().map(|c| c.references.len()).sum();
        let resolved: usize = model
            .components
            .iter()
            .flat_map(|c| c.references.iter())
            .filter(|r| r.confidence > 0.0)
            .count();
        let confidence_sum: f64 = model
            .components
            .iter()
            .flat_map(|c| c.references.iter())
            .map(|r| r.confidence)
            .sum();

        model.stats.total_references = total_refs;
        model.stats.resolved_references = resolved;
        model.stats.avg_resolution_confidence = if total_refs == 0 {
            0.0
        } else {
            confidence_sum / total_refs as f64
        };

        model
    }
}

/// A logical component (service, library, module) in the system.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Component {
    pub name: String,
    pub language: SupportedLanguage,
    pub interfaces: Vec<Interface>,
    pub dependencies: Vec<Dependency>,
    pub sinks: Vec<Sink>,
    pub symbols: Vec<Symbol>,
    pub imports: Vec<ImportInfo>,
    pub references: Vec<Reference>,
    pub data_models: Vec<DataModel>,
    pub module_boundaries: Vec<ModuleBoundary>,
    /// Environment variable references aggregated from all files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_dependencies: Vec<EnvDependency>,
}

/// Location of a route parameter within the HTTP request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParameterLocation {
    /// URL path segment (e.g., `/users/:id`).
    Path,
    /// URL query string (e.g., `?page=1`).
    Query,
    /// HTTP header.
    Header,
    /// Request body field.
    Body,
}

/// A parameter associated with a route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteParameter {
    pub name: String,
    pub location: ParameterLocation,
    /// Type annotation, if available (e.g., `"string"`, `"int"`).
    /// `None` for untyped frameworks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param_type: Option<String>,
}

/// An HTTP endpoint exposed by a component.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Interface {
    pub method: HttpMethod,
    pub path: String,
    pub auth: Option<AuthKind>,
    /// Source location of the route definition in the CST.
    #[serde(flatten)]
    pub anchor: SourceAnchor,
    /// Route parameters extracted from the path pattern and framework decorators.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<RouteParameter>,
    /// Name of the handler function/method for this route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handler_name: Option<String>,
    /// Type name of the request body (e.g., `"CreateUserDto"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_body_type: Option<String>,
}

/// HTTP methods supported by the extractor.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
    All,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Get => write!(f, "GET"),
            Self::Post => write!(f, "POST"),
            Self::Put => write!(f, "PUT"),
            Self::Patch => write!(f, "PATCH"),
            Self::Delete => write!(f, "DELETE"),
            Self::Options => write!(f, "OPTIONS"),
            Self::Head => write!(f, "HEAD"),
            Self::All => write!(f, "ALL"),
        }
    }
}

/// Kind of authentication detected on an endpoint.
///
/// Different frameworks express auth in different ways:
/// - Express/Gin/Rails: middleware functions in the route handler chain
/// - FastAPI/Flask/Django: decorators on route handler functions
/// - Spring Boot: annotations on controller methods
/// - ASP.NET Core: attributes on action methods
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthKind {
    /// Express/Gin/Rails: `app.get('/x', authMiddleware, handler)`
    Middleware(String),
    /// Python: `@login_required`, `@jwt_required`
    Decorator(String),
    /// Java/Kotlin: `@PreAuthorize`, `@Secured`
    Annotation(String),
    /// C#: `[Authorize]`, `[Authorize(Roles="admin")]`
    Attribute(String),
}

/// An external dependency (HTTP call, DB connection, etc.)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dependency {
    pub target: String,
    pub dependency_type: DependencyType,
    /// Source location of the dependency call in the CST.
    #[serde(flatten)]
    pub anchor: SourceAnchor,
}

/// Type of external dependency.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DependencyType {
    HttpCall,
}

/// An environment variable reference detected in source code.
///
/// Captures the variable name and source location. Dynamic access
/// (e.g., `process.env[varName]`) is represented as `var_name: "<dynamic>"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvDependency {
    pub var_name: String,
    /// Source location of the env access in the CST.
    #[serde(flatten)]
    pub anchor: SourceAnchor,
}

/// A logging or output sink detected in the source code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sink {
    pub sink_type: SinkType,
    /// Source location of the log/sink call in the CST.
    #[serde(flatten)]
    pub anchor: SourceAnchor,
    pub text: String,
    pub contains_pii: bool,
}

/// Type of sink.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SinkType {
    Log,
}

/// Visibility/access modifier of a code symbol.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Public,
    Private,
    Protected,
    Internal,
}
