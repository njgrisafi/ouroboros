use serde::Deserialize;
use std::fmt;

/// Errors that can occur when loading or validating a config.
#[derive(Debug)]
pub enum ConfigError {
    /// TOML deserialization failed.
    Parse(toml::de::Error),
    /// A validation rule was violated.
    Validation(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Parse(e) => write!(f, "config parse error: {e}"),
            ConfigError::Validation(msg) => write!(f, "config validation error: {msg}"),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Parse(e) => Some(e),
            ConfigError::Validation(_) => None,
        }
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(e: toml::de::Error) -> Self {
        ConfigError::Parse(e)
    }
}

/// Project configuration, typically deserialized from `oboros.toml`.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Config {
    /// First-party source roots relative to the project root.
    #[serde(rename = "source-roots")]
    pub source_roots: Vec<String>,

    /// Parser configuration.
    #[serde(default)]
    pub parse: ParseConfig,

    /// Resolver configuration.
    #[serde(default)]
    pub resolve: ResolveConfig,

    /// Cycle reporting configuration.
    #[serde(default)]
    pub cycles: CyclesConfig,

    /// Paths (files or directories) to exclude from analysis seeds.
    /// Excluded files reachable via imports from non-excluded files are still reported.
    #[serde(default)]
    pub exclude: Vec<String>,
}

/// Configuration for the parser subsystem.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct ParseConfig {
    /// Whether to include imports nested inside functions, methods, and
    /// control-flow blocks (i.e. "local" imports).
    ///
    /// Defaults to `false`, which means only top-level imports are
    /// considered when building the dependency graph.
    #[serde(rename = "local-imports", default)]
    pub local_imports: bool,
}

/// Configuration for the resolver subsystem.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ResolveConfig {
    /// Whether to also record dependency edges to the `__init__.py` files of
    /// every first-party ancestor package of an imported module.
    ///
    /// Importing `a.b.c` executes `a/__init__.py` and `a/b/__init__.py` at
    /// runtime, so those ancestor packages are genuine import-time
    /// dependencies. Enabling this surfaces real cycles that pass through an
    /// eager parent `__init__.py`. Defaults to `true`.
    #[serde(rename = "include-ancestor-init", default = "default_true")]
    pub include_ancestor_init: bool,

    /// Restore suppressed self/in-tree ancestor-init edges that close a cycle
    /// (e.g. `P/__init__.py -> P.child -> P`). Opt-in; defaults to `false`.
    /// Has no effect when `include-ancestor-init` is disabled.
    #[serde(rename = "include-self-ancestor-init", default)]
    pub include_self_ancestor_init: bool,
}

fn default_true() -> bool {
    true
}

