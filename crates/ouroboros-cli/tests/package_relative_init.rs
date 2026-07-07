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
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/package_relative_init/oboros.toml")
}

#[test]
fn cycle_through_package_init_relative_import_is_reported() {
    let config = fixture_config();
    let output = Command::new(binary_path())
        .args(["--config", config.to_str().unwrap(), "--format", "json"])
        .output()
        .expect("failed to run oboros");

    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout should be valid JSON: {e}\nstdout: {stdout}"));

    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        !cycles.is_empty(),
        "cycle closing through a package __init__ relative import should be reported: {parsed}"
    );

    let touches_staff = cycles.iter().any(|cycle| {
        cycle["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["path"].as_str() == Some("svc/staff.py"))
    });
    assert!(
        touches_staff,
        "the reported cycle should include svc/staff.py: {parsed}"
    );
}
