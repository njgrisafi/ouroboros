use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::config::{CyclesConfig, IgnoredCycle};
use crate::graph::FileCycle;

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
            if let Some(max) = config.max_scc_size {
                if size > max {
                    return false;
                }
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

fn package_of(path: &Path) -> Option<&OsStr> {
    let mut components = path.components();
    let first = components.next()?;
    if components.next().is_some() {
        Some(first.as_os_str())
    } else {
        None
    }
}

pub fn filter_cycles_by_package(cycles: Vec<FileCycle>) -> Vec<FileCycle> {
    cycles
        .into_iter()
        .filter(|cycle| {
            let mut iter = cycle.iter();
            let first_pkg = match iter.next().and_then(|p| package_of(p)) {
                Some(pkg) => pkg,
                None => return false,
            };
            iter.all(|p| package_of(p) == Some(first_pkg))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Helper: create a cycle of `n` dummy files.
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
    fn package_of_nested_file() {
        assert_eq!(package_of(Path::new("pkg/a.py")), Some(OsStr::new("pkg")));
    }

    #[test]
    fn package_of_deeply_nested() {
        assert_eq!(
            package_of(Path::new("pkg/sub/deep/a.py")),
            Some(OsStr::new("pkg"))
        );
    }

    #[test]
    fn package_of_root_level() {
        assert_eq!(package_of(Path::new("root.py")), None);
    }

    #[test]
    fn package_filter_single_package_kept() {
        let cycles = vec![
            vec![PathBuf::from("pkg/a.py"), PathBuf::from("pkg/b.py")],
            vec![PathBuf::from("other/x.py"), PathBuf::from("other/y.py")],
        ];
        let result = filter_cycles_by_package(cycles);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn package_filter_cross_package_excluded() {
        let cycles = vec![vec![PathBuf::from("pkg1/a.py"), PathBuf::from("pkg2/b.py")]];
        let result = filter_cycles_by_package(cycles);
        assert!(result.is_empty());
    }

    #[test]
    fn package_filter_mixed_cycles() {
        let cycles = vec![
            vec![PathBuf::from("pkg/a.py"), PathBuf::from("pkg/b.py")],
            vec![PathBuf::from("pkg1/a.py"), PathBuf::from("pkg2/b.py")],
            vec![PathBuf::from("other/x.py"), PathBuf::from("other/y.py")],
        ];
        let result = filter_cycles_by_package(cycles);
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
        let result = filter_cycles_by_package(cycles);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn package_filter_root_level_files_excluded() {
        let cycles = vec![vec![PathBuf::from("root.py"), PathBuf::from("pkg/a.py")]];
        let result = filter_cycles_by_package(cycles);
        assert!(result.is_empty());
    }

    #[test]
    fn package_filter_all_root_level_excluded() {
        let cycles = vec![vec![PathBuf::from("a.py"), PathBuf::from("b.py")]];
        let result = filter_cycles_by_package(cycles);
        assert!(result.is_empty());
    }

    #[test]
    fn package_filter_empty_cycles() {
        let cycles: Vec<FileCycle> = vec![];
        let result = filter_cycles_by_package(cycles);
        assert!(result.is_empty());
    }

    #[test]
    fn package_filter_prefix_not_substring() {
        let cycles = vec![vec![
            PathBuf::from("pkg/a.py"),
            PathBuf::from("pkg_other/b.py"),
        ]];
        let result = filter_cycles_by_package(cycles);
        assert!(result.is_empty());
    }

    #[test]
    fn package_filter_three_files_same_package() {
        let cycles = vec![vec![
            PathBuf::from("pkg/a.py"),
            PathBuf::from("pkg/b.py"),
            PathBuf::from("pkg/c.py"),
        ]];
        let result = filter_cycles_by_package(cycles);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn package_filter_three_files_one_different() {
        let cycles = vec![vec![
            PathBuf::from("pkg/a.py"),
            PathBuf::from("pkg/b.py"),
            PathBuf::from("other/c.py"),
        ]];
        let result = filter_cycles_by_package(cycles);
        assert!(result.is_empty());
    }
}
