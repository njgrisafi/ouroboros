use std::path::PathBuf;

/// Errors that can occur during file discovery.
#[derive(Debug)]
pub enum DiscoveryError {
    /// A configured source root does not exist or is not a directory.
    InvalidSourceRoot { path: PathBuf, reason: String },
    /// An I/O error occurred while walking a directory tree.
    Walk {
        root: PathBuf,
        source: std::io::Error,
    },
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiscoveryError::InvalidSourceRoot { path, reason } => {
                write!(f, "invalid source root '{}': {}", path.display(), reason)
            }
            DiscoveryError::Walk { root, source } => {
                write!(
                    f,
                    "error walking source root '{}': {}",
                    root.display(),
                    source
                )
            }
        }
    }
}

impl std::error::Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DiscoveryError::Walk { source, .. } => Some(source),
            _ => None,
        }
    }
}
