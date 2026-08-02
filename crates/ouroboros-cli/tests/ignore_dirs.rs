//! Integration coverage for `[cycles] ignore-dirs`.
//!
//! Two fixtures under `tests/fixtures/`:
//!
//! `ignore_dirs_basic/` — `source-roots = ["app"]`, `ignore-dirs = ["app/protos/"]`:
//!
//! ```text
//! app/foo.py                  -> from protos import z   (cross-boundary cycle)
//! app/protos/z/__init__.py    -> import foo             (cross-boundary cycle)
//! app/protos/x/__init__.py    -> from protos import y   (intra-protos cycle)
//! app/protos/y/__init__.py    -> from protos import x   (intra-protos cycle)
//! app/protos/__init__.py      (empty ancestor)
//! ```
//!
//! The `foo <-> protos/z` cycle straddles the ignored dir (foo is outside) and
//! is reported; the `protos/x <-> protos/y` cycle lives entirely under
//! `app/protos/` and is suppressed. `oboros-no-ignore.toml` drops the
//! ignore-dirs line so both cycles surface (control).
//!
//! `ignore_dirs_strict/` — only the `protos/x <-> protos/y` intra-dir cycle,
//! for the `--strict` exit-code checks.

use std::path::PathBuf;
use std::process::Command;

fn fixture(name: &str, file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}/{file}"))
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

fn all_cycle_files(parsed: &serde_json::Value) -> Vec<String> {
    parsed["cycles"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|cycle| {
            cycle["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| f["path"].as_str().unwrap().to_string())
        })
        .collect()
}

// ── (a) intra-dir cycle vanishes from the JSON cycles[] ───────────────────────

#[test]
fn intra_dir_cycle_suppressed_from_json_cycles() {
    // Given a project whose protos/x <-> protos/y cycle is fully under an
    // ignore-dirs entry, When we render the JSON report, Then that cycle's
    // files are absent from cycles[] and it is counted as suppressed.
    let cfg = fixture("ignore_dirs_basic", "oboros.toml");
    let parsed = run_json(&["--config", cfg.to_str().unwrap(), "--format", "json"]);

    let files = all_cycle_files(&parsed);
    assert!(
        !files.iter().any(|p| p == "app/protos/x/__init__.py"),
        "intra-protos file x must not appear in reported cycles, got: {files:?}"
    );
    assert!(
        !files.iter().any(|p| p == "app/protos/y/__init__.py"),
        "intra-protos file y must not appear in reported cycles, got: {files:?}"
    );
    assert_eq!(
        parsed["summary"]["cycles_reported"].as_u64().unwrap(),
        1,
        "only the cross-boundary cycle is reported"
    );
    assert_eq!(
        parsed["summary"]["cycles_suppressed"].as_u64().unwrap(),
        1,
        "the dir-ignored cycle flows into cycles_suppressed"
    );
}

// ── (b) cross-boundary cycle is still reported ────────────────────────────────

#[test]
fn cross_boundary_cycle_still_reported() {
    // Given the foo <-> protos/z cycle has one file outside app/protos/,
    // When we render the JSON report, Then it survives ignore-dirs.
    let cfg = fixture("ignore_dirs_basic", "oboros.toml");
    let parsed = run_json(&["--config", cfg.to_str().unwrap(), "--format", "json"]);

    let cycles = parsed["cycles"].as_array().unwrap();
    assert_eq!(cycles.len(), 1, "exactly the cross-boundary cycle remains");

    let files = all_cycle_files(&parsed);
    assert!(
        files.iter().any(|p| p == "app/foo.py"),
        "cross-boundary cycle must include app/foo.py, got: {files:?}"
    );
    assert!(
        files.iter().any(|p| p == "app/protos/z/__init__.py"),
        "cross-boundary cycle must include app/protos/z/__init__.py, got: {files:?}"
    );
}

// ── (c) --strict passes when only intra-dir cycles exist ──────────────────────

#[test]
fn strict_exits_zero_when_only_intra_dir_cycles() {
    // Given a project whose sole cycle is fully under an ignore-dirs entry,
    // When we run --strict, Then it exits 0.
    let cfg = fixture("ignore_dirs_strict", "oboros.toml");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--strict"]);
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "strict should pass when the only cycle is dir-ignored"
    );
}

#[test]
fn human_output_reports_dir_ignored_cycles_without_ignore_list_suppressions() {
    // Given a project whose sole cycle is fully under an ignore-dirs entry,
    // When we render the human report, Then it reports ignore-dirs and does
    // not claim any [[cycles.ignore]] suppression.
    let cfg = fixture("ignore_dirs_strict", "oboros.toml");
    let output = run_raw(&["--config", cfg.to_str().unwrap()]);
    let stdout = String::from_utf8(output.stdout).unwrap();

    assert!(
        output.status.success(),
        "human report should succeed for dir-ignored-only fixture\nstdout: {stdout}"
    );
    assert!(
        stdout.contains("ignored by ignore-dirs"),
        "human report must mention ignore-dirs suppression\nstdout: {stdout}"
    );
    assert!(
        !stdout.contains("suppressed by ignore list"),
        "human report must not claim ignore-list suppression when only ignore-dirs matched\nstdout: {stdout}"
    );
}

// ── (d) --dump-cyclic-files omits the suppressed intra-dir files ──────────────

#[test]
fn dump_cyclic_files_omits_dir_ignored_files() {
    // Given the intra-protos cycle is dir-ignored, When we dump the cyclic-file
    // baseline, Then x and y are absent while the kept cross-boundary members
    // remain.
    let cfg = fixture("ignore_dirs_basic", "oboros.toml");
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
    assert_eq!(
        paths,
        vec!["app/foo.py", "app/protos/z/__init__.py"],
        "baseline must exclude the dir-ignored x/y files"
    );
}

// ── (e) control: without ignore-dirs the intra-dir cycle IS reported ──────────

#[test]
fn without_ignore_dirs_intra_dir_cycle_is_reported() {
    // Given the same project with the ignore-dirs line removed, When we render
    // the JSON report, Then both cycles surface and nothing is suppressed.
    let cfg = fixture("ignore_dirs_basic", "oboros-no-ignore.toml");
    let parsed = run_json(&["--config", cfg.to_str().unwrap(), "--format", "json"]);

    assert_eq!(
        parsed["cycles"].as_array().unwrap().len(),
        2,
        "both cycles are reported without ignore-dirs"
    );
    assert_eq!(
        parsed["summary"]["cycles_suppressed"].as_u64().unwrap(),
        0,
        "nothing is suppressed without ignore-dirs"
    );

    let files = all_cycle_files(&parsed);
    assert!(
        files.iter().any(|p| p == "app/protos/x/__init__.py")
            && files.iter().any(|p| p == "app/protos/y/__init__.py"),
        "the intra-protos cycle must be reported without ignore-dirs, got: {files:?}"
    );
}

#[test]
fn without_ignore_dirs_strict_fails_on_intra_dir_cycle() {
    // Given the strict fixture with ignore-dirs removed, When we run --strict,
    // Then the intra-dir cycle causes exit 1.
    let cfg = fixture("ignore_dirs_strict", "oboros-no-ignore.toml");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--strict"]);
    assert_eq!(
        output.status.code().unwrap(),
        1,
        "strict should fail on the intra-dir cycle when ignore-dirs is absent"
    );
}
