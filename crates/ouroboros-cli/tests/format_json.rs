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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny/oboros.toml")
}

#[test]
fn json_output_is_valid_json() {
    let output = Command::new(binary_path())
        .args([
            "--config",
            fixture_config().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run oboros");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let _parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout is not valid JSON: {e}\nstdout was: {stdout}"));
}

#[test]
fn json_output_has_correct_structure() {
    let output = Command::new(binary_path())
        .args([
            "--config",
            fixture_config().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run oboros");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(parsed["version"], 1, "version should be 1");
    assert!(parsed["summary"].is_object(), "summary should be an object");
    assert!(parsed["cycles"].is_array(), "cycles should be an array");

    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        !cycles.is_empty(),
        "fixture has cycles, array should be non-empty"
    );

    for cycle in cycles {
        assert!(
            cycle["packages"].is_array(),
            "every cycle should have a packages array"
        );
        assert!(cycle["index"].is_u64(), "every cycle should have an index");
        assert!(cycle["size"].is_u64(), "every cycle should have a size");
    }
}

#[test]
fn json_format_suppresses_verbose_output() {
    let output = Command::new(binary_path())
        .args([
            "--config",
            fixture_config().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run oboros");

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        !stdout.contains("--- imports ---"),
        "verbose imports section should be suppressed in json format"
    );
    assert!(
        !stdout.contains("--- dependency graph ---"),
        "verbose graph section should be suppressed in json format"
    );
}

#[test]
fn human_format_is_default() {
    let output = Command::new(binary_path())
        .args(["--config", fixture_config().to_str().unwrap()])
        .output()
        .expect("failed to run oboros");

    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("dependency cycles"),
        "human format should show cycle section header\nstdout: {stdout}"
    );
}

#[test]
fn strict_with_json_exits_nonzero() {
    let output = Command::new(binary_path())
        .args([
            "--config",
            fixture_config().to_str().unwrap(),
            "--format",
            "json",
            "--strict",
        ])
        .output()
        .expect("failed to run oboros");

    assert_ne!(
        output.status.code().unwrap(),
        0,
        "--strict with cycles should exit nonzero"
    );

    let stdout = String::from_utf8(output.stdout).unwrap();
    let _parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout should still be valid JSON on nonzero exit: {e}"));
}

#[test]
fn dump_ignores_json() {
    let output = Command::new(binary_path())
        .args([
            "--config",
            fixture_config().to_str().unwrap(),
            "--format",
            "json",
            "--dump-ignores",
        ])
        .output()
        .expect("failed to run oboros");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!("dump-ignores json output not valid JSON: {e}\nstdout: {stdout}")
    });

    assert_eq!(parsed["version"], 1);
    assert!(
        parsed["ignore_entries"].is_array(),
        "should have ignore_entries array"
    );
}
