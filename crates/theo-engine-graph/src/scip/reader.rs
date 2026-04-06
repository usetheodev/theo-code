//! SCIP index reader — parses index.scip protobuf into lookup tables.
//!
//! The `ScipIndex` struct provides O(1) symbol resolution and file-level
//! reference lookups, replacing heuristic name matching in bridge.rs.

use std::collections::HashMap;
use std::path::Path;

/// Parsed SCIP index with fast lookup tables.
#[derive(Debug)]
pub struct ScipIndex {
    /// Canonical symbol ID → definition file path.
    pub symbol_definitions: HashMap<String, String>,

    /// Canonical symbol ID → Vec<(file_path, line, role)>.
    pub symbol_references: HashMap<String, Vec<(String, u32, String)>>,

    /// File path → Vec<(symbol_id, line, role)>.
    pub file_symbols: HashMap<String, Vec<(String, u32, String)>>,

    /// Symbol name (short) → Vec<canonical symbol ID>.
    /// For lookup by unqualified name (e.g., "propagate_attention" → full SCIP ID).
    pub name_to_symbols: HashMap<String, Vec<String>>,

    /// Number of documents in the index.
    pub document_count: usize,

    /// Number of total occurrences.
    pub occurrence_count: usize,
}

impl ScipIndex {
    /// Load a SCIP index from a protobuf file.
    ///
    /// Returns None if the file doesn't exist or can't be parsed.
    #[cfg(feature = "scip")]
    pub fn load(path: &Path) -> Option<Self> {
        use scip::types::Index;

        let bytes = std::fs::read(path).ok()?;
        let index = <Index as prost::Message>::decode(bytes.as_slice()).ok()?;
        Some(Self::from_proto(index))
    }

    /// Build lookup tables from a parsed SCIP protobuf Index.
    #[cfg(feature = "scip")]
    pub fn from_proto(index: scip::types::Index) -> Self {
        let mut symbol_definitions: HashMap<String, String> = HashMap::new();
        let mut symbol_references: HashMap<String, Vec<(String, u32, String)>> = HashMap::new();
        let mut file_symbols: HashMap<String, Vec<(String, u32, String)>> = HashMap::new();
        let mut name_to_symbols: HashMap<String, Vec<String>> = HashMap::new();
        let mut occurrence_count = 0usize;

        let document_count = index.documents.len();

        for doc in &index.documents {
            let file_path = doc.relative_path.clone();

            for occ in &doc.occurrences {
                if occ.symbol.is_empty() || occ.symbol.starts_with("local ") {
                    continue; // Skip local variables
                }

                occurrence_count += 1;
                let line = occ.range.first().copied().unwrap_or(0) as u32;
                let role = decode_role(occ.symbol_roles);

                // Definition: record where the symbol is defined
                if role == "definition" {
                    symbol_definitions.insert(occ.symbol.clone(), file_path.clone());
                }

                // All references
                symbol_references
                    .entry(occ.symbol.clone())
                    .or_default()
                    .push((file_path.clone(), line, role.clone()));

                // File → symbols index
                file_symbols
                    .entry(file_path.clone())
                    .or_default()
                    .push((occ.symbol.clone(), line, role));
            }

            // Symbol information (for name resolution)
            for sym_info in &doc.symbols {
                if sym_info.symbol.is_empty() {
                    continue;
                }
                // Extract short name from SCIP symbol (last descriptor component)
                if let Some(short_name) = extract_short_name(&sym_info.symbol) {
                    name_to_symbols
                        .entry(short_name)
                        .or_default()
                        .push(sym_info.symbol.clone());
                }
            }
        }

        ScipIndex {
            symbol_definitions,
            symbol_references,
            file_symbols,
            name_to_symbols,
            document_count,
            occurrence_count,
        }
    }

