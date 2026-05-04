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

/// Resolution results for a single file.
#[derive(Debug)]
pub struct FileResolution {
    /// First-party dependency edges found in this file.
    pub deps: Vec<ResolvedDep>,
    /// Imports that did not match any first-party module.
    pub unresolved: Vec<UnresolvedImport>,
}

/// Aggregated, deduplicated resolution results for the whole project.
#[derive(Debug)]
pub struct ResolveResult {
    /// Deduplicated first-party dependency edges.
    pub deps: Vec<ResolvedDep>,
    /// All imports that could not be resolved to first-party modules.
    pub unresolved: Vec<UnresolvedImport>,
}

/// Resolve imports from a single file against the first-party module index.
///
/// This is the per-file entry point. For bulk resolution across an entire
/// project, see [`resolve_all`].
pub fn resolve_file(
    source_module: &str,
    imports: &[RawImport],
    index: &ModuleIndex,
) -> FileResolution {
    resolve::resolve_file_imports(source_module, imports, index)
}

/// Resolve all imports for every discovered file in the project.
///
/// Reads each Python source file, extracts imports, and resolves them
/// against the first-party module index. Returns aggregated, deduplicated
/// results.
///
/// Files that cannot be read or parsed are silently skipped (warnings are
/// the CLI's responsibility).
pub fn resolve_all(
    discovery: &DiscoveryResult,
    index: &ModuleIndex,
    config: &Config,
) -> ResolveResult {
    let include_local = config.parse.local_imports;
    let mut all_deps = Vec::new();
    let mut all_unresolved = Vec::new();

    for root in &discovery.roots {
        for file in &root.files {
            let abs_path = root.path.join(&file.rel_path);

            let source = match std::fs::read_to_string(&abs_path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            let imports = match crate::parser::extract_imports(&source, include_local) {
                Ok(imports) => imports,
                Err(_) => continue,
            };

            let resolution = resolve_file(&file.module_name, &imports, index);
            all_deps.extend(resolution.deps);
            all_unresolved.extend(resolution.unresolved);
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

    ResolveResult {
        deps: all_deps,
        unresolved: all_unresolved,
    }
}
