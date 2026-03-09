use std::path::{Path, PathBuf};

use crate::discovery::error::DiscoveryError;

/// Recursively walk `root` and collect all `.py` file paths.
///
/// Returns paths relative to `root`, sorted for deterministic output.
pub(crate) fn walk_python_files(root: &Path) -> Result<Vec<PathBuf>, DiscoveryError> {
    if !root.is_dir() {
        return Err(DiscoveryError::InvalidSourceRoot {
            path: root.to_path_buf(),
            reason: if root.exists() {
                "not a directory".to_string()
            } else {
                "does not exist".to_string()
            },
        });
    }

    let mut files = Vec::new();
    collect_python_files(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

/// Recursively descend into `dir`, collecting `.py` files relative to `root`.
fn collect_python_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), DiscoveryError> {
    let entries = std::fs::read_dir(dir).map_err(|e| DiscoveryError::Walk {
        root: root.to_path_buf(),
        source: e,
    })?;

    // Collect and sort entries for deterministic traversal order.
    let mut sorted_entries: Vec<_> = entries
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| DiscoveryError::Walk {
            root: root.to_path_buf(),
            source: e,
        })?;
    sorted_entries.sort_by_key(|e| e.file_name());

    for entry in sorted_entries {
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| DiscoveryError::Walk {
            root: root.to_path_buf(),
            source: e,
        })?;

        if file_type.is_dir() {
            collect_python_files(root, &path, out)?;
        } else if file_type.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "py" {
                    // Unwrap is safe: `path` is under `root` by construction.
                    let rel = path.strip_prefix(root).expect("path is under root");
                    out.push(rel.to_path_buf());
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a temp directory with the given file paths.
    fn make_tree(files: &[&str]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for f in files {
            let full = tmp.path().join(f);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(&full, "# placeholder").unwrap();
        }
        tmp
    }

    #[test]
    fn finds_py_files() {
        let tmp = make_tree(&[
            "app.py",
            "core/__init__.py",
            "core/engine.py",
            "readme.md",
        ]);
        let files = walk_python_files(tmp.path()).unwrap();
        assert_eq!(
            files,
            vec![
                PathBuf::from("app.py"),
                PathBuf::from("core/__init__.py"),
                PathBuf::from("core/engine.py"),
            ]
        );
    }

    #[test]
    fn empty_directory_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let files = walk_python_files(tmp.path()).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn nonexistent_root_is_error() {
        let result = walk_python_files(Path::new("/tmp/does-not-exist-ouroboros-test"));
        assert!(result.is_err());
    }

    #[test]
    fn deterministic_order() {
        let tmp = make_tree(&[
            "z.py",
            "a.py",
            "m/b.py",
            "m/a.py",
        ]);
        let files = walk_python_files(tmp.path()).unwrap();
        assert_eq!(
            files,
            vec![
                PathBuf::from("a.py"),
                PathBuf::from("m/a.py"),
                PathBuf::from("m/b.py"),
                PathBuf::from("z.py"),
            ]
        );
    }
}
