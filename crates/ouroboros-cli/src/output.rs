use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use ouroboros_core::graph::{
    EdgeMetadata, FileDependencyGraph, PathKind, PathMatch, condensation, match_path,
    reachable_cycles_from, strongly_connected_components,
};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JsonReport {
    pub version: u32,
    pub summary: JsonSummary,
    pub cycles: Vec<JsonCycle>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub traced: Vec<JsonTrace>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_paths: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JsonSummary {
    pub cycles_reported: usize,
    pub cycles_suppressed: usize,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JsonCycle {
    pub index: usize,
    pub packages: Vec<String>,
    pub size: usize,
    pub files: Vec<JsonCycleFile>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JsonCycleFile {
    pub path: String,
    pub import_lines: Vec<u32>,
    pub edges: Vec<JsonEdge>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JsonEdge {
    pub to: String,
    pub lines: Vec<u32>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JsonTrace {
    pub path: String,
    pub kind: String,
    pub files: Vec<JsonTraceFile>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JsonTraceFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub impacts: Vec<JsonImpactEntry>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JsonImpactEntry {
    pub cycle_index: usize,
    pub relationship: String,
    pub entry: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from_lines: Vec<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<JsonBranchHop>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct JsonBranchHop {
    pub from: String,
    pub to: String,
    pub lines: Vec<u32>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct JsonDumpIgnoresReport {
    pub version: u32,
    pub ignore_entries: Vec<JsonIgnoreEntry>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct JsonIgnoreEntry {
    pub files: Vec<String>,
}

/// Collect sorted, deduplicated import lines for a file within a cycle.
///
/// This is the shared logic used by both human and JSON reporters.
pub fn collect_import_lines(
    path: &Path,
    cycle: &[PathBuf],
    edge_metadata: &EdgeMetadata,
) -> Vec<u32> {
    let mut import_lines: Vec<u32> = cycle
        .iter()
        .filter(|other| *other != path)
        .flat_map(|other| {
            edge_metadata
                .lines
                .get(&(path.to_path_buf(), other.clone()))
                .map(|v| v.as_slice())
                .unwrap_or(&[])
                .iter()
                .copied()
        })
        .collect();
    import_lines.sort();
    import_lines.dedup();
    import_lines
}

fn package_of(path: &Path) -> Option<&str> {
    let mut components = path.components();
    let first = components.next()?;
    if components.next().is_some() {
        first.as_os_str().to_str()
    } else {
        None
    }
}

pub(crate) fn packages_for_cycle(cycle: &[PathBuf]) -> Vec<String> {
    let mut pkgs: Vec<String> = cycle
        .iter()
        .filter_map(|p| package_of(p).map(|s| s.to_string()))
        .collect();
    pkgs.sort();
    pkgs.dedup();
    pkgs
}

/// Returns kept cycles in canonical display order: sorted by (packages[0], size).
/// 1-based index = position + 1.
pub fn order_cycles(cycles: &[Vec<PathBuf>]) -> Vec<(Vec<String>, &Vec<PathBuf>)> {
    let mut cycle_data: Vec<(Vec<String>, &Vec<PathBuf>)> = cycles
        .iter()
        .map(|cycle| (packages_for_cycle(cycle), cycle))
        .collect();

    cycle_data.sort_by(|a, b| {
        let pkg_ord = match (a.0.first(), b.0.first()) {
            (Some(pa), Some(pb)) => pa.cmp(pb),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        pkg_ord.then_with(|| a.1.len().cmp(&b.1.len()))
    });

    cycle_data
}

pub fn build_json_report(
    kept_cycles: &[Vec<PathBuf>],
    suppressed_count: usize,
    edge_metadata: &EdgeMetadata,
    traced: Vec<JsonTrace>,
    unknown_paths: Vec<String>,
) -> JsonReport {
    let cycle_data = order_cycles(kept_cycles);

    let cycles = cycle_data
        .iter()
        .enumerate()
        .map(|(i, (packages, cycle))| {
            let files = cycle
                .iter()
                .map(|path| {
                    let edges: Vec<JsonEdge> = cycle
                        .iter()
                        .filter(|other| *other != path)
                        .filter_map(|other| {
                            edge_metadata
                                .lines
                                .get(&(path.to_path_buf(), other.clone()))
                                .map(|lines| {
                                    let mut sorted = lines.clone();
                                    sorted.sort();
                                    sorted.dedup();
                                    JsonEdge {
                                        to: other.display().to_string(),
                                        lines: sorted,
                                    }
                                })
                        })
                        .collect();
                    JsonCycleFile {
                        path: path.display().to_string(),
                        import_lines: collect_import_lines(path, cycle, edge_metadata),
                        edges,
                    }
                })
                .collect();
            JsonCycle {
                index: i + 1,
                packages: packages.clone(),
                size: cycle.len(),
                files,
            }
        })
        .collect();

    JsonReport {
        version: 1,
        summary: JsonSummary {
            cycles_reported: kept_cycles.len(),
            cycles_suppressed: suppressed_count,
        },
        cycles,
        traced,
        unknown_paths,
    }
}

pub fn build_traces(
    raw_traces: &[String],
    kept_cycles: &[Vec<PathBuf>],
    graph: &FileDependencyGraph,
    edge_metadata: &EdgeMetadata,
    source_roots: &[String],
) -> (Vec<JsonTrace>, Vec<String>) {
    let node_paths: BTreeSet<PathBuf> = graph.keys().cloned().collect();
    let sccs = strongly_connected_components(graph);
    let cond = condensation(graph, &sccs);

    let ordered = order_cycles(kept_cycles);
    let cycle_sccs: HashSet<usize> = kept_cycles
        .iter()
        .filter_map(|cycle| cycle.first())
        .filter_map(|first| cond.node_to_scc.get(first))
        .copied()
        .collect();

    let cycle_index_map: HashMap<usize, usize> = ordered
        .iter()
        .enumerate()
        .filter_map(|(i, (_, cycle))| {
            cycle
                .first()
                .and_then(|first| cond.node_to_scc.get(first))
                .map(|&scc_id| (scc_id, i + 1))
        })
        .collect();

    let mut traced_results = Vec::new();
    let mut unknown_paths = Vec::new();
    let mut seen_raw = Vec::new();

    for raw in raw_traces {
        if seen_raw.contains(raw) {
            continue;
        }
        seen_raw.push(raw.clone());

        let normalized = normalize_trace_path(raw);
        let had_trailing_slash = raw.trim_end().ends_with('/');
        let path_to_match = PathBuf::from(&normalized);
        let matched = match_trace_candidate(&node_paths, &path_to_match, had_trailing_slash)
            .map(|matched| (matched, normalized.clone()))
            .or_else(|| {
                for root in source_roots {
                    let root_prefix = root.trim_end_matches('/').to_string() + "/";
                    if let Some(stripped) = normalized.strip_prefix(&root_prefix) {
                        let stripped_path = PathBuf::from(stripped);
                        if let Some(matched) =
                            match_trace_candidate(&node_paths, &stripped_path, had_trailing_slash)
                        {
                            return Some((matched, stripped.to_string()));
                        }
                    }
                }
                None
            });

        let Some((matched, resolved)) = matched else {
            eprintln!("warning: trace path '{raw}' matched no first-party files");
            unknown_paths.push(raw.clone());
            continue;
        };

        let (kind, display_path) = match matched.kind {
            PathKind::File => ("file".to_string(), resolved.clone()),
            PathKind::Directory => ("directory".to_string(), format!("{resolved}/")),
        };

        let files = matched
            .nodes
            .iter()
            .map(|node| {
                let reachable = reachable_cycles_from(graph, node, &cond.node_to_scc, &cycle_sccs);
                let impacts = reachable
                    .iter()
                    .filter_map(|rc| {
                        let cycle_index = *cycle_index_map.get(&rc.scc_id)?;

                        if rc.is_direct {
                            return Some(JsonImpactEntry {
                                cycle_index,
                                relationship: "member".to_string(),
                                entry: rc.entry.display().to_string(),
                                from_lines: vec![],
                                path: vec![],
                            });
                        }

                        let hops: Vec<JsonBranchHop> = rc
                            .path
                            .windows(2)
                            .map(|window| {
                                let from = &window[0];
                                let to = &window[1];
                                let mut lines = edge_metadata
                                    .lines
                                    .get(&(from.clone(), to.clone()))
                                    .cloned()
                                    .unwrap_or_default();
                                lines.sort();
                                lines.dedup();
                                JsonBranchHop {
                                    from: from.display().to_string(),
                                    to: to.display().to_string(),
                                    lines,
                                }
                            })
                            .collect();
                        let from_lines = hops
                            .first()
                            .map(|hop| hop.lines.clone())
                            .unwrap_or_default();

                        Some(JsonImpactEntry {
                            cycle_index,
                            relationship: "reachable".to_string(),
                            entry: rc.entry.display().to_string(),
                            from_lines,
                            path: hops,
                        })
                    })
                    .collect();

                JsonTraceFile {
                    path: node.display().to_string(),
                    impacts,
                }
            })
            .collect();

        traced_results.push(JsonTrace {
            path: display_path,
            kind,
            files,
        });
    }

    (traced_results, unknown_paths)
}

fn normalize_trace_path(raw: &str) -> String {
    let trimmed = raw.trim();
    let stripped = trimmed.strip_prefix("./").unwrap_or(trimmed);
    stripped.trim_end_matches('/').replace('\\', "/")
}

fn match_trace_candidate(
    node_paths: &BTreeSet<PathBuf>,
    path: &Path,
    force_directory: bool,
) -> Option<PathMatch> {
    if !force_directory {
        return match_path(node_paths, path);
    }

    let dir_prefix = format!("{}/", path.to_string_lossy());
    let nodes: Vec<PathBuf> = node_paths
        .iter()
        .filter(|node| node.to_string_lossy().starts_with(&dir_prefix))
        .cloned()
        .collect();

    if nodes.is_empty() {
        None
    } else {
        Some(PathMatch {
            kind: PathKind::Directory,
            nodes,
        })
    }
}

pub fn build_dump_ignores_report(cycles: &[Vec<PathBuf>]) -> JsonDumpIgnoresReport {
    let ignore_entries = cycles
        .iter()
        .map(|cycle| {
            let mut files: Vec<String> = cycle.iter().map(|p| p.display().to_string()).collect();
            files.sort();
            JsonIgnoreEntry { files }
        })
        .collect();

    JsonDumpIgnoresReport {
        version: 1,
        ignore_entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ouroboros_core::graph::EdgeMetadata;
    use std::collections::{BTreeSet, HashMap};
    use std::path::PathBuf;

    fn make_edge_metadata(edges: &[(&str, &str, Vec<u32>)]) -> EdgeMetadata {
        let mut lines = HashMap::new();
        for (from, to, line_nums) in edges {
            lines.insert((PathBuf::from(from), PathBuf::from(to)), line_nums.clone());
        }
        EdgeMetadata { lines }
    }

    #[test]
    fn empty_report_serializes_correctly() {
        let edge_metadata = make_edge_metadata(&[]);
        let report = build_json_report(&[], 0, &edge_metadata, vec![], vec![]);

        assert_eq!(report.version, 1);
        assert_eq!(report.summary.cycles_reported, 0);
        assert_eq!(report.summary.cycles_suppressed, 0);
        assert!(report.cycles.is_empty());

        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["version"], 1);
    }

    #[test]
    fn single_cycle_with_import_lines() {
        let kept = vec![vec![PathBuf::from("a.py"), PathBuf::from("b.py")]];
        let edge_metadata =
            make_edge_metadata(&[("a.py", "b.py", vec![10]), ("b.py", "a.py", vec![5])]);
        let report = build_json_report(&kept, 0, &edge_metadata, vec![], vec![]);

        assert_eq!(report.cycles.len(), 1);
        assert_eq!(report.cycles[0].index, 1);
        assert_eq!(report.cycles[0].size, 2);
        assert_eq!(report.cycles[0].packages, Vec::<String>::new());
        assert_eq!(report.cycles[0].files[0].path, "a.py");
        assert_eq!(report.cycles[0].files[0].import_lines, vec![10]);
        assert_eq!(report.cycles[0].files[1].path, "b.py");
        assert_eq!(report.cycles[0].files[1].import_lines, vec![5]);
    }

    #[test]
    fn multiple_cycles_with_suppressed() {
        let kept = vec![
            vec![PathBuf::from("a.py"), PathBuf::from("b.py")],
            vec![PathBuf::from("x.py"), PathBuf::from("y.py")],
        ];
        let edge_metadata = make_edge_metadata(&[]);
        let report = build_json_report(&kept, 1, &edge_metadata, vec![], vec![]);

        assert_eq!(report.summary.cycles_reported, 2);
        assert_eq!(report.summary.cycles_suppressed, 1);
        assert_eq!(report.cycles.len(), 2);
        assert_eq!(report.cycles[0].index, 1);
        assert_eq!(report.cycles[1].index, 2);
    }

    #[test]
    fn import_lines_sorted_and_deduped() {
        let kept = vec![vec![PathBuf::from("a.py"), PathBuf::from("b.py")]];
        let edge_metadata = make_edge_metadata(&[("a.py", "b.py", vec![30, 10, 10, 20])]);
        let report = build_json_report(&kept, 0, &edge_metadata, vec![], vec![]);

        assert_eq!(report.cycles[0].files[0].import_lines, vec![10, 20, 30]);
    }

    #[test]
    fn order_cycles_identical_to_current_sort() {
        let kept = vec![
            vec![PathBuf::from("a.py"), PathBuf::from("b.py")],
            vec![
                PathBuf::from("beta/a.py"),
                PathBuf::from("beta/b.py"),
                PathBuf::from("beta/c.py"),
            ],
            vec![PathBuf::from("alpha/a.py"), PathBuf::from("alpha/b.py")],
            vec![PathBuf::from("beta/x.py"), PathBuf::from("beta/y.py")],
        ];

        let ordered = order_cycles(&kept);

        assert_eq!(ordered[0].0, vec!["alpha".to_string()]);
        assert_eq!(ordered[0].1.len(), 2);
        assert_eq!(ordered[1].0, vec!["beta".to_string()]);
        assert_eq!(ordered[1].1.len(), 2);
        assert_eq!(ordered[2].0, vec!["beta".to_string()]);
        assert_eq!(ordered[2].1.len(), 3);
        assert_eq!(ordered[3].0, Vec::<String>::new());
    }

    #[test]
    fn file_with_no_import_lines() {
        let kept = vec![vec![
            PathBuf::from("a.py"),
            PathBuf::from("b.py"),
            PathBuf::from("c.py"),
        ]];
        let edge_metadata =
            make_edge_metadata(&[("a.py", "b.py", vec![1]), ("b.py", "c.py", vec![2])]);
        let report = build_json_report(&kept, 0, &edge_metadata, vec![], vec![]);

        assert_eq!(report.cycles[0].files[2].path, "c.py");
        assert!(report.cycles[0].files[2].import_lines.is_empty());
    }

    #[test]
    fn json_round_trip_is_valid() {
        let kept = vec![vec![PathBuf::from("a.py"), PathBuf::from("b.py")]];
        let edge_metadata = make_edge_metadata(&[("a.py", "b.py", vec![7])]);
        let report = build_json_report(&kept, 0, &edge_metadata, vec![], vec![]);

        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["version"], 1);
        assert_eq!(parsed["cycles"][0]["files"][0]["path"], "a.py");
        assert_eq!(parsed["cycles"][0]["files"][0]["import_lines"][0], 7);
    }

    #[test]
    fn dump_ignores_report_structure() {
        let cycles = vec![
            vec![PathBuf::from("b.py"), PathBuf::from("a.py")],
            vec![PathBuf::from("x.py"), PathBuf::from("y.py")],
        ];
        let report = build_dump_ignores_report(&cycles);

        assert_eq!(report.version, 1);
        assert_eq!(report.ignore_entries.len(), 2);
        assert_eq!(report.ignore_entries[0].files, vec!["a.py", "b.py"]);
        assert_eq!(report.ignore_entries[1].files, vec!["x.py", "y.py"]);
    }

    #[test]
    fn collect_import_lines_aggregates_across_cycle_members() {
        let cycle = vec![
            PathBuf::from("a.py"),
            PathBuf::from("b.py"),
            PathBuf::from("c.py"),
        ];
        let edge_metadata =
            make_edge_metadata(&[("a.py", "b.py", vec![5]), ("a.py", "c.py", vec![10])]);
        let lines = collect_import_lines(Path::new("a.py"), &cycle, &edge_metadata);
        assert_eq!(lines, vec![5, 10]);
    }

    #[test]
    fn packages_for_cycle_all_same() {
        let cycle = vec![PathBuf::from("pkg/a.py"), PathBuf::from("pkg/b.py")];
        assert_eq!(packages_for_cycle(&cycle), vec!["pkg".to_string()]);
    }

    #[test]
    fn packages_for_cycle_cross_package() {
        let cycle = vec![PathBuf::from("pkg1/a.py"), PathBuf::from("pkg2/b.py")];
        assert_eq!(
            packages_for_cycle(&cycle),
            vec!["pkg1".to_string(), "pkg2".to_string()]
        );
    }

    #[test]
    fn packages_for_cycle_root_level() {
        let cycle = vec![PathBuf::from("a.py"), PathBuf::from("b.py")];
        assert_eq!(packages_for_cycle(&cycle), Vec::<String>::new());
    }

    #[test]
    fn packages_for_cycle_mixed_root_and_pkg() {
        let cycle = vec![PathBuf::from("root.py"), PathBuf::from("pkg/a.py")];
        assert_eq!(packages_for_cycle(&cycle), vec!["pkg".to_string()]);
    }

    #[test]
    fn packages_for_cycle_sorted_and_deduped() {
        let cycle = vec![
            PathBuf::from("zebra/a.py"),
            PathBuf::from("alpha/b.py"),
            PathBuf::from("zebra/c.py"),
        ];
        assert_eq!(
            packages_for_cycle(&cycle),
            vec!["alpha".to_string(), "zebra".to_string()]
        );
    }

    #[test]
    fn json_report_packages_field_present() {
        let kept = vec![
            vec![PathBuf::from("pkg/a.py"), PathBuf::from("pkg/b.py")],
            vec![PathBuf::from("pkg1/a.py"), PathBuf::from("pkg2/b.py")],
            vec![PathBuf::from("a.py"), PathBuf::from("b.py")],
        ];
        let edge_metadata = make_edge_metadata(&[]);
        let report = build_json_report(&kept, 0, &edge_metadata, vec![], vec![]);

        assert_eq!(report.cycles[0].packages, vec!["pkg".to_string()]);
        assert_eq!(
            report.cycles[1].packages,
            vec!["pkg1".to_string(), "pkg2".to_string()]
        );
        assert_eq!(report.cycles[2].packages, Vec::<String>::new());
    }

    #[test]
    fn json_report_cycles_sorted_by_packages_then_size() {
        let kept = vec![
            vec![PathBuf::from("a.py"), PathBuf::from("b.py")],
            vec![
                PathBuf::from("beta/a.py"),
                PathBuf::from("beta/b.py"),
                PathBuf::from("beta/c.py"),
            ],
            vec![PathBuf::from("alpha/a.py"), PathBuf::from("alpha/b.py")],
            vec![PathBuf::from("beta/x.py"), PathBuf::from("beta/y.py")],
        ];
        let edge_metadata = make_edge_metadata(&[]);
        let report = build_json_report(&kept, 0, &edge_metadata, vec![], vec![]);

        assert_eq!(report.cycles[0].packages, vec!["alpha".to_string()]);
        assert_eq!(report.cycles[0].size, 2);
        assert_eq!(report.cycles[1].packages, vec!["beta".to_string()]);
        assert_eq!(report.cycles[1].size, 2);
        assert_eq!(report.cycles[2].packages, vec!["beta".to_string()]);
        assert_eq!(report.cycles[2].size, 3);
        assert_eq!(report.cycles[3].packages, Vec::<String>::new());
        assert_eq!(report.cycles[3].size, 2);

        for (i, cycle) in report.cycles.iter().enumerate() {
            assert_eq!(cycle.index, i + 1);
        }
    }

    #[test]
    fn json_report_empty_packages_sorts_last() {
        let kept = vec![
            vec![PathBuf::from("root.py"), PathBuf::from("other.py")],
            vec![PathBuf::from("pkg/a.py"), PathBuf::from("pkg/b.py")],
        ];
        let edge_metadata = make_edge_metadata(&[]);
        let report = build_json_report(&kept, 0, &edge_metadata, vec![], vec![]);

        assert_eq!(report.cycles[0].packages, vec!["pkg".to_string()]);
        assert_eq!(report.cycles[1].packages, Vec::<String>::new());
    }

    #[test]
    fn json_report_no_package_scoped_field() {
        let kept = vec![vec![PathBuf::from("a.py"), PathBuf::from("b.py")]];
        let edge_metadata = make_edge_metadata(&[]);
        let report = build_json_report(&kept, 0, &edge_metadata, vec![], vec![]);

        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed["summary"].get("package_scoped").is_none());
    }

    #[test]
    fn build_traces_member_has_no_path_or_from_lines() {
        let kept = vec![vec![PathBuf::from("a.py"), PathBuf::from("b.py")]];
        let mut graph = FileDependencyGraph::new();
        graph.insert(
            PathBuf::from("a.py"),
            BTreeSet::from([PathBuf::from("b.py")]),
        );
        graph.insert(
            PathBuf::from("b.py"),
            BTreeSet::from([PathBuf::from("a.py")]),
        );
        let edge_metadata =
            make_edge_metadata(&[("a.py", "b.py", vec![1]), ("b.py", "a.py", vec![2])]);

        let (traced, unknown) =
            build_traces(&["a.py".to_string()], &kept, &graph, &edge_metadata, &[]);

        assert!(unknown.is_empty());
        assert_eq!(traced.len(), 1);
        let impacts = &traced[0].files[0].impacts;
        assert_eq!(impacts.len(), 1);
        assert_eq!(impacts[0].relationship, "member");
        assert!(impacts[0].path.is_empty());
        assert!(impacts[0].from_lines.is_empty());
    }

    #[test]
    fn build_traces_clean_file_has_no_impacts() {
        let kept = vec![vec![PathBuf::from("a.py"), PathBuf::from("b.py")]];
        let mut graph = FileDependencyGraph::new();
        graph.insert(
            PathBuf::from("a.py"),
            BTreeSet::from([PathBuf::from("b.py")]),
        );
        graph.insert(
            PathBuf::from("b.py"),
            BTreeSet::from([PathBuf::from("a.py")]),
        );
        graph.insert(PathBuf::from("clean.py"), BTreeSet::new());
        let edge_metadata = make_edge_metadata(&[]);

        let (traced, _) = build_traces(
            &["clean.py".to_string()],
            &kept,
            &graph,
            &edge_metadata,
            &[],
        );

        assert_eq!(traced[0].files[0].impacts.len(), 0);
    }

    #[test]
    fn build_traces_unknown_path_recorded() {
        let kept: Vec<Vec<PathBuf>> = vec![];
        let graph = FileDependencyGraph::new();
        let edge_metadata = make_edge_metadata(&[]);

        let (traced, unknown) =
            build_traces(&["nope.py".to_string()], &kept, &graph, &edge_metadata, &[]);

        assert!(traced.is_empty());
        assert_eq!(unknown, vec!["nope.py".to_string()]);
    }

    #[test]
    fn build_traces_traced_and_unknown_paths_omitted_when_empty() {
        let kept: Vec<Vec<PathBuf>> = vec![];
        let edge_metadata = make_edge_metadata(&[]);
        let mut graph = FileDependencyGraph::new();
        graph.insert(PathBuf::from("a.py"), BTreeSet::new());

        let (traced, unknown) = build_traces(&[], &kept, &graph, &edge_metadata, &[]);

        assert!(traced.is_empty());
        assert!(unknown.is_empty());

        let report = build_json_report(&kept, 0, &edge_metadata, traced, unknown);
        let json = serde_json::to_string_pretty(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("traced").is_none());
        assert!(parsed.get("unknown_paths").is_none());
    }
}