impl Default for ResolveConfig {
    fn default() -> Self {
        ResolveConfig {
            include_ancestor_init: true,
            include_self_ancestor_init: false,
        }
    }
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct IgnoredCycle {
    pub files: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CyclesConfig {
    #[serde(rename = "min-scc-size", default = "default_min_scc_size")]
    pub min_scc_size: usize,

    #[serde(rename = "max-scc-size", default)]
    pub max_scc_size: Option<usize>,

    #[serde(default)]
    pub ignore: Vec<IgnoredCycle>,

    #[serde(rename = "known-cyclic-files", default)]
    pub known_cyclic_files: Vec<String>,

    #[serde(rename = "ignore-derived-ancestor-init", default)]
    pub ignore_derived_ancestor_init: bool,

    #[serde(rename = "ignore-dirs", default)]
    pub ignore_dirs: Vec<String>,
}

fn default_min_scc_size() -> usize {
    2
}

impl Default for CyclesConfig {
    fn default() -> Self {
        CyclesConfig {
            min_scc_size: default_min_scc_size(),
            max_scc_size: None,
            ignore: Vec::new(),
            known_cyclic_files: Vec::new(),
            ignore_derived_ancestor_init: false,
            ignore_dirs: Vec::new(),
        }
    }
}

impl Config {
    /// Parse a `Config` from a TOML string, then validate it.
    pub fn from_toml(s: &str) -> Result<Config, ConfigError> {
        let config: Config = toml::from_str(s)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate the config after deserialization.
    fn validate(&self) -> Result<(), ConfigError> {
        self.validate_source_roots()?;

        if self.cycles.min_scc_size < 1 {
            return Err(ConfigError::Validation(
                "min-scc-size must be at least 1".to_string(),
            ));
        }

        if let Some(max) = self.cycles.max_scc_size {
            if max < 1 {
                return Err(ConfigError::Validation(
                    "max-scc-size must be at least 1".to_string(),
                ));
            }
            if max < self.cycles.min_scc_size {
                return Err(ConfigError::Validation(
                    "max-scc-size must be greater than or equal to min-scc-size".to_string(),
                ));
            }
        }

        for entry in &self.cycles.ignore {
            if entry.files.is_empty() {
                return Err(ConfigError::Validation(
                    "[[cycles.ignore]] entry must have at least one file".to_string(),
                ));
            }
        }

        for entry in &self.cycles.known_cyclic_files {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                return Err(ConfigError::Validation(
                    "known-cyclic-files entry must not be empty".to_string(),
                ));
            }

            let normalized = trimmed.replace('\\', "/");
            if normalized.starts_with('/') {
                return Err(ConfigError::Validation(format!(
                    "known-cyclic-files entry must be a relative path, got absolute: {entry}"
                )));
            }
            if normalized
                .split('/')
                .next()
                .is_some_and(|seg| seg.contains(':'))
            {
                return Err(ConfigError::Validation(format!(
                    "known-cyclic-files entry must be a relative path, got absolute: {entry}"
                )));
            }
        }

        for entry in &self.exclude {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                return Err(ConfigError::Validation(
                    "exclude entry must not be empty".to_string(),
                ));
            }

            let normalized = trimmed.replace('\\', "/");
            if normalized.starts_with('/') {
                return Err(ConfigError::Validation(format!(
                    "exclude entry must be a relative path, got absolute: {entry}"
                )));
            }
            if normalized
                .split('/')
                .next()
                .is_some_and(|seg| seg.contains(':'))
            {
                return Err(ConfigError::Validation(format!(
                    "exclude entry must be a relative path, got absolute: {entry}"
                )));
            }
        }

        for entry in &self.cycles.ignore_dirs {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                return Err(ConfigError::Validation(
                    "ignore-dirs entry must not be empty".to_string(),
                ));
            }

            let normalized = trimmed.replace('\\', "/");
            if normalized.starts_with('/') {
                return Err(ConfigError::Validation(format!(
                    "ignore-dirs entry must be a relative path, got absolute: {entry}"
                )));
            }
            if normalized
                .split('/')
                .next()
                .is_some_and(|seg| seg.contains(':'))
            {
                return Err(ConfigError::Validation(format!(
                    "ignore-dirs entry must be a relative path, got absolute: {entry}"
                )));
            }
        }

        Ok(())
    }

