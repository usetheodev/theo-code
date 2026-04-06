//! Code Intelligence Provider — trait for precise cross-file symbol resolution.
//!
//! Defines the contract for compiler-level code intelligence (SCIP, LSP, etc.).
//! The trait lives in theo-domain (pure types); implementations live in
//! theo-engine-graph behind feature gates.
//!
//! Design: Strategy Pattern — Tree-Sitter (approximate) and SCIP (exact) are
//! interchangeable strategies for the same contract. The consumer (bridge.rs)
//! doesn't know which backend is providing the data.

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A precise reference to a symbol at a specific location in a file.
#[derive(Debug, Clone, PartialEq)]
pub struct SymbolReference {
    /// File path relative to project root.
    pub file_path: String,
    /// Line number (0-based).
    pub line: u32,
    /// Column number (0-based).
    pub column: u32,
    /// The role of this reference.
    pub role: ReferenceRole,
    /// The canonical symbol identifier (e.g., SCIP symbol string).
    pub symbol_id: String,
}

/// The role a reference plays at a specific location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceRole {
    /// Symbol is defined here.
    Definition,
    /// Symbol is imported here (use/import statement).
    Import,
    /// Symbol is called here (function invocation).
    Call,
    /// Symbol is read here (variable access, type annotation).
    ReadAccess,
    /// Symbol is written here (assignment).
    WriteAccess,
    /// Symbol is implemented here (trait impl, interface impl).
    Implementation,
    /// Unknown role.
    Reference,
}

/// A location in a source file.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceLocation {
    /// File path relative to project root.
    pub file_path: String,
    /// Line number (0-based).
    pub line: u32,
    /// Column number (0-based).
    pub column: u32,
}

// ---------------------------------------------------------------------------
// Trait (DIP — depended on by bridge.rs, implemented by SCIP/TreeSitter)
// ---------------------------------------------------------------------------

/// Provider of precise code intelligence data.
///
/// Two implementations follow the Strategy Pattern:
/// - `ScipAdapter` (theo-engine-graph, feature "scip") — exact, compiler-verified
/// - `TreeSitterFallback` (theo-engine-graph) — approximate, always available
///
/// The bridge layer consumes this trait without knowing which backend is active.
/// When SCIP is available, it provides exact cross-file references.
/// When SCIP is unavailable (project doesn't compile, indexer missing),
/// Tree-Sitter heuristics are used as fallback.
pub trait CodeIntelProvider: Send + Sync {
    /// Whether this provider has precise data available.
    ///
    /// Returns `true` for SCIP when index.scip exists and is fresh.
    /// Returns `true` for Tree-Sitter always (approximate is always available).
    fn is_available(&self) -> bool;

    /// Whether this provider gives exact (compiler-verified) results.
    ///
    /// SCIP = true, Tree-Sitter = false.
    fn is_precise(&self) -> bool;

    /// Find all references to a symbol by name.
    ///
    /// For SCIP: exact references from compiler analysis.
    /// For Tree-Sitter: approximate matches by name.
    fn resolve_references(&self, symbol_name: &str) -> Vec<SymbolReference>;

    /// Find where a symbol is defined.
    ///
    /// For SCIP: exact definition location.
    /// For Tree-Sitter: heuristic match by qualified name.
    fn find_definitions(&self, symbol_name: &str) -> Vec<SourceLocation>;

    /// Find all implementations of a trait/interface.
    ///
    /// For SCIP: exact `is_implementation` relationships.
    /// For Tree-Sitter: not available (returns empty).
    fn find_implementations(&self, trait_name: &str) -> Vec<SourceLocation>;

    /// Find all files that reference symbols defined in the given file.
    ///
    /// This is the key method for reverse dependency boost:
    /// "given graph_attention.rs, find all files that call its functions."
    fn find_dependents(&self, file_path: &str) -> Vec<String>;
}

/// Null implementation — returns empty for everything.
/// Used when no code intelligence is available.
pub struct NullCodeIntelProvider;

impl CodeIntelProvider for NullCodeIntelProvider {
    fn is_available(&self) -> bool { false }
    fn is_precise(&self) -> bool { false }
    fn resolve_references(&self, _: &str) -> Vec<SymbolReference> { vec![] }
    fn find_definitions(&self, _: &str) -> Vec<SourceLocation> { vec![] }
    fn find_implementations(&self, _: &str) -> Vec<SourceLocation> { vec![] }
    fn find_dependents(&self, _: &str) -> Vec<String> { vec![] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_provider_returns_empty() {
        let provider = NullCodeIntelProvider;
        assert!(!provider.is_available());
        assert!(!provider.is_precise());
        assert!(provider.resolve_references("anything").is_empty());
        assert!(provider.find_definitions("anything").is_empty());
        assert!(provider.find_implementations("anything").is_empty());
        assert!(provider.find_dependents("anything").is_empty());
    }

    #[test]
    fn reference_role_equality() {
        assert_eq!(ReferenceRole::Call, ReferenceRole::Call);
        assert_ne!(ReferenceRole::Call, ReferenceRole::Import);
    }
}
