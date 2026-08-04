//! Binding-target resolution: maps a single [`RawImport`] to the set of
//! first-party modules the names it introduces ultimately refer to.
//!
//! Where the primary resolver ([`resolve_file_imports`](super::resolve)) only
//! records `source -> target` edges, this helper additionally tracks the
//! *local binding* — the syntactic name visible in the importing namespace —
//! so later analysis (e.g. lazy-import checks) can associate a usage site with
//! the module it points at. The resolution rules mirror the primary resolver
//! exactly, minus ancestor-`__init__` edges (always
//! `include_ancestor_init = false` semantics).

use crate::parser::{ImportKind, ImportedName, RawImport};

use super::index::ModuleIndex;
use super::relative::resolve_relative;

/// A single name binding introduced by an import statement, mapped to the
/// first-party module it ultimately targets.
///
/// [`resolve_binding_target`] produces one `BindingTarget` per name that the
/// importing module can reference and that resolves to a first-party module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BindingTarget {
    /// First dotted component of `local_prefix` — the multimap key used to
    /// look a binding up by the leading name seen at a usage site.
    pub root_name: String,
    /// The syntactic path visible in the importing namespace (e.g. `a.b.c`
    /// for `import a.b.c`, or the bound local name for `from` imports).
    pub local_prefix: String,
    /// The resolved first-party module to emit as the edge target.
    pub target_module: String,
}

/// Map a single [`RawImport`] to the set of first-party [`BindingTarget`]s it
/// introduces.
///
/// Mirrors the resolution rules of
/// [`resolve_file_imports`](super::resolve::resolve_file_imports) but tracks
/// the local binding alongside the resolved target module. Returns an empty
/// vector for imports that do not resolve to any first-party module
/// (stdlib/third-party, star imports, relative imports that escape the root,
/// and dotted paths absent from the index — `import a.b.c` never falls back to
/// a parent prefix).
#[allow(dead_code)]
pub(crate) fn resolve_binding_target(
    source_module: &str,
    imp: &RawImport,
    index: &ModuleIndex,
    source_is_package: bool,
) -> Vec<BindingTarget> {
    match imp.kind {
        ImportKind::Import => resolve_import_bindings(imp, index),
        ImportKind::ImportFrom => {
            resolve_import_from_bindings(source_module, imp, index, source_is_package)
        }
    }
}

/// Resolve bindings for an `import a.b.c` / `import a.b.c as x` statement.
///
/// Exact full-path matching only: `import a.b.c` binds only when `a.b.c` is
/// itself a first-party module, never falling back to `a` or `a.b`.
fn resolve_import_bindings(imp: &RawImport, index: &ModuleIndex) -> Vec<BindingTarget> {
    let mut out = Vec::new();

    for name in &imp.names {
        if !index.contains(&name.name) {
            continue;
        }

        let local_prefix = binding_local_prefix(name);
        out.push(BindingTarget {
            root_name: root_of(&local_prefix),
            local_prefix,
            target_module: name.name.clone(),
        });
    }

    out
}

/// Resolve bindings for a `from X import ...` statement (absolute or relative).
///
/// Mirrors `resolve_import_from_stmt`'s `any_resolved` fallback: submodule
/// names win outright, and only when no name is a submodule does the base
/// module become the shared target for every bound (non-star) name.
fn resolve_import_from_bindings(
    source_module: &str,
    imp: &RawImport,
    index: &ModuleIndex,
    source_is_package: bool,
) -> Vec<BindingTarget> {
    let base_module = match resolve_base_module(source_module, imp, source_is_package) {
        Some(base) => base,
        None => return Vec::new(),
    };

    let mut out = Vec::new();

    for name in &imp.names {
        if name.name == "*" {
            continue;
        }

        let qualified = qualify(&base_module, &name.name);
        if index.contains(&qualified) {
            let local_prefix = binding_local_prefix(name);
            out.push(BindingTarget {
                root_name: root_of(&local_prefix),
                local_prefix,
                target_module: qualified,
            });
        }
    }

    if !out.is_empty() {
        return out;
    }

    if !base_module.is_empty() && index.contains(&base_module) {
        for name in &imp.names {
            if name.name == "*" {
                continue;
            }
            let local_prefix = binding_local_prefix(name);
            out.push(BindingTarget {
                root_name: root_of(&local_prefix),
                local_prefix,
                target_module: base_module.clone(),
            });
        }
    }

    out
}

