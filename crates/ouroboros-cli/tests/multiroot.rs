//! Multi-source-root integration coverage.
//!
//! Fixture `tests/fixtures/multiroot/` uses `source-roots = ["src", "lib"]`:
//!
//! ```text
//! src/pkg/a.py         -> from pkg import b   (cycle with b)
//! src/pkg/b.py         -> from pkg import a   (cycle with a)
//! src/pkg/__init__.py  (empty)
//! src/utils/helper.py  (no imports)
//! lib/pkg/c.py         -> from pkg import a   (cross-root edge into src)
//! lib/pkg/__init__.py  (empty)
//! lib/utils/helper.py  (no imports)
//! ```
//!
//! Two module-name collisions arise across roots: `pkg` (both `__init__.py`)
//! and `utils.helper` (both `helper.py`). The single reported cycle is
//! `src/pkg/a.py <-> src/pkg/b.py`.

use std::path::PathBuf;
use std::process::Command;

fn fixture_config(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/multiroot/{name}"))
}

fn run_json(args: &[&str]) -> serde_json::Value {
    let output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args(args)
        .output()
        .expect("failed to run oboros");
    let stdout = String::from_utf8(output.stdout).unwrap();
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("not valid JSON: {e}\nstdout: {stdout}"))
}

fn run_raw(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args(args)
        .output()
        .expect("failed to run oboros")
}

/// The one cycle across the whole graph: intra-package `pkg` a<->b.
fn cycle_file_paths(parsed: &serde_json::Value) -> Vec<String> {
    let cycles = parsed["cycles"].as_array().unwrap();
    assert_eq!(cycles.len(), 1, "fixture should report exactly one cycle");
    cycles[0]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_string())
        .collect()
}

// ── (a) full project-root-relative paths across roots ─────────────────────────

/// Cycle files carry the full `src/` prefix, not a bare `pkg/a.py`.
#[test]
fn cycle_files_use_full_src_prefixed_paths() {
    let cfg = fixture_config("oboros.toml");
    let parsed = run_json(&["--config", cfg.to_str().unwrap(), "--format", "json"]);

    let files = cycle_file_paths(&parsed);
    assert!(
        files.iter().any(|p| p == "src/pkg/a.py"),
        "expected full path src/pkg/a.py, got: {files:?}"
    );
    assert!(
        files.iter().any(|p| p == "src/pkg/b.py"),
        "expected full path src/pkg/b.py, got: {files:?}"
    );
    assert!(
        !files.iter().any(|p| p == "pkg/a.py" || p == "pkg/b.py"),
        "paths must not be source-root-stripped (bare pkg/a.py), got: {files:?}"
    );
}

/// The cross-root file `lib/pkg/c.py` keeps its full `lib/` prefix and is
/// reachable into the `src`-rooted cycle.
#[test]
fn cross_root_file_uses_full_lib_prefixed_path() {
    let cfg = fixture_config("oboros.toml");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--trace",
        "lib/pkg/c.py",
    ]);

    let traced = parsed["traced"].as_array().unwrap();
    assert_eq!(traced.len(), 1);
    assert_eq!(traced[0]["path"], "lib/pkg/c.py");

    assert!(
        parsed.get("unknown_paths").is_none()
            || parsed["unknown_paths"].as_array().unwrap().is_empty(),
        "lib/pkg/c.py is a real cross-root node, not unknown"
    );

    let files = traced[0]["files"].as_array().unwrap();
    let c = files
        .iter()
        .find(|f| f["path"] == "lib/pkg/c.py")
        .expect("traced file lib/pkg/c.py present");
    let impacts = c["impacts"].as_array().unwrap();
    assert_eq!(impacts[0]["relationship"], "reachable");
    // The cross-root import lands on the src-rooted cycle member.
    assert_eq!(impacts[0]["entry"], "src/pkg/a.py");
}

// ── (b) same module name in two roots = two distinct nodes ────────────────────

/// `src/utils/helper.py` and `lib/utils/helper.py` share the module name
/// `utils.helper` yet remain distinct graph nodes — both resolve when traced.
#[test]
fn same_module_name_in_two_roots_are_distinct_nodes() {
    let cfg = fixture_config("oboros.toml");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--trace",
        "src/utils/helper.py",
        "--trace",
        "lib/utils/helper.py",
    ]);

    let traced = parsed["traced"].as_array().unwrap();
    assert_eq!(traced.len(), 2, "both helper files should trace");

    let traced_paths: Vec<&str> = traced.iter().map(|t| t["path"].as_str().unwrap()).collect();
    assert!(traced_paths.contains(&"src/utils/helper.py"));
    assert!(traced_paths.contains(&"lib/utils/helper.py"));

    // Neither is an unknown path: both are real, distinct nodes in the graph.
    assert!(
        parsed.get("unknown_paths").is_none()
            || parsed["unknown_paths"].as_array().unwrap().is_empty(),
        "both helper files must be known distinct nodes"
    );

    for t in traced {
        let files = t["files"].as_array().unwrap();
        assert_eq!(
            files.len(),
            1,
            "each helper trace resolves to its own single node"
        );
        assert_eq!(files[0]["path"], t["path"]);
    }
}

// ── (c) cross-root module-name collision warning on stderr ────────────────────

