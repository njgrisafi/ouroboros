use std::path::PathBuf;
use std::process::Command;

fn fixture_config() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/impact_branch/oboros.toml")
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

#[test]
fn trace_file_entry_py_reachable() {
    let cfg = fixture_config();
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--trace",
        "app/entry.py",
    ]);

    let traced = &parsed["traced"];
    assert!(traced.is_array());
    assert_eq!(traced.as_array().unwrap().len(), 1);

    let trace = &traced[0];
    assert_eq!(trace["path"], "app/entry.py");
    assert_eq!(trace["kind"], "file");

    let files = trace["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], "app/entry.py");

    let impacts = files[0]["impacts"].as_array().unwrap();
    assert_eq!(impacts.len(), 1);
    assert_eq!(impacts[0]["relationship"], "reachable");
    assert_eq!(impacts[0]["entry"], "app/core_a.py");
    assert_eq!(impacts[0]["from_lines"], serde_json::json!([1]));

    let path_hops = impacts[0]["path"].as_array().unwrap();
    assert_eq!(path_hops.len(), 2);
    assert_eq!(path_hops[0]["from"], "app/entry.py");
    assert_eq!(path_hops[0]["to"], "app/mid.py");
    assert_eq!(path_hops[0]["lines"], serde_json::json!([1]));
    assert_eq!(path_hops[1]["from"], "app/mid.py");
    assert_eq!(path_hops[1]["to"], "app/core_a.py");
    assert_eq!(path_hops[1]["lines"], serde_json::json!([1]));
}

#[test]
fn trace_file_core_a_member() {
    let cfg = fixture_config();
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--trace",
        "app/core_a.py",
    ]);

    let files = &parsed["traced"][0]["files"];
    let impacts = files[0]["impacts"].as_array().unwrap();
    assert_eq!(impacts.len(), 1);
    assert_eq!(impacts[0]["relationship"], "member");
    assert_eq!(impacts[0]["entry"], "app/core_a.py");
    assert!(impacts[0].get("path").is_none() || impacts[0]["path"].as_array().unwrap().is_empty());
    assert!(
        impacts[0].get("from_lines").is_none()
            || impacts[0]["from_lines"].as_array().unwrap().is_empty()
    );
}

#[test]
fn trace_directory_app() {
    let cfg = fixture_config();
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--trace",
        "app/",
    ]);

    let trace = &parsed["traced"][0];
    assert_eq!(trace["kind"], "directory");
    assert_eq!(trace["path"], "app/");

    let files = trace["files"].as_array().unwrap();
    assert_eq!(files.len(), 6);

    let mid = files.iter().find(|f| f["path"] == "app/mid.py").unwrap();
    let mid_impacts = mid["impacts"].as_array().unwrap();
    assert_eq!(mid_impacts[0]["relationship"], "reachable");
    let mid_hops = mid_impacts[0]["path"].as_array().unwrap();
    assert_eq!(mid_hops.len(), 1);

    let entry = files.iter().find(|f| f["path"] == "app/entry.py").unwrap();
    let entry_impacts = entry["impacts"].as_array().unwrap();
    let entry_hops = entry_impacts[0]["path"].as_array().unwrap();
    assert_eq!(entry_hops.len(), 2);

    let init = files
        .iter()
        .find(|f| f["path"] == "app/__init__.py")
        .unwrap();
    assert!(init.get("impacts").is_none() || init["impacts"].as_array().unwrap().is_empty());

    let isolated = files
        .iter()
        .find(|f| f["path"] == "app/isolated.py")
        .unwrap();
    assert!(
        isolated.get("impacts").is_none() || isolated["impacts"].as_array().unwrap().is_empty()
    );
}

#[test]
fn trace_unknown_path() {
    let cfg = fixture_config();
    let output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--format",
            "json",
            "--trace",
            "does/not/exist.py",
        ])
        .output()
        .expect("failed to run oboros");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let unknown = parsed["unknown_paths"].as_array().unwrap();
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0], "does/not/exist.py");
    assert!(stderr.contains("does/not/exist.py"));
}

#[test]
fn trace_source_root_prefix_stripping() {
    let cfg = fixture_config();
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--trace",
        "src/app/core_a.py",
    ]);

    let traced = parsed["traced"].as_array().unwrap();
    assert_eq!(traced.len(), 1);
    let files = traced[0]["files"].as_array().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["path"], "app/core_a.py");
    let impacts = files[0]["impacts"].as_array().unwrap();
    assert_eq!(impacts[0]["relationship"], "member");
}

#[test]
fn trace_short_alias() {
    let cfg = fixture_config();
    let with_long = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--trace",
        "app/entry.py",
    ]);
    let with_short = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "-t",
        "app/entry.py",
    ]);
    assert_eq!(with_long["traced"], with_short["traced"]);
}

