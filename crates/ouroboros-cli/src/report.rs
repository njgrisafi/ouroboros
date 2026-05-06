use crate::output::{JsonCycle, JsonReport};
use chrono::Local;
use ouroboros_core::config::Config;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};

pub struct ReportStats {
    pub total_cycles: usize,
    pub total_suppressed: usize,
    pub total_files: usize,
    pub package_frequency: Vec<(String, usize)>,
    pub size_distribution: Vec<(usize, usize)>,
}

impl ReportStats {
    pub fn from_report(report: &JsonReport) -> Self {
        let mut unique_files = HashSet::new();
        for cycle in &report.cycles {
            for file in &cycle.files {
                unique_files.insert(file.path.as_str());
            }
        }

        let mut pkg_counts: HashMap<String, usize> = HashMap::new();
        for cycle in &report.cycles {
            if cycle.packages.is_empty() {
                *pkg_counts.entry("(root-level)".to_string()).or_default() += 1;
            } else {
                for pkg in &cycle.packages {
                    *pkg_counts.entry(pkg.clone()).or_default() += 1;
                }
            }
        }
        let mut package_frequency: Vec<(String, usize)> = pkg_counts.into_iter().collect();
        package_frequency.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        let mut size_counts: HashMap<usize, usize> = HashMap::new();
        for cycle in &report.cycles {
            *size_counts.entry(cycle.size).or_default() += 1;
        }
        let mut size_distribution: Vec<(usize, usize)> = size_counts.into_iter().collect();
        size_distribution.sort_by_key(|(size, _)| *size);

        ReportStats {
            total_cycles: report.summary.cycles_reported,
            total_suppressed: report.summary.cycles_suppressed,
            total_files: unique_files.len(),
            package_frequency,
            size_distribution,
        }
    }
}

pub fn load_json_report(path: &Path) -> Result<JsonReport, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let report: JsonReport =
        serde_json::from_str(&contents).map_err(|e| format!("failed to parse JSON report: {e}"))?;
    if report.version != 1 {
        return Err(format!(
            "unsupported report version: {} (expected 1)",
            report.version
        ));
    }
    if report.summary.cycles_reported != report.cycles.len() {
        return Err(format!(
            "invalid report summary: cycles_reported={} but cycles array has {} entries",
            report.summary.cycles_reported,
            report.cycles.len()
        ));
    }
    for (i, cycle) in report.cycles.iter().enumerate() {
        let expected_index = i + 1;
        if cycle.index != expected_index {
            return Err(format!(
                "invalid cycle index: expected {} but found {}",
                expected_index, cycle.index
            ));
        }
        if cycle.size != cycle.files.len() {
            return Err(format!(
                "invalid cycle size for cycle {}: size={} but files array has {} entries",
                cycle.index,
                cycle.size,
                cycle.files.len()
            ));
        }
    }
    Ok(report)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn resolve_source_roots(explicit: Option<&Path>) -> Vec<PathBuf> {
    if let Some(root) = explicit {
        return vec![root.to_path_buf()];
    }

    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(_) => return vec![],
    };

    let config_path = match crate::find_config(&cwd) {
        Some(p) => p,
        None => return vec![],
    };

    let project_root = match config_path.parent() {
        Some(p) => p.to_path_buf(),
        None => return vec![],
    };

    let contents = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let config = match Config::from_toml(&contents) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    config
        .source_roots
        .iter()
        .map(|sr| project_root.join(sr))
        .collect()
}

