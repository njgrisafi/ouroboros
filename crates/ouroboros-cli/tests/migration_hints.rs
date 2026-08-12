//! Migration-hint and deprecation tests for the 0.6.0 project-root-relative
//! path change. These assert the CLI surfaces actionable hints when a config
//! still uses pre-0.6.0 source-root-relative paths, and that the deprecated
//! `report --source-root` alias still works while warning.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture_config(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}/oboros.toml"))
}

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}"))
}

fn run_raw(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args(args)
        .output()
        .expect("failed to run oboros")
}

/// A `[[cycles.ignore]]` entry using pre-0.6.0 bare `app/`-prefixed paths does
/// NOT match the project-root-relative cycle, so the cycle stays visible AND
/// the migration hint fires.
#[test]
fn ignore_migration_hint() {
    let cfg = fixture_config("ignore_migration_hint");
    let output = run_raw(&["--config", cfg.to_str().unwrap()]);

    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();

    // (a) emits the "looks pre-0.6.0" hint to stderr
    assert!(
        stderr.contains("looks pre-0.6.0"),
        "stderr should contain the pre-0.6.0 migration hint; stderr was: {stderr}"
    );

    // (b) does NOT suppress the cycle — it still appears in output
    assert!(
        stdout.contains("dependency cycles"),
        "cycle section should still be reported; stdout was: {stdout}"
    );
    assert!(
        stdout.contains("src/app/a.py") && stdout.contains("src/app/b.py"),
        "the cycle files should still be listed (not suppressed); stdout was: {stdout}"
    );
}

/// `--check-cyclic-files` with a pre-0.6.0 bare `cyclic-files` list fails
/// (exit 1) and emits the regeneration hint.
#[test]
fn check_cyclic_files_migration_hint() {
    let cfg = fixture_config("cyclic_migration_hint");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--check-cyclic-files"]);

    let stderr = String::from_utf8(output.stderr).unwrap();

    // (a) exits 1
    assert_eq!(
        output.status.code().unwrap(),
        1,
        "should exit 1 when cyclic-files uses pre-0.6.0 paths; stderr was: {stderr}"
    );

    // (b) emits the "cyclic-files uses pre-0.6.0" hint
    assert!(
        stderr.contains("cyclic-files uses pre-0.6.0"),
        "stderr should contain the cyclic-files migration hint; stderr was: {stderr}"
    );
}

/// `--trace app/entry.py` (pre-0.6.0 source-root-relative form) against a
/// fixture whose node is `src/app/entry.py` matches nothing, exits 0, and
/// suggests the project-root-relative form.
#[test]
fn trace_pre_060_path_suggests_src_prefix() {
    let cfg = fixture_config("impact_branch");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--trace", "app/entry.py"]);

    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(
        output.status.code().unwrap(),
        0,
        "trace with no match and no --strict should exit 0; stderr was: {stderr}"
    );
    assert!(
        stderr.contains("did you mean 'src/app/entry.py'"),
        "stderr should suggest the src/-prefixed path; stderr was: {stderr}"
    );
}

/// `report --source-root <dir>` still works (exit 0) but emits a deprecation
/// warning pointing at `--root`.
#[test]
fn report_source_root_is_deprecated() {
    let dir = std::env::temp_dir();
    let input_path = dir.join("oboros_migration_report_in.json");
    let output_path = dir.join("oboros_migration_report_out.html");
    let _ = fs::remove_file(&output_path);

    let json = r#"{"version":2,"summary":{"cycles_reported":0,"cycles_suppressed":0},"cycles":[]}"#;
    fs::write(&input_path, json).unwrap();

    let root = fixture_dir("cyclic_basic");
    let output = run_raw(&[
        "report",
        "--source-root",
        root.to_str().unwrap(),
        "--output",
        output_path.to_str().unwrap(),
        input_path.to_str().unwrap(),
    ]);

    let stderr = String::from_utf8(output.stderr).unwrap();

    assert_eq!(
        output.status.code().unwrap(),
        0,
        "report --source-root should still succeed; stderr was: {stderr}"
    );
    assert!(
        stderr.contains("deprecated") && stderr.contains("--source-root"),
        "stderr should warn that --source-root is deprecated; stderr was: {stderr}"
    );
}
