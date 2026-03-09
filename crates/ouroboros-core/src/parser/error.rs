/// Errors that can occur during Python source parsing.
#[derive(Debug)]
pub enum ParseError {
    /// The Python source could not be parsed into a valid AST.
    InvalidSyntax {
        /// Human-readable description of the parse failure.
        message: String,
    },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::InvalidSyntax { message } => {
                write!(f, "invalid Python syntax: {message}")
            }
        }
    }
}

impl std::error::Error for ParseError {}