#[test]
fn cross_root_module_collision_warns_on_stderr() {
    let cfg = fixture_config("oboros.toml");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--format", "json"]);
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert!(
        stderr.contains("utils.helper") && stderr.contains("multiple files"),
        "expected a cross-root collision warning for utils.helper, got stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("src/utils/helper.py") && stderr.contains("lib/utils/helper.py"),
        "collision warning should name both colliding files, got stderr:\n{stderr}"
    );
}

// ── (d) --package groups by `pkg`, never by the source root ───────────────────

#[test]
fn package_flag_groups_by_pkg_not_source_root() {
    let cfg = fixture_config("oboros.toml");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--package",
    ]);

    let cycles = parsed["cycles"].as_array().unwrap();
    assert_eq!(
        cycles.len(),
        1,
        "the intra-package pkg cycle survives --package"
    );

    let packages: Vec<&str> = cycles[0]["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    assert_eq!(
        packages,
        vec!["pkg"],
        "package must be `pkg` (source-root prefix stripped), not `src`/`lib`"
    );
}

// ── (e) --trace on a cycle member reports the cycle ───────────────────────────

#[test]
fn trace_cycle_member_reports_cycle() {
    let cfg = fixture_config("oboros.toml");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--trace",
        "src/pkg/a.py",
    ]);

    let traced = parsed["traced"].as_array().unwrap();
    assert_eq!(traced.len(), 1);
    assert_eq!(traced[0]["path"], "src/pkg/a.py");

    let files = traced[0]["files"].as_array().unwrap();
    let a = files
        .iter()
        .find(|f| f["path"] == "src/pkg/a.py")
        .expect("traced file src/pkg/a.py present");
    let impacts = a["impacts"].as_array().unwrap();
    assert!(!impacts.is_empty(), "cycle member should have an impact");
    assert_eq!(impacts[0]["relationship"], "member");
    assert_eq!(impacts[0]["entry"], "src/pkg/a.py");

    // The cycle itself is still present in the report.
    assert_eq!(cycle_file_paths(&parsed).len(), 2);
}

// ── (f) --exclude drops lib files from the analysis seeds ──────────────────────

/// Excluding `lib/pkg/` removes `lib/pkg/c.py` from the seeds. Since nothing
/// imports it, it is pruned from the graph and becomes an unknown trace path.
#[test]
fn exclude_lib_dir_removes_lib_seed_from_graph() {
    let cfg = fixture_config("oboros.toml");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--exclude",
        "lib/pkg/",
        "--trace",
        "lib/pkg/c.py",
    ]);

    let excluded = parsed["excluded"].as_array().unwrap();
    assert!(
        !excluded.is_empty(),
        "excluded field should list the applied lib/pkg/ pattern"
    );

    let unknown = parsed["unknown_paths"].as_array().unwrap();
    assert!(
        unknown
            .iter()
            .any(|p| p.as_str().unwrap_or("").contains("lib/pkg/c.py")),
        "lib/pkg/c.py should be pruned (excluded seed, unreachable), got: {unknown:?}"
    );

    // The src-rooted cycle is untouched by excluding the lib seed.
    assert_eq!(cycle_file_paths(&parsed).len(), 2);
}

// ── (g) [[cycles.ignore]] with project-root-relative paths suppresses ─────────

#[test]
fn ignore_entry_with_project_relative_paths_suppresses_cycle() {
    // Baseline: the cycle is present.
    let base = run_json(&[
        "--config",
        fixture_config("oboros.toml").to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert_eq!(base["cycles"].as_array().unwrap().len(), 1);

    // With the ignore config, the cycle is suppressed.
    let ignored = run_json(&[
        "--config",
        fixture_config("oboros-ignore.toml").to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert!(
        ignored["cycles"].as_array().unwrap().is_empty(),
        "cycle should be suppressed by [[cycles.ignore]] with project-root-relative paths"
    );
    assert_eq!(
        ignored["summary"]["cycles_suppressed"].as_u64().unwrap(),
        1,
        "one cycle should be counted as suppressed"
    );
    assert_eq!(ignored["summary"]["cycles_reported"].as_u64().unwrap(), 0);
}

/// `--strict` exits zero once the only cycle is suppressed by the ignore list.
#[test]
fn ignore_entry_makes_strict_pass() {
    let output = run_raw(&[
        "--config",
        fixture_config("oboros-ignore.toml").to_str().unwrap(),
        "--strict",
    ]);
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "strict should pass when the sole cycle is ignored"
    );
}

// ── (h) known-cyclic-files with project-root-relative paths passes check ──────

#[test]
fn known_cyclic_files_project_relative_passes_check() {
    let cfg = fixture_config("oboros-known.toml");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--check-cyclic-files"]);
    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(
        output.status.code().unwrap(),
        0,
        "check should pass when known-cyclic-files matches the computed set; stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("unchanged"),
        "stderr should report the cyclic-file set unchanged; got:\n{stderr}"
    );
}

/// The computed cyclic-file set is exactly the two project-root-relative paths.
#[test]
fn dump_cyclic_files_lists_project_relative_paths() {
    let cfg = fixture_config("oboros.toml");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--dump-cyclic-files",
    ]);
    let paths: Vec<&str> = parsed["cyclic_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(paths, vec!["src/pkg/a.py", "src/pkg/b.py"]);
}
