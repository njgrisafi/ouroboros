use std::path::PathBuf;
use std::process::Command;

fn fixture_config(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}/oboros.toml"))
}

fn run_raw(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args(args)
        .output()
        .expect("failed to run oboros")
}

#[test]
fn cap_equal_to_count_exits_0() {
    let cfg = fixture_config("cyclic_basic");
    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--check-max-cycles",
        "--max-cycles",
        "1",
    ]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "should exit 0 when count equals cap"
    );
    assert!(
        stderr.contains("cycle count 1 within max-cycles 1"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn cap_below_count_exits_1() {
    let cfg = fixture_config("cyclic_basic");
    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--check-max-cycles",
        "--max-cycles",
        "0",
    ]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        1,
        "should exit 1 when count exceeds cap"
    );
    assert!(
        stderr.contains("cycle count 1 exceeds max-cycles 0"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn no_cap_anywhere_exits_2() {
    let cfg = fixture_config("cyclic_basic");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--check-max-cycles"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        2,
        "should exit 2 when no cap is set"
    );
    assert!(
        stderr.contains("--check-max-cycles requires"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn combined_with_check_cyclic_files_exits_2() {
    let cfg = fixture_config("cyclic_basic");
    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--check-max-cycles",
        "--check-cyclic-files",
        "--max-cycles",
        "1",
    ]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        2,
        "should exit 2 when both check modes are passed"
    );
    assert!(
        stderr.contains("cannot be combined with --check-cyclic-files"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn config_cap_is_used() {
    let cfg = fixture_config("cycle_cap");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--check-max-cycles"]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "should exit 0 when count is within the configured cap"
    );
    assert!(
        stderr.contains("cycle count 1 within max-cycles 1"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn cli_flag_overrides_config_cap() {
    let cfg = fixture_config("cycle_cap");
    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--check-max-cycles",
        "--max-cycles",
        "0",
    ]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        1,
        "CLI cap should override the configured cap"
    );
    assert!(
        stderr.contains("cycle count 1 exceeds max-cycles 0"),
        "unexpected stderr: {stderr}"
    );
}

#[test]
fn ignored_cycles_do_not_count_toward_budget() {
    // cyclic_ignore has one cycle (a <-> b) suppressed by a [[cycles.ignore]]
    // entry. The post-filter cycle count is 0, so a budget of 0 should pass.
    let cfg = fixture_config("cyclic_ignore");
    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--check-max-cycles",
        "--max-cycles",
        "0",
    ]);
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "ignored cycles should not count toward the budget"
    );
    assert!(
        stderr.contains("cycle count 0 within max-cycles 0"),
        "unexpected stderr: {stderr}"
    );
}