    /// Reject overlapping, nested, or duplicate source roots.
    ///
    /// `""` and `"."` both normalize to the project root, so a `.` root
    /// conflicts only with another `.`/`""` root — `.` and a named root such
    /// as `lib` are intentionally treated as non-overlapping.
    fn validate_source_roots(&self) -> Result<(), ConfigError> {
        let mut roots: Vec<(String, &str)> = self
            .source_roots
            .iter()
            .map(|r| {
                let normalized = r.trim().replace('\\', "/");
                let normalized = normalized.trim_end_matches('/');
                let normalized = if normalized == "." {
                    String::new()
                } else {
                    normalized.to_string()
                };
                (normalized, r.as_str())
            })
            .collect();
        // Sorting by normalized value places any prefix `a` before its
        // extension `a/b`, so only the earlier element can be a prefix.
        roots.sort_by(|a, b| a.0.cmp(&b.0));

        for i in 0..roots.len() {
            for j in (i + 1)..roots.len() {
                let (a_norm, a_orig) = &roots[i];
                let (b_norm, b_orig) = &roots[j];

                if a_norm == b_norm {
                    return Err(ConfigError::Validation(format!(
                        "source roots must not overlap: duplicate source root '{a_orig}'"
                    )));
                }

                // An empty (project-root) `a_norm` yields the prefix "/", which
                // no normalized root starts with, so `.` never conflicts here.
                let prefix = format!("{a_norm}/");
                if b_norm.starts_with(&prefix) {
                    return Err(ConfigError::Validation(format!(
                        "source roots must not overlap: '{a_orig}' is a prefix of '{b_orig}'"
                    )));
                }
            }
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            source_roots: vec!["src".to_string()],
            parse: ParseConfig::default(),
            resolve: ResolveConfig::default(),
            cycles: CyclesConfig::default(),
            exclude: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_toml() {
        let toml_str = r#"source-roots = ["src", "lib"]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(
            config.source_roots,
            vec!["src".to_string(), "lib".to_string()]
        );
        assert!(!config.parse.local_imports);
    }

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
    fn include_ancestor_init_defaults_to_true() {
        let toml_str = r#"source-roots = ["src"]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(config.resolve.include_ancestor_init);
    }

    #[test]
    fn resolve_section_can_disable_ancestor_init() {
        let toml_str = r#"
source-roots = ["src"]

[resolve]
include-ancestor-init = false
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(!config.resolve.include_ancestor_init);
    }

    #[test]
    fn empty_resolve_section_keeps_default_true() {
        let toml_str = r#"
source-roots = ["src"]

[resolve]
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(config.resolve.include_ancestor_init);
    }

    #[test]
    fn include_self_ancestor_init_defaults_to_false() {
        let config = Config::default();
        assert!(!config.resolve.include_self_ancestor_init);
    }

    #[test]
    fn resolve_section_can_enable_self_ancestor_init() {
        let toml = r#"
source-roots = ["."]
[resolve]
include-self-ancestor-init = true
"#;
        let config = Config::from_toml(toml).unwrap();
        assert!(config.resolve.include_self_ancestor_init);
    }

    #[test]
    fn include_self_ancestor_init_omitted_defaults_false() {
        let toml = r#"
source-roots = ["."]
[resolve]
include-ancestor-init = true
"#;
        let config = Config::from_toml(toml).unwrap();
        assert!(!config.resolve.include_self_ancestor_init);
    }

    #[test]
    fn missing_source_roots_is_error() {
        let toml_str = "";
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn default_config() {
        let config = Config::default();
        assert_eq!(config.source_roots, vec!["src".to_string()]);
        assert!(!config.parse.local_imports);
        assert!(config.resolve.include_ancestor_init);
        assert_eq!(config.cycles.min_scc_size, 2);
        assert_eq!(config.cycles.max_scc_size, None);
    }

    // --- cycles config tests ---

    #[test]
    fn no_cycles_section_uses_defaults() {
        let toml_str = r#"source-roots = ["."]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(config.cycles.min_scc_size, 2);
        assert_eq!(config.cycles.max_scc_size, None);
    }

    #[test]
    fn cycles_exact_size_2() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
min-scc-size = 2
max-scc-size = 2
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(config.cycles.min_scc_size, 2);
        assert_eq!(config.cycles.max_scc_size, Some(2));
    }

    #[test]
    fn cycles_min_only() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
min-scc-size = 3
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(config.cycles.min_scc_size, 3);
        assert_eq!(config.cycles.max_scc_size, None);
    }

    #[test]
    fn cycles_min_scc_size_1_is_valid() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
min-scc-size = 1
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(config.cycles.min_scc_size, 1);
    }

    #[test]
    fn cycles_invalid_bounds_max_less_than_min() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
