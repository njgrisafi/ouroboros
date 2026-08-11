//! Resolution of string-literal import candidates (ruff-style "string
//! imports") against the first-party module index.

use super::index::ModuleIndex;
use super::resolve::push_ancestor_package_deps;
use super::{ResolveOptions, ResolvedDep, SuppressedAncestorEdge};
use crate::parser::RawImport;

/// Resolve a string-literal import candidate.
///
/// Tries the full dotted path first, then progressively shorter prefixes —
/// trailing components may be attributes rather than modules, so
/// `"a.b.c.MyClass"` resolves to `a.b.c`. Prefixes with fewer dots than
/// `options.string_imports_min_dots` are not tried (ruff's
/// `ancestors().take(count - min_dots)`).
///
/// Two deliberate deviations from statement-import resolution:
///
/// - **Unresolved candidates are dropped silently** (not recorded in
///   `unresolved`). The scan-every-string heuristic produces many candidates
///   that were never meant as imports; recording them would bury the
///   unresolved-imports report in noise.
/// - **Self-edges are dropped**: a module string-importing itself
///   (`importlib.import_module(__name__)`) is a `sys.modules` hit at runtime,
///   never a cycle. Matters because docstrings are scanned — without this,
///   `min-scc-size = 1` users would see bogus 1-file "cycles".
///
/// Direct edges to the source module's own ancestors are kept, matching
/// statement-import behavior.
pub(crate) fn resolve_string_import(
    source_module: &str,
    imp: &RawImport,
    index: &ModuleIndex,
    options: &ResolveOptions,
    deps: &mut Vec<ResolvedDep>,
    suppressed: &mut Vec<SuppressedAncestorEdge>,
) {
    for name in &imp.names {
        resolve_candidate(
            source_module,
            &name.name,
            imp.line,
            index,
            options,
            deps,
            suppressed,
        );
    }
}

