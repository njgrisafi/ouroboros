use std::collections::HashSet;

use crate::discovery::DiscoveryResult;

/// A fast lookup index of all known first-party module names.
///
/// Built from [`DiscoveryResult`] by collecting every discovered
/// `PythonFile`'s dotted module name into a `HashSet`.
#[derive(Debug)]
pub struct ModuleIndex {
    modules: HashSet<String>,
}

impl ModuleIndex {
    /// Build a module index from discovery results.
    ///
    /// Iterates every [`PythonFile`](crate::discovery::PythonFile) across
    /// all source roots and inserts its `module_name`. Empty module names
    /// (from root-level `__init__.py` files) are skipped.
    pub fn from_discovery(result: &DiscoveryResult) -> Self {
        let mut modules = HashSet::new();

        for root in &result.roots {
            for file in &root.files {
                if !file.module_name.is_empty() {
                    modules.insert(file.module_name.clone());
                }
            }
        }

        ModuleIndex { modules }
    }

    /// Check whether a dotted module name is a known first-party module.
    pub fn contains(&self, module: &str) -> bool {
        self.modules.contains(module)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{PythonFile, SourceRoot};
    use std::path::PathBuf;

    fn make_discovery(modules: &[&str]) -> DiscoveryResult {
        let files = modules
            .iter()
            .map(|m| PythonFile {
                rel_path: PathBuf::from(m.replace('.', "/") + ".py"),
                module_name: m.to_string(),
            })
            .collect();

        DiscoveryResult {
            roots: vec![SourceRoot {
                path: PathBuf::from("/fake/root"),
                files,
            }],
        }
    }

    #[test]
    fn contains_known_modules() {
        let result = make_discovery(&["core.engine", "models.user", "app"]);
        let index = ModuleIndex::from_discovery(&result);

        assert!(index.contains("core.engine"));
        assert!(index.contains("models.user"));
        assert!(index.contains("app"));
    }

    #[test]
    fn does_not_contain_unknown_modules() {
        let result = make_discovery(&["core.engine", "models.user"]);
        let index = ModuleIndex::from_discovery(&result);

        assert!(!index.contains("os"));
        assert!(!index.contains("sys"));
        assert!(!index.contains("numpy"));
        assert!(!index.contains("core.missing"));
    }

    #[test]
    fn skips_empty_module_names() {
        let result = DiscoveryResult {
            roots: vec![SourceRoot {
                path: PathBuf::from("/fake/root"),
                files: vec![
                    PythonFile {
                        rel_path: PathBuf::from("__init__.py"),
                        module_name: "".to_string(),
                    },
                    PythonFile {
                        rel_path: PathBuf::from("app.py"),
                        module_name: "app".to_string(),
                    },
                ],
            }],
        };
        let index = ModuleIndex::from_discovery(&result);

        assert!(!index.contains(""));
        assert!(index.contains("app"));
    }

    #[test]
    fn multiple_roots() {
        let result = DiscoveryResult {
            roots: vec![
                SourceRoot {
                    path: PathBuf::from("/fake/src"),
                    files: vec![PythonFile {
                        rel_path: PathBuf::from("core/engine.py"),
                        module_name: "core.engine".to_string(),
                    }],
                },
                SourceRoot {
                    path: PathBuf::from("/fake/lib"),
                    files: vec![PythonFile {
                        rel_path: PathBuf::from("utils/helpers.py"),
                        module_name: "utils.helpers".to_string(),
                    }],
                },
            ],
        };
        let index = ModuleIndex::from_discovery(&result);

        assert!(index.contains("core.engine"));
        assert!(index.contains("utils.helpers"));
    }
}
