use std::path::PathBuf;
use std::process::Command;

fn binary_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("oboros")
}

fn fixture_config(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}/oboros.toml"))
}

fn run(fixture: &str, extra_args: &[&str]) -> serde_json::Value {
    let config = fixture_config(fixture);
    let mut args = vec!["--config", config.to_str().unwrap(), "--format", "json"];
    args.extend_from_slice(extra_args);

    let output = Command::new(binary_path())
        .args(&args)
        .output()
        .expect("failed to run oboros");

    let stdout = String::from_utf8(output.stdout).unwrap();
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout should be valid JSON: {e}\nstdout: {stdout}"))
}

#[test]
fn self_tree_cycle_hidden_by_default() {
    let parsed = run("self_ancestor_init", &[]);
    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        cycles.is_empty(),
        "self-tree init cycle must be hidden by default (opt-in off): {parsed}"
    );
}

#[test]
fn self_tree_cycle_reported_with_flag() {
    let parsed = run("self_ancestor_init", &["--include-self-ancestor-init"]);
    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        !cycles.is_empty(),
        "self-tree init cycle must be reported with --include-self-ancestor-init: {parsed}"
    );
    // Verify the cycle includes both __init__.py and child.py
    let files: Vec<String> = cycles[0]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_string())
        .collect();
    assert!(
        files.iter().any(|f| f.contains("__init__.py")),
        "cycle must include __init__.py: {files:?}"
    );
    assert!(
        files.iter().any(|f| f.contains("child.py")),
        "cycle must include child.py: {files:?}"
    );
    // Verify the restored edge carries its import line into JSON edge_metadata
    // (proves EdgeMetadata wiring reached the JSON output)
    let has_edge_lines = cycles[0]["files"].as_array().unwrap().iter().any(|f| {
        f["edges"]
            .as_array()
            .map(|edges| {
                edges
                    .iter()
                    .any(|e| !e["lines"].as_array().unwrap_or(&vec![]).is_empty())
            })
            .unwrap_or(false)
    });
    assert!(
        has_edge_lines,
        "restored edge must carry import line numbers in JSON: {parsed}"
    );
}

#[test]
fn flag_is_noop_without_ancestor_init() {
    let config = fixture_config("self_ancestor_init");
    let output = std::process::Command::new(binary_path())
        .args([
            "--config",
            config.to_str().unwrap(),
            "--format",
            "json",
            "--include-self-ancestor-init",
            "--no-include-ancestor-init",
        ])
        .output()
        .expect("failed to run oboros");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout should be valid JSON: {e}\nstdout: {stdout}"));
    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        cycles.is_empty(),
        "no cycles when ancestor-init disabled: {parsed}"
    );
    assert!(
        stderr.contains("has no effect when include-ancestor-init is disabled"),
        "no-op warning must appear in stderr: {stderr}"
    );
}

#[test]
fn restored_cycle_is_suppressible_via_ignore() {
    let parsed = run(
        "self_ancestor_init_ignore",
        &["--include-self-ancestor-init"],
    );
    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        cycles.is_empty(),
        "restored cycle must be suppressible via [[cycles.ignore]]: {parsed}"
    );
}

#[test]
fn self_tree_cycle_reported_via_config() {
    // Config sets include-self-ancestor-init = true; no CLI flag needed.
    let parsed = run("self_ancestor_init_config", &[]);
    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        !cycles.is_empty(),
        "self-tree init cycle must be reported when enabled via config: {parsed}"
    );
}

#[test]
fn normal_child_reexport_is_not_a_self_cycle() {
    // pkg/__init__.py re-exports from pkg.child, but child does NOT import back.
    // Even with min-scc-size=1 (in the fixture's oboros.toml), no self-loop cycle
    // should be reported — guards against the pkg->pkg self-edge fabrication bug.
    let parsed = run("self_reexport", &["--include-self-ancestor-init"]);
    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        cycles.is_empty(),
        "normal package re-export must not fabricate a self-loop cycle: {parsed}"
    );
}
