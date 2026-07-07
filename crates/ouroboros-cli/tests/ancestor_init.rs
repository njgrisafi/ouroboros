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

fn fixture_config() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ancestor_init/oboros.toml")
}

fn run(extra_args: &[&str]) -> serde_json::Value {
    let config = fixture_config();
    let mut args = vec!["--config", config.to_str().unwrap(), "--format", "json"];
    args.extend_from_slice(extra_args);

    let output = Command::new(binary_path())
        .args(&args)
        .output()
        .expect("failed to run oboros");

    let stdout = String::from_utf8(output.stdout).unwrap();
    serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("stdout should be valid JSON: {e}\nstdout: {stdout}"))
}

#[test]
fn cycle_through_ancestor_init_is_reported_by_default() {
    let parsed = run(&[]);
    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        !cycles.is_empty(),
        "cycle closing through an eager parent __init__.py should be reported by default: {parsed}"
    );
}

#[test]
fn cycle_through_ancestor_init_is_hidden_when_disabled() {
    let parsed = run(&["--no-include-ancestor-init"]);
    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        cycles.is_empty(),
        "without ancestor __init__ edges the fixture has no cycle: {parsed}"
    );
}