    /// Find all files that reference symbols defined in the given file.
    pub fn find_dependents(&self, file_path: &str) -> Vec<String> {
        let mut dependents: std::collections::HashSet<String> = std::collections::HashSet::new();

        // Find symbols defined in this file
        if let Some(syms) = self.file_symbols.get(file_path) {
            for (sym_id, _, role) in syms {
                if role == "definition" {
                    // Find all files that reference this symbol
                    if let Some(refs) = self.symbol_references.get(sym_id) {
                        for (ref_file, _, _) in refs {
                            if ref_file != file_path {
                                dependents.insert(ref_file.clone());
                            }
                        }
                    }
                }
            }
        }

        dependents.into_iter().collect()
    }

    /// Resolve a short symbol name to its canonical SCIP IDs.
    pub fn resolve_name(&self, name: &str) -> &[String] {
        self.name_to_symbols
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// Decode SCIP symbol_roles bitfield to a role string.
#[cfg(feature = "scip")]
fn decode_role(roles: i32) -> String {
    // SCIP SymbolRole bits: Definition=1, Import=2, WriteAccess=4, ReadAccess=8, Generated=16, Test=32
    if roles & 1 != 0 {
        "definition".to_string()
    } else if roles & 2 != 0 {
        "import".to_string()
    } else if roles & 4 != 0 {
        "write".to_string()
    } else if roles & 8 != 0 {
        "read".to_string()
    } else {
        "reference".to_string()
    }
}

/// Extract the short name from a SCIP symbol string.
///
/// SCIP symbol: "rust-analyzer cargo theo-engine-graph 0.1.0 graph_attention/propagate_attention()."
/// Short name: "propagate_attention"
fn extract_short_name(symbol: &str) -> Option<String> {
    // Find the last descriptor: after the last space, split by / and take the last segment
    let descriptors = symbol.rsplit(' ').next()?;
    let last_segment = descriptors.rsplit('/').next()?;

    // Remove type suffixes: () for methods, # for types, . for terms
    let name = last_segment
        .trim_end_matches('.')
        .trim_end_matches("()")
        .trim_end_matches('#')
        .trim_end_matches('!');

    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_short_name_function() {
        assert_eq!(
            extract_short_name("rust-analyzer cargo theo 0.1.0 graph_attention/propagate_attention()."),
            Some("propagate_attention".to_string())
        );
    }

    #[test]
    fn extract_short_name_type() {
        assert_eq!(
            extract_short_name("rust-analyzer cargo theo 0.1.0 model/CodeGraph#"),
            Some("CodeGraph".to_string())
        );
    }

    #[test]
    fn extract_short_name_nested() {
        assert_eq!(
            extract_short_name("rust-analyzer cargo theo 0.1.0 cluster/louvain_phase1()."),
            Some("louvain_phase1".to_string())
        );
    }

    #[test]
    fn extract_short_name_empty() {
        assert_eq!(extract_short_name(""), None);
    }

    #[test]
    fn find_dependents_empty_index() {
        let index = ScipIndex {
            symbol_definitions: HashMap::new(),
            symbol_references: HashMap::new(),
            file_symbols: HashMap::new(),
            name_to_symbols: HashMap::new(),
            document_count: 0,
            occurrence_count: 0,
        };
        assert!(index.find_dependents("any_file.rs").is_empty());
    }

    #[test]
    fn find_dependents_with_data() {
        let mut index = ScipIndex {
            symbol_definitions: HashMap::new(),
            symbol_references: HashMap::new(),
            file_symbols: HashMap::new(),
            name_to_symbols: HashMap::new(),
            document_count: 2,
            occurrence_count: 3,
        };

        // graph_attention.rs defines propagate_attention
        index.file_symbols.insert(
            "src/graph_attention.rs".to_string(),
            vec![("sym:propagate_attention".to_string(), 33, "definition".to_string())],
        );

        // search.rs references propagate_attention
        index.symbol_references.insert(
            "sym:propagate_attention".to_string(),
            vec![
                ("src/graph_attention.rs".to_string(), 33, "definition".to_string()),
                ("src/search.rs".to_string(), 12, "import".to_string()),
                ("src/search.rs".to_string(), 867, "read".to_string()),
            ],
        );

        let deps = index.find_dependents("src/graph_attention.rs");
        assert_eq!(deps.len(), 1);
        assert!(deps.contains(&"src/search.rs".to_string()));
    }
}
