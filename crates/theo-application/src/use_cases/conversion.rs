//! Shared conversion functions between parser types and graph DTOs.
//!
//! Eliminates duplication between extraction.rs and graph_context_service.rs.

use theo_engine_graph::bridge::{ReferenceKindDto, SymbolKindDto};
use theo_engine_parser::types::{ReferenceKind, SymbolKind};

/// Convert parser SymbolKind to graph SymbolKindDto.
pub fn convert_symbol_kind(kind: &SymbolKind) -> SymbolKindDto {
    match kind {
        SymbolKind::Class => SymbolKindDto::Class,
        SymbolKind::Function => SymbolKindDto::Function,
        SymbolKind::Method => SymbolKindDto::Method,
        SymbolKind::Module => SymbolKindDto::Module,
        SymbolKind::Interface => SymbolKindDto::Interface,
        SymbolKind::Trait => SymbolKindDto::Trait,
        SymbolKind::Enum => SymbolKindDto::Enum,
        SymbolKind::Struct => SymbolKindDto::Struct,
    }
}

/// Convert parser ReferenceKind to graph ReferenceKindDto.
pub fn convert_reference_kind(kind: &ReferenceKind) -> ReferenceKindDto {
    match kind {
        ReferenceKind::Call => ReferenceKindDto::Call,
        ReferenceKind::Extends => ReferenceKindDto::Extends,
        ReferenceKind::Implements => ReferenceKindDto::Implements,
        ReferenceKind::TypeUsage => ReferenceKindDto::TypeUsage,
        ReferenceKind::Import => ReferenceKindDto::Import,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_kind_covers_all_variants() {
        let variants = [
            SymbolKind::Class,
            SymbolKind::Function,
            SymbolKind::Method,
            SymbolKind::Module,
            SymbolKind::Interface,
            SymbolKind::Trait,
            SymbolKind::Enum,
            SymbolKind::Struct,
        ];
        for v in &variants {
            let _ = convert_symbol_kind(v); // Should not panic
        }
    }

    #[test]
    fn reference_kind_covers_all_variants() {
        let variants = [
            ReferenceKind::Call,
            ReferenceKind::Extends,
            ReferenceKind::Implements,
            ReferenceKind::TypeUsage,
            ReferenceKind::Import,
        ];
        for v in &variants {
            let _ = convert_reference_kind(v); // Should not panic
        }
    }
}
