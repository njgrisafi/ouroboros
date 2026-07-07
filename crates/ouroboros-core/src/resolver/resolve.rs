use crate::parser::{ImportKind, RawImport};

use super::error::ResolveError;
use super::index::ModuleIndex;
use super::relative::resolve_relative;
use super::{FileResolution, ResolvedDep, UnresolvedImport};

/// Resolve all imports from a single file against the first-party module index.
///
/// For each [`RawImport`], determines whether it references a first-party
/// module (producing a [`ResolvedDep`]) or not (producing an
/// [`UnresolvedImport`]).
///
/// Relative imports are first converted to absolute paths via
/// [`resolve_relative`]. If a relative import escapes the root, it is
/// recorded as unresolved rather than propagating the error.
pub(crate) fn resolve_file_imports(
    source_module: &str,
    imports: &[RawImport],
    index: &ModuleIndex,
    source_is_package: bool,
) -> FileResolution {
    let mut deps = Vec::new();
    let mut unresolved = Vec::new();

    for imp in imports {
        match imp.kind {
            ImportKind::Import => {
                resolve_import_stmt(source_module, imp, index, &mut deps, &mut unresolved);
            }
            ImportKind::ImportFrom => {
                resolve_import_from_stmt(
                    source_module,
                    imp,
                    index,
                    &mut deps,
                    &mut unresolved,
                    source_is_package,
                );
            }
        }
    }

    FileResolution { deps, unresolved }
}

/// Resolve an `import X` statement.
///
/// Each name in the import is a full module path (e.g. `import os` or
/// `import core.engine`). Check each against the index.
fn resolve_import_stmt(
    source_module: &str,
    imp: &RawImport,
    index: &ModuleIndex,
    deps: &mut Vec<ResolvedDep>,
    unresolved: &mut Vec<UnresolvedImport>,
) {
    for name in &imp.names {
        if index.contains(&name.name) {
            deps.push(ResolvedDep {
                source: source_module.to_string(),
                target: name.name.clone(),
                line: imp.line,
            });
        } else {
            unresolved.push(UnresolvedImport {
                source: source_module.to_string(),
                import_path: name.name.clone(),
            });
        }
    }
}

