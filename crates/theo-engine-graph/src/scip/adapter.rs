//! SCIP Adapter — implements CodeIntelProvider using a parsed SCIP index.
//!
//! Adapter Pattern: converts SCIP protobuf types into domain types.
//! Strategy Pattern: interchangeable with TreeSitterFallback.

#[cfg(feature = "scip")]
use super::reader::ScipIndex;
use theo_domain::code_intel::*;

/// SCIP-backed code intelligence provider.
///
/// Wraps a parsed ScipIndex and implements CodeIntelProvider with exact
/// compiler-verified cross-file references.
#[cfg(feature = "scip")]
pub struct ScipCodeIntelProvider {
    index: ScipIndex,
}

#[cfg(feature = "scip")]
impl ScipCodeIntelProvider {
    pub fn new(index: ScipIndex) -> Self {
        Self { index }
    }

    pub fn from_file(path: &std::path::Path) -> Option<Self> {
        ScipIndex::load(path).map(|index| Self { index })
    }
}

#[cfg(feature = "scip")]
impl CodeIntelProvider for ScipCodeIntelProvider {
    fn is_available(&self) -> bool {
        self.index.document_count > 0
    }

    fn is_precise(&self) -> bool {
        true // SCIP = compiler-verified
    }

    fn resolve_references(&self, symbol_name: &str) -> Vec<SymbolReference> {
        let mut results = Vec::new();

        // Resolve short name to canonical SCIP symbol IDs
        let canonical_ids = self.index.resolve_name(symbol_name);

        for sym_id in canonical_ids {
            if let Some(refs) = self.index.symbol_references.get(sym_id) {
                for (file_path, line, role_str) in refs {
                    let role = match role_str.as_str() {
                        "definition" => ReferenceRole::Definition,
                        "import" => ReferenceRole::Import,
                        "write" => ReferenceRole::WriteAccess,
                        "read" => ReferenceRole::ReadAccess,
                        _ => ReferenceRole::Reference,
                    };
                    results.push(SymbolReference {
                        file_path: file_path.clone(),
                        line: *line,
                        column: 0, // SCIP has column but we simplify
                        role,
                        symbol_id: sym_id.clone(),
                    });
                }
            }
        }

        results
    }

    fn find_definitions(&self, symbol_name: &str) -> Vec<SourceLocation> {
        let canonical_ids = self.index.resolve_name(symbol_name);
        let mut results = Vec::new();

        for sym_id in canonical_ids {
            if let Some(file_path) = self.index.symbol_definitions.get(sym_id) {
                // Find the exact line from references
                let line = self
                    .index
                    .symbol_references
                    .get(sym_id)
                    .and_then(|refs| refs.iter().find(|(_, _, r)| r == "definition"))
                    .map(|(_, l, _)| *l)
                    .unwrap_or(0);

                results.push(SourceLocation {
                    file_path: file_path.clone(),
                    line,
                    column: 0,
                });
            }
        }

        results
    }

    fn find_implementations(&self, _trait_name: &str) -> Vec<SourceLocation> {
        // TODO: Parse SCIP Relationship::is_implementation
        // For now, return empty — requires walking SymbolInformation.relationships
        vec![]
    }

    fn find_dependents(&self, file_path: &str) -> Vec<String> {
        self.index.find_dependents(file_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "scip")]
    #[test]
    fn scip_provider_with_empty_index() {
        use super::super::reader::ScipIndex;
        use std::collections::HashMap;

        let index = ScipIndex {
            symbol_definitions: HashMap::new(),
            symbol_references: HashMap::new(),
            file_symbols: HashMap::new(),
            name_to_symbols: HashMap::new(),
            document_count: 0,
            occurrence_count: 0,
        };

        let provider = ScipCodeIntelProvider::new(index);
        assert!(!provider.is_available());
        assert!(provider.is_precise());
        assert!(provider.resolve_references("anything").is_empty());
    }
}
