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
fn deferred_import_cycle_hidden_by_default() {
    let parsed = run("local_imports", &[]);
    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        cycles.is_empty(),
        "cycle closed by a deferred (local) import must be hidden by default: {parsed}"
    );
}

#[test]
fn deferred_import_cycle_reported_with_flag() {
    let parsed = run("local_imports", &["--local-imports"]);
    let cycles = parsed["cycles"].as_array().unwrap();
    assert_eq!(
        cycles.len(),
        1,
        "--local-imports must surface the deferred-import cycle: {parsed}"
    );

    let files: Vec<String> = cycles[0]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap().to_string())
        .collect();
    assert!(
        files.iter().any(|f| f.ends_with("a.py")),
        "cycle must include a.py: {files:?}"
    );
    assert!(
        files.iter().any(|f| f.ends_with("b.py")),
        "cycle must include b.py: {files:?}"
    );
}

#[test]
fn flag_overrides_config_default_false() {
    let config_only = run("local_imports", &[]);
    let flag = run("local_imports", &["--local-imports"]);
    assert!(
        config_only["cycles"].as_array().unwrap().is_empty()
            && !flag["cycles"].as_array().unwrap().is_empty(),
        "--local-imports must override the [parse] local-imports config default of false"
    );
}
