use super::error::ResolveError;

/// Resolve a relative import to an absolute dotted module path.
///
/// Given the importing module's dotted name, the relative level (number of
/// leading dots), and the optional module suffix, computes the absolute
/// module path.
///
/// # Algorithm
///
/// 1. Split `source_module` by `"."` to get its package ancestry.
/// 2. Drop `level` components from the end (level 1 = parent package).
/// 3. Append `module` (the part after the dots) if present.
///
/// # Examples
///
/// | source_module | level | module | result |
/// |---|---|---|---|
/// | `services.auth.login` | 1 | `Some("session")` | `services.auth.session` |
/// | `services.auth.tokens` | 2 | `Some("notifications.email")` | `services.notifications.email` |
/// | `core.runner` | 1 | `None` | `core` |
///
/// # Errors
///
/// Returns [`ResolveError::RelativeEscapesRoot`] if `level` exceeds the
/// number of components in `source_module`.
pub(crate) fn resolve_relative(
    source_module: &str,
    level: u32,
    module: Option<&str>,
) -> Result<String, ResolveError> {
    let components: Vec<&str> = if source_module.is_empty() {
        Vec::new()
    } else {
        source_module.split('.').collect()
    };

    let level = level as usize;

    // Level 1 means "go up to the parent package", which means dropping the
    // last component (the module itself). Each additional level drops one more.
    if level > components.len() {
        return Err(ResolveError::RelativeEscapesRoot {
            source_module: source_module.to_string(),
            level: level as u32,
        });
    }

    let base = &components[..components.len() - level];

    match module {
        Some(suffix) if !suffix.is_empty() => {
            if base.is_empty() {
                Ok(suffix.to_string())
            } else {
                Ok(format!("{}.{}", base.join("."), suffix))
            }
        }
        _ => Ok(base.join(".")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_dot_with_module() {
        // from .session import create_session  (in services.auth.login)
        let result = resolve_relative("services.auth.login", 1, Some("session")).unwrap();
        assert_eq!(result, "services.auth.session");
    }

    #[test]
    fn single_dot_no_module() {
        // from . import engine  (in core.runner — level=1, module=None)
        let result = resolve_relative("core.runner", 1, None).unwrap();
        assert_eq!(result, "core");
    }

    #[test]
    fn double_dot_with_module() {
        // from ..notifications.email import send_email  (in services.auth.tokens)
        let result =
            resolve_relative("services.auth.tokens", 2, Some("notifications.email")).unwrap();
        assert_eq!(result, "services.notifications.email");
    }

    #[test]
    fn triple_dot() {
        // from ...deep import x  (in a.b.c.d)
        let result = resolve_relative("a.b.c.d", 3, Some("deep")).unwrap();
        assert_eq!(result, "a.deep");
    }

    #[test]
    fn level_equals_depth() {
        // from .. import x  (in pkg.mod — level 2 = go up 2, leaving empty base)
        let result = resolve_relative("pkg.mod", 2, Some("other")).unwrap();
        assert_eq!(result, "other");
    }

    #[test]
    fn level_equals_depth_no_module() {
        // from .. import x  (in pkg.mod — level 2, no module suffix)
        let result = resolve_relative("pkg.mod", 2, None).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn escapes_root() {
        // from ...x import y  (in pkg.mod — level 3 but only 2 components)
        let result = resolve_relative("pkg.mod", 3, Some("x"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("escapes root"));
    }

    #[test]
    fn escapes_root_top_level() {
        // from . import x  (in app — level 1 but only 1 component)
        let result = resolve_relative("app", 1, Some("x")).unwrap();
        assert_eq!(result, "x");
    }

    #[test]
    fn escapes_root_top_level_double_dot() {
        // from .. import x  (in app — level 2 but only 1 component)
        let result = resolve_relative("app", 2, Some("x"));
        assert!(result.is_err());
    }

    #[test]
    fn empty_source_module() {
        // Relative import in a root __init__.py — source_module is ""
        let result = resolve_relative("", 1, Some("x"));
        assert!(result.is_err());
    }
}
