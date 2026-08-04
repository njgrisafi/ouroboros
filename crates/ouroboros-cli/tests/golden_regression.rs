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

fn run(args: &[&str]) -> String {
    let out = Command::new(binary_path())
        .args(args)
        .output()
        .expect("failed to run oboros");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn non_lazy_output_unchanged() {
    let config =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cyclic_basic/oboros.toml");
    let config = config.to_str().unwrap();

    let out1 = run(&["--config", config, "--format", "json"]);
    let out2 = run(&["--config", config, "--format", "json"]);
    assert_eq!(out1, out2, "non-lazy output must be deterministic");

    let parsed: serde_json::Value = serde_json::from_str(&out1).unwrap();
    assert_eq!(parsed["version"], 2);
    assert!(
        parsed.get("analysis").is_none(),
        "non-lazy report must not carry an analysis field"
    );
}

#[test]
fn check_lazy_output_is_valid_and_tagged() {
    let config =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cyclic_basic/oboros.toml");
    let config = config.to_str().unwrap();

    let lazy_out = run(&["--config", config, "--format", "json", "--check-lazy"]);
    let lazy_json: serde_json::Value = serde_json::from_str(&lazy_out).unwrap();
    assert_eq!(lazy_json["version"], 2);
    assert_eq!(lazy_json["analysis"], "lazy");
}
