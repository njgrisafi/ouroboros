//! Parser subsystem: extracts raw import statements from Python source code.

pub mod error;
mod imports;

use rustpython_parser::{Parse, ast};

pub use error::ParseError;

/// Default minimum number of dots for a string literal to be considered a
/// module-path candidate (matches ruff's `string-imports-min-dots` default,
/// which matches the Pants default).
pub const DEFAULT_STRING_IMPORTS_MIN_DOTS: usize = 2;

/// The kind of Python import statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    /// `import x` or `import x, y`
    Import,
    /// `from x import y` or `from . import y`
    ImportFrom,
    /// A string literal that looks like a module path, e.g. the `"a.b.c"` in
    /// `importlib.import_module("a.b.c")` (ruff-style "string import").
    ///
    /// Field mapping on [`RawImport`]: the full dotted candidate is the single
    /// entry in `names`; `module` is `None` and `level` is `0`. Unlike real
    /// import statements, trailing components may be attributes rather than
    /// modules — the resolver tries progressively shorter prefixes.
    StringImport,
}

/// A single name within an import statement, possibly aliased.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedName {
    /// The imported name (e.g. `path` in `from os import path`).
    pub name: String,
    /// The alias, if any (e.g. `p` in `from os import path as p`).
    pub asname: Option<String>,
}

/// A raw import extracted from Python source — syntax-level facts only.
///
/// This struct records what the source code says, without resolving
/// whether the import is first-party, third-party, or stdlib.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawImport {
    /// Whether this is an `import`, `from ... import ...` statement, or a
    /// string-literal module-path candidate.
    pub kind: ImportKind,
    /// The module being imported from, if any.
    ///
    /// - `import os` → `None` (module is captured in `names`)
    /// - `from os import path` → `Some("os")`
    /// - `from . import x` → `None`
    pub module: Option<String>,
    /// The names imported by this statement.
    pub names: Vec<ImportedName>,
    /// The relative import level (number of leading dots).
    ///
    /// `0` for absolute imports, `1` for `from .`, `2` for `from ..`, etc.
    pub level: u32,
    /// The 1-indexed line number of this import statement in the source file.
    pub line: u32,
}

/// Options controlling import extraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractOptions {
    /// Whether to include imports (and string-import candidates) nested
    /// inside functions, classes, and control-flow blocks ("local" imports).
    pub include_local: bool,
    /// Whether to detect string literals that look like module paths
    /// (ruff-style "string imports"). Respects the same nesting gate as
    /// `include_local`: when `include_local` is off, only module-level
    /// string literals are scanned.
    pub string_imports: bool,
    /// Minimum number of dots for a string literal to be considered a
    /// module-path candidate. Only relevant when `string_imports` is on.
    pub string_imports_min_dots: usize,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            include_local: false,
            string_imports: false,
            string_imports_min_dots: DEFAULT_STRING_IMPORTS_MIN_DOTS,
        }
    }
}

/// Parse Python source code and extract import statements.
///
/// When `options.include_local` is `false`, only top-level import statements
/// are extracted. When `true`, imports nested inside functions, classes, and
/// control-flow blocks are also included.
///
/// When `options.string_imports` is `true`, string literals that look like
/// dotted module paths (at least `options.string_imports_min_dots` dots) are
/// additionally extracted as [`ImportKind::StringImport`] records.
///
/// Returns a list of [`RawImport`] records representing the raw syntax-level
/// import facts found in the source. Does not resolve imports to files or
/// classify them as first-party vs third-party.
///
/// # Errors
///
/// Returns [`ParseError`] if the source cannot be parsed as valid Python.
pub fn extract_imports(
    source: &str,
    options: &ExtractOptions,
) -> Result<Vec<RawImport>, ParseError> {
    let suite = ast::Suite::parse(source, "<source>").map_err(|e| ParseError::InvalidSyntax {
        message: e.to_string(),
    })?;

    Ok(imports::collect_imports(suite, source, options))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_from_valid_source() {
        let source = "\
import os
from sys import argv
";
        let imports = extract_imports(source, &ExtractOptions::default()).unwrap();
        assert_eq!(imports.len(), 2);
    }

    #[test]
    fn extract_from_empty_source() {
        let imports = extract_imports("", &ExtractOptions::default()).unwrap();
        assert!(imports.is_empty());
    }

    #[test]
    fn extract_from_invalid_syntax() {
        let result = extract_imports("def (broken syntax", &ExtractOptions::default());
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("invalid Python syntax"));
    }
}
