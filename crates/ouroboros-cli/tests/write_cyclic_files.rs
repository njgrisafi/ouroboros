//! Integration tests for the `--write` flag.
//!
//! These run the real `oboros` binary against tempdir copies of committed
//! fixtures (never mutating the fixtures themselves) and assert on the
//! patched `oboros.toml` contents.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// Recursively copy a directory tree from `src` into `dst`.
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

/// Copy a committed fixture (config + Python sources) into a fresh tempdir so
/// tests can mutate `oboros.toml` without touching the repository.
fn copy_fixture_to_tempdir(name: &str) -> TempDir {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}"));
    let dir = TempDir::new().expect("create tempdir");
    copy_dir_all(&src, dir.path()).expect("copy fixture into tempdir");
    dir
}

fn run_raw(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args(args)
        .output()
        .expect("failed to run oboros")
}

// ---------------------------------------------------------------------------
// --dump-cyclic-files --write
// ---------------------------------------------------------------------------

#[test]
fn write_cyclic_files_patches_config() {
    let dir = copy_fixture_to_tempdir("cyclic_basic");
    let cfg = dir.path().join("oboros.toml");
    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--dump-cyclic-files",
        "--write",
    ]);
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "write should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let contents = fs::read_to_string(&cfg).expect("read patched config");
    assert!(
        contents.contains("known-cyclic-files"),
        "config should contain known-cyclic-files after write:\n{contents}"
    );
    assert!(
        contents.contains("\"src/app/a.py\""),
        "config should list src/app/a.py:\n{contents}"
    );
    assert!(
        contents.contains("\"src/app/b.py\""),
        "config should list src/app/b.py:\n{contents}"
    );
}

#[test]
fn write_cyclic_files_is_idempotent() {
    let dir = copy_fixture_to_tempdir("cyclic_basic");
    let cfg = dir.path().join("oboros.toml");
    let cfg_str = cfg.to_str().unwrap();

    let first = run_raw(&["--config", cfg_str, "--dump-cyclic-files", "--write"]);
    assert_eq!(
        first.status.code().unwrap(),
        0,
        "first write should exit 0; stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let after_first = fs::read_to_string(&cfg).expect("read config after first write");

    let second = run_raw(&["--config", cfg_str, "--dump-cyclic-files", "--write"]);
    assert_eq!(
        second.status.code().unwrap(),
        0,
        "second write should exit 0; stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let after_second = fs::read_to_string(&cfg).expect("read config after second write");

    assert_eq!(
        after_first, after_second,
        "second --write should not change file contents"
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("unchanged"),
        "second run stderr should say unchanged, got: {stderr}"
    );
}

#[test]
fn write_cyclic_files_preserves_comments() {
    // Copy the fixture for its Python sources, then overwrite oboros.toml with
    // a config that carries a comment above [cycles].
    let dir = copy_fixture_to_tempdir("cyclic_basic");
    let cfg = dir.path().join("oboros.toml");
    let custom = "source-roots = [\"src\"]\n\n\
# keep me: known cyclic files are tracked below\n\
[cycles]\n";
    fs::write(&cfg, custom).expect("write custom config with comment");

    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--dump-cyclic-files",
        "--write",
    ]);
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "write should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let contents = fs::read_to_string(&cfg).expect("read patched config");
    assert!(
        contents.contains("# keep me: known cyclic files are tracked below"),
        "comment above [cycles] should be preserved:\n{contents}"
    );
    assert!(
        contents.contains("\"src/app/a.py\""),
        "cyclic files should still be written:\n{contents}"
    );
}

// ---------------------------------------------------------------------------
// --dump-ignores --write
// ---------------------------------------------------------------------------

#[test]
fn write_ignores_appends_new_entries() {
    let dir = copy_fixture_to_tempdir("cyclic_basic");
    let cfg = dir.path().join("oboros.toml");
    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--dump-ignores",
        "--write",
    ]);
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "write should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let contents = fs::read_to_string(&cfg).expect("read patched config");
    assert!(
        contents.contains("[[cycles.ignore]]"),
        "should append a [[cycles.ignore]] entry:\n{contents}"
    );
    assert!(
        contents.contains("\"src/app/a.py\""),
        "ignore entry should list src/app/a.py:\n{contents}"
    );
    assert!(
        contents.contains("\"src/app/b.py\""),
        "ignore entry should list src/app/b.py:\n{contents}"
    );
}