/// Determine the absolute base module for a `from` import.
///
/// Returns `None` when the import is malformed (`from import x`, no module and
/// no leading dots) or when a relative import escapes the source root.
fn resolve_base_module(
    source_module: &str,
    imp: &RawImport,
    source_is_package: bool,
) -> Option<String> {
    if imp.level > 0 {
        // Inside a package's `__init__.py`, a leading dot refers to the package
        // itself, so one fewer level is stripped than for a regular module.
        let effective_level = if source_is_package {
            imp.level.saturating_sub(1)
        } else {
            imp.level
        };
        resolve_relative(source_module, effective_level, imp.module.as_deref()).ok()
    } else {
        imp.module.clone()
    }
}

fn qualify(base_module: &str, name: &str) -> String {
    if base_module.is_empty() {
        name.to_string()
    } else {
        format!("{base_module}.{name}")
    }
}

fn binding_local_prefix(name: &ImportedName) -> String {
    match &name.asname {
        Some(alias) => alias.clone(),
        None => name.name.clone(),
    }
}

fn root_of(local_prefix: &str) -> String {
    match local_prefix.split_once('.') {
        Some((head, _)) => head.to_string(),
        None => local_prefix.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{DiscoveryResult, PythonFile, SourceRoot};
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

    fn name_as(n: &str, a: &str) -> ImportedName {
        ImportedName {
            name: n.to_string(),
            asname: Some(a.to_string()),
        }
    }

    fn import(names: Vec<ImportedName>) -> RawImport {
        RawImport {
            kind: ImportKind::Import,
            module: None,
            names,
            level: 0,
            line: 0,
        }
    }

    fn import_from(module: Option<&str>, level: u32, names: Vec<ImportedName>) -> RawImport {
        RawImport {
            kind: ImportKind::ImportFrom,
            module: module.map(str::to_string),
            names,
            level,
            line: 0,
        }
    }

    fn triple(bt: &BindingTarget) -> (&str, &str, &str) {
        (
            bt.root_name.as_str(),
            bt.local_prefix.as_str(),
            bt.target_module.as_str(),
        )
    }

    // ---- `import ...` ----

    #[test]
    fn import_dotted_exact_match() {
        let index = make_index(&["a", "a.b", "a.b.c"]);
        let imp = import(vec![name("a.b.c")]);
        let out = resolve_binding_target("app", &imp, &index, false);
        assert_eq!(out.len(), 1);
        assert_eq!(triple(&out[0]), ("a", "a.b.c", "a.b.c"));
    }

    #[test]
    fn import_dotted_no_parent_fallback() {
        // Only `a` and `a.b` exist, not `a.b.c` — exact match only, no fallback.
        let index = make_index(&["a", "a.b"]);
        let imp = import(vec![name("a.b.c")]);
        let out = resolve_binding_target("app", &imp, &index, false);
        assert!(out.is_empty());
    }

    #[test]
    fn import_dotted_aliased() {
        let index = make_index(&["a", "a.b", "a.b.c"]);
        let imp = import(vec![name_as("a.b.c", "x")]);
        let out = resolve_binding_target("app", &imp, &index, false);
        assert_eq!(out.len(), 1);
        assert_eq!(triple(&out[0]), ("x", "x", "a.b.c"));
    }

    #[test]
    fn import_stdlib_empty() {
        let index = make_index(&["a.b.c"]);
        let imp = import(vec![name("os")]);
        let out = resolve_binding_target("app", &imp, &index, false);
        assert!(out.is_empty());
    }

    // ---- `from ... import ...` ----

    #[test]
    fn from_import_submodule() {
        let index = make_index(&["a", "a.b"]);
        let imp = import_from(Some("a"), 0, vec![name("b")]);
        let out = resolve_binding_target("app", &imp, &index, false);
        assert_eq!(out.len(), 1);
        assert_eq!(triple(&out[0]), ("b", "b", "a.b"));
    }

    #[test]
    fn from_import_submodule_and_attribute_subset() {
        // `a.b` is a submodule, `a.B` is not — only `b` yields a binding.
        let index = make_index(&["a", "a.b"]);
        let imp = import_from(Some("a"), 0, vec![name("b"), name("B")]);
        let out = resolve_binding_target("app", &imp, &index, false);
        assert_eq!(out.len(), 1);
        assert_eq!(triple(&out[0]), ("b", "b", "a.b"));
    }

    #[test]
    fn from_import_symbols_fallback_to_base() {
        // Neither `a.B` nor `a.C` is a module, but `a` exists → base fallback.
        let index = make_index(&["a"]);
        let imp = import_from(Some("a"), 0, vec![name("B"), name_as("C", "D")]);
        let out = resolve_binding_target("app", &imp, &index, false);
        assert_eq!(out.len(), 2);
        assert_eq!(triple(&out[0]), ("B", "B", "a"));
        assert_eq!(triple(&out[1]), ("D", "D", "a"));
    }

    #[test]
    fn from_import_submodule_aliased() {
        let index = make_index(&["a", "a.b"]);
        let imp = import_from(Some("a"), 0, vec![name_as("b", "c")]);
        let out = resolve_binding_target("app", &imp, &index, false);
        assert_eq!(out.len(), 1);
        assert_eq!(triple(&out[0]), ("c", "c", "a.b"));
    }

    #[test]
    fn from_import_multiple_submodules() {
        let index = make_index(&["a", "a.b", "a.c"]);
        let imp = import_from(Some("a"), 0, vec![name("b"), name("c")]);
        let out = resolve_binding_target("app", &imp, &index, false);
        assert_eq!(out.len(), 2);
        assert_eq!(triple(&out[0]), ("b", "b", "a.b"));
        assert_eq!(triple(&out[1]), ("c", "c", "a.c"));
    }

    #[test]
    fn from_import_star_empty() {
        let index = make_index(&["a", "a.b"]);
        let imp = import_from(Some("a"), 0, vec![name("*")]);
        let out = resolve_binding_target("app", &imp, &index, false);
        assert!(out.is_empty());
    }

    // ---- relative `from ... import ...` ----

    #[test]
    fn from_relative_dot_import_sibling() {
        // `from . import sib` in `pkg.mod` (regular module, not a package).
        let index = make_index(&["pkg", "pkg.mod", "pkg.sib"]);
        let imp = import_from(None, 1, vec![name("sib")]);
        let out = resolve_binding_target("pkg.mod", &imp, &index, false);
        assert_eq!(out.len(), 1);
        assert_eq!(triple(&out[0]), ("sib", "sib", "pkg.sib"));
    }

    #[test]
    fn from_relative_double_dot_with_module() {
        // `from ..pkg import m` in `a.b.c` → base `a.pkg`, `a.pkg.m` submodule.
        let index = make_index(&["a.pkg", "a.pkg.m"]);
        let imp = import_from(Some("pkg"), 2, vec![name("m")]);
        let out = resolve_binding_target("a.b.c", &imp, &index, false);
        assert_eq!(out.len(), 1);
        assert_eq!(triple(&out[0]), ("m", "m", "a.pkg.m"));
    }

    #[test]
    fn from_relative_dot_import_missing_empty() {
        // `from . import missing`: base `pkg` is not first-party (no __init__)
        // and `pkg.missing` is not a submodule → no binding.
        let index = make_index(&["pkg.mod", "pkg.sib"]);
        let imp = import_from(None, 1, vec![name("missing")]);
        let out = resolve_binding_target("pkg.mod", &imp, &index, false);
        assert!(out.is_empty());
    }

    #[test]
    fn from_relative_escapes_root_empty() {
        // level 3 with only 2 source components → escapes root → no binding.
        let index = make_index(&["pkg.mod"]);
        let imp = import_from(Some("x"), 3, vec![name("y")]);
        let out = resolve_binding_target("pkg.mod", &imp, &index, false);
        assert!(out.is_empty());
    }

    #[test]
    fn from_relative_bare_import_from_package_init() {
        // `from . import staff_service` inside `pkg/services/__init__.py`:
        // source_is_package strips one level so `.` is the package itself.
        let index = make_index(&["pkg.services", "pkg.services.staff_service"]);
        let imp = import_from(None, 1, vec![name("staff_service")]);
        let out = resolve_binding_target("pkg.services", &imp, &index, true);
        assert_eq!(out.len(), 1);
        assert_eq!(
            triple(&out[0]),
            (
                "staff_service",
                "staff_service",
                "pkg.services.staff_service"
            )
        );
    }
}
