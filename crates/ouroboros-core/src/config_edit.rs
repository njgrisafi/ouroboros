//! In-place surgical patching of `oboros.toml`.
//!
//! This module edits an `oboros.toml` file with [`toml_edit`], preserving
//! comments, key ordering, and unrelated formatting. It backs the `--write`
//! feature, which persists discovered cyclic files and ignored cycles back
//! into the user's config without rewriting the whole document.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

/// Errors that can occur while patching a config file.
#[derive(Debug)]
pub enum PatchError {
    /// Reading or writing the config file failed.
    Io(std::io::Error),
    /// The existing config file was not valid TOML.
    TomlParse(toml_edit::TomlError),
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatchError::Io(e) => write!(f, "config file I/O error: {e}"),
            PatchError::TomlParse(e) => write!(f, "config parse error: {e}"),
        }
    }
}

impl std::error::Error for PatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PatchError::Io(e) => Some(e),
            PatchError::TomlParse(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for PatchError {
    fn from(e: std::io::Error) -> Self {
        PatchError::Io(e)
    }
}

impl From<toml_edit::TomlError> for PatchError {
    fn from(e: toml_edit::TomlError) -> Self {
        PatchError::TomlParse(e)
    }
}

/// Outcome of a [`patch_config_file`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchResult {
    /// `true` if the file was modified and written back to disk.
    pub changed: bool,
    /// The path that was patched.
    pub path: PathBuf,
}

/// Normalize a path string for storage in the config: backslashes become
/// forward slashes so entries are portable across platforms.
fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// Return the canonical, sorted key for a cycle's file list. Two cycles are
/// considered equal iff their sorted, normalized file lists match — mirroring
/// the set-based comparison used by `cycles::filter::filter_ignored_cycles`.
fn cycle_key(files: &[String]) -> Vec<String> {
    let mut key: Vec<String> = files.iter().map(|f| normalize_path(f)).collect();
    key.sort();
    key
}

/// Ensure `doc["cycles"]` exists and is a table, returning a mutable reference
/// to it. Only replaces the value when it is missing or not already a table,
/// so an existing `[cycles]` table (with its comments and keys) is preserved.
fn cycles_table_mut(doc: &mut DocumentMut) -> Option<&mut Table> {
    match doc.get("cycles") {
        Some(item) if item.is_table() => {}
        _ => {
            doc["cycles"] = Item::Table(Table::new());
        }
    }
    doc.get_mut("cycles").and_then(Item::as_table_mut)
}

/// Replace the `known-cyclic-files` array under `[cycles]`, creating the
/// `[cycles]` table if it is missing.
///
/// Paths are normalized (backslashes to forward slashes) and sorted, then
/// written as a multiline array with one quoted path per line and a trailing
/// comma:
///
/// ```toml
/// known-cyclic-files = [
///     "src/app/a.py",
///     "src/app/b.py",
/// ]
/// ```
///
/// An empty `paths` slice writes an inline empty array (`[]`). All other keys
/// and comments in the document are preserved.
pub fn set_known_cyclic_files(doc: &mut DocumentMut, paths: &[String]) {
    let mut normalized: Vec<String> = paths.iter().map(|p| normalize_path(p)).collect();
    normalized.sort();

    let mut arr = Array::new();
    for path in &normalized {
        arr.push(path.as_str());
    }

    if !normalized.is_empty() {
        // Place each element on its own 4-space-indented line, and close the
        // bracket on a fresh line after a trailing comma.
        for item in arr.iter_mut() {
            item.decor_mut().set_prefix("\n    ");
        }
        arr.set_trailing("\n");
        arr.set_trailing_comma(true);
    }

    let Some(cycles) = cycles_table_mut(doc) else {
        return;
    };
    cycles["known-cyclic-files"] = value(arr);
}

/// Collect the set of file-list keys for the existing `[[cycles.ignore]]`
/// entries in the document.
fn existing_ignore_keys(doc: &DocumentMut) -> HashSet<Vec<String>> {
    let mut keys = HashSet::new();
    let Some(ignore) = doc
        .get("cycles")
        .and_then(Item::as_table)
        .and_then(|t| t.get("ignore"))
        .and_then(Item::as_array_of_tables)
    else {
        return keys;
    };

    for table in ignore.iter() {
        let Some(files) = table.get("files").and_then(Item::as_array) else {
            continue;
        };
        let collected: Vec<String> = files
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        keys.insert(cycle_key(&collected));
    }
    keys
}