#[test]
fn no_trace_no_extra_keys() {
    let cfg = fixture_config();
    let parsed = run_json(&["--config", cfg.to_str().unwrap(), "--format", "json"]);

    assert!(
        parsed.get("traced").is_none(),
        "traced key should be absent when --trace not used"
    );
    assert!(
        parsed.get("unknown_paths").is_none(),
        "unknown_paths key should be absent when --trace not used"
    );
}

#[test]
fn strict_with_trace_impacted_exits_nonzero() {
    let cfg = fixture_config();
    let output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--trace",
            "app/entry.py",
            "--strict",
        ])
        .output()
        .expect("failed to run oboros");

    assert_ne!(
        output.status.code().unwrap(),
        0,
        "--strict with impacted trace should exit nonzero"
    );
}

#[test]
fn strict_with_trace_clean_exits_zero() {
    let cfg = fixture_config();
    let output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--trace",
            "app/isolated.py",
            "--strict",
        ])
        .output()
        .expect("failed to run oboros");

    assert_eq!(
        output.status.code().unwrap(),
        0,
        "--strict with clean trace should exit 0"
    );
}

#[test]
fn human_trace_directory_contains_expected_output() {
    let cfg = fixture_config();
    let output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args(["--config", cfg.to_str().unwrap(), "--trace", "app/"])
        .output()
        .expect("failed to run oboros");

    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(
        stdout.contains("--- cycle impact ---"),
        "should have cycle impact section"
    );
    assert!(
        stdout.contains("4 of 6 files impacted"),
        "should show impacted count"
    );
    assert!(
        stdout.contains("reachable via app/entry.py:1 -> app/mid.py:1 -> app/core_a.py"),
        "should show branch chain"
    );
    assert!(
        !stdout.contains("app/isolated.py"),
        "clean files should not appear in human output"
    );
    assert!(
        !stdout.contains("app/__init__.py"),
        "clean files should not appear in human output"
    );
    assert!(
        !stdout.contains("not impacted by any cycle"),
        "per-file 'not impacted' should be suppressed"
    );
}

#[test]
fn human_trace_clean_file_shows_not_impacted() {
    let cfg = fixture_config();
    let output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--trace",
            "app/isolated.py",
        ])
        .output()
        .expect("failed to run oboros");

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("not impacted by any cycle"));
}

#[test]
fn human_no_trace_no_cycle_impact_section() {
    let cfg = fixture_config();
    let output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args(["--config", cfg.to_str().unwrap()])
        .output()
        .expect("failed to run oboros");

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        !stdout.contains("--- cycle impact ---"),
        "cycle impact section should not appear without --trace"
    );
    assert!(
        stdout.contains("--- dependency cycles"),
        "dependency cycles section should still appear"
    );
}

#[test]
fn html_report_with_traced_contains_cycle_impact_section() {
    use std::fs;

    let cfg = fixture_config();

    let json_output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--format",
            "json",
            "--trace",
            "app/",
        ])
        .output()
        .expect("failed to run oboros");

    let json_path = std::env::temp_dir().join("oboros_trace_test.json");
    fs::write(&json_path, &json_output.stdout).unwrap();

    let html_path = std::env::temp_dir().join("oboros_trace_test.html");

    let report_output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args([
            "report",
            json_path.to_str().unwrap(),
            "--output",
            html_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run oboros report");

    assert!(
        report_output.status.success(),
        "report command should succeed"
    );

    let html = fs::read_to_string(&html_path).unwrap();
    assert!(
        html.contains("Cycle Impact"),
        "HTML should contain Cycle Impact section"
    );
    assert!(
        html.contains("app/"),
        "HTML should contain traced path in index"
    );

    let traces_path = std::env::temp_dir().join("oboros_trace_test_trace_app.html");
    assert!(traces_path.exists(), "per-trace file should be written alongside report");
    let traces_html = fs::read_to_string(&traces_path).unwrap();
    assert!(
        traces_html.contains("reachable"),
        "traces HTML should show reachable relationship"
    );
    assert!(
        traces_html.contains("app/entry.py"),
        "traces HTML should contain traced file detail"
    );
}

#[test]
fn html_report_without_traced_has_no_cycle_impact_section() {
    use std::fs;

    let cfg = fixture_config();

    let json_output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args(["--config", cfg.to_str().unwrap(), "--format", "json"])
        .output()
        .expect("failed to run oboros");

    let json_path = std::env::temp_dir().join("oboros_notrace_test.json");
    fs::write(&json_path, &json_output.stdout).unwrap();

    let html_path = std::env::temp_dir().join("oboros_notrace_test.html");

    let report_output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args([
            "report",
            json_path.to_str().unwrap(),
            "--output",
            html_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run oboros report");

    assert!(
        report_output.status.success(),
        "report command should succeed"
    );

    let html = fs::read_to_string(&html_path).unwrap();
    assert!(
        !html.contains("Cycle Impact"),
        "HTML should NOT contain Cycle Impact section when no traced data"
    );
}