min-scc-size = 4
max-scc-size = 2
"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("max-scc-size must be greater than or equal to min-scc-size"),
            "unexpected error: {err_msg}"
        );
    }

    #[test]
    fn cycles_defaults_when_section_empty() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(config.cycles.min_scc_size, 2);
        assert_eq!(config.cycles.max_scc_size, None);
    }

    #[test]
    fn parse_cycles_ignore_single() {
        let toml_str = r#"
source-roots = ["."]

[[cycles.ignore]]
files = ["pkg/a.py", "pkg/b.py"]
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(config.cycles.ignore.len(), 1);
        assert_eq!(config.cycles.ignore[0].files, vec!["pkg/a.py", "pkg/b.py"]);
        assert_eq!(config.cycles.ignore[0].reason, None);
    }

    #[test]
    fn parse_cycles_ignore_with_reason() {
        let toml_str = r#"
source-roots = ["."]

[[cycles.ignore]]
files = ["a.py", "b.py"]
reason = "legacy"
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(config.cycles.ignore[0].reason.as_deref(), Some("legacy"));
    }

    #[test]
    fn parse_cycles_ignore_multiple() {
        let toml_str = r#"
source-roots = ["."]

[[cycles.ignore]]
files = ["a.py", "b.py"]

[[cycles.ignore]]
files = ["x.py", "y.py"]
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(config.cycles.ignore.len(), 2);
    }

    #[test]
    fn no_ignore_section_defaults_to_empty() {
        let toml_str = r#"source-roots = ["."]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(config.cycles.ignore.is_empty());
    }

    #[test]
    fn parse_cycles_ignore_empty_files_is_error() {
        let toml_str = r#"
source-roots = ["."]

[[cycles.ignore]]
files = []
"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn known_cyclic_files_parses() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
known-cyclic-files = ["pkg/a.py", "pkg/b.py"]
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(
            config.cycles.known_cyclic_files,
            vec!["pkg/a.py".to_string(), "pkg/b.py".to_string()]
        );
    }

    #[test]
    fn known_cyclic_files_omitted_defaults_to_empty() {
        let toml_str = r#"source-roots = ["."]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(config.cycles.known_cyclic_files.is_empty());
    }

    #[test]
    fn known_cyclic_files_empty_list_is_valid() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
known-cyclic-files = []
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(config.cycles.known_cyclic_files.is_empty());
    }

    #[test]
    fn known_cyclic_files_empty_string_entry_is_error() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
known-cyclic-files = [""]
"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("empty"), "expected 'empty' in: {msg}");
    }

    #[test]
    fn known_cyclic_files_absolute_path_is_error() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
known-cyclic-files = ["/abs/x.py"]
"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("absolute"), "expected 'absolute' in: {msg}");
    }

    #[test]
    fn known_cyclic_files_default_is_empty() {
        let config = CyclesConfig::default();
        assert!(config.known_cyclic_files.is_empty());
    }

    #[test]
    fn ignore_derived_ancestor_init_parses_true() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
ignore-derived-ancestor-init = true
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(config.cycles.ignore_derived_ancestor_init);
    }

    #[test]
    fn ignore_derived_ancestor_init_parses_false() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