/// Ensure `[cycles].ignore` exists as an array of tables, returning a mutable
/// reference to it. Preserves an existing array of tables (and thus every
/// entry, including `reason` fields).
fn ignore_array_mut(doc: &mut DocumentMut) -> Option<&mut ArrayOfTables> {
    let cycles = cycles_table_mut(doc)?;
    match cycles.get("ignore") {
        Some(item) if item.is_array_of_tables() => {}
        _ => {
            cycles["ignore"] = Item::ArrayOfTables(ArrayOfTables::new());
        }
    }
    cycles
        .get_mut("ignore")
        .and_then(Item::as_array_of_tables_mut)
}

/// Append new `[[cycles.ignore]]` entries for each cycle in `new_cycles` that
/// is not already present in the document.
///
/// Cycles are compared by their sorted, normalized file lists (the same
/// set-based comparison used elsewhere for ignore matching), so entry order
/// within a cycle does not matter. Existing entries — including their `reason`
/// fields — are left untouched. The `[cycles]` table and the
/// `[[cycles.ignore]]` array are created on demand.
///
/// This function is idempotent: calling it again with the same `new_cycles`
/// adds nothing further. Each appended entry's `files` list is written sorted.
pub fn merge_ignored_cycles(doc: &mut DocumentMut, new_cycles: &[Vec<String>]) {
    // Seed the seen set with existing entries, then extend it as we stage new
    // ones so duplicates within `new_cycles` are also collapsed.
    let mut seen = existing_ignore_keys(doc);
    let mut to_add: Vec<Vec<String>> = Vec::new();
    for cycle in new_cycles {
        let key = cycle_key(cycle);
        if seen.insert(key.clone()) {
            to_add.push(key);
        }
    }

    if to_add.is_empty() {
        return;
    }

    // Sort new entries deterministically by their first file path so the
    // append order is stable regardless of cycle-detection traversal order.
    to_add.sort();

    let Some(ignore) = ignore_array_mut(doc) else {
        return;
    };

    for files in to_add {
        let mut table = Table::new();
        let mut arr = Array::new();
        for file in &files {
            arr.push(file.as_str());
        }
        table["files"] = value(arr);
        ignore.push(table);
    }
}

