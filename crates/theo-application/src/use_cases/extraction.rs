/// Full extraction: parse source files with tree-sitter and convert to bridge::FileData.
///
/// This module connects `theo-code-parser` (extraction) to `theo-code-graph` (graph model)
/// via the bridge DTOs. It replaces the need for the separate Intently engine.
use std::path::Path;
use std::time::Instant;

use rayon::prelude::*;
use theo_engine_graph::bridge::{
    DataModelData, FileData, ImportData, ReferenceData, ReferenceKindDto, SymbolData, SymbolKindDto,
};
use theo_engine_parser::extractors;
use theo_engine_parser::tree_sitter::{detect_language, parse_source};
use theo_engine_parser::types::{ReferenceKind, SymbolKind};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Statistics from a full extraction run.
#[derive(Debug, Clone, Default)]
pub struct ExtractionStats {
    pub files_found: usize,
    pub files_parsed: usize,
    pub files_skipped: usize,
    pub symbols_extracted: usize,
    pub references_extracted: usize,
    pub elapsed_ms: u64,
}

/// Walk a repository directory, parse all supported source files with tree-sitter,
/// extract symbols/references/imports/data_models, and return bridge-compatible FileData.
///
/// Uses rayon for parallel extraction across files.
pub fn extract_repo(repo_root: &Path) -> (Vec<FileData>, ExtractionStats) {
    let start = Instant::now();

    // Discover source files.
    // Uses theo-domain EXCLUDED_DIRS as source of truth + .gitignore + .theoignore.
    let mut walker_builder = ignore::WalkBuilder::new(repo_root);
    walker_builder.hidden(true).git_ignore(true);
    // Fallback: read .gitignore even when .git/ is absent (rsync, tarballs)
    let _ = walker_builder.add_ignore(repo_root.join(".gitignore"));
    // Custom ignore: projects can add .theoignore for graph-specific exclusions
    walker_builder.add_custom_ignore_filename(".theoignore");
    walker_builder.filter_entry(|entry| {
        // Skip excluded directories (but not files with those names, e.g. build.rs)
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            if let Some(name) = entry.file_name().to_str() {
                return !theo_domain::graph_context::EXCLUDED_DIRS.contains(&name);
            }
        }
        true
    });
    let walker = walker_builder.build();

    let mut source_files: Vec<(String, String)> = Vec::new(); // (rel_path, abs_path)

    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        if detect_language(path).is_none() {
            continue;
        }
        let rel_path = path
            .strip_prefix(repo_root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        let abs_path = path.to_string_lossy().to_string();
        source_files.push((rel_path, abs_path));
    }

    let files_found = source_files.len();

    // Parallel extraction
    let results: Vec<Option<(FileData, usize, usize)>> = source_files
        .par_iter()
        .map(|(rel_path, abs_path)| extract_single_file(rel_path, abs_path))
        .collect();

    // Collect results
    let mut files = Vec::with_capacity(files_found);
    let mut stats = ExtractionStats {
        files_found,
        ..Default::default()
    };

    for result in results {
        match result {
            Some((file_data, sym_count, ref_count)) => {
                stats.files_parsed += 1;
                stats.symbols_extracted += sym_count;
                stats.references_extracted += ref_count;
                files.push(file_data);
            }
            None => {
                stats.files_skipped += 1;
            }
        }
    }

    stats.elapsed_ms = start.elapsed().as_millis() as u64;
    (files, stats)
}

/// Extract a single file given the repository root and a relative path.
///
/// Computes the absolute path from `repo_root` + `rel_path`, parses the file
/// with tree-sitter, and returns bridge-compatible `FileData`. Returns `None`
/// if the file cannot be read, is not a supported language, or parsing fails.
pub fn extract_single_file_from_repo(repo_root: &Path, rel_path: &str) -> Option<FileData> {
    let abs_path = repo_root.join(rel_path);
    let abs_str = abs_path.to_string_lossy().to_string();
    extract_single_file(rel_path, &abs_str).map(|(file_data, _, _)| file_data)
}

// ---------------------------------------------------------------------------
// Single file extraction
// ---------------------------------------------------------------------------

fn extract_single_file(rel_path: &str, abs_path: &str) -> Option<(FileData, usize, usize)> {
    let path = Path::new(abs_path);
    let language = detect_language(path)?;

    let source = std::fs::read_to_string(path).ok()?;
    let line_count = source.lines().count();

    let last_modified = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
        })
        .unwrap_or(0.0);

    // Parse with tree-sitter
    let parsed = parse_source(path, &source, language, None).ok()?;

    // Extract using tree-sitter extractors
    let extraction = extractors::extract(path, &source, &parsed.tree, language);

    // Convert to bridge FileData
    let symbols: Vec<SymbolData> = extraction
        .symbols
        .iter()
        .map(|s| SymbolData {
            qualified_name: format!("{}::{}", s.parent.as_deref().unwrap_or(""), s.name)
                .trim_start_matches("::")
                .to_string(),
            name: s.name.clone(),
            kind: convert_symbol_kind(&s.kind),
            line_start: s.anchor.line,
            line_end: s.anchor.end_line,
            signature: s.signature.clone(),
            is_test: s.is_test,
            parent: s.parent.clone(),
            doc: s.doc.clone(),
        })
        .collect();

    let imports: Vec<ImportData> = extraction
        .imports
        .iter()
        .map(|i| ImportData {
            source: i.source.clone(),
            specifiers: i.specifiers.clone(),
            line: i.line,
        })
        .collect();

    let references: Vec<ReferenceData> = extraction
        .references
        .iter()
        .map(|r| ReferenceData {
            source_symbol: r.source_symbol.clone(),
            source_file: r.source_file.to_string_lossy().to_string(),
            target_symbol: r.target_symbol.clone(),
            target_file: r
                .target_file
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            kind: convert_reference_kind(&r.reference_kind),
        })
        .collect();

    let data_models: Vec<DataModelData> = extraction
        .data_models
        .iter()
        .map(|dm| DataModelData {
            name: dm.name.clone(),
            file_path: rel_path.to_string(),
            line_start: dm.anchor.line,
            line_end: dm.anchor.end_line,
            parent_type: dm.parent_type.clone(),
            implemented_interfaces: dm.implemented_interfaces.clone(),
        })
        .collect();

    let sym_count = symbols.len();
    let ref_count = references.len();

    Some((
        FileData {
            path: rel_path.to_string(),
            language: format!("{:?}", language).to_lowercase(),
            line_count,
            last_modified,
            symbols,
            imports,
            references,
            data_models,
        },
        sym_count,
        ref_count,
    ))
}

// ---------------------------------------------------------------------------
// Type conversions
// ---------------------------------------------------------------------------

use crate::use_cases::conversion::{convert_reference_kind, convert_symbol_kind};
