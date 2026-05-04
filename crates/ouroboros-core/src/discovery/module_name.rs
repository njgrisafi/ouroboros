use std::path::Path;

/// Convert a relative Python file path to its canonical dotted module name.
///
/// The input `rel_path` must be relative to a source root and end with `.py`.
///
/// # Examples
///
/// | `rel_path`             | result         |
/// |------------------------|----------------|
/// | `app.py`               | `"app"`        |
/// | `core/engine.py`       | `"core.engine"`|
/// | `core/__init__.py`     | `"core"`       |
/// | `__init__.py`          | `""`           |
pub fn module_name_for_path(rel_path: &Path) -> String {
    // Strip the `.py` extension to get the stem path (e.g. `core/engine`).
    let without_ext = rel_path.with_extension("");

    // Collect path components as strings.
    let components: Vec<&str> = without_ext
        .components()
        .map(|c| c.as_os_str().to_str().expect("path is valid UTF-8"))
        .collect();

    // If the last component is `__init__`, drop it — the module name is
    // the package path (the parent directory components).
    let parts: &[&str] = if components.last() == Some(&"__init__") {
        &components[..components.len() - 1]
    } else {
        &components
    };

    parts.join(".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn simple_file() {
        assert_eq!(module_name_for_path(&PathBuf::from("app.py")), "app");
    }

    #[test]
    fn nested_file() {
        assert_eq!(
            module_name_for_path(&PathBuf::from("pkg/sub/mod.py")),
            "pkg.sub.mod"
        );
    }

    #[test]
    fn package_init() {
        assert_eq!(
            module_name_for_path(&PathBuf::from("pkg/__init__.py")),
            "pkg"
        );
    }

    #[test]
    fn nested_package_init() {
        assert_eq!(
            module_name_for_path(&PathBuf::from("pkg/sub/__init__.py")),
            "pkg.sub"
        );
    }

    #[test]
    fn root_init() {
        assert_eq!(module_name_for_path(&PathBuf::from("__init__.py")), "");
    }
}