/// Resolve a `from X import y` statement (absolute or relative).
///
/// For relative imports (level > 0), first resolves to an absolute path.
/// Then checks whether the module itself, or `module.name` for each imported
/// name, is in the index.
fn resolve_import_from_stmt(
    source_module: &str,
    imp: &RawImport,
    index: &ModuleIndex,
    deps: &mut Vec<ResolvedDep>,
    unresolved: &mut Vec<UnresolvedImport>,
    source_is_package: bool,
) {
    // Step 1: Determine the absolute module path.
    let base_module = if imp.level > 0 {
        // Relative import — resolve to absolute. Inside a package's
        // `__init__.py`, a leading dot refers to the package itself, so one
        // fewer level is stripped than for a regular module.
        let effective_level = if source_is_package {
            imp.level.saturating_sub(1)
        } else {
            imp.level
        };
        match resolve_relative(source_module, effective_level, imp.module.as_deref()) {
            Ok(resolved) => resolved,
            Err(ResolveError::RelativeEscapesRoot { .. }) => {
                // Cannot resolve — record as unresolved.
                let dots = ".".repeat(imp.level as usize);
                let suffix = imp.module.as_deref().unwrap_or("");
                unresolved.push(UnresolvedImport {
                    source: source_module.to_string(),
                    import_path: format!("{dots}{suffix}"),
                });
                return;
            }
        }
    } else {
        // Absolute import — module is directly available.
        match &imp.module {
            Some(m) => m.clone(),
            None => {
                // `from import x` with no module — malformed, skip.
                return;
            }
        }
    };

    // Step 2: Check whether the imported names are submodules first.
    // If any name resolves as a submodule (`base_module.name`), those are the
    // real dependencies and we should NOT also add the base module itself.
    // Only fall back to the base module when the names are symbols (classes,
    // functions, etc.) rather than submodules.
    let mut any_resolved = false;

    for name in &imp.names {
        if name.name == "*" {
            continue;
        }

        let qualified = if base_module.is_empty() {
            name.name.clone()
        } else {
            format!("{}.{}", base_module, name.name)
        };

        if index.contains(&qualified) {
            deps.push(ResolvedDep {
                source: source_module.to_string(),
                target: qualified,
                line: imp.line,
            });
            any_resolved = true;
        }
    }

    // If no imported names resolved as submodules, the names must be symbols
    // inside the base module — add the base module itself as the dependency.
    if !any_resolved && !base_module.is_empty() && index.contains(&base_module) {
        deps.push(ResolvedDep {
            source: source_module.to_string(),
            target: base_module.clone(),
            line: imp.line,
        });
        any_resolved = true;
    }

    // If nothing resolved at all, record as unresolved.
    if !any_resolved {
        unresolved.push(UnresolvedImport {
            source: source_module.to_string(),
            import_path: base_module,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{DiscoveryResult, PythonFile, SourceRoot};
    use crate::parser::ImportedName;
    use std::path::PathBuf;

    fn make_index(modules: &[&str]) -> ModuleIndex {
        let files = modules
            .iter()
            .map(|m| PythonFile {
                rel_path: PathBuf::from(m.replace('.', "/") + ".py"),
                module_name: m.to_string(),
            })
            .collect();

        let result = DiscoveryResult {
            roots: vec![SourceRoot {
                path: PathBuf::from("/fake"),
                files,
            }],
        };

        ModuleIndex::from_discovery(&result)
    }

    fn name(n: &str) -> ImportedName {
        ImportedName {
            name: n.to_string(),
            asname: None,
        }
    }

    #[test]
    fn absolute_import_first_party() {
        let index = make_index(&["core.engine", "models.user"]);
        let imports = vec![RawImport {
            kind: ImportKind::Import,
            module: None,
            names: vec![name("core.engine")],
            level: 0,
            line: 0,
        }];

        let result = resolve_file_imports("app", &imports, &index, false);
        assert_eq!(result.deps.len(), 1);
        assert_eq!(result.deps[0].source, "app");
        assert_eq!(result.deps[0].target, "core.engine");
        assert!(result.unresolved.is_empty());
    }

    #[test]
    fn absolute_import_third_party() {
        let index = make_index(&["core.engine"]);
        let imports = vec![RawImport {
            kind: ImportKind::Import,
            module: None,
            names: vec![name("os")],
            level: 0,
            line: 0,
        }];

        let result = resolve_file_imports("app", &imports, &index, false);
        assert!(result.deps.is_empty());
        assert_eq!(result.unresolved.len(), 1);
        assert_eq!(result.unresolved[0].import_path, "os");
    }

    #[test]
    fn from_import_first_party_module() {
        let index = make_index(&["core.engine", "models.user"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("core.engine".to_string()),
            names: vec![name("Engine")],
            level: 0,
            line: 0,
        }];

        let result = resolve_file_imports("app", &imports, &index, false);
        assert_eq!(result.deps.len(), 1);
        assert_eq!(result.deps[0].target, "core.engine");
        assert!(result.unresolved.is_empty());
    }

    #[test]
    fn from_import_submodule() {
        // `from models import user` where models.user is a known module.
        let index = make_index(&["models", "models.user"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("models".to_string()),
            names: vec![name("user")],
            level: 0,
            line: 0,
        }];

        let result = resolve_file_imports("app", &imports, &index, false);
        assert_eq!(result.deps.len(), 1);
        assert_eq!(result.deps[0].target, "models.user");
        assert!(result.unresolved.is_empty());
    }

    #[test]
    fn from_import_stdlib() {
        let index = make_index(&["core.engine"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("os".to_string()),
            names: vec![name("path")],
            level: 0,
            line: 0,
        }];

        let result = resolve_file_imports("app", &imports, &index, false);
        assert!(result.deps.is_empty());
        assert_eq!(result.unresolved.len(), 1);
        assert_eq!(result.unresolved[0].import_path, "os");
    }

    #[test]
    fn relative_import_single_dot() {
        let index = make_index(&["services.auth.login", "services.auth.session"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("session".to_string()),
            names: vec![name("create_session")],
            level: 1,
            line: 0,
        }];

        let result = resolve_file_imports("services.auth.login", &imports, &index, false);
        assert_eq!(result.deps.len(), 1);
        assert_eq!(result.deps[0].target, "services.auth.session");
    }

    #[test]
    fn relative_import_double_dot() {
        let index = make_index(&["services.auth.tokens", "services.notifications.email"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("notifications.email".to_string()),
            names: vec![name("send_email")],
            level: 2,
            line: 0,
        }];

        let result = resolve_file_imports("services.auth.tokens", &imports, &index, false);
        assert_eq!(result.deps.len(), 1);
        assert_eq!(result.deps[0].target, "services.notifications.email");
    }

    #[test]
    fn relative_import_dot_import_sibling_module() {
        let index = make_index(&["core", "core.runner", "core.engine"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: None,
            names: vec![name("engine")],
            level: 1,
            line: 0,
        }];

        let result = resolve_file_imports("core.runner", &imports, &index, false);
        assert_eq!(result.deps.len(), 1);
        assert_eq!(result.deps[0].target, "core.engine");
    }

    #[test]
    fn submodule_resolution_excludes_base_package() {
        let index = make_index(&["models", "models.user", "models.base"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("models".to_string()),
            names: vec![name("user"), name("base")],
            level: 0,
            line: 0,
        }];

        let result = resolve_file_imports("app", &imports, &index, false);
        let targets: Vec<&str> = result.deps.iter().map(|d| d.target.as_str()).collect();
        assert_eq!(targets.len(), 2);
        assert!(targets.contains(&"models.user"));
        assert!(targets.contains(&"models.base"));
        assert!(!targets.contains(&"models"));
        assert!(result.unresolved.is_empty());
    }

    #[test]
    fn base_package_added_when_names_are_symbols() {
        let index = make_index(&["core.engine"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("core.engine".to_string()),
            names: vec![name("Engine")],
            level: 0,
            line: 0,
        }];

        let result = resolve_file_imports("app", &imports, &index, false);
        assert_eq!(result.deps.len(), 1);
        assert_eq!(result.deps[0].target, "core.engine");
        assert!(result.unresolved.is_empty());
    }

    #[test]
    fn from_import_symbol_from_init_py() {
        let index = make_index(&["models", "models.user"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("models".to_string()),
            names: vec![name("Base")],
            level: 0,
            line: 0,
        }];

        let result = resolve_file_imports("app", &imports, &index, false);
        assert_eq!(result.deps.len(), 1);
        assert_eq!(result.deps[0].target, "models");
        assert!(result.unresolved.is_empty());
    }

    #[test]
    fn relative_import_escapes_root() {
        let index = make_index(&["pkg.mod"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("x".to_string()),
            names: vec![name("y")],
            level: 3,
            line: 0,
        }];

        let result = resolve_file_imports("pkg.mod", &imports, &index, false);
        assert!(result.deps.is_empty());
        assert_eq!(result.unresolved.len(), 1);
        assert!(result.unresolved[0].import_path.contains("..."));
    }

    #[test]
    fn mixed_first_party_and_stdlib() {
        let index = make_index(&["core.engine", "models.user"]);
        let imports = vec![
            RawImport {
                kind: ImportKind::ImportFrom,
                module: Some("core.engine".to_string()),
                names: vec![name("Engine")],
                level: 0,
                line: 0,
            },
            RawImport {
                kind: ImportKind::Import,
                module: None,
                names: vec![name("os")],
                level: 0,
                line: 0,
            },
            RawImport {
                kind: ImportKind::ImportFrom,
                module: Some("pathlib".to_string()),
                names: vec![name("Path")],
                level: 0,
                line: 0,
            },
        ];

        let result = resolve_file_imports("app", &imports, &index, false);
        assert_eq!(result.deps.len(), 1);
        assert_eq!(result.deps[0].target, "core.engine");
        assert_eq!(result.unresolved.len(), 2);
    }

    #[test]
    fn star_import() {
        let index = make_index(&["core.engine"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("core.engine".to_string()),
            names: vec![name("*")],
            level: 0,
            line: 0,
        }];

        let result = resolve_file_imports("app", &imports, &index, false);
        assert_eq!(result.deps.len(), 1);
        assert_eq!(result.deps[0].target, "core.engine");
    }

    #[test]
    fn empty_imports() {
        let index = make_index(&["core.engine"]);
        let result = resolve_file_imports("app", &[], &index, false);
        assert!(result.deps.is_empty());
        assert!(result.unresolved.is_empty());
    }

    #[test]
    fn relative_import_from_package_init_stays_in_package() {
        let index = make_index(&["pkg.services", "pkg.services.staff_service"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("staff_service".to_string()),
            names: vec![name("do_thing")],
            level: 1,
            line: 0,
        }];

        let result = resolve_file_imports("pkg.services", &imports, &index, true);
        let targets: Vec<&str> = result.deps.iter().map(|d| d.target.as_str()).collect();
        assert_eq!(targets, vec!["pkg.services.staff_service"]);
        assert!(result.unresolved.is_empty());
    }

    #[test]
    fn relative_bare_import_from_package_init_resolves_submodule() {
        let index = make_index(&["pkg.services", "pkg.services.staff_service"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: None,
            names: vec![name("staff_service")],
            level: 1,
            line: 0,
        }];

        let result = resolve_file_imports("pkg.services", &imports, &index, true);
        let targets: Vec<&str> = result.deps.iter().map(|d| d.target.as_str()).collect();
        assert_eq!(targets, vec!["pkg.services.staff_service"]);
        assert!(result.unresolved.is_empty());
    }

    #[test]
    fn relative_import_from_regular_module_ascends_to_parent() {
        let index = make_index(&["pkg.services.api", "pkg.services.staff_service"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("staff_service".to_string()),
            names: vec![name("do_thing")],
            level: 1,
            line: 0,
        }];

        let result = resolve_file_imports("pkg.services.api", &imports, &index, false);
        let targets: Vec<&str> = result.deps.iter().map(|d| d.target.as_str()).collect();
        assert_eq!(targets, vec!["pkg.services.staff_service"]);
        assert!(result.unresolved.is_empty());
    }

    #[test]
    fn regular_module_relative_import_does_not_stay_in_self() {
        let index = make_index(&["pkg.services", "pkg.services.staff_service"]);
        let imports = vec![RawImport {
            kind: ImportKind::ImportFrom,
            module: Some("staff_service".to_string()),
            names: vec![name("do_thing")],
            level: 1,
            line: 0,
        }];

        let result = resolve_file_imports("pkg.services", &imports, &index, false);
        assert!(result.deps.is_empty());
        assert_eq!(result.unresolved.len(), 1);
    }
}
