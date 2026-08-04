use std::path::PathBuf;
use std::process::{Command, Output};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_oboros")
}

fn fixture_config(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}/oboros.toml"))
}

fn run_output(fixture: &str, extra_args: &[&str]) -> Output {
    let config = fixture_config(fixture);
    let mut args = vec!["--config", config.to_str().unwrap()];
    args.extend_from_slice(extra_args);
    Command::new(binary())
        .args(&args)
        .output()
        .expect("failed to run oboros")
}

fn run_json(fixture: &str, extra_args: &[&str]) -> serde_json::Value {
    let mut args = vec!["--format", "json"];
    args.extend_from_slice(extra_args);
    let output = run_output(fixture, &args);
    let stdout = String::from_utf8(output.stdout).unwrap();
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout should be valid JSON: {e}\nstdout: {stdout}"))
}

fn cycles(parsed: &serde_json::Value) -> &Vec<serde_json::Value> {
    parsed["cycles"]
        .as_array()
        .expect("cycles must be an array")
}

fn trace_relationships(parsed: &serde_json::Value) -> Vec<String> {
    parsed["traced"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|t| t["files"].as_array().into_iter().flatten())
        .flat_map(|f| f["impacts"].as_array().into_iter().flatten())
        .map(|imp| imp["relationship"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn lazy_basic_without_flag_reports_plain_import_cycle() {
    let parsed = run_json("lazy_basic", &[]);
    assert!(
        !cycles(&parsed).is_empty(),
        "lazy_basic must have a normal import cycle without --check-lazy: {parsed}"
    );
    assert!(
        parsed.get("analysis").is_none(),
        "analysis field must be absent without --check-lazy: {parsed}"
    );
}

#[test]
fn lazy_basic_check_lazy_reports_lazy_cycle_with_blocker_context() {
    let parsed = run_json("lazy_basic", &["--check-lazy"]);
    assert_eq!(
        parsed["analysis"], "lazy",
        "analysis must be lazy: {parsed}"
    );
    assert_eq!(
        cycles(&parsed).len(),
        1,
        "lazy_basic must have exactly one lazy cycle: {parsed}"
    );
    let has_blocker = cycles(&parsed).iter().any(|c| {
        c["files"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|f| f["edges"].as_array().into_iter().flatten())
            .any(|e| !e["blocker_context"].is_null())
    });
    assert!(
        has_blocker,
        "at least one cycle edge must carry a blocker_context: {parsed}"
    );
}

#[test]
fn lazy_deferred_without_flag_reports_one_import_cycle() {
    let parsed = run_json("lazy_deferred", &[]);
    assert_eq!(
        cycles(&parsed).len(),
        1,
        "lazy_deferred must have one import cycle without --check-lazy: {parsed}"
    );
}

#[test]
fn lazy_deferred_check_lazy_finds_no_cycle() {
    let parsed = run_json("lazy_deferred", &["--check-lazy"]);
    assert!(
        cycles(&parsed).is_empty(),
        "deferred (function-body) use must not form a lazy cycle: {parsed}"
    );
}

#[test]
fn check_lazy_strict_exits_one_on_lazy_cycle() {
    let output = run_output("lazy_basic", &["--check-lazy", "--strict"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "--strict must exit 1 when a lazy cycle exists"
    );
}

#[test]
fn check_lazy_strict_exits_zero_without_lazy_cycle() {
    let output = run_output("lazy_deferred", &["--check-lazy", "--strict"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "--strict must exit 0 when no lazy cycle exists"
    );
}

#[test]
fn check_lazy_strict_json_exits_one_and_emits_valid_json() {
    let output = run_output(
        "lazy_basic",
        &["--check-lazy", "--format", "json", "--strict"],
    );
    assert_eq!(output.status.code(), Some(1), "--strict must exit 1");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("strict mode must still emit valid JSON: {e}\nstdout: {stdout}")
    });
    assert_eq!(parsed["analysis"], "lazy");
    assert_eq!(cycles(&parsed).len(), 1);
}

#[test]
fn check_lazy_with_dump_cyclic_files_is_rejected() {
    let output = run_output("lazy_basic", &["--check-lazy", "--dump-cyclic-files"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "guarded flag combination must exit 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be combined"),
        "stderr must explain the guard: {stderr}"
    );
}

#[test]
fn check_lazy_with_local_imports_warns_but_emits_valid_json() {
    let output = run_output(
        "lazy_basic",
        &["--check-lazy", "--local-imports", "--format", "json"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no effect"),
        "stderr must warn that --local-imports has no effect: {stderr}"
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout must remain valid JSON: {e}\nstdout: {stdout}"));
    assert_eq!(parsed["analysis"], "lazy");
}

#[test]
fn trace_member_of_lazy_cycle() {
    let parsed = run_json("lazy_basic", &["--check-lazy", "--trace", "models.py"]);
    let relationships = trace_relationships(&parsed);
    assert!(
        relationships.iter().any(|r| r == "member"),
        "models.py is part of the lazy cycle, so trace must report a member impact: {parsed}"
    );
}

#[test]
fn trace_reachable_to_lazy_cycle() {
    let parsed = run_json("lazy_basic", &["--check-lazy", "--trace", "main.py"]);
    let relationships = trace_relationships(&parsed);
    assert!(
        relationships.iter().any(|r| r == "reachable"),
        "main.py reaches the lazy cycle but is not a member, so trace must report reachable: {parsed}"
    );
    assert!(
        !relationships.iter().any(|r| r == "member"),
        "main.py must not be a member of the lazy cycle: {parsed}"
    );
}

#[test]
fn trace_has_no_impact_when_no_lazy_cycle() {
    let parsed = run_json("lazy_deferred", &["--check-lazy", "--trace", "a.py"]);
    let relationships = trace_relationships(&parsed);
    assert!(
        relationships.is_empty(),
        "with no lazy cycle, no file may report an impact: {parsed}"
    );
}
