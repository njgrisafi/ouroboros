pub mod build;
pub mod impact;
pub mod restore;
pub mod scc;

pub use build::{
    EdgeMetadata, FileDependencyGraph, FileGraphResult, InitUseGraphResult,
    build_file_dependency_graph, build_init_use_graph,
};
pub use impact::{
    Condensation, PathKind, PathMatch, ReachableCycle, apply_exclusions, condensation, match_path,
    nodes_reaching_cycles, reachable_cycles_from, reachable_cycles_from_pruned, reachable_from,
};
pub use restore::restore_self_ancestor_init_edges;
pub use scc::{FileCycle, dependency_cycles, strongly_connected_components};

pub fn strip_source_root_prefix(
    path: &std::path::Path,
    source_roots: &[String],
) -> std::path::PathBuf {
    use std::cmp::Reverse;
    use std::path::PathBuf;

    if source_roots.is_empty() {
        return path.to_path_buf();
    }

    let mut roots: Vec<&str> = source_roots.iter().map(|s| s.as_str()).collect();
    roots.sort_by_key(|s| Reverse(s.len()));

    let path_fwd = path.to_string_lossy().replace('\\', "/");

    for root in roots {
        let root_normalized = root.trim_end_matches(['/', '\\']);
        if root_normalized.is_empty() || root_normalized == "." {
            continue;
        }

        let prefix = format!("{root_normalized}/");
        if let Some(remainder) = path_fwd.strip_prefix(&prefix) {
            return PathBuf::from(remainder);
        }
    }

    path.to_path_buf()
}

#[cfg(test)]
mod path_util_tests {
    use super::strip_source_root_prefix;
    use std::path::Path;

    #[test]
    fn strips_single_src_root() {
        assert_eq!(
            strip_source_root_prefix(Path::new("src/pkg/a.py"), &["src".to_string()]),
            Path::new("pkg/a.py")
        );
    }

    #[test]
    fn no_roots_returns_path_unchanged() {
        assert_eq!(
            strip_source_root_prefix(Path::new("pkg/a.py"), &[]),
            Path::new("pkg/a.py")
        );
    }

    #[test]
    fn strips_file_directly_under_root() {
        assert_eq!(
            strip_source_root_prefix(Path::new("src/a.py"), &["src".to_string()]),
            Path::new("a.py")
        );
    }

    #[test]
    fn strips_second_root_when_first_does_not_match() {
        assert_eq!(
            strip_source_root_prefix(
                Path::new("lib/x.py"),
                &["src".to_string(), "lib".to_string()]
            ),
            Path::new("x.py")
        );
    }

    #[test]
    fn longest_match_wins_for_nested_roots() {
        // "src/pkg" is longer than "src", so it should win.
        assert_eq!(
            strip_source_root_prefix(
                Path::new("src/pkg/a.py"),
                &["src".to_string(), "src/pkg".to_string()]
            ),
            Path::new("a.py")
        );
    }

    #[test]
    fn no_match_returns_original() {
        assert_eq!(
            strip_source_root_prefix(Path::new("app/a.py"), &["src".to_string()]),
            Path::new("app/a.py")
        );
    }

    #[test]
    fn dot_root_returns_path_unchanged() {
        assert_eq!(
            strip_source_root_prefix(Path::new("app/a.py"), &[".".to_string()]),
            Path::new("app/a.py")
        );
    }

    #[test]
    fn empty_string_root_returns_path_unchanged() {
        assert_eq!(
            strip_source_root_prefix(Path::new("app/a.py"), &["".to_string()]),
            Path::new("app/a.py")
        );
    }

    #[test]
    fn root_with_trailing_slash_stripped_before_matching() {
        // "src/" normalized to "src" — should still strip.
        assert_eq!(
            strip_source_root_prefix(Path::new("src/pkg/a.py"), &["src/".to_string()]),
            Path::new("pkg/a.py")
        );
    }
}