/// Read the TOML file at `path`, apply `edit_fn` to the parsed document, and
/// write the result back **only if** the serialized content changed.
///
/// The comparison is made between the document serialized before and after
/// `edit_fn` runs, so formatting quirks that `toml_edit` normalizes on parse
/// do not by themselves trigger a rewrite — the file is only touched when the
/// edit produced a real change.
///
/// Returns a [`PatchResult`] describing whether the file was rewritten.
pub fn patch_config_file<F>(path: &Path, edit_fn: F) -> Result<PatchResult, PatchError>
where
    F: FnOnce(&mut DocumentMut),
{
    let content = std::fs::read_to_string(path)?;
    let mut doc: DocumentMut = content.parse()?;

    let before = doc.to_string();
    edit_fn(&mut doc);
    let after = doc.to_string();

    if before == after {
        return Ok(PatchResult {
            changed: false,
            path: path.to_path_buf(),
        });
    }

    std::fs::write(path, after)?;
    Ok(PatchResult {
        changed: true,
        path: path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn parse(s: &str) -> DocumentMut {
        s.parse().expect("valid toml")
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // --- set_known_cyclic_files ---

    #[test]
    fn set_known_cyclic_files_creates_cycles_table_if_missing() {
        let mut doc = parse("source-roots = [\"src\"]\n");
        set_known_cyclic_files(&mut doc, &strings(&["pkg/a.py", "pkg/b.py"]));
        let out = doc.to_string();
        assert!(out.contains("[cycles]"), "missing [cycles] header: {out}");
        assert!(out.contains("known-cyclic-files"), "missing key: {out}");
        assert!(out.contains("\"pkg/a.py\""));
        assert!(out.contains("\"pkg/b.py\""));
    }

    #[test]
    fn set_known_cyclic_files_replaces_existing_array() {
        let mut doc = parse(
            "source-roots = [\"src\"]\n\n[cycles]\nknown-cyclic-files = [\"old/x.py\", \"old/y.py\"]\n",
        );
        set_known_cyclic_files(&mut doc, &strings(&["new/a.py"]));
        let out = doc.to_string();
        assert!(!out.contains("old/x.py"), "old entry not removed: {out}");
        assert!(!out.contains("old/y.py"), "old entry not removed: {out}");
        assert!(out.contains("\"new/a.py\""), "new entry missing: {out}");
    }

    #[test]
    fn set_known_cyclic_files_preserves_comments_and_other_keys() {
        let input = "\
# top-level comment
source-roots = [\"src\"]

[cycles]
min-scc-size = 3 # keep this comment
";
        let mut doc = parse(input);
        set_known_cyclic_files(&mut doc, &strings(&["pkg/a.py"]));
        let out = doc.to_string();
        assert!(out.contains("# top-level comment"), "lost top comment: {out}");
        assert!(out.contains("source-roots = [\"src\"]"), "lost roots: {out}");
        assert!(out.contains("min-scc-size = 3"), "lost min-scc-size: {out}");
        assert!(out.contains("# keep this comment"), "lost inline comment: {out}");
        assert!(out.contains("known-cyclic-files"), "missing new key: {out}");
    }

    #[test]
    fn set_known_cyclic_files_normalizes_backslashes() {
        let mut doc = parse("source-roots = [\"src\"]\n");
        set_known_cyclic_files(&mut doc, &strings(&["pkg\\a.py", "pkg\\sub\\b.py"]));
        let out = doc.to_string();
        assert!(out.contains("\"pkg/a.py\""), "backslash not normalized: {out}");
        assert!(out.contains("\"pkg/sub/b.py\""), "backslash not normalized: {out}");
        assert!(!out.contains('\\'), "backslash remains: {out}");
    }

    #[test]
    fn set_known_cyclic_files_sorts_paths() {
        let mut doc = parse("source-roots = [\"src\"]\n");
        set_known_cyclic_files(&mut doc, &strings(&["c.py", "a.py", "b.py"]));
        let out = doc.to_string();
        let a = out.find("\"a.py\"").expect("a present");
        let b = out.find("\"b.py\"").expect("b present");
        let c = out.find("\"c.py\"").expect("c present");
        assert!(a < b && b < c, "paths not sorted: {out}");
    }

    #[test]
    fn set_known_cyclic_files_writes_multiline_format() {
        let mut doc = parse("source-roots = [\"src\"]\n");
        set_known_cyclic_files(&mut doc, &strings(&["a.py", "b.py"]));
        let out = doc.to_string();
        assert!(
            out.contains("known-cyclic-files = [\n    \"a.py\",\n    \"b.py\",\n]"),
            "unexpected multiline format: {out}"
        );
    }

    #[test]
    fn set_known_cyclic_files_roundtrips_via_config() {
        let mut doc = parse("source-roots = [\"src\"]\n");
        set_known_cyclic_files(&mut doc, &strings(&["pkg/b.py", "pkg/a.py"]));
        let cfg = crate::config::Config::from_toml(&doc.to_string()).expect("valid config");
        assert_eq!(
            cfg.cycles.known_cyclic_files,
            vec!["pkg/a.py".to_string(), "pkg/b.py".to_string()]
        );
    }

    // --- merge_ignored_cycles ---

    #[test]
    fn merge_ignored_cycles_appends_new_entries() {
        let mut doc = parse("source-roots = [\"src\"]\n");
        merge_ignored_cycles(&mut doc, &[strings(&["a.py", "b.py"])]);
        let cfg = crate::config::Config::from_toml(&doc.to_string()).expect("valid config");
        assert_eq!(cfg.cycles.ignore.len(), 1);
        assert_eq!(cfg.cycles.ignore[0].files, vec!["a.py", "b.py"]);
    }

    #[test]
    fn merge_ignored_cycles_creates_section_if_missing() {
        let mut doc = parse("source-roots = [\"src\"]\n");
        merge_ignored_cycles(&mut doc, &[strings(&["x.py", "y.py"])]);
        let out = doc.to_string();
        assert!(out.contains("[[cycles.ignore]]"), "missing AoT header: {out}");
        assert!(out.contains("files = [\"x.py\", \"y.py\"]"), "missing files: {out}");
    }

    #[test]
    fn merge_ignored_cycles_is_idempotent() {
        let mut doc = parse("source-roots = [\"src\"]\n");
        merge_ignored_cycles(&mut doc, &[strings(&["a.py", "b.py"])]);
        let after_first = doc.to_string();
        // Re-run with the same cycle, and also an unsorted permutation.
        merge_ignored_cycles(&mut doc, &[strings(&["a.py", "b.py"])]);
        merge_ignored_cycles(&mut doc, &[strings(&["b.py", "a.py"])]);
        assert_eq!(after_first, doc.to_string(), "duplicate entry added");
        let cfg = crate::config::Config::from_toml(&doc.to_string()).expect("valid config");
        assert_eq!(cfg.cycles.ignore.len(), 1);
    }

    #[test]
    fn merge_ignored_cycles_does_not_duplicate_existing() {
        let input = "\
source-roots = [\"src\"]

[[cycles.ignore]]
files = [\"a.py\", \"b.py\"]
";
        let mut doc = parse(input);
        merge_ignored_cycles(&mut doc, &[strings(&["a.py", "b.py"])]);
        let cfg = crate::config::Config::from_toml(&doc.to_string()).expect("valid config");
        assert_eq!(cfg.cycles.ignore.len(), 1, "existing entry duplicated");
    }

    #[test]
    fn merge_ignored_cycles_preserves_existing_reason() {
        let input = "\
source-roots = [\"src\"]

[[cycles.ignore]]
files = [\"a.py\", \"b.py\"]
reason = \"legacy debt\"
";
        let mut doc = parse(input);
        merge_ignored_cycles(&mut doc, &[strings(&["c.py", "d.py"])]);
        let out = doc.to_string();
        assert!(out.contains("reason = \"legacy debt\""), "reason lost: {out}");
        let cfg = crate::config::Config::from_toml(&out).expect("valid config");
        assert_eq!(cfg.cycles.ignore.len(), 2);
        let with_reason = cfg
            .cycles
            .ignore
            .iter()
            .find(|e| e.files == vec!["a.py", "b.py"])
            .expect("original entry present");
        assert_eq!(with_reason.reason.as_deref(), Some("legacy debt"));
    }

    #[test]
    fn merge_ignored_cycles_adds_only_new_among_mixed() {
        let input = "\
source-roots = [\"src\"]

[[cycles.ignore]]
files = [\"a.py\", \"b.py\"]
";
        let mut doc = parse(input);
        merge_ignored_cycles(
            &mut doc,
            &[strings(&["a.py", "b.py"]), strings(&["x.py", "y.py"])],
        );
        let cfg = crate::config::Config::from_toml(&doc.to_string()).expect("valid config");
        assert_eq!(cfg.cycles.ignore.len(), 2);
    }

    // --- patch_config_file ---

    #[test]
    fn patch_config_file_no_change_returns_false() {
        let mut file = NamedTempFile::new().expect("temp file");
        let original = "source-roots = [\"src\"]\n";
        file.write_all(original.as_bytes()).expect("write");
        let path = file.path().to_path_buf();

        let result = patch_config_file(&path, |_doc| {}).expect("patch ok");
        assert!(!result.changed, "expected no change");
        assert_eq!(result.path, path);

        let on_disk = std::fs::read_to_string(&path).expect("read");
        assert_eq!(on_disk, original, "file should be untouched");
    }

    #[test]
    fn patch_config_file_change_returns_true_and_writes() {
        let mut file = NamedTempFile::new().expect("temp file");
        file.write_all(b"source-roots = [\"src\"]\n").expect("write");
        let path = file.path().to_path_buf();

        let result = patch_config_file(&path, |doc| {
            set_known_cyclic_files(doc, &strings(&["pkg/a.py"]));
        })
        .expect("patch ok");
        assert!(result.changed, "expected change");
        assert_eq!(result.path, path);

        let on_disk = std::fs::read_to_string(&path).expect("read");
        assert!(on_disk.contains("known-cyclic-files"), "not written: {on_disk}");
        assert!(on_disk.contains("\"pkg/a.py\""), "path not written: {on_disk}");
    }

    #[test]
    fn patch_config_file_missing_file_is_io_error() {
        let path = Path::new("/nonexistent/does-not-exist/oboros.toml");
        let err = patch_config_file(path, |_doc| {}).expect_err("should fail");
        assert!(matches!(err, PatchError::Io(_)), "expected Io error, got {err:?}");
    }

    #[test]
    fn patch_config_file_invalid_toml_is_parse_error() {
        let mut file = NamedTempFile::new().expect("temp file");
        file.write_all(b"this is = = not valid toml\n").expect("write");
        let path = file.path().to_path_buf();
        let err = patch_config_file(&path, |_doc| {}).expect_err("should fail");
        assert!(
            matches!(err, PatchError::TomlParse(_)),
            "expected TomlParse error, got {err:?}"
        );
    }
}
