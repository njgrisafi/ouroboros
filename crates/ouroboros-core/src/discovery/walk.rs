use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::discovery::error::DiscoveryError;

/// Recursively walk `root` and collect all `.py` file paths.
///
/// Returns paths relative to `root`, sorted for deterministic output.
///
/// Uses the `ignore` crate's parallel walker for speed on large trees, with
/// all ignore-file handling (`.gitignore`, `.ignore`, hidden-file skipping)
/// disabled so the file set matches a plain recursive `read_dir` walk.
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

    let files: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
    let walk_error: Mutex<Option<DiscoveryError>> = Mutex::new(None);

    let mut builder = CollectorBuilder {
        global: &files,
        root,
        error: &walk_error,
    };

    ignore::WalkBuilder::new(root)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        .build_parallel()
        .visit(&mut builder);

    if let Some(error) = walk_error.into_inner().expect("walk_error mutex poisoned") {
        return Err(error);
    }

    let mut files = files.into_inner().expect("files mutex poisoned");
    files.sort();
    Ok(files)
}

struct CollectorBuilder<'a> {
    global: &'a Mutex<Vec<PathBuf>>,
    root: &'a Path,
    error: &'a Mutex<Option<DiscoveryError>>,
}

impl<'s, 'a: 's> ignore::ParallelVisitorBuilder<'s> for CollectorBuilder<'a> {
    fn build(&mut self) -> Box<dyn ignore::ParallelVisitor + 's> {
        Box::new(Collector {
            local: Vec::new(),
            global: self.global,
            root: self.root,
            error: self.error,
        })
    }
}

/// Per-thread walk visitor: accumulates matches locally and flushes them to
/// the shared vector once on drop, avoiding per-file lock contention.
struct Collector<'a> {
    local: Vec<PathBuf>,
    global: &'a Mutex<Vec<PathBuf>>,
    root: &'a Path,
    error: &'a Mutex<Option<DiscoveryError>>,
}

impl ignore::ParallelVisitor for Collector<'_> {
    fn visit(&mut self, entry: Result<ignore::DirEntry, ignore::Error>) -> ignore::WalkState {
        match entry {
            Ok(entry) => {
                // Symlinks are not followed (follow_links(false)), so a
                // symlinked file reports is_file() == false here — matching
                // the previous read_dir walk, which also skipped them.
                let is_python_file = entry.file_type().is_some_and(|ft| ft.is_file())
                    && entry.path().extension().is_some_and(|ext| ext == "py");
                if is_python_file {
                    // Unwrap is safe: `path` is under `root` by construction.
                    let rel = entry
                        .path()
                        .strip_prefix(self.root)
                        .expect("path is under root");
                    self.local.push(rel.to_path_buf());
                }
            }
            Err(err) => {
                let mut guard = self.error.lock().expect("walk_error mutex poisoned");
                if guard.is_none() {
                    *guard = Some(DiscoveryError::Walk {
                        root: self.root.to_path_buf(),
                        source: std::io::Error::other(err.to_string()),
                    });
                }
            }
        }
        ignore::WalkState::Continue
    }
}

impl Drop for Collector<'_> {
    fn drop(&mut self) {
        self.global
            .lock()
            .expect("files mutex poisoned")
            .append(&mut self.local);
    }
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
        let tmp = make_tree(&["app.py", "core/__init__.py", "core/engine.py", "readme.md"]);
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
    fn hidden_and_ignored_files_are_included() {
        // All ignore-file handling is disabled: dotfiles, dot-directories,
        // and .gitignore'd paths must all be walked.
        let tmp = make_tree(&[
            ".hidden.py",
            ".git/config.py",
            "ignored/generated.py",
            "kept.py",
        ]);
        fs::write(tmp.path().join(".gitignore"), "ignored/\n").unwrap();
        let files = walk_python_files(tmp.path()).unwrap();
        assert_eq!(
            files,
            vec![
                PathBuf::from(".git/config.py"),
                PathBuf::from(".hidden.py"),
                PathBuf::from("ignored/generated.py"),
                PathBuf::from("kept.py"),
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
        let tmp = make_tree(&["z.py", "a.py", "m/b.py", "m/a.py"]);
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
