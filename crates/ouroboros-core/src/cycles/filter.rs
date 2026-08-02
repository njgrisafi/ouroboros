use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::config::{CyclesConfig, IgnoredCycle};
use crate::graph::{FileCycle, strip_source_root_prefix};

/// Filter cycles (SCCs) by size using the given configuration.
///
/// Retains only cycles whose length is within `[min_scc_size, max_scc_size]`.
/// If `max_scc_size` is `None`, there is no upper bound.
pub fn filter_cycles_by_size(cycles: Vec<FileCycle>, config: &CyclesConfig) -> Vec<FileCycle> {
    cycles
        .into_iter()
        .filter(|cycle| {
            let size = cycle.len();
            if size < config.min_scc_size {
                return false;
            }
            if let Some(max) = config.max_scc_size
                && size > max
            {
                return false;
            }
            true
        })
        .collect()
}

pub struct FilterResult {
    pub kept: Vec<FileCycle>,
    pub suppressed: Vec<FileCycle>,
}

pub fn filter_ignored_cycles(cycles: Vec<FileCycle>, ignored: &[IgnoredCycle]) -> FilterResult {
    let ignore_set: HashSet<Vec<PathBuf>> = ignored
        .iter()
        .map(|ic| {
            let mut paths: Vec<PathBuf> = ic.files.iter().map(PathBuf::from).collect();
            paths.sort();
            paths
        })
        .collect();

    let mut kept = Vec::new();
    let mut suppressed = Vec::new();

    for cycle in cycles {
        if ignore_set.contains(&cycle) {
            suppressed.push(cycle);
        } else {
            kept.push(cycle);
        }
    }

    FilterResult { kept, suppressed }
}

fn package_of_stripped(path: &Path, source_roots: &[String]) -> Option<PathBuf> {
    let stripped = strip_source_root_prefix(path, source_roots);
    let mut components = stripped.components();
    let first = components.next()?;
    if components.next().is_some() {
        Some(PathBuf::from(first.as_os_str()))
    } else {
        None
    }
}

pub fn filter_cycles_by_package(cycles: Vec<FileCycle>, source_roots: &[String]) -> Vec<FileCycle> {
    cycles
        .into_iter()
        .filter(|cycle| {
            let mut iter = cycle.iter();
            let first_pkg = match iter
                .next()
                .and_then(|p| package_of_stripped(p, source_roots))
            {
                Some(pkg) => pkg,
                None => return false,
            };
            iter.all(|p| package_of_stripped(p, source_roots) == Some(first_pkg.clone()))
        })
        .collect()
}

fn path_under_dir(file: &Path, dir_normalized: &str) -> bool {
    let file_str = file.to_string_lossy().replace('\\', "/");
    file_str == dir_normalized || file_str.starts_with(&format!("{dir_normalized}/"))
}

