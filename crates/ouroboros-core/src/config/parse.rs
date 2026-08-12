//! The `[parse]` section: configuration for the parser subsystem.

use serde::Deserialize;

use crate::parser::StringImportsMode;

/// Configuration for the parser subsystem.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ParseConfig {
    /// Whether to include imports nested inside functions, methods, and
    /// control-flow blocks (i.e. "local" imports).
    ///
    /// Defaults to `false`, which means only top-level imports are
    /// considered when building the dependency graph.
    #[serde(rename = "local-imports", default)]
    pub local_imports: bool,

    /// Whether to detect string literals that look like module paths
    /// (ruff-style "string imports"). Respects the `local-imports` nesting
    /// gate: when `local-imports` is off, only module-level string literals
    /// are scanned. Defaults to `false`.
    #[serde(rename = "string-imports", default)]
    pub string_imports: bool,

    /// Minimum number of dots for a string literal to be considered a
    /// module-path candidate. Only relevant in `string-imports-mode = "all"`;
    /// ignored in `"call-sites"` mode, where candidates resolve exactly.
    /// Defaults to 2 (matches ruff/Pants); `0` disables the dot requirement.
    #[serde(
        rename = "string-imports-min-dots",
        default = "default_string_imports_min_dots"
    )]
    pub string_imports_min_dots: usize,

    /// Which string literals count as module-path candidates:
    /// `"call-sites"` (default) only scans first arguments of
    /// `import_module`/`__import__` calls; `"all"` scans every string
    /// literal (ruff parity, aggressive).
    #[serde(rename = "string-imports-mode", default)]
    pub string_imports_mode: StringImportsMode,
}

fn default_string_imports_min_dots() -> usize {
    crate::parser::DEFAULT_STRING_IMPORTS_MIN_DOTS
}

impl Default for ParseConfig {
    fn default() -> Self {
        Self {
            local_imports: false,
            string_imports: false,
            string_imports_min_dots: default_string_imports_min_dots(),
            string_imports_mode: StringImportsMode::default(),
        }
    }
}

impl From<&ParseConfig> for crate::parser::ExtractOptions {
    fn from(config: &ParseConfig) -> Self {
        crate::parser::ExtractOptions {
            include_local: config.local_imports,
            string_imports: config.string_imports,
            string_imports_mode: config.string_imports_mode,
            string_imports_min_dots: config.string_imports_min_dots,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::parser::StringImportsMode;

    #[test]
    fn parse_toml_with_parse_section() {
        let toml_str = r#"
source-roots = ["src"]

[parse]
local-imports = true
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(config.source_roots, vec!["src".to_string()]);
        assert!(config.parse.local_imports);
    }

    #[test]
    fn parse_toml_without_parse_section_defaults_to_false() {
        let toml_str = r#"source-roots = ["src"]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(!config.parse.local_imports);
    }

    #[test]
    fn parse_toml_with_string_imports() {
        let toml_str = r#"
source-roots = ["src"]

[parse]
string-imports = true
string-imports-min-dots = 1
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(config.parse.string_imports);
        assert_eq!(config.parse.string_imports_min_dots, 1);
    }

    #[test]
    fn string_imports_default_off_with_min_dots_two() {
        let toml_str = r#"source-roots = ["src"]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(!config.parse.string_imports);
        assert_eq!(config.parse.string_imports_min_dots, 2);
    }

    #[test]
    fn string_imports_min_dots_defaults_to_two_when_only_flag_set() {
        let toml_str = r#"
source-roots = ["src"]

[parse]
string-imports = true
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(config.parse.string_imports);
        assert_eq!(config.parse.string_imports_min_dots, 2);
    }

    #[test]
    fn string_imports_min_dots_zero_is_valid() {
        let toml_str = r#"
source-roots = ["src"]

[parse]
string-imports = true
string-imports-min-dots = 0
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(config.parse.string_imports_min_dots, 0);
    }

    #[test]
    fn string_imports_mode_defaults_to_call_sites() {
        let toml_str = r#"source-roots = ["src"]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(
            config.parse.string_imports_mode,
            StringImportsMode::CallSites
        );
    }

    #[test]
    fn string_imports_mode_all() {
        let toml_str = r#"
source-roots = ["src"]

[parse]
string-imports = true
string-imports-mode = "all"
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(config.parse.string_imports_mode, StringImportsMode::All);
    }

    #[test]
    fn string_imports_mode_invalid_rejected() {
        let toml_str = r#"
source-roots = ["src"]

[parse]
string-imports-mode = "everything"
"#;
        assert!(Config::from_toml(toml_str).is_err());
    }
}
