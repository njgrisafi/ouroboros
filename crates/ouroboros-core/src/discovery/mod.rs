//! Discovery subsystem: finds first-party Python files in configured source roots.

pub mod error;
mod module_name;
mod walk;

pub use module_name::module_name_for_path;

use std::path::{Path, PathBuf};

use crate::config::Config;
pub use error::DiscoveryError;

/// A single discovered Python source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PythonFile {
    /// Path relative to the project root (e.g. `src/core/engine.py`).
    pub rel_path: PathBuf,
    /// Canonical dotted Python module name (e.g. `core.engine`).
    pub module_name: String,
}

/// Discovery results for one source root.
#[derive(Debug, Clone)]
pub struct SourceRoot {
    /// The resolved absolute path of this source root.
    pub path: PathBuf,
    /// Python files found under this root, sorted by relative path.
    pub files: Vec<PythonFile>,
}

/// Aggregated discovery results across all configured source roots.
#[derive(Debug, Clone)]
pub struct DiscoveryResult {
    /// Per-root results, in the order they appear in config.
    pub roots: Vec<SourceRoot>,
}

impl DiscoveryResult {
    /// Total number of Python files discovered across all roots.
    pub fn total_files(&self) -> usize {
        self.roots.iter().map(|r| r.files.len()).sum()
    }
}

fn node_path(src_root: &str, walk_rel: &Path) -> PathBuf {
    let root_normalized = src_root.trim_end_matches(['/', '\\']);
    if root_normalized.is_empty() || root_normalized == "." {
        walk_rel.to_path_buf()
    } else {
        PathBuf::from(root_normalized).join(walk_rel)
    }
}

pub fn discover(config: &Config, project_root: &Path) -> Result<DiscoveryResult, DiscoveryError> {
    let mut roots = Vec::with_capacity(config.source_roots.len());

    for src_root in &config.source_roots {
        let resolved = project_root.join(src_root);
        let rel_paths = walk::walk_python_files(&resolved)?;

        let files = rel_paths
            .into_iter()
            .map(|walk_rel| {
                let module_name = module_name::module_name_for_path(&walk_rel);
                let rel_path = node_path(src_root, &walk_rel);
                PythonFile {
                    rel_path,
                    module_name,
                }
            })
            .collect();

        roots.push(SourceRoot {
            path: resolved,
            files,
        });
    }

    Ok(DiscoveryResult { roots })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_project(files: &[&str], source_roots: &[&str]) -> (tempfile::TempDir, Config) {
        let tmp = tempfile::tempdir().unwrap();
        for f in files {
            let full = tmp.path().join(f);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(&full, "# placeholder").unwrap();
        }
        let config = Config {
            source_roots: source_roots.iter().map(|s| s.to_string()).collect(),
            ..Config::default()
        };
        (tmp, config)
    }

    #[test]
    fn discover_single_root() {
        let (tmp, config) = make_project(
            &["src/app.py", "src/core/__init__.py", "src/core/engine.py"],
            &["src"],
        );
        let result = discover(&config, tmp.path()).unwrap();
        assert_eq!(result.roots.len(), 1);
        assert_eq!(result.total_files(), 3);

        let names: Vec<_> = result.roots[0]
            .files
            .iter()
            .map(|f| f.rel_path.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            names,
            vec!["src/app.py", "src/core/__init__.py", "src/core/engine.py"]
        );

        let module_names: Vec<_> = result.roots[0]
            .files
            .iter()
            .map(|f| f.module_name.as_str())
            .collect();
        assert_eq!(module_names, vec!["app", "core", "core.engine"]);
    }

    #[test]
    fn discover_single_root_module_names_not_prefixed() {
        let (tmp, config) = make_project(&["src/core/engine.py"], &["src"]);
        let result = discover(&config, tmp.path()).unwrap();
        let file = &result.roots[0].files[0];
        assert_eq!(file.rel_path, PathBuf::from("src/core/engine.py"));
        assert_eq!(file.module_name, "core.engine");
        assert_ne!(file.module_name, "src.core.engine");
    }

    #[test]
    fn discover_dot_root() {
        let (tmp, config) = make_project(&["app.py", "models/user.py"], &["."]);
        let result = discover(&config, tmp.path()).unwrap();
        assert_eq!(result.total_files(), 2);
        let paths: Vec<_> = result.roots[0]
            .files
            .iter()
            .map(|f| f.rel_path.to_string_lossy().to_string())
            .collect();
        assert!(paths.contains(&"app.py".to_string()));
        assert!(paths.contains(&"models/user.py".to_string()));
    }

    #[test]
    fn discover_multiple_roots() {
        let (tmp, config) = make_project(&["src/a.py", "lib/b.py"], &["src", "lib"]);
        let result = discover(&config, tmp.path()).unwrap();
        assert_eq!(result.roots.len(), 2);
        assert_eq!(result.total_files(), 2);
        assert_eq!(result.roots[0].files[0].rel_path, PathBuf::from("src/a.py"));
        assert_eq!(result.roots[1].files[0].rel_path, PathBuf::from("lib/b.py"));
    }

    #[test]
    fn invalid_root_is_error() {
        let tmp = tempfile::tempdir().unwrap();
        let config = Config {
            source_roots: vec!["nonexistent".to_string()],
            ..Config::default()
        };
        let result = discover(&config, tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn node_path_normalization() {
        let cases: &[(&str, &str, &str)] = &[
            (".", "a/b.py", "a/b.py"),
            ("", "a/b.py", "a/b.py"),
            ("src", "a/b.py", "src/a/b.py"),
            ("src/", "a/b.py", "src/a/b.py"),
        ];
        for (root, file, expected) in cases {
            assert_eq!(
                node_path(root, Path::new(file)),
                PathBuf::from(expected),
                "root={root:?} file={file:?}"
            );
        }
    }
}
