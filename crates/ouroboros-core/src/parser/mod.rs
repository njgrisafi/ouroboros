//! Parser subsystem: extracts raw import statements from Python source code.

pub mod error;
mod imports;

use rustpython_parser::{Parse, ast};

pub use error::ParseError;

/// The kind of Python import statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    /// `import x` or `import x, y`
    Import,
    /// `from x import y` or `from . import y`
    ImportFrom,
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
    /// Whether this is an `import` or `from ... import ...` statement.
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

/// Parse Python source code and extract import statements.
///
/// When `include_local` is `false`, only top-level import statements are
/// extracted. When `true`, imports nested inside functions, classes, and
/// control-flow blocks are also included.
///
/// Returns a list of [`RawImport`] records representing the raw syntax-level
/// import facts found in the source. Does not resolve imports to files or
/// classify them as first-party vs third-party.
///
/// # Errors
///
/// Returns [`ParseError`] if the source cannot be parsed as valid Python.
pub fn extract_imports(source: &str, include_local: bool) -> Result<Vec<RawImport>, ParseError> {
    let suite = ast::Suite::parse(source, "<source>").map_err(|e| ParseError::InvalidSyntax {
        message: e.to_string(),
    })?;

    Ok(imports::collect_imports(&suite, source, include_local))
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
        let imports = extract_imports(source, false).unwrap();
        assert_eq!(imports.len(), 2);
    }

    #[test]
    fn extract_from_empty_source() {
        let imports = extract_imports("", false).unwrap();
        assert!(imports.is_empty());
    }

    #[test]
    fn extract_from_invalid_syntax() {
        let result = extract_imports("def (broken syntax", false);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("invalid Python syntax"));
    }
}