fn read_source_line(source_roots: &[PathBuf], file_path: &str, line_number: u32) -> Option<String> {
    let line_idx = (line_number as usize).checked_sub(1)?;
    for root in source_roots {
        let full_path = root.join(file_path);
        if let Ok(contents) = std::fs::read_to_string(&full_path) {
            let all_lines: Vec<&str> = contents.lines().collect();
            if let Some(&first_line) = all_lines.get(line_idx) {
                let trimmed = first_line.trim();
                // If the line has an opening paren but no closing paren,
                // it's a multi-line import — collect continuation lines.
                if trimmed.contains('(') && !trimmed.contains(')') {
                    let mut parts = vec![trimmed.to_string()];
                    for &next_line in &all_lines[line_idx + 1..] {
                        let next_trimmed = next_line.trim();
                        parts.push(next_trimmed.to_string());
                        if next_trimmed.contains(')') {
                            break;
                        }
                    }
                    return Some(parts.join(" "));
                }
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

pub fn generate_html(report: &JsonReport, stats: &ReportStats, source_roots: &[PathBuf]) -> String {
    let mut html = String::with_capacity(16384);
    let date = Local::now().format("%Y-%m-%d").to_string();

    write_head(&mut html, &date);
    write_summary(&mut html, stats);
    write_package_table(&mut html, &stats.package_frequency);
    write_size_table(&mut html, &stats.size_distribution);
    write_cycle_table(&mut html, &report.cycles, source_roots);
    write_scripts(&mut html);
    html.push_str("</body>\n</html>\n");
    html
}

fn write_head(html: &mut String, date: &str) {
    html.push_str(r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Ouroboros - Circular Import Report</title>
    <style>
        *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            color: #1a1a2e; background: #f5f5fa; line-height: 1.6;
            max-width: 960px; margin: 0 auto; padding: 2rem 1rem;
        }
        h1 { font-size: 1.75rem; margin-bottom: 0.25rem; }
        h2 { font-size: 1.25rem; margin: 2rem 0 1rem; border-bottom: 2px solid #e0e0e8; padding-bottom: 0.5rem; }
        .date { color: #666; font-size: 0.9rem; margin-bottom: 1.5rem; }
        .cards { display: flex; gap: 1rem; margin-bottom: 2rem; flex-wrap: wrap; }
        .card {
            flex: 1; min-width: 140px; background: #fff; border-radius: 8px;
            padding: 1.25rem; text-align: center; box-shadow: 0 1px 3px rgba(0,0,0,0.1);
        }
        .card-value { font-size: 2rem; font-weight: 700; color: #4a4ae0; }
        .card-label { font-size: 0.85rem; color: #666; margin-top: 0.25rem; }
        table { width: 100%; border-collapse: collapse; background: #fff; border-radius: 8px; overflow: hidden; box-shadow: 0 1px 3px rgba(0,0,0,0.1); margin-bottom: 1rem; }
        th, td { padding: 0.6rem 1rem; text-align: left; border-bottom: 1px solid #eee; }
        th { background: #fafaff; font-weight: 600; font-size: 0.85rem; text-transform: uppercase; color: #555; }
        tr:last-child td { border-bottom: none; }
        .bar-cell { width: 40%; }
        .bar { height: 1.2rem; background: #4a4ae0; border-radius: 3px; min-width: 2px; }
        .files { font-family: "SF Mono", "Fira Code", monospace; font-size: 0.8rem; color: #444; }
        .pkg-tag {
            display: inline-block; background: #e8e8f8; color: #4a4ae0;
            border-radius: 3px; padding: 0.1rem 0.4rem; font-size: 0.75rem; margin-right: 0.25rem;
        }
        th.sortable { cursor: pointer; user-select: none; position: relative; padding-right: 1.5rem; }
        th.sortable::after { content: '\2195'; position: absolute; right: 0.4rem; opacity: 0.3; }
        th.sortable.asc::after { content: '\25B2'; opacity: 0.7; }
        th.sortable.desc::after { content: '\25BC'; opacity: 0.7; }
        .search-container { margin-bottom: 1rem; position: relative; }
        .search-input {
            width: 100%; padding: 0.6rem 2.5rem 0.6rem 1rem;
            font-family: inherit; font-size: 0.9rem;
            border: 1px solid #ddd; border-radius: 8px;
            background: #fff; box-shadow: 0 1px 3px rgba(0,0,0,0.1);
            outline: none;
        }
        .search-input:focus { border-color: #4a4ae0; box-shadow: 0 0 0 3px rgba(74,74,224,0.15); }
        .search-clear {
            position: absolute; right: 0.6rem; top: 50%; transform: translateY(-50%);
            background: none; border: none; font-size: 1.1rem; color: #999;
            cursor: pointer; display: none; padding: 0.2rem;
        }
        .search-clear:hover { color: #333; }
        .search-count { font-size: 0.8rem; color: #666; margin-top: 0.4rem; }
        .line-nums { color: #888; font-size: 0.75rem; }
        .cycle-row { cursor: pointer; }
        .cycle-row:hover { background: #f0f0ff; }
        .cycle-row.expanded { background: #f0f0ff; }
        .cycle-detail td { padding: 0; border-bottom: 1px solid #eee; }
        .diff-view { padding: 0.75rem 1rem; background: #1e1e2e; border-radius: 0 0 6px 6px; }
        .diff-file { margin-bottom: 0.5rem; }
        .diff-file:last-child { margin-bottom: 0; }
        .diff-header {
            font-family: "SF Mono", "Fira Code", monospace; font-size: 0.8rem;
            color: #fff; font-weight: 600; padding: 0.3rem 0;
            border-bottom: 1px solid #333;
        }
        .diff-line {
            font-family: "SF Mono", "Fira Code", monospace; font-size: 0.8rem;
            color: #a0d0a0; padding: 0.15rem 0 0.15rem 1rem;
        }
        .diff-lineno { color: #666; margin-right: 0.75rem; min-width: 2.5rem; display: inline-block; text-align: right; }
        .diff-code { color: #dcdcaa; margin-right: 0.5rem; font-family: "SF Mono", "Fira Code", monospace; font-size: 0.8rem; }
        .diff-arrow { color: #e06060; margin: 0 0.4rem; }
        .diff-empty { color: #666; font-style: italic; font-size: 0.8rem; padding: 0.3rem 0 0.3rem 1rem; }
    </style>
</head>
<body>
    <h1>Circular Import Report</h1>
"#);
    let _ = writeln!(
        html,
        "    <p class=\"date\">Generated on {}</p>",
        html_escape(date)
    );
}

fn write_summary(html: &mut String, stats: &ReportStats) {
    let _ = write!(
        html,
        r#"
    <div class="cards">
        <div class="card"><div class="card-value">{}</div><div class="card-label">Cycles Detected</div></div>
        <div class="card"><div class="card-value">{}</div><div class="card-label">Cycles Suppressed</div></div>
        <div class="card"><div class="card-value">{}</div><div class="card-label">Files Involved</div></div>
    </div>
"#,
        stats.total_cycles, stats.total_suppressed, stats.total_files
    );
}

fn write_package_table(html: &mut String, package_frequency: &[(String, usize)]) {
    html.push_str("\n    <h2>Package Frequency</h2>\n");
    html.push_str(
        "    <table class=\"sortable\">\n        <tr><th class=\"sortable\">Package</th><th class=\"sortable\" data-sort-type=\"number\">Cycles</th><th class=\"bar-cell\"></th></tr>\n",
    );
    let max_count = package_frequency.iter().map(|(_, c)| *c).max().unwrap_or(1);
    for (pkg, count) in package_frequency {
        let width_pct = (*count as f64 / max_count as f64) * 100.0;
        let _ = writeln!(
            html,
            "        <tr><td>{}</td><td>{}</td><td class=\"bar-cell\"><div class=\"bar\" style=\"width: {:.1}%\"></div></td></tr>",
            html_escape(pkg),
            count,
            width_pct
        );
    }
    html.push_str("    </table>\n");
}

fn write_size_table(html: &mut String, size_distribution: &[(usize, usize)]) {
    html.push_str("\n    <h2>Cycle Size Distribution</h2>\n");
    html.push_str(
        "    <table class=\"sortable\">\n        <tr><th class=\"sortable\" data-sort-type=\"number\">Cycle Size (files)</th><th class=\"sortable\" data-sort-type=\"number\">Count</th><th class=\"bar-cell\"></th></tr>\n",
    );
    let max_count = size_distribution.iter().map(|(_, c)| *c).max().unwrap_or(1);
    for (size, count) in size_distribution {
        let width_pct = (*count as f64 / max_count as f64) * 100.0;
        let _ = writeln!(
            html,
            "        <tr><td>{}</td><td>{}</td><td class=\"bar-cell\"><div class=\"bar\" style=\"width: {:.1}%\"></div></td></tr>",
            size, count, width_pct
        );
    }
    html.push_str("    </table>\n");
}

fn write_cycle_table(html: &mut String, cycles: &[JsonCycle], source_roots: &[PathBuf]) {
    html.push_str("\n    <h2>All Cycles</h2>\n");
    html.push_str(
        "    <div class=\"search-container\"><input type=\"text\" id=\"pkg-search\" class=\"search-input\" placeholder=\"Filter by package...\"><button id=\"pkg-search-clear\" class=\"search-clear\">&times;</button></div>\n    <div id=\"pkg-search-count\" class=\"search-count\"></div>\n",
    );
    html.push_str(
        "    <table id=\"cycles-table\" class=\"sortable\">\n        <tr><th class=\"sortable\" data-sort-type=\"number\">#</th><th class=\"sortable\">Packages</th><th class=\"sortable\" data-sort-type=\"number\">Size</th><th class=\"sortable\">Files</th></tr>\n",
    );
    for cycle in cycles {
        let pkg_tags = if cycle.packages.is_empty() {
            "<span class=\"pkg-tag\">(root-level)</span>".to_string()
        } else {
            cycle
                .packages
                .iter()
                .map(|p| format!("<span class=\"pkg-tag\">{}</span>", html_escape(p)))
                .collect::<Vec<_>>()
                .join("")
        };
        let file_list = cycle
            .files
            .iter()
            .map(|f| {
                let path = html_escape(&f.path);
                if f.import_lines.is_empty() {
                    path
                } else {
                    let lines = f
                        .import_lines
                        .iter()
                        .map(|l| format!("L{l}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{} <span class=\"line-nums\">({})</span>", path, lines)
                }
            })
            .collect::<Vec<_>>()
            .join("<br>");
        let data_packages = if cycle.packages.is_empty() {
            "(root-level)".to_string()
        } else {
            cycle
                .packages
                .iter()
                .map(|p| html_escape(p))
                .collect::<Vec<_>>()
                .join(",")
        };
        let _ = writeln!(
            html,
            "        <tr data-packages=\"{}\" class=\"cycle-row\"><td>{}</td><td>{}</td><td>{}</td><td class=\"files\">{}</td></tr>",
            data_packages, cycle.index, pkg_tags, cycle.size, file_list
        );
        html.push_str("        <tr class=\"cycle-detail\" style=\"display:none\"><td colspan=\"4\"><div class=\"diff-view\">\n");
        for file in &cycle.files {
            let _ = writeln!(
                html,
                "            <div class=\"diff-file\"><div class=\"diff-header\">{}</div>",
                html_escape(&file.path)
            );
            if file.edges.is_empty() {
                html.push_str("                <div class=\"diff-empty\">no outgoing imports in cycle</div>\n");
            } else {
                for edge in &file.edges {
                    for &line_num in &edge.lines {
                        let source_text = if source_roots.is_empty() {
                            None
                        } else {
                            read_source_line(source_roots, &file.path, line_num)
                        };
                        if let Some(code) = source_text {
                            let _ = writeln!(
                                html,
                                "                <div class=\"diff-line\"><span class=\"diff-lineno\">{}</span><span class=\"diff-code\">{}</span> <span class=\"diff-arrow\">&rarr;</span> {}</div>",
                                line_num,
                                html_escape(&code),
                                html_escape(&edge.to)
                            );
                        } else {
                            let _ = writeln!(
                                html,
                                "                <div class=\"diff-line\"><span class=\"diff-lineno\">L{}</span><span class=\"diff-arrow\">&rarr;</span> {}</div>",
                                line_num,
                                html_escape(&edge.to)
                            );
                        }
                    }
                }
            }
            html.push_str("            </div>\n");
        }
        html.push_str("        </div></td></tr>\n");
    }
    html.push_str("    </table>\n");
}

fn write_scripts(html: &mut String) {
    html.push_str(
        r#"<script>
document.addEventListener('DOMContentLoaded', function() {
    document.querySelectorAll('th.sortable').forEach(function(th) {
        th.addEventListener('click', function() {
            var table = th.closest('table');
            var idx = Array.from(th.parentNode.children).indexOf(th);
            var isNum = th.dataset.sortType === 'number';
            var asc = !th.classList.contains('asc');
            th.parentNode.querySelectorAll('th').forEach(function(h) { h.classList.remove('asc','desc'); });
            th.classList.add(asc ? 'asc' : 'desc');
            var cycleRows = Array.from(table.querySelectorAll('tr.cycle-row'));
            if (cycleRows.length > 0) {
                var pairs = cycleRows.map(function(r) {
                    var detail = r.nextElementSibling;
                    return { row: r, detail: detail && detail.classList.contains('cycle-detail') ? detail : null };
                });
                pairs.sort(function(a, b) {
                    var av = a.row.children[idx].textContent.trim();
                    var bv = b.row.children[idx].textContent.trim();
                    if (isNum) return asc ? parseFloat(av) - parseFloat(bv) : parseFloat(bv) - parseFloat(av);
                    return asc ? av.localeCompare(bv) : bv.localeCompare(av);
                });
                pairs.forEach(function(p) { table.appendChild(p.row); if (p.detail) table.appendChild(p.detail); });
            } else {
                var rows = Array.from(table.querySelectorAll('tr')).slice(1);
                rows.sort(function(a, b) {
                    var av = a.children[idx].textContent.trim();
                    var bv = b.children[idx].textContent.trim();
                    if (isNum) return asc ? parseFloat(av) - parseFloat(bv) : parseFloat(bv) - parseFloat(av);
                    return asc ? av.localeCompare(bv) : bv.localeCompare(av);
                });
                rows.forEach(function(r) { table.appendChild(r); });
            }
        });
    });
    document.querySelectorAll('.cycle-row').forEach(function(row) {
        row.addEventListener('click', function() {
            var detail = row.nextElementSibling;
            if (detail && detail.classList.contains('cycle-detail')) {
                detail.style.display = detail.style.display === 'none' ? '' : 'none';
                row.classList.toggle('expanded');
            }
        });
    });
    var si = document.getElementById('pkg-search');
    var cb = document.getElementById('pkg-search-clear');
    var ct = document.getElementById('pkg-search-count');
    var tbl = document.getElementById('cycles-table');
    if (si && tbl) {
        var allRows = Array.from(tbl.querySelectorAll('tr.cycle-row'));
        function filterRows() {
            var term = si.value.toLowerCase().trim();
            cb.style.display = term ? 'block' : 'none';
            var visible = 0;
            allRows.forEach(function(row) {
                var pkgs = (row.dataset.packages || '').toLowerCase();
                var show = !term || pkgs.indexOf(term) !== -1;
                row.style.display = show ? '' : 'none';
                var detail = row.nextElementSibling;
                if (detail && detail.classList.contains('cycle-detail')) {
                    if (!show) detail.style.display = 'none';
                }
                if (show) visible++;
            });
            ct.textContent = 'Showing ' + visible + ' of ' + allRows.length + ' cycles';
        }
        si.addEventListener('input', filterRows);
        cb.addEventListener('click', function() { si.value = ''; filterRows(); si.focus(); });
        filterRows();
    }
});
</script>
"#,
    );
}

pub fn run(input: &Path, output: &Path, source_root: Option<&Path>) {
    let report = load_json_report(input).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    let source_roots = resolve_source_roots(source_root);

    let stats = ReportStats::from_report(&report);
    let html = generate_html(&report, &stats, &source_roots);

    std::fs::write(output, html).unwrap_or_else(|e| {
        eprintln!("error: failed to write {}: {e}", output.display());
        std::process::exit(1);
    });

    eprintln!("report written to {}", output.display());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::{JsonCycleFile, JsonSummary};

    fn make_report(cycles: Vec<JsonCycle>, suppressed: usize) -> JsonReport {
        JsonReport {
            version: 1,
            summary: JsonSummary {
                cycles_reported: cycles.len(),
                cycles_suppressed: suppressed,
            },
            cycles,
        }
    }

    fn make_cycle(index: usize, packages: &[&str], files: &[&str]) -> JsonCycle {
        JsonCycle {
            index,
            packages: packages.iter().map(|s| s.to_string()).collect(),
            size: files.len(),
            files: files
                .iter()
                .map(|f| JsonCycleFile {
                    path: f.to_string(),
                    import_lines: vec![],
                    edges: vec![],
                })
                .collect(),
        }
    }

    #[test]
    fn stats_empty_report() {
        let report = make_report(vec![], 0);
        let stats = ReportStats::from_report(&report);
        assert_eq!(stats.total_cycles, 0);
        assert_eq!(stats.total_suppressed, 0);
        assert_eq!(stats.total_files, 0);
        assert!(stats.package_frequency.is_empty());
        assert!(stats.size_distribution.is_empty());
    }

    #[test]
    fn stats_single_cycle() {
        let report = make_report(
            vec![make_cycle(1, &["auth"], &["auth/a.py", "auth/b.py"])],
            0,
        );
        let stats = ReportStats::from_report(&report);
        assert_eq!(stats.total_cycles, 1);
        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.package_frequency, vec![("auth".to_string(), 1)]);
        assert_eq!(stats.size_distribution, vec![(2, 1)]);
    }

    #[test]
    fn stats_package_frequency_sorted_by_count_desc() {
        let report = make_report(
            vec![
                make_cycle(1, &["auth"], &["auth/a.py", "auth/b.py"]),
                make_cycle(2, &["auth"], &["auth/c.py", "auth/d.py"]),
                make_cycle(3, &["models"], &["models/x.py", "models/y.py"]),
            ],
            0,
        );
        let stats = ReportStats::from_report(&report);
        assert_eq!(stats.package_frequency[0], ("auth".to_string(), 2));
        assert_eq!(stats.package_frequency[1], ("models".to_string(), 1));
    }

    #[test]
    fn stats_package_frequency_ties_sorted_by_name() {
        let report = make_report(
            vec![
                make_cycle(1, &["zebra"], &["zebra/a.py", "zebra/b.py"]),
                make_cycle(2, &["alpha"], &["alpha/x.py", "alpha/y.py"]),
            ],
            0,
        );
        let stats = ReportStats::from_report(&report);
        assert_eq!(stats.package_frequency[0], ("alpha".to_string(), 1));
        assert_eq!(stats.package_frequency[1], ("zebra".to_string(), 1));
    }

    #[test]
    fn stats_cross_package_cycle_counts_each_package() {
        let report = make_report(
            vec![make_cycle(
                1,
                &["auth", "models"],
                &["auth/a.py", "models/b.py"],
            )],
            0,
        );
        let stats = ReportStats::from_report(&report);
        assert_eq!(stats.package_frequency.len(), 2);
        assert!(stats.package_frequency.contains(&("auth".to_string(), 1)));
        assert!(stats.package_frequency.contains(&("models".to_string(), 1)));
    }

    #[test]
    fn stats_root_level_cycles_use_sentinel() {
        let report = make_report(vec![make_cycle(1, &[], &["a.py", "b.py"])], 0);
        let stats = ReportStats::from_report(&report);
        assert_eq!(
            stats.package_frequency,
            vec![("(root-level)".to_string(), 1)]
        );
    }

    #[test]
    fn stats_size_distribution_sorted_by_size() {
        let report = make_report(
            vec![
                make_cycle(1, &["a"], &["a/1.py", "a/2.py", "a/3.py"]),
                make_cycle(2, &["b"], &["b/1.py", "b/2.py"]),
                make_cycle(3, &["c"], &["c/1.py", "c/2.py", "c/3.py", "c/4.py"]),
                make_cycle(4, &["d"], &["d/1.py", "d/2.py"]),
            ],
            0,
        );
        let stats = ReportStats::from_report(&report);
        assert_eq!(stats.size_distribution, vec![(2, 2), (3, 1), (4, 1)]);
    }

    #[test]
    fn stats_unique_files_deduped_across_cycles() {
        let report = make_report(
            vec![
                make_cycle(1, &["pkg"], &["pkg/a.py", "pkg/b.py"]),
                make_cycle(2, &["pkg"], &["pkg/a.py", "pkg/c.py"]),
            ],
            0,
        );
        let stats = ReportStats::from_report(&report);
        assert_eq!(stats.total_files, 3);
    }

    #[test]
    fn stats_suppressed_count_from_summary() {
        let report = make_report(vec![], 5);
        let stats = ReportStats::from_report(&report);
        assert_eq!(stats.total_suppressed, 5);
    }

    #[test]
    fn escape_ampersand() {
        assert_eq!(html_escape("a&b"), "a&amp;b");
    }

    #[test]
    fn escape_angle_brackets() {
        assert_eq!(html_escape("<div>"), "&lt;div&gt;");
    }

    #[test]
    fn escape_quotes() {
        assert_eq!(html_escape("a\"b"), "a&quot;b");
    }

    #[test]
    fn escape_clean_string_unchanged() {
        assert_eq!(html_escape("hello world"), "hello world");
    }

    #[test]
    fn load_missing_file_returns_error() {
        let result = load_json_report(Path::new("/nonexistent/path.json"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("failed to read"));
    }

    #[test]
    fn load_invalid_json_returns_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("oboros_test_invalid.json");
        std::fs::write(&path, "not json").unwrap();
        let result = load_json_report(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("failed to parse JSON report"));
    }

    #[test]
    fn load_wrong_version_returns_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("oboros_test_wrongver.json");
        let json =
            r#"{"version":99,"summary":{"cycles_reported":0,"cycles_suppressed":0},"cycles":[]}"#;
        std::fs::write(&path, json).unwrap();
        let result = load_json_report(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unsupported report version"));
    }

    #[test]
    fn load_mismatched_cycle_count_returns_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("oboros_test_bad_count.json");
        let json = r#"{"version":1,"summary":{"cycles_reported":2,"cycles_suppressed":0},"cycles":[{"index":1,"packages":["auth"],"size":2,"files":[{"path":"auth/a.py","import_lines":[],"edges":[]},{"path":"auth/b.py","import_lines":[],"edges":[]}]}]}"#;
        std::fs::write(&path, json).unwrap();
        let result = load_json_report(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid report summary"));
    }

    #[test]
    fn load_mismatched_cycle_size_returns_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("oboros_test_bad_size.json");
        let json = r#"{"version":1,"summary":{"cycles_reported":1,"cycles_suppressed":0},"cycles":[{"index":1,"packages":["auth"],"size":3,"files":[{"path":"auth/a.py","import_lines":[],"edges":[]},{"path":"auth/b.py","import_lines":[],"edges":[]}]}]}"#;
        std::fs::write(&path, json).unwrap();
        let result = load_json_report(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid cycle size"));
    }

    #[test]
    fn load_non_sequential_cycle_index_returns_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("oboros_test_bad_index.json");
        let json = r#"{"version":1,"summary":{"cycles_reported":1,"cycles_suppressed":0},"cycles":[{"index":7,"packages":["auth"],"size":2,"files":[{"path":"auth/a.py","import_lines":[],"edges":[]},{"path":"auth/b.py","import_lines":[],"edges":[]}]}]}"#;
        std::fs::write(&path, json).unwrap();
        let result = load_json_report(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid cycle index"));
    }

    #[test]
    fn load_valid_json() {
        let dir = std::env::temp_dir();
        let path = dir.join("oboros_test_valid.json");
        let json = r#"{"version":1,"summary":{"cycles_reported":1,"cycles_suppressed":0},"cycles":[{"index":1,"packages":["auth"],"size":2,"files":[{"path":"auth/a.py","import_lines":[],"edges":[]},{"path":"auth/b.py","import_lines":[],"edges":[]}]}]}"#;
        std::fs::write(&path, json).unwrap();
        let result = load_json_report(&path);
        assert!(result.is_ok());
        let report = result.unwrap();
        assert_eq!(report.version, 1);
        assert_eq!(report.summary.cycles_reported, 1);
        assert_eq!(report.cycles.len(), 1);
        assert_eq!(report.cycles[0].packages, vec!["auth"]);
    }

    #[test]
    fn html_contains_title() {
        let report = make_report(vec![], 0);
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains("Circular Import Report"));
    }

    #[test]
    fn html_is_self_contained() {
        let report = make_report(vec![], 0);
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<style>"));
        assert!(html.contains("<script>"));
        assert!(html.contains("</html>"));
        assert!(!html.contains("href=\"http"));
        assert!(!html.contains("src=\"http"));
    }

    #[test]
    fn html_contains_summary_values() {
        let report = make_report(
            vec![make_cycle(1, &["auth"], &["auth/a.py", "auth/b.py"])],
            3,
        );
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains(">1<"));
        assert!(html.contains(">3<"));
        assert!(html.contains(">2<"));
    }

    #[test]
    fn html_contains_package_names() {
        let report = make_report(
            vec![make_cycle(1, &["auth"], &["auth/a.py", "auth/b.py"])],
            0,
        );
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains("auth"));
    }

    #[test]
    fn html_contains_cycle_files() {
        let report = make_report(
            vec![make_cycle(1, &["pkg"], &["pkg/foo.py", "pkg/bar.py"])],
            0,
        );
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains("pkg/foo.py"));
        assert!(html.contains("pkg/bar.py"));
    }

    #[test]
    fn html_escapes_special_characters() {
        let report = make_report(
            vec![make_cycle(1, &["<script>"], &["<script>/a.py", "b&c.py"])],
            0,
        );
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("b&amp;c.py"));
        assert!(!html.contains("<script>/a.py"));
    }

    #[test]
    fn html_empty_report_has_table_headers() {
        let report = make_report(vec![], 0);
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains("Package Frequency"));
        assert!(html.contains("Cycle Size Distribution"));
        assert!(html.contains("All Cycles"));
    }

    #[test]
    fn html_has_sortable_tables() {
        let report = make_report(
            vec![make_cycle(1, &["auth"], &["auth/a.py", "auth/b.py"])],
            0,
        );
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains("class=\"sortable\""));
        assert!(html.contains("data-sort-type=\"number\""));
    }

    #[test]
    fn html_has_search_bar() {
        let report = make_report(
            vec![make_cycle(1, &["auth"], &["auth/a.py", "auth/b.py"])],
            0,
        );
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains("id=\"pkg-search\""));
        assert!(html.contains("Filter by package"));
        assert!(html.contains("id=\"cycles-table\""));
    }

    #[test]
    fn html_cycle_rows_have_data_packages() {
        let report = make_report(
            vec![make_cycle(
                1,
                &["auth", "models"],
                &["auth/a.py", "models/b.py"],
            )],
            0,
        );
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains("data-packages=\"auth,models\""));
    }

    #[test]
    fn html_shows_import_line_numbers() {
        let mut cycle = make_cycle(1, &["auth"], &["auth/a.py", "auth/b.py"]);
        cycle.files[0].import_lines = vec![5, 12];
        cycle.files[1].import_lines = vec![3];
        let report = JsonReport {
            version: 1,
            summary: JsonSummary {
                cycles_reported: 1,
                cycles_suppressed: 0,
            },
            cycles: vec![cycle],
        };
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains("L5, L12"));
        assert!(html.contains("L3"));
    }

    #[test]
    fn html_has_diff_view() {
        let mut cycle = make_cycle(1, &["auth"], &["auth/a.py", "auth/b.py"]);
        cycle.files[0].edges = vec![crate::output::JsonEdge {
            to: "auth/b.py".to_string(),
            lines: vec![5],
        }];
        let report = JsonReport {
            version: 1,
            summary: JsonSummary {
                cycles_reported: 1,
                cycles_suppressed: 0,
            },
            cycles: vec![cycle],
        };
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains("diff-view"));
        assert!(html.contains("diff-header"));
        assert!(html.contains("diff-line"));
        assert!(html.contains("auth/b.py"));
    }

    #[test]
    fn html_diff_view_no_edges_shows_empty_message() {
        let report = make_report(
            vec![make_cycle(1, &["auth"], &["auth/a.py", "auth/b.py"])],
            0,
        );
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[]);
        assert!(html.contains("diff-empty"));
        assert!(html.contains("no outgoing imports in cycle"));
    }

    #[test]
    fn html_diff_shows_source_line_when_source_root_provided() {
        use std::fs;
        let dir = std::env::temp_dir().join("oboros_test_source");
        let _ = fs::create_dir_all(dir.join("auth"));
        fs::write(
            dir.join("auth/a.py"),
            "# line 1\n# line 2\n# line 3\n# line 4\nfrom auth.b import handler\n",
        )
        .unwrap();

        let mut cycle = make_cycle(1, &["auth"], &["auth/a.py", "auth/b.py"]);
        cycle.files[0].edges = vec![crate::output::JsonEdge {
            to: "auth/b.py".to_string(),
            lines: vec![5],
        }];
        let report = JsonReport {
            version: 1,
            summary: JsonSummary {
                cycles_reported: 1,
                cycles_suppressed: 0,
            },
            cycles: vec![cycle],
        };
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, std::slice::from_ref(&dir));
        assert!(html.contains("from auth.b import handler"));
        assert!(html.contains("diff-code"));
    }

    #[test]
    fn html_diff_falls_back_when_source_file_missing() {
        let fake_root = std::env::temp_dir().join("oboros_test_nosource");
        let _ = std::fs::create_dir_all(&fake_root);

        let mut cycle = make_cycle(1, &["auth"], &["auth/a.py", "auth/b.py"]);
        cycle.files[0].edges = vec![crate::output::JsonEdge {
            to: "auth/b.py".to_string(),
            lines: vec![5],
        }];
        let report = JsonReport {
            version: 1,
            summary: JsonSummary {
                cycles_reported: 1,
                cycles_suppressed: 0,
            },
            cycles: vec![cycle],
        };
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, &[fake_root]);
        assert!(html.contains("L5"));
        assert!(html.contains("auth/b.py"));
    }

    #[test]
    fn html_diff_shows_multiline_import() {
        use std::fs;
        let dir = std::env::temp_dir().join("oboros_test_multiline");
        let _ = fs::create_dir_all(dir.join("billing"));
        fs::write(
            dir.join("billing/__init__.py"),
            "from billing.managers import (\n    InvoiceManager,\n    LineManager,\n)\n",
        )
        .unwrap();

        let mut cycle = make_cycle(
            1,
            &["billing"],
            &["billing/__init__.py", "billing/managers.py"],
        );
        cycle.files[0].edges = vec![crate::output::JsonEdge {
            to: "billing/managers.py".to_string(),
            lines: vec![1],
        }];
        let report = JsonReport {
            version: 1,
            summary: JsonSummary {
                cycles_reported: 1,
                cycles_suppressed: 0,
            },
            cycles: vec![cycle],
        };
        let stats = ReportStats::from_report(&report);
        let html = generate_html(&report, &stats, std::slice::from_ref(&dir));
        assert!(html.contains("InvoiceManager"));
        assert!(html.contains("LineManager"));
        assert!(html.contains("from billing.managers import"));
    }
}
