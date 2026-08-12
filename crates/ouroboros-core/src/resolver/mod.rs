//! Resolver subsystem: resolves raw imports against the first-party module
//! inventory and produces dependency edges.
//!
//! The resolver takes [`RawImport`](crate::parser::RawImport) records produced
//! by the parser and the [`DiscoveryResult`](crate::discovery::DiscoveryResult)
//! from the discovery phase, and classifies each import as either a first-party
//! dependency edge or an unresolved import (stdlib/third-party).

pub mod error;
mod index;
mod relative;
mod resolve;
mod string;

pub use error::ResolveError;
pub use index::ModuleIndex;

use crate::config::Config;
use crate::discovery::DiscoveryResult;
use crate::parser::RawImport;

/// A resolved first-party dependency edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDep {
    /// The module that contains the import statement.
    pub source: String,
    /// The first-party module being depended on.
    pub target: String,
    /// The 1-indexed line number of the import statement.
    pub line: u32,
}

/// An in-tree ancestor-package edge dropped from [`ResolvedDep`] output by the
/// ancestor-or-self guard.
///
/// When the ancestor package is a proper ancestor of the importing module, its
/// `__init__.py` is already on the import stack, so no dependency edge is
/// emitted. Recording the suppressed edge lets downstream analysis reason about
/// ancestor-init relationships without changing the dependency graph. Only
/// proper ancestors are recorded; the self prefix is never recorded.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SuppressedAncestorEdge {
    pub source: String,
    pub ancestor_package: String,
    pub line: u32,
}

/// An import that could not be resolved to a first-party module.
///
/// These are typically stdlib or third-party imports, but may also be
/// genuinely broken imports. Stored for potential future analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedImport {
    /// The module that contains the import statement.
    pub source: String,
    /// The absolute dotted path that was attempted (after relative resolution).
    pub import_path: String,
}

/// Options controlling import resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolveOptions {
    /// Whether to record dependency edges to ancestor package `__init__.py`
    /// files of imported modules.
    pub include_ancestor_init: bool,
    /// Whether the importing file is itself a package `__init__.py` (affects
    /// relative-import resolution).
    pub source_is_package: bool,
    /// Which string literals were scanned; controls whether candidates
    /// resolve exactly (`CallSites`) or with prefix shortening (`All`).
    pub string_imports_mode: crate::parser::StringImportsMode,
    /// Minimum dots for string-import candidates; bounds the prefix
    /// shortening applied to candidates like `"a.b.c.MyClass"` in `All` mode.
    pub string_imports_min_dots: usize,
}

/// Resolution results for a single file.
#[derive(Debug)]
pub struct FileResolution {
    /// First-party dependency edges found in this file.
    pub deps: Vec<ResolvedDep>,
    /// Imports that did not match any first-party module.
    pub unresolved: Vec<UnresolvedImport>,
    /// In-tree ancestor-package edges dropped by the ancestor-or-self guard.
    pub suppressed_ancestor_edges: Vec<SuppressedAncestorEdge>,
}

/// Aggregated, deduplicated resolution results for the whole project.
#[derive(Debug)]
pub struct ResolveResult {
    /// Deduplicated first-party dependency edges.
    pub deps: Vec<ResolvedDep>,
    /// All imports that could not be resolved to first-party modules.
    pub unresolved: Vec<UnresolvedImport>,
    /// Deduplicated in-tree ancestor-package edges dropped by the guard.
    pub suppressed_ancestor_edges: Vec<SuppressedAncestorEdge>,
}

/// Resolve imports from a single file against the first-party module index.
///
/// This is the per-file entry point. For bulk resolution across an entire
/// project, see [`resolve_all`].
pub fn resolve_file(
    source_module: &str,
    imports: &[RawImport],
    index: &ModuleIndex,
    options: &ResolveOptions,
) -> FileResolution {
    resolve::resolve_file_imports(source_module, imports, index, options)
}

