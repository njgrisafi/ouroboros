use std::path::{Path, PathBuf};
use std::process::Command;

fn binary_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("oboros")
}

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}"))
}

fn run_with_config(config: &Path, extra_args: &[&str]) -> std::process::Output {
    let mut args = vec!["--config", config.to_str().unwrap(), "--format", "json"];
    args.extend_from_slice(extra_args);

    Command::new(binary_path())
        .args(&args)
        .output()
        .expect("failed to run oboros")
}

fn run(fixture: &str, extra_args: &[&str]) -> serde_json::Value {
    let output = run_with_config(&fixture_dir(fixture).join("oboros.toml"), extra_args);
    let stdout = String::from_utf8(output.stdout).unwrap();
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout should be valid JSON: {e}\nstdout: {stdout}"))
}

/// Sorted list of cycle member file names (basename only) for each cycle.
fn cycle_file_sets(parsed: &serde_json::Value) -> Vec<Vec<String>> {
    let mut sets: Vec<Vec<String>> = parsed["cycles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cycle| {
            let mut files: Vec<String> = cycle["files"]
                .as_array()
                .unwrap()
                .iter()
                .map(|f| {
                    f["path"]
                        .as_str()
                        .unwrap()
                        .rsplit('/')
                        .next()
                        .unwrap()
                        .to_string()
                })
                .collect();
            files.sort();
            files
        })
        .collect();
    sets.sort();
    sets
}

#[test]
fn string_import_cycle_hidden_by_default() {
    let parsed = run("string_imports", &[]);
    assert!(
        parsed["cycles"].as_array().unwrap().is_empty(),
        "cycles closed only via string literals must be hidden by default: {parsed}"
    );
}

#[test]
fn string_import_cycle_reported_with_flag() {
    let parsed = run("string_imports", &["--include-string-imports"]);
    let sets = cycle_file_sets(&parsed);
    assert_eq!(
        sets,
        vec![vec!["a.py", "b.py"]],
        "module-level string import must close the a <-> b cycle: {parsed}"
    );
}

#[test]
fn local_imports_gates_nested_string_scanning() {
    // --include-string-imports alone: only module-level strings are scanned,
    // so c -> d exists but the d -> c return edge (nested in a function) does
    // not, and no c/d cycle forms.
    let strings_only = run("string_imports", &["--include-string-imports"]);
    assert_eq!(
        cycle_file_sets(&strings_only),
        vec![vec!["a.py", "b.py"]],
        "nested strings must not be scanned without --local-imports: {strings_only}"
    );

    // Both flags: nested strings are scanned too, closing the c <-> d cycle.
    let both = run(
        "string_imports",
        &["--include-string-imports", "--local-imports"],
    );
    assert_eq!(
        cycle_file_sets(&both),
        vec![vec!["a.py", "b.py"], vec!["c.py", "d.py"]],
        "with both flags the nested d -> c string must close the c <-> d cycle: {both}"
    );
}

#[test]
fn local_imports_alone_scans_no_strings() {
    let parsed = run("string_imports", &["--local-imports"]);
    assert!(
        parsed["cycles"].as_array().unwrap().is_empty(),
        "--local-imports without --include-string-imports must not scan strings: {parsed}"
    );
}

#[test]
fn self_string_does_not_form_cycle_at_min_scc_size_one() {
    // The fixture config sets min-scc-size = 1; c.py's SELF = "app.c" must be
    // dropped by the resolver rather than surfacing as a 1-file cycle.
    let parsed = run(
        "string_imports",
        &["--include-string-imports", "--local-imports"],
    );
    let sets = cycle_file_sets(&parsed);
    assert!(
        sets.iter().all(|set| set.len() > 1),
        "self-strings must never surface as 1-file cycles: {parsed}"
    );
    assert!(
        !sets.iter().any(|set| set == &vec!["c.py"]),
        "c.py must not be a singleton cycle: {parsed}"
    );
}

#[test]
fn min_dots_config_suppresses_low_dot_candidates() {
    // All fixture module paths have exactly two dots ("my.app.a"), so
    // min-dots 3 rejects every candidate even with the flag enabled.
    let output = run_with_config(
        &fixture_dir("string_imports").join("oboros.min-dots-3.toml"),
        &["--include-string-imports"],
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(
        parsed["cycles"].as_array().unwrap().is_empty(),
        "min-dots 3 must reject all one-dot candidates: {parsed}"
    );
}

#[test]
fn unresolvable_strings_do_not_trip_strict() {
    let output = run_with_config(
        &fixture_dir("string_noise").join("oboros.toml"),
        &["--include-string-imports", "--strict"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "unresolvable string candidates must be dropped silently, not reported as \
         unresolved imports that trip --strict; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn json_output_stable_across_runs() {
    let first = run_with_config(&fixture_dir("string_imports").join("oboros.toml"), &[]);
    let second = run_with_config(&fixture_dir("string_imports").join("oboros.toml"), &[]);
    assert_eq!(
        String::from_utf8(first.stdout).unwrap(),
        String::from_utf8(second.stdout).unwrap(),
        "default JSON output must be byte-identical across runs"
    );
}

#[test]
fn dump_cyclic_files_includes_string_only_cycle_members() {
    let parsed = run(
        "string_imports",
        &["--include-string-imports", "--dump-cyclic-files"],
    );
    let files: Vec<String> = parsed["cyclic_files"]
        .as_array()
        .expect("dump-cyclic-files JSON must have a cyclic_files array")
        .iter()
        .map(|f| f.as_str().unwrap().to_string())
        .collect();
    assert!(
        files.iter().any(|f| f.ends_with("my/app/a.py"))
            && files.iter().any(|f| f.ends_with("my/app/b.py")),
        "dump-cyclic-files must include members of string-closed cycles: {files:?}"
    );
}

#[test]
fn flag_overrides_config_default_false() {
    let config_only = run("string_imports", &[]);
    let flag = run("string_imports", &["--include-string-imports"]);
    assert!(
        config_only["cycles"].as_array().unwrap().is_empty()
            && !flag["cycles"].as_array().unwrap().is_empty(),
        "--include-string-imports must override the [parse] string-imports default of false"
    );
}

#[test]
fn verbose_prints_string_imports() {
    let args = vec![
        "--config".to_string(),
        fixture_dir("string_imports")
            .join("oboros.toml")
            .to_str()
            .unwrap()
            .to_string(),
        "--include-string-imports".to_string(),
        "--verbose".to_string(),
    ];
    let output = Command::new(binary_path())
        .args(&args)
        .output()
        .expect("failed to run oboros");
    let stdout = String::from_utf8(output.stdout).unwrap();
    // Match the exact verbose arm so this can't pass on incidental text like
    // the config dump ("string_imports: true") or module names ("my.app.a").
    assert!(
        stdout.contains("string  (my.app.a)"),
        "verbose import listing must render the string-import arm: {stdout}"
    );
}
