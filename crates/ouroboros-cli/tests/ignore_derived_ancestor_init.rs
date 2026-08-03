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
fn ancestor_only_listed_by_default() {
    // Without the flag, the ancestor-only cycle IS in the baseline.
    let cfg = fixture_config("cyclic_ancestor_only");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--dump-cyclic-files",
    ]);
    let files = parsed["cyclic_files"].as_array().unwrap();
    let paths: Vec<&str> = files.iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(paths, vec!["src/alpha/__init__.py", "src/beta/helpers.py"]);
}

#[test]
fn ancestor_only_excluded_with_flag() {
    // With the flag, the ancestor-only cycle is NOT in the baseline.
    let cfg = fixture_config("cyclic_ancestor_only");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--dump-cyclic-files",
        "--ignore-derived-ancestor-init",
    ]);
    let files = parsed["cyclic_files"].as_array().unwrap();
    assert!(
        files.is_empty(),
        "ancestor-only files should be excluded from baseline with flag"
    );
}

#[test]
fn ancestor_only_excluded_via_config() {
    // Config opt-in (no CLI flag) also excludes ancestor-only files.
    let cfg = fixture_config("cyclic_ancestor_only_optin");
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
        "ancestor-only files should be excluded via config opt-in"
    );
}

#[test]
fn direct_init_still_listed_with_flag() {
    // A genuine direct __init__.py cycle is still counted even with the flag.
    // (Naming note: "direct_init" = a DIRECT cycle involving __init__.py, NOT ancestor-derived.)
    let cfg = fixture_config("cyclic_direct_init");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--dump-cyclic-files",
        "--ignore-derived-ancestor-init",
    ]);
    let files = parsed["cyclic_files"].as_array().unwrap();
    let paths: Vec<&str> = files.iter().map(|v| v.as_str().unwrap()).collect();
    assert_eq!(paths, vec!["src/pkg/__init__.py", "src/pkg/mod.py"]);
}

#[test]
fn check_passes_when_ancestor_only_ignored() {
    // With the flag and no known-cyclic-files, the direct-only baseline is empty == empty known list.
    let cfg = fixture_config("cyclic_ancestor_only");
    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--check-cyclic-files",
        "--ignore-derived-ancestor-init",
    ]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "should exit 0: direct-only baseline empty == empty known list"
    );
    assert!(stderr.contains("unchanged"), "stderr should say unchanged");
}

#[test]
fn check_fails_ancestor_only_without_flag() {
    // Without the flag, the ancestor-only files appear in the baseline and fail the check.
    let cfg = fixture_config("cyclic_ancestor_only");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--check-cyclic-files"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        1,
        "should exit 1: ancestor-only files in baseline"
    );
    assert!(
        stderr.contains("+ src/alpha/__init__.py"),
        "stderr should list alpha/__init__.py as added"
    );
    assert!(
        stderr.contains("+ src/beta/helpers.py"),
        "stderr should list beta/helpers.py as added"
    );
}

#[test]
fn show_reflects_direct_only() {
    // --show-cyclic-files with the flag: cyclic_files key absent (empty -> omitted).
    let cfg = fixture_config("cyclic_ancestor_only");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--show-cyclic-files",
        "--ignore-derived-ancestor-init",
    ]);
    assert!(
        parsed.get("cyclic_files").is_none(),
        "cyclic_files should be absent when direct-only baseline is empty"
    );
}

#[test]
fn report_unaffected_by_option() {
    // The normal cycle report still shows the ancestor-init cycle even with the flag.
    // (Scope is baseline-only; the report path is untouched.)
    let cfg = fixture_config("cyclic_ancestor_only");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--ignore-derived-ancestor-init",
    ]);
    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        !cycles.is_empty(),
        "normal report should still show the ancestor-init cycle"
    );
    let all_files: Vec<&str> = cycles
        .iter()
        .flat_map(|c| c["files"].as_array().unwrap())
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert!(
        all_files
            .iter()
            .any(|p| p.contains("src/alpha/__init__.py")),
        "alpha/__init__.py should appear in the normal cycle report"
    );
}

#[test]
fn noop_with_no_include_ancestor_init() {
    // --no-include-ancestor-init + --ignore-derived-ancestor-init: no crash, empty baseline,
    // and a warning is printed to stderr.
    let cfg = fixture_config("cyclic_ancestor_only");
    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--dump-cyclic-files",
        "--no-include-ancestor-init",
        "--ignore-derived-ancestor-init",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("not valid JSON: {e}\nstdout: {stdout}"));
    let files = parsed["cyclic_files"].as_array().unwrap();
    assert!(
        files.is_empty(),
        "cyclic_files should be empty (no ancestor edges to strip)"
    );
    assert_eq!(output.status.code().unwrap(), 0);
    assert!(
        stderr.contains("has no effect when include-ancestor-init is disabled"),
        "should warn that the option is a no-op; stderr: {stderr}"
    );
}

#[test]
fn check_fails_on_stale_list_after_enabling_flag() {
    // Config lists both ancestor-only files (generated before the flag was enabled).
    // Enabling the flag makes the computed baseline empty, so both entries are stale.
    let cfg = fixture_config("cyclic_ancestor_only_listed");
    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--check-cyclic-files",
        "--ignore-derived-ancestor-init",
    ]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        1,
        "should exit 1: stale entries in known list"
    );
    assert!(
        stderr.contains("- src/alpha/__init__.py"),
        "stderr should show alpha/__init__.py as removed"
    );
    assert!(
        stderr.contains("- src/beta/helpers.py"),
        "stderr should show beta/helpers.py as removed"
    );
}

#[test]
fn dump_json_includes_ignore_flag_true() {
    let cfg = fixture_config("cyclic_ancestor_only");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--dump-cyclic-files",
        "--ignore-derived-ancestor-init",
    ]);
    assert_eq!(parsed["ignore_derived_ancestor_init"], true);
}

#[test]
fn dump_json_includes_ignore_flag_false_by_default() {
    let cfg = fixture_config("cyclic_ancestor_only");
    let parsed = run_json(&[
        "--config",
        cfg.to_str().unwrap(),
        "--format",
        "json",
        "--dump-cyclic-files",
    ]);
    assert_eq!(parsed["ignore_derived_ancestor_init"], false);
}

#[test]
fn dump_human_includes_ignore_flag_when_on() {
    // cyclic_direct_init has a genuine direct cycle, so the baseline is non-empty even with the flag.
    let cfg = fixture_config("cyclic_direct_init");
    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--dump-cyclic-files",
        "--ignore-derived-ancestor-init",
    ]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("ignore-derived-ancestor-init = true"),
        "human fragment should carry the flag; got:\n{stdout}"
    );
    assert!(
        stdout.contains("known-cyclic-files = ["),
        "should still list the direct cycle"
    );
    assert_eq!(output.status.code().unwrap(), 0);
}

#[test]
fn dump_human_omits_ignore_flag_by_default() {
    let cfg = fixture_config("cyclic_ancestor_only");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--dump-cyclic-files"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        !stdout.contains("ignore-derived-ancestor-init"),
        "flag line should be omitted at default; got:\n{stdout}"
    );
}

#[test]
fn dump_human_includes_ignore_flag_via_config() {
    // Config opt-in (no CLI flag) should also surface the flag in the human fragment.
    let cfg = fixture_config("cyclic_ancestor_only_optin");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--dump-cyclic-files"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("ignore-derived-ancestor-init = true"),
        "config opt-in should surface the flag; got:\n{stdout}"
    );
}
