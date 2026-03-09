/// Errors that can occur during import resolution.
#[derive(Debug)]
pub enum ResolveError {
    /// A relative import's level exceeds the depth of the source module,
    /// meaning it would escape above the package root.
    ///
    /// For example, `from ...x import y` in a top-level module has level 3
    /// but the module only has depth 0.
    RelativeEscapesRoot {
        /// The dotted module name that contains the import.
        source_module: String,
        /// The relative import level (number of leading dots).
        level: u32,
    },
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::RelativeEscapesRoot {
                source_module,
                level,
            } => {
                write!(
                    f,
                    "relative import with level {} escapes root from module '{}'",
                    level, source_module
                )
            }
        }
    }
}

impl std::error::Error for ResolveError {}
