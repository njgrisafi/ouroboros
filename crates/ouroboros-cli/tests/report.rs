use std::fs;
use std::process::Command;

fn oboros_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oboros"))
}

#[test]
fn report_missing_input_exits_nonzero() {
    let output = oboros_bin()
        .args(["report", "nonexistent_file_that_does_not_exist.json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed to read"), "stderr was: {stderr}");
}

#[test]
fn report_invalid_json_exits_nonzero() {
    let dir = std::env::temp_dir();
    let input_path = dir.join("oboros_integration_invalid.json");
    fs::write(&input_path, "not json at all").unwrap();

    let output = oboros_bin()
        .args(["report", input_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to parse JSON report"),
        "stderr was: {stderr}"
    );
}

#[test]
fn report_inconsistent_json_exits_nonzero() {
    let dir = std::env::temp_dir();
    let input_path = dir.join("oboros_integration_inconsistent.json");
    fs::write(
        &input_path,
        r#"{"version":1,"summary":{"cycles_reported":2,"cycles_suppressed":0},"cycles":[{"index":1,"packages":["auth"],"size":2,"files":[{"path":"auth/a.py","import_lines":[],"edges":[]},{"path":"auth/b.py","import_lines":[],"edges":[]}]}]}"#,
    )
    .unwrap();

    let output = oboros_bin()
        .args(["report", input_path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid report summary"),
        "stderr was: {stderr}"
    );
}

#[test]
fn report_produces_html_file() {
    let dir = std::env::temp_dir();
    let input_path = dir.join("oboros_integration_valid.json");
    let output_path = dir.join("oboros_integration_report.html");

    let json = r#"{
        "version": 1,
        "summary": { "cycles_reported": 1, "cycles_suppressed": 0 },
        "cycles": [{
            "index": 1,
            "packages": ["auth"],
            "size": 2,
            "files": [
                { "path": "auth/a.py", "import_lines": [5], "edges": [{"to": "auth/b.py", "lines": [5]}] },
                { "path": "auth/b.py", "import_lines": [3], "edges": [{"to": "auth/a.py", "lines": [3]}] }
            ]
        }]
    }"#;
    fs::write(&input_path, json).unwrap();

    let status = oboros_bin()
        .args([
            "report",
            input_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let html = fs::read_to_string(&output_path).unwrap();
    assert!(html.contains("<!DOCTYPE html>"));
    assert!(html.contains("Circular Import Report"));
    assert!(html.contains("auth/a.py"));
}

#[test]
fn report_html_matches_cycle_count() {
    let dir = std::env::temp_dir();
    let input_path = dir.join("oboros_integration_count.json");
    let output_path = dir.join("oboros_integration_count.html");

    let json = r#"{
        "version": 1,
        "summary": { "cycles_reported": 2, "cycles_suppressed": 1 },
        "cycles": [
            {
                "index": 1, "packages": ["auth"], "size": 2,
                "files": [
                    { "path": "auth/a.py", "import_lines": [], "edges": [] },
                    { "path": "auth/b.py", "import_lines": [], "edges": [] }
                ]
            },
            {
                "index": 2, "packages": ["models"], "size": 2,
                "files": [
                    { "path": "models/x.py", "import_lines": [], "edges": [] },
                    { "path": "models/y.py", "import_lines": [], "edges": [] }
                ]
            }
        ]
    }"#;
    fs::write(&input_path, json).unwrap();

    let status = oboros_bin()
        .args([
            "report",
            input_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let html = fs::read_to_string(&output_path).unwrap();
    assert!(html.contains(">2<"));
    assert!(html.contains(">1<"));
    assert!(html.contains("auth"));
    assert!(html.contains("models"));
}

#[test]
fn report_custom_output_path() {
    let dir = std::env::temp_dir();
    let input_path = dir.join("oboros_integration_custom_in.json");
    let output_path = dir.join("oboros_integration_custom_out.html");

    let _ = fs::remove_file(&output_path);

    let json = r#"{"version":1,"summary":{"cycles_reported":0,"cycles_suppressed":0},"cycles":[]}"#;
    fs::write(&input_path, json).unwrap();

    let status = oboros_bin()
        .args([
            "report",
            input_path.to_str().unwrap(),
            "--output",
            output_path.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(output_path.exists());
}
