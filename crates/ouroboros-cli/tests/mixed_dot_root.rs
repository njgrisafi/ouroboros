use std::path::PathBuf;
use std::process::Command;

fn mixed_dot_root_config() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mixed_dot_root/oboros.toml")
}

fn dot_root_flat_config() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dot_root_flat/oboros.toml")
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

fn cycle_file_paths(parsed: &serde_json::Value) -> Vec<String> {
    parsed["cycles"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|cycle| cycle["files"].as_array().unwrap())
        .map(|file| file["path"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn dot_root_files_appear_unprefixed() {
    // Given a project with source-roots = [".", "lib"] and a cross-root cycle,
    // When we detect cycles,
    // Then files under the "." root keep their project-root-relative path
    // with no source-root prefix.
    let parsed = run_json(&[
        "--config",
        mixed_dot_root_config().to_str().unwrap(),
        "--format",
        "json",
    ]);

    let paths = cycle_file_paths(&parsed);
    assert!(
        paths.contains(&"app.py".to_string()),
        "dot-root top-level file should appear unprefixed as 'app.py', got: {paths:?}"
    );
    assert!(
        paths.contains(&"models/user.py".to_string()),
        "dot-root nested file should appear as 'models/user.py', got: {paths:?}"
    );
}

#[test]
fn lib_root_files_keep_lib_prefix() {
    // Given the same [".", "lib"] project,
    // When we detect cycles,
    // Then files discovered under "lib" appear with their "lib/" prefix
    // intact (the prefix is only stripped for package grouping, not display).
    let parsed = run_json(&[
        "--config",
        mixed_dot_root_config().to_str().unwrap(),
        "--format",
        "json",
    ]);

    let paths = cycle_file_paths(&parsed);
    assert!(
        paths.contains(&"lib/utils.py".to_string()),
        "lib-root file should appear as 'lib/utils.py', got: {paths:?}"
    );
    assert!(
        !paths.contains(&"utils.py".to_string()),
        "lib-root file must not appear stripped to 'utils.py', got: {paths:?}"
    );
}

#[test]
fn mixed_dot_root_has_no_dot_slash_prefix() {
    // Given the [".", "lib"] project,
    // When we detect cycles,
    // Then no reported path is prefixed with "./" — dot-root paths are bare.
    let parsed = run_json(&[
        "--config",
        mixed_dot_root_config().to_str().unwrap(),
        "--format",
        "json",
    ]);

    for path in cycle_file_paths(&parsed) {
        assert!(
            !path.starts_with("./"),
            "no path should carry a './' prefix, got: {path}"
        );
    }
}

#[test]
fn package_filter_drops_dot_root_level_files() {
    // Given source-roots = ["."] where all files sit at the project root,
    // When we filter to intra-package cycles with --package,
    // Then every cycle is dropped because root-level files have no package
    // component.
    let without = run_json(&[
        "--config",
        dot_root_flat_config().to_str().unwrap(),
        "--format",
        "json",
    ]);
    assert!(
        !without["cycles"].as_array().unwrap().is_empty(),
        "dot-root fixture should have a root-level cycle without --package"
    );

    let with = run_json(&[
        "--config",
        dot_root_flat_config().to_str().unwrap(),
        "--format",
        "json",
        "--package",
    ]);
    assert!(
        with["cycles"].as_array().unwrap().is_empty(),
        "with --package, dot-root root-level files have no package and are dropped"
    );
}

#[test]
fn package_filter_dot_root_strict_exits_zero() {
    // Given the dot-root fixture whose only cycle is root-level,
    // When we run --package --strict,
    // Then the exit code is zero because the filtered cycle set is empty.
    let output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args([
            "--config",
            dot_root_flat_config().to_str().unwrap(),
            "--package",
            "--strict",
        ])
        .output()
        .expect("failed to run oboros");
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "--package --strict on a root-level-only dot-root fixture should exit 0"
    );
}