ignore-derived-ancestor-init = false
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(!config.cycles.ignore_derived_ancestor_init);
    }

    #[test]
    fn ignore_derived_ancestor_init_omitted_defaults_to_false() {
        let toml_str = r#"source-roots = ["."]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(!config.cycles.ignore_derived_ancestor_init);
    }

    #[test]
    fn ignore_derived_ancestor_init_default_is_false() {
        let config = CyclesConfig::default();
        assert!(!config.ignore_derived_ancestor_init);
    }

    #[test]
    fn exclude_field_parses() {
        let toml_str = r#"
source-roots = ["."]
exclude = ["tests", "a/b.py"]
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(
            config.exclude,
            vec!["tests".to_string(), "a/b.py".to_string()]
        );
    }

    #[test]
    fn exclude_omitted_defaults_to_empty() {
        let toml_str = r#"source-roots = ["."]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(config.exclude.is_empty());
    }

    #[test]
    fn exclude_empty_list_is_valid() {
        let toml_str = r#"
source-roots = ["."]
exclude = []
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(config.exclude.is_empty());
    }

    #[test]
    fn exclude_empty_string_entry_is_error() {
        let toml_str = r#"
source-roots = ["."]
exclude = [""]
"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("empty"), "expected 'empty' in: {msg}");
    }

    #[test]
    fn exclude_absolute_path_is_error() {
        let toml_str = r#"
source-roots = ["."]
exclude = ["/abs/path.py"]
"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("absolute"), "expected 'absolute' in: {msg}");
    }

    #[test]
    fn exclude_windows_absolute_path_is_error() {
        let toml_str = r#"
source-roots = ["."]
exclude = ["C:/Users/project/app.py"]
"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("absolute"), "expected 'absolute' in: {msg}");
    }

    #[test]
    fn default_config_exclude_is_empty() {
        let config = Config::default();
        assert!(config.exclude.is_empty());
    }

    // --- cycles ignore-dirs tests ---

    #[test]
    fn ignore_dirs_parses() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
ignore-dirs = ["app/protos", "app/migrations/"]
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(
            config.cycles.ignore_dirs,
            vec!["app/protos".to_string(), "app/migrations/".to_string()]
        );
    }

    #[test]
    fn ignore_dirs_omitted_defaults_to_empty() {
        let toml_str = r#"source-roots = ["."]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(config.cycles.ignore_dirs.is_empty());
    }

    #[test]
    fn ignore_dirs_default_is_empty() {
        let config = CyclesConfig::default();
        assert!(config.ignore_dirs.is_empty());
    }

    #[test]
    fn ignore_dirs_empty_list_is_valid() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
ignore-dirs = []
"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert!(config.cycles.ignore_dirs.is_empty());
    }

    #[test]
    fn ignore_dirs_empty_string_entry_is_error() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
ignore-dirs = [""]
"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("empty"), "expected 'empty' in: {msg}");
    }

    #[test]
    fn ignore_dirs_absolute_path_is_error() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
ignore-dirs = ["/abs/"]
"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("absolute"), "expected 'absolute' in: {msg}");
    }

    #[test]
    fn ignore_dirs_windows_absolute_path_is_error() {
        let toml_str = r#"
source-roots = ["."]

[cycles]
ignore-dirs = ["C:/Users/project/app"]
"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("absolute"), "expected 'absolute' in: {msg}");
    }

    // --- source-root overlap validation tests ---

    #[test]
    fn source_roots_nested_is_error() {
        let toml_str = r#"source-roots = ["src", "src/pkg"]"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("prefix") && msg.contains("src"),
            "expected prefix-overlap message, got: {msg}"
        );
    }

    #[test]
    fn source_roots_duplicate_is_error() {
        let toml_str = r#"source-roots = ["src", "src"]"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("duplicate"),
            "expected duplicate message, got: {msg}"
        );
    }

    #[test]
    fn source_roots_disjoint_is_ok() {
        let toml_str = r#"source-roots = ["src", "lib"]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(
            config.source_roots,
            vec!["src".to_string(), "lib".to_string()]
        );
    }

    #[test]
    fn source_roots_dot_and_named_is_ok() {
        let toml_str = r#"source-roots = [".", "lib"]"#;
        let config = Config::from_toml(toml_str).unwrap();
        assert_eq!(
            config.source_roots,
            vec![".".to_string(), "lib".to_string()]
        );
    }

    #[test]
    fn source_roots_duplicate_dot_is_error() {
        let toml_str = r#"source-roots = [".", "."]"#;
        let result = Config::from_toml(toml_str);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("duplicate"),
            "expected duplicate message, got: {msg}"
        );
    }

    #[test]
    fn source_roots_trailing_slash_treated_as_overlap() {
        let toml_str = r#"source-roots = ["src/", "src"]"#;
        let result = Config::from_toml(toml_str);
        assert!(
            result.is_err(),
            "trailing slash should normalize to a duplicate"
        );
    }
}