/// Partition cycles into `(kept, ignored)` by directory scope.
///
/// A cycle is `ignored` iff every file in it lives under at least one of the
/// (normalized) `ignore_dirs`. Directory entries are normalized once: `\` is
/// mapped to `/`, a leading `./` and trailing `/` are stripped, and entries
/// that normalize to empty or `.` are skipped. When no usable directory
/// remains the input is returned unchanged as `(cycles, vec![])`. Input order
/// is preserved in `kept`.
pub fn partition_dir_ignored(
    cycles: Vec<FileCycle>,
    ignore_dirs: &[String],
) -> (Vec<FileCycle>, Vec<FileCycle>) {
    let normalized_dirs: Vec<String> = ignore_dirs
        .iter()
        .filter_map(|dir| {
            let replaced = dir.replace('\\', "/");
            let trimmed = replaced
                .strip_prefix("./")
                .unwrap_or(&replaced)
                .trim_end_matches('/');
            if trimmed.is_empty() || trimmed == "." {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect();

    if normalized_dirs.is_empty() {
        return (cycles, Vec::new());
    }

    let mut kept = Vec::new();
    let mut ignored = Vec::new();

    for cycle in cycles {
        let all_under = cycle
            .iter()
            .all(|file| normalized_dirs.iter().any(|dir| path_under_dir(file, dir)));
        if all_under {
            ignored.push(cycle);
        } else {
            kept.push(cycle);
        }
    }

    (kept, ignored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_cycle(n: usize) -> FileCycle {
        (0..n)
            .map(|i| PathBuf::from(format!("file_{i}.py")))
            .collect()
    }

    #[test]
    fn filter_to_exact_size_2() {
        let cycles = vec![make_cycle(2), make_cycle(3), make_cycle(5)];
        let config = CyclesConfig {
            min_scc_size: 2,
            max_scc_size: Some(2),
            ignore: vec![],
            known_cyclic_files: vec![],
            ignore_derived_ancestor_init: false,
            ignore_dirs: vec![],
        };
        let result = filter_cycles_by_size(cycles, &config);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 2);
    }

    #[test]
    fn filter_range_2_to_5() {
        let cycles = vec![make_cycle(1), make_cycle(2), make_cycle(4), make_cycle(6)];
        let config = CyclesConfig {
            min_scc_size: 2,
            max_scc_size: Some(5),
            ignore: vec![],
            known_cyclic_files: vec![],
            ignore_derived_ancestor_init: false,
            ignore_dirs: vec![],
        };
        let result = filter_cycles_by_size(cycles, &config);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 2);
        assert_eq!(result[1].len(), 4);
    }

    #[test]
    fn filter_no_max() {
        let cycles = vec![make_cycle(1), make_cycle(2), make_cycle(10)];
        let config = CyclesConfig {
            min_scc_size: 2,
            max_scc_size: None,
            ignore: vec![],
            known_cyclic_files: vec![],
            ignore_derived_ancestor_init: false,
            ignore_dirs: vec![],
        };
        let result = filter_cycles_by_size(cycles, &config);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].len(), 2);
        assert_eq!(result[1].len(), 10);
    }

    #[test]
    fn filter_empty_input() {
        let cycles: Vec<FileCycle> = vec![];
        let config = CyclesConfig::default();
        let result = filter_cycles_by_size(cycles, &config);
        assert!(result.is_empty());
    }

    #[test]
    fn filter_all_removed() {
        let cycles = vec![make_cycle(1)];
        let config = CyclesConfig {
            min_scc_size: 2,
            max_scc_size: None,
            ignore: vec![],
            known_cyclic_files: vec![],
            ignore_derived_ancestor_init: false,
            ignore_dirs: vec![],
        };
        let result = filter_cycles_by_size(cycles, &config);
        assert!(result.is_empty());
    }

    fn make_ignore(files: &[&str]) -> IgnoredCycle {
        IgnoredCycle {
            files: files.iter().map(|s| s.to_string()).collect(),
            reason: None,
        }
    }

    #[test]
    fn ignore_exact_match() {
        let cycle = vec![PathBuf::from("a.py"), PathBuf::from("b.py")];
        let ignored = vec![make_ignore(&["a.py", "b.py"])];
        let result = filter_ignored_cycles(vec![cycle], &ignored);
        assert!(result.kept.is_empty());
        assert_eq!(result.suppressed.len(), 1);
    }

    #[test]
    fn ignore_no_match() {
        let cycle = vec![PathBuf::from("a.py"), PathBuf::from("b.py")];
        let ignored = vec![make_ignore(&["x.py", "y.py"])];
        let result = filter_ignored_cycles(vec![cycle], &ignored);
        assert_eq!(result.kept.len(), 1);
        assert!(result.suppressed.is_empty());
    }

    #[test]
    fn ignore_partial_overlap_not_removed() {
        let cycle = vec![
            PathBuf::from("a.py"),
            PathBuf::from("b.py"),
            PathBuf::from("c.py"),
        ];
        let ignored = vec![make_ignore(&["a.py", "b.py"])];
        let result = filter_ignored_cycles(vec![cycle], &ignored);
        assert_eq!(result.kept.len(), 1);
        assert!(result.suppressed.is_empty());
    }

    #[test]
    fn ignore_empty_list_keeps_all() {
        let cycles = vec![
            vec![PathBuf::from("a.py"), PathBuf::from("b.py")],
            vec![PathBuf::from("x.py"), PathBuf::from("y.py")],
        ];
        let result = filter_ignored_cycles(cycles, &[]);
        assert_eq!(result.kept.len(), 2);
        assert!(result.suppressed.is_empty());
    }

    #[test]
    fn ignore_multiple_entries() {
        let cycles = vec![
            vec![PathBuf::from("a.py"), PathBuf::from("b.py")],
            vec![PathBuf::from("x.py"), PathBuf::from("y.py")],
            vec![PathBuf::from("p.py"), PathBuf::from("q.py")],
        ];
        let ignored = vec![
            make_ignore(&["a.py", "b.py"]),
            make_ignore(&["x.py", "y.py"]),
        ];
        let result = filter_ignored_cycles(cycles, &ignored);
        assert_eq!(result.kept.len(), 1);
        assert_eq!(result.suppressed.len(), 2);
    }

    #[test]
    fn package_filter_single_package_kept() {
        let cycles = vec![
            vec![PathBuf::from("pkg/a.py"), PathBuf::from("pkg/b.py")],
            vec![PathBuf::from("other/x.py"), PathBuf::from("other/y.py")],
        ];
        let result = filter_cycles_by_package(cycles, &[]);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn package_filter_cross_package_excluded() {
        let cycles = vec![vec![PathBuf::from("pkg1/a.py"), PathBuf::from("pkg2/b.py")]];
        let result = filter_cycles_by_package(cycles, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn package_filter_mixed_cycles() {
        let cycles = vec![
            vec![PathBuf::from("pkg/a.py"), PathBuf::from("pkg/b.py")],
            vec![PathBuf::from("pkg1/a.py"), PathBuf::from("pkg2/b.py")],
            vec![PathBuf::from("other/x.py"), PathBuf::from("other/y.py")],
        ];
        let result = filter_cycles_by_package(cycles, &[]);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0][0], PathBuf::from("pkg/a.py"));
        assert_eq!(result[1][0], PathBuf::from("other/x.py"));
    }

    #[test]
    fn package_filter_nested_paths_same_package() {
        let cycles = vec![vec![
            PathBuf::from("pkg/sub/a.py"),
            PathBuf::from("pkg/other/b.py"),
        ]];
        let result = filter_cycles_by_package(cycles, &[]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn package_filter_root_level_files_excluded() {
        let cycles = vec![vec![PathBuf::from("root.py"), PathBuf::from("pkg/a.py")]];
        let result = filter_cycles_by_package(cycles, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn package_filter_all_root_level_excluded() {
        let cycles = vec![vec![PathBuf::from("a.py"), PathBuf::from("b.py")]];
        let result = filter_cycles_by_package(cycles, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn package_filter_empty_cycles() {
        let cycles: Vec<FileCycle> = vec![];
        let result = filter_cycles_by_package(cycles, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn package_filter_prefix_not_substring() {
        let cycles = vec![vec![
            PathBuf::from("pkg/a.py"),
            PathBuf::from("pkg_other/b.py"),
        ]];
        let result = filter_cycles_by_package(cycles, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn package_filter_three_files_same_package() {
        let cycles = vec![vec![
            PathBuf::from("pkg/a.py"),
            PathBuf::from("pkg/b.py"),
            PathBuf::from("pkg/c.py"),
        ]];
        let result = filter_cycles_by_package(cycles, &[]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn package_filter_three_files_one_different() {
        let cycles = vec![vec![
            PathBuf::from("pkg/a.py"),
            PathBuf::from("pkg/b.py"),
            PathBuf::from("other/c.py"),
        ]];
        let result = filter_cycles_by_package(cycles, &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn package_filter_strips_source_root_before_grouping() {
        let cycles = vec![vec![
            PathBuf::from("src/pkg/a.py"),
            PathBuf::from("src/pkg/b.py"),
        ]];
        let result = filter_cycles_by_package(cycles, &["src".to_string()]);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn package_filter_no_strip_without_source_roots() {
        let cycles = vec![vec![
            PathBuf::from("src/pkg/a.py"),
            PathBuf::from("src/pkg/b.py"),
        ]];
        let result = filter_cycles_by_package(cycles, &[]);
        assert_eq!(
            result.len(),
            1,
            "src is the grouping component without stripping"
        );
    }

    fn cyc(files: &[&str]) -> FileCycle {
        files.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn partition_dir_ignored_fully_inside_is_ignored() {
        let cycles = vec![cyc(&["app/protos/a.py", "app/protos/b.py"])];
        let (kept, ignored) = partition_dir_ignored(cycles, &["app/protos/".to_string()]);
        assert!(kept.is_empty());
        assert_eq!(ignored.len(), 1);
    }

    #[test]
    fn partition_dir_ignored_cross_boundary_is_kept() {
        let cycles = vec![cyc(&["app/protos/a.py", "app/foo.py"])];
        let (kept, ignored) = partition_dir_ignored(cycles, &["app/protos/".to_string()]);
        assert_eq!(kept.len(), 1);
        assert!(ignored.is_empty());
    }

    #[test]
    fn partition_dir_ignored_union_across_two_dirs() {
        let cycles = vec![cyc(&["app/protos/a.py", "app/migrations/m.py"])];
        let dirs = vec!["app/protos/".to_string(), "app/migrations/".to_string()];
        let (kept, ignored) = partition_dir_ignored(cycles, &dirs);
        assert!(kept.is_empty());
        assert_eq!(ignored.len(), 1);
    }

    #[test]
    fn partition_dir_ignored_exact_file_entry() {
        let cycles = vec![cyc(&["app/protos/a.py", "app/protos/b.py"])];
        let dirs = vec!["app/protos/a.py".to_string(), "app/protos/b.py".to_string()];
        let (kept, ignored) = partition_dir_ignored(cycles, &dirs);
        assert!(kept.is_empty());
        assert_eq!(ignored.len(), 1);
    }

    #[test]
    fn partition_dir_ignored_empty_dirs_is_noop() {
        let cycles = vec![
            cyc(&["app/protos/a.py", "app/protos/b.py"]),
            cyc(&["app/foo.py", "app/bar.py"]),
        ];
        let (kept, ignored) = partition_dir_ignored(cycles, &[]);
        assert_eq!(kept.len(), 2);
        assert!(ignored.is_empty());
    }

    #[test]
    fn partition_dir_ignored_preserves_kept_order() {
        let cycles = vec![
            cyc(&["app/protos/a.py", "app/protos/b.py"]),
            cyc(&["app/foo.py", "app/bar.py"]),
            cyc(&["app/baz.py", "app/qux.py"]),
        ];
        let (kept, ignored) = partition_dir_ignored(cycles, &["app/protos/".to_string()]);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0][0], PathBuf::from("app/foo.py"));
        assert_eq!(kept[1][0], PathBuf::from("app/baz.py"));
        assert_eq!(ignored.len(), 1);
    }

    #[test]
    fn path_under_dir_prefix_boundary() {
        assert!(!path_under_dir(
            &PathBuf::from("app/protosX/a.py"),
            "app/protos"
        ));
        assert!(path_under_dir(&PathBuf::from("app/protos"), "app/protos"));
        assert!(path_under_dir(
            &PathBuf::from("app/protos/a.py"),
            "app/protos"
        ));
    }
}