/// Per-file extraction outcome reported to the optional [`resolve_all`] sink.
///
/// Lets callers (e.g. the CLI's verbose mode) observe the per-file imports
/// that `resolve_all` already computes, and surface read/parse warnings,
/// without re-walking and re-parsing every file.
pub enum ExtractEvent<'a> {
    /// Imports were successfully extracted from a file.
    Imports {
        /// The file's module name.
        module: &'a str,
        /// The extracted raw imports.
        imports: &'a [RawImport],
    },
    /// A file could not be read; it is skipped.
    ReadError {
        /// The absolute path that failed.
        path: &'a std::path::Path,
        /// The I/O error message.
        message: String,
    },
    /// A file could not be parsed; it is skipped.
    ParseError {
        /// The file's module name.
        module: &'a str,
        /// The parse error message.
        message: String,
    },
}

/// Resolve all imports for every discovered file in the project.
///
/// Reads each Python source file, extracts imports, and resolves them
/// against the first-party module index. Returns aggregated, deduplicated
/// results.
///
/// Files that cannot be read or parsed are silently skipped; pass `sink` to
/// observe per-file extraction outcomes (warnings are the CLI's
/// responsibility).
pub fn resolve_all(
    discovery: &DiscoveryResult,
    index: &ModuleIndex,
    config: &Config,
    project_root: &std::path::Path,
    mut sink: Option<&mut dyn FnMut(ExtractEvent<'_>)>,
) -> ResolveResult {
    let extract_options = crate::parser::ExtractOptions::from(&config.parse);
    // Hoisted out of the loop: only `source_is_package` varies per file.
    let mut resolve_options = ResolveOptions {
        include_ancestor_init: config.resolve.include_ancestor_init,
        source_is_package: false,
        string_imports_mode: config.parse.string_imports_mode,
        string_imports_min_dots: config.parse.string_imports_min_dots,
    };
    let mut all_deps = Vec::new();
    let mut all_unresolved = Vec::new();
    let mut all_suppressed = Vec::new();

    for root in &discovery.roots {
        for file in &root.files {
            let abs_path = project_root.join(&file.rel_path);

            let source = match std::fs::read_to_string(&abs_path) {
                Ok(s) => s,
                Err(e) => {
                    if let Some(sink) = sink.as_deref_mut() {
                        sink(ExtractEvent::ReadError {
                            path: &abs_path,
                            message: e.to_string(),
                        });
                    }
                    continue;
                }
            };

            let imports = match crate::parser::extract_imports(&source, &extract_options) {
                Ok(imports) => imports,
                Err(e) => {
                    if let Some(sink) = sink.as_deref_mut() {
                        sink(ExtractEvent::ParseError {
                            module: &file.module_name,
                            message: e.to_string(),
                        });
                    }
                    continue;
                }
            };

            if let Some(sink) = sink.as_deref_mut() {
                sink(ExtractEvent::Imports {
                    module: &file.module_name,
                    imports: &imports,
                });
            }

            resolve_options.source_is_package = file
                .rel_path
                .file_name()
                .is_some_and(|name| name == "__init__.py");
            let resolution = resolve_file(&file.module_name, &imports, index, &resolve_options);
            all_deps.extend(resolution.deps);
            all_unresolved.extend(resolution.unresolved);
            all_suppressed.extend(resolution.suppressed_ancestor_edges);
        }
    }

    // Deduplicate deps (same source→target edge may appear from multiple
    // import statements).
    all_deps.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.target.cmp(&b.target))
            .then(a.line.cmp(&b.line))
    });
    all_deps.dedup();

    // Deduplicate unresolved imports.
    all_unresolved.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.import_path.cmp(&b.import_path))
    });
    all_unresolved.dedup();

    // Dedup is on the full (source, ancestor_package, line) triple by design:
    // the same ancestor package suppressed at different lines is preserved.
    all_suppressed.sort();
    all_suppressed.dedup();

    ResolveResult {
        deps: all_deps,
        unresolved: all_unresolved,
        suppressed_ancestor_edges: all_suppressed,
    }
}