fn resolve_candidate(
    source_module: &str,
    candidate: &str,
    line: u32,
    index: &ModuleIndex,
    options: &ResolveOptions,
    deps: &mut Vec<ResolvedDep>,
    suppressed: &mut Vec<SuppressedAncestorEdge>,
) {
    let components: Vec<&str> = candidate.split('.').collect();
    // Try the full path and progressively shorter prefixes, stopping before
    // prefixes with fewer than min_dots dots (len - 1 >= min_dots).
    let tries = components
        .len()
        .saturating_sub(options.string_imports_min_dots);
    for len in (1..=components.len()).rev().take(tries) {
        let prefix = components[..len].join(".");
        if !index.contains(&prefix) {
            continue;
        }
        if prefix == source_module {
            return;
        }
        if options.include_ancestor_init {
            push_ancestor_package_deps(source_module, &prefix, line, index, deps, suppressed);
        }
        deps.push(ResolvedDep {
            source: source_module.to_string(),
            target: prefix,
            line,
        });
        return;
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::discovery::{DiscoveryResult, PythonFile, SourceRoot};
    use crate::parser::{ImportKind, ImportedName};

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

    fn string_import(candidate: &str) -> RawImport {
        RawImport {
            kind: ImportKind::StringImport,
            module: None,
            names: vec![ImportedName {
                name: candidate.to_string(),
                asname: None,
            }],
            level: 0,
            line: 7,
        }
    }

    fn options(include_ancestor_init: bool) -> ResolveOptions {
        ResolveOptions {
            include_ancestor_init,
            source_is_package: false,
            string_imports_min_dots: 2,
        }
    }

    fn resolve(
        source_module: &str,
        candidate: &str,
        index: &ModuleIndex,
        options: &ResolveOptions,
    ) -> (Vec<ResolvedDep>, Vec<SuppressedAncestorEdge>) {
        let mut deps = Vec::new();
        let mut suppressed = Vec::new();
        resolve_string_import(
            source_module,
            &string_import(candidate),
            index,
            options,
            &mut deps,
            &mut suppressed,
        );
        (deps, suppressed)
    }

    #[test]
    fn full_path_hit() {
        let index = make_index(&["a.b.c"]);
        let (deps, _) = resolve("other", "a.b.c", &index, &options(false));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].source, "other");
        assert_eq!(deps[0].target, "a.b.c");
        assert_eq!(deps[0].line, 7);
    }

    #[test]
    fn attribute_suffix_shortens_to_module() {
        // "a.b.c.MyClass" — MyClass is an attribute of module a.b.c.
        let index = make_index(&["a.b.c"]);
        let (deps, _) = resolve("other", "a.b.c.MyClass", &index, &options(false));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].target, "a.b.c");
    }

    #[test]
    fn shortening_stops_at_min_dots() {
        // Only "a.b.c" (2 dots) is in the index, but with min_dots=3 the
        // candidate "a.b" (1 dot) must not be tried... and neither may the
        // 2-dot prefix be skipped past: "x.y.z.w" shortens to "x.y.z" (2 dots
        // >= min 2) but not to "x.y" (1 dot < min 2).
        let index = make_index(&["x.y"]);
        let (deps, _) = resolve("other", "x.y.z.w", &index, &options(false));
        assert!(
            deps.is_empty(),
            "prefixes with fewer than min-dots dots must not be tried: {deps:?}"
        );

        let index = make_index(&["x.y.z"]);
        let (deps, _) = resolve("other", "x.y.z.w", &index, &options(false));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].target, "x.y.z");
    }

    #[test]
    fn min_dots_zero_resolves_single_segment() {
        let index = make_index(&["utils"]);
        let opts = ResolveOptions {
            string_imports_min_dots: 0,
            ..options(false)
        };
        let (deps, _) = resolve("other", "utils.helper.run", &index, &opts);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].target, "utils");
    }

    #[test]
    fn unresolved_candidate_dropped_silently() {
        let index = make_index(&["a.b.c"]);
        let (deps, _) = resolve("other", "no.such.module", &index, &options(false));
        assert!(deps.is_empty());
    }

    #[test]
    fn self_edge_dropped() {
        let index = make_index(&["a.b.c"]);
        let (deps, _) = resolve("a.b.c", "a.b.c", &index, &options(false));
        assert!(deps.is_empty(), "self-edge must be dropped: {deps:?}");

        // Attribute path under self also shortens to self and drops.
        let (deps, _) = resolve("a.b.c", "a.b.c.MyClass", &index, &options(false));
        assert!(
            deps.is_empty(),
            "self-edge via shortening must be dropped: {deps:?}"
        );
    }

    #[test]
    fn direct_ancestor_edge_kept() {
        // Parity with statement imports: a string naming an ancestor of the
        // source is a direct edge (the ancestor-or-self guard only applies to
        // derived ancestor-init edges). Note min-dots bounds shortening, so
        // the candidate must shorten to a prefix with >= 2 dots.
        let index = make_index(&["a", "a.b", "a.b.c", "a.b.c.d"]);
        let (deps, _) = resolve("a.b.c.d", "a.b.c.X", &index, &options(false));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].target, "a.b.c");
    }

    #[test]
    fn ancestor_init_edges_emitted_when_enabled() {
        let index = make_index(&["a", "a.b", "a.b.c"]);
        let (deps, _) = resolve("x", "a.b.c", &index, &options(true));
        let targets: Vec<&str> = deps.iter().map(|d| d.target.as_str()).collect();
        assert!(targets.contains(&"a.b.c"));
        assert!(targets.contains(&"a.b"));
        assert!(targets.contains(&"a"));
        assert_eq!(targets.len(), 3);
    }

    #[test]
    fn ancestor_init_edges_off() {
        let index = make_index(&["a", "a.b", "a.b.c"]);
        let (deps, _) = resolve("x", "a.b.c", &index, &options(false));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].target, "a.b.c");
    }

    #[test]
    fn shortening_with_ancestor_guard_records_suppressed() {
        // Source inside a.b string-imports "a.b.c.MyClass": shortens to a.b.c;
        // ancestor-init edges to a.b / a hit the ancestor-or-self guard and
        // are recorded as suppressed, exactly as for real imports.
        let index = make_index(&["a", "a.b", "a.b.c", "a.b.other"]);
        let (deps, suppressed) = resolve("a.b.other", "a.b.c.MyClass", &index, &options(true));
        let targets: Vec<&str> = deps.iter().map(|d| d.target.as_str()).collect();
        assert_eq!(targets, vec!["a.b.c"]);

        let suppressed_pairs: Vec<(&str, &str)> = suppressed
            .iter()
            .map(|e| (e.source.as_str(), e.ancestor_package.as_str()))
            .collect();
        assert!(suppressed_pairs.contains(&("a.b.other", "a.b")));
        assert!(suppressed_pairs.contains(&("a.b.other", "a")));
    }

    #[test]
    fn case_sensitive_miss_dropped() {
        let index = make_index(&["a.b.c"]);
        let (deps, _) = resolve("other", "A.b.c", &index, &options(false));
        assert!(deps.is_empty());
    }

    #[test]
    fn first_hit_wins() {
        // The longest resolving prefix wins; no additional edges for shorter
        // prefixes.
        let index = make_index(&["a", "a.b", "a.b.c"]);
        let (deps, _) = resolve("x", "a.b.c", &index, &options(false));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].target, "a.b.c");
    }
}
