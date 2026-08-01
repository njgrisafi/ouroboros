use std::path::PathBuf;
use std::process::Command;

fn fixture_config(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}/oboros.toml"))
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

#[test]
fn dump_json_lists_sorted_union() {
    let cfg = fixture_config("cyclic_basic");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--dump-cyclic-files",
    ]);
    let files = parsed["cyclic_files"].as_array().unwrap();
    let paths: Vec<&str> = files.iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(paths, vec!["src/app/a.py", "src/app/b.py"]);
    assert_eq!(parsed["version"], 1);
}

#[test]
fn dump_human_prints_toml_fragment() {
    let cfg = fixture_config("cyclic_basic");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--dump-cyclic-files"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("known-cyclic-files = ["),
        "should contain array header"
    );
    assert!(
        stdout.contains("\"src/app/a.py\""),
        "should contain src/app/a.py"
    );
    assert!(
        stdout.contains("\"src/app/b.py\""),
        "should contain src/app/b.py"
    );
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn dump_ignores_package_view_filter() {
    let cfg = fixture_config("cyclic_basic");
    let without_pkg = run_raw(&["--config", cfg.to_str().unwrap(), "--dump-cyclic-files"]);
    let with_pkg = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--dump-cyclic-files",
        "--package",
    ]);
    assert_eq!(
        String::from_utf8(without_pkg.stdout).unwrap(),
        String::from_utf8(with_pkg.stdout).unwrap(),
        "--package should not affect --dump-cyclic-files output"
    );
}

#[test]
fn check_pass_on_exact_match() {
    let cfg = fixture_config("cyclic_match");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--check-cyclic-files"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "should exit 0 on exact match"
    );
    assert!(stderr.contains("unchanged"), "stderr should say unchanged");
}

#[test]
fn check_fail_on_new_cyclic_file() {
    let cfg = fixture_config("cyclic_new");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--check-cyclic-files"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        1,
        "should exit 1 on new cyclic file"
    );
    assert!(
        stderr.contains("+ src/app/b.py"),
        "stderr should show newly cyclic file"
    );
}

#[test]
fn check_fail_on_stale_entry() {
    let cfg = fixture_config("cyclic_stale");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--check-cyclic-files"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        1,
        "should exit 1 on stale entry"
    );
    assert!(
        stderr.contains("- src/app/ghost.py"),
        "stderr should show stale entry"
    );
}

#[test]
fn check_fail_on_all_resolved() {
    let cfg = fixture_config("cyclic_resolved");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--check-cyclic-files"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        1,
        "should exit 1 when all cycles resolved"
    );
    assert!(
        stderr.contains("- src/app/a.py"),
        "stderr should show removed a.py"
    );
    assert!(
        stderr.contains("- src/app/b.py"),
        "stderr should show removed b.py"
    );
    assert!(!stderr.contains("+ "), "stderr should have no additions");
}

#[test]
fn dump_empty_when_no_cycles() {
    let cfg = fixture_config("cyclic_resolved");
    // JSON mode
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--dump-cyclic-files",
    ]);
    let files = parsed["cyclic_files"].as_array().unwrap();
    assert!(
        files.is_empty(),
        "cyclic_files should be empty when no cycles"
    );
    assert_eq!(parsed["version"], 1);
    // Human mode
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--dump-cyclic-files"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("known-cyclic-files = []"),
        "human mode should print empty array"
    );
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn check_fail_when_no_known_list_but_cycles() {
    let cfg = fixture_config("cyclic_basic");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--check-cyclic-files"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        1,
        "should exit 1 when no known list but cycles exist"
    );
    assert!(
        stderr.contains("+ src/app/a.py"),
        "stderr should list src/app/a.py as added"
    );
    assert!(
        stderr.contains("+ src/app/b.py"),
        "stderr should list src/app/b.py as added"
    );
}

#[test]
fn check_independent_of_ignore() {
    let cfg = fixture_config("cyclic_ignore");
    // check should pass (known list matches computed, even though cycle is suppressed by ignore)
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--check-cyclic-files"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "should exit 0: files counted despite ignore suppression"
    );
    assert!(stderr.contains("unchanged"), "stderr should say unchanged");
    // dump should still list both files
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--dump-cyclic-files",
    ]);
    let files = parsed["cyclic_files"].as_array().unwrap();
    let paths: Vec<&str> = files.iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(paths, vec!["src/app/a.py", "src/app/b.py"]);
}

#[test]
fn show_adds_json_field() {
    let cfg = fixture_config("cyclic_basic");
    // With --show-cyclic-files: field present
    let with_show = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--show-cyclic-files",
    ]);
    let files = with_show["cyclic_files"].as_array().unwrap();
    let paths: Vec<&str> = files.iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(paths, vec!["src/app/a.py", "src/app/b.py"]);
    // Without --show-cyclic-files: field absent
    let without_show = run_json(&["--config", cfg.to_str().unwrap(), "--format", "json"]);
    assert!(
        without_show.get("cyclic_files").is_none(),
        "cyclic_files should be absent without --show-cyclic-files"
    );
}

#[test]
fn check_does_not_print_report() {
    let cfg = fixture_config("cyclic_match");
    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--check-cyclic-files",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code().unwrap(), 0);
    // stdout should NOT be a normal JSON report (no "cycles" array)
    assert!(
        !stdout.contains("\"cycles\""),
        "stdout should not contain normal JSON report when --check-cyclic-files is active"
    );
}
