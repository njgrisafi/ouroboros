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

fn tiny_config() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny/oboros.toml")
}

fn multi_pkg_config() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/multi_pkg/oboros.toml")
}

#[test]
fn packages_field_always_present() {
    let output = Command::new(binary_path())
        .args([
            "--config",
            tiny_config().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let cycles = parsed["cycles"].as_array().unwrap();
    for cycle in cycles {
        assert!(
            cycle["packages"].is_array(),
            "every cycle should have packages array"
        );
    }
}

#[test]
fn cycles_sorted_by_package_then_size() {
    let output = Command::new(binary_path())
        .args([
            "--config",
            multi_pkg_config().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let cycles = parsed["cycles"].as_array().unwrap();
    for (i, cycle) in cycles.iter().enumerate() {
        assert_eq!(cycle["index"].as_u64().unwrap(), (i + 1) as u64);
    }
    assert_eq!(
        cycles[0]["packages"].as_array().unwrap().len(),
        1,
        "first cycle should be intra-package (single package)"
    );
}

#[test]
fn package_flag_filters_to_intra_package() {
    let output = Command::new(binary_path())
        .args([
            "--config",
            multi_pkg_config().to_str().unwrap(),
            "--format",
            "json",
            "--package",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        !cycles.is_empty(),
        "multi_pkg fixture should have intra-package cycles"
    );
    for cycle in cycles {
        let pkgs = cycle["packages"].as_array().unwrap();
        assert_eq!(
            pkgs.len(),
            1,
            "with --package, every cycle should have exactly 1 package"
        );
    }
}

#[test]
fn package_flag_reduces_cycle_count() {
    let without = Command::new(binary_path())
        .args([
            "--config",
            multi_pkg_config().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    let with = Command::new(binary_path())
        .args([
            "--config",
            multi_pkg_config().to_str().unwrap(),
            "--format",
            "json",
            "--package",
        ])
        .output()
        .unwrap();
    let without_parsed: serde_json::Value =
        serde_json::from_str(&String::from_utf8(without.stdout).unwrap()).unwrap();
    let with_parsed: serde_json::Value =
        serde_json::from_str(&String::from_utf8(with.stdout).unwrap()).unwrap();
    let without_count = without_parsed["cycles"].as_array().unwrap().len();
    let with_count = with_parsed["cycles"].as_array().unwrap().len();
    assert!(
        with_count < without_count,
        "package filter should reduce cycles: {with_count} < {without_count}"
    );
}

#[test]
fn package_flag_human_output_shows_filter_notice() {
    let output = Command::new(binary_path())
        .args([
            "--config",
            multi_pkg_config().to_str().unwrap(),
            "--package",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("filtered to intra-package cycles"),
        "human output should note package filtering\nstdout: {stdout}"
    );
}

#[test]
fn no_package_scoped_field_in_summary() {
    let output = Command::new(binary_path())
        .args([
            "--config",
            multi_pkg_config().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(
        parsed["summary"].get("package_scoped").is_none(),
        "summary should not have package_scoped field"
    );
}

#[test]
fn package_flag_on_root_level_fixture_filters_all() {
    let output = Command::new(binary_path())
        .args([
            "--config",
            tiny_config().to_str().unwrap(),
            "--format",
            "json",
            "--package",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let cycles = parsed["cycles"].as_array().unwrap();
    assert!(
        cycles.is_empty(),
        "tiny fixture has only root-level files, --package should filter all cycles"
    );
}

#[test]
fn package_flag_with_strict_exits_zero_when_filtered_empty() {
    let output = Command::new(binary_path())
        .args([
            "--config",
            tiny_config().to_str().unwrap(),
            "--package",
            "--strict",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "with --package --strict on root-level-only fixture, should exit 0"
    );
}