#[test]
fn write_ignores_is_idempotent() {
    let dir = copy_fixture_to_tempdir("cyclic_basic");
    let cfg = dir.path().join("oboros.toml");
    let cfg_str = cfg.to_str().unwrap();

    let first = run_raw(&["--config", cfg_str, "--dump-ignores", "--write"]);
    assert_eq!(
        first.status.code().unwrap(),
        0,
        "first write should exit 0; stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let after_first = fs::read_to_string(&cfg).expect("read config after first write");

    let second = run_raw(&["--config", cfg_str, "--dump-ignores", "--write"]);
    assert_eq!(
        second.status.code().unwrap(),
        0,
        "second write should exit 0; stderr: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    let after_second = fs::read_to_string(&cfg).expect("read config after second write");

    assert_eq!(
        after_first, after_second,
        "second --dump-ignores --write should not change file contents"
    );
    let count = after_second.matches("[[cycles.ignore]]").count();
    assert_eq!(
        count, 1,
        "ignore entry should not be duplicated on a second write:\n{after_second}"
    );
    let stderr = String::from_utf8_lossy(&second.stderr);
    assert!(
        stderr.contains("unchanged") || stderr.contains("no new ignore"),
        "second run stderr should report no new entries, got: {stderr}"
    );
}

#[test]
fn write_ignores_preserves_existing_reason() {
    // Copy the fixture for its Python sources, then overwrite oboros.toml with
    // an existing ignore entry (matching the detected cycle) that has a reason.
    let dir = copy_fixture_to_tempdir("cyclic_basic");
    let cfg = dir.path().join("oboros.toml");
    let custom = "source-roots = [\"src\"]\n\n\
[[cycles.ignore]]\n\
files = [\"src/app/a.py\", \"src/app/b.py\"]\n\
reason = \"legacy\"\n";
    fs::write(&cfg, custom).expect("write custom config with existing ignore reason");

    let output = run_raw(&[
        "--config",
        cfg.to_str().unwrap(),
        "--dump-ignores",
        "--write",
    ]);
    assert_eq!(
        output.status.code().unwrap(),
        0,
        "write should exit 0; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let contents = fs::read_to_string(&cfg).expect("read patched config");
    assert!(
        contents.contains("reason = \"legacy\""),
        "existing ignore entry reason should be preserved:\n{contents}"
    );
    let count = contents.matches("[[cycles.ignore]]").count();
    assert_eq!(
        count, 1,
        "existing ignore entry (same file set) should not be duplicated:\n{contents}"
    );
}

// ---------------------------------------------------------------------------
// Validation errors
// ---------------------------------------------------------------------------

#[test]
fn write_requires_dump_action() {
    // --write without a dump flag is a usage error (validated before config load).
    let dir = copy_fixture_to_tempdir("cyclic_basic");
    let cfg = dir.path().join("oboros.toml");
    let output = run_raw(&["--config", cfg.to_str().unwrap(), "--write"]);
    assert_eq!(
        output.status.code().unwrap(),
        2,
        "--write with no dump action should exit 2"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--write requires"),
        "stderr should explain --write needs a dump flag, got: {stderr}"
    );
}

#[test]
fn write_requires_config_file() {
    // Run from an empty tempdir with no oboros.toml in its parent chain and no
    // --config, so config discovery finds nothing.
    let dir = TempDir::new().expect("create empty tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_oboros"))
        .args(["--dump-cyclic-files", "--write"])
        .current_dir(dir.path())
        .output()
        .expect("failed to run oboros");
    assert_eq!(
        output.status.code().unwrap(),
        2,
        "--write with no discoverable config should exit 2; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("oboros.toml") || stderr.contains("--write requires"),
        "stderr should explain missing config, got: {stderr}"
    );
}
