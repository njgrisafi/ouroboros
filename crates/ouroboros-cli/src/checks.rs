//! Short-circuit "check" modes that compare the live cycle state against a
//! recorded baseline and exit without printing the normal report.
//!
//! These are sibling features:
//! - [`run_check_max_cycles`] — fail if the *number* of cycles exceeds a budget.
//! - [`run_check_cyclic_files`] — fail if the *set* of cyclic files changes.
//!
//! Both are invoked from `main` after discovery, filtering, and the spinner
//! have finished, and both short-circuit the normal report on success or
//! failure. [`validate_max_cycles_cap`] is the pre-flight companion that
//! rejects a missing budget before the project scan. The combo guard
//! (`--check-max-cycles` + `--check-cyclic-files`) is config-independent and
//! lives in `main` next to the other flag-combo guards.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ouroboros_core::config::Config;

/// Pre-flight validation: ensure `--check-max-cycles` has a budget.
///
/// Runs after the CLI override has been folded into `config` (see `main`),
/// so it reads the single source of truth. The combo guard
/// (`--check-max-cycles` + `--check-cyclic-files`) is config-independent and
/// is handled earlier in `main`.
pub fn validate_max_cycles_cap(check_max_cycles: bool, config: &Config) {
    if check_max_cycles && config.cycles.max_cycles.is_none() {
        eprintln!(
            "error: --check-max-cycles requires --max-cycles or [cycles] max-cycles in oboros.toml"
        );
        std::process::exit(2);
    }
}

/// Enforce the `--check-max-cycles` budget.
///
/// `cap` is the resolved budget (CLI flag overriding config); the caller
/// establishes it is `Some` via [`validate_max_cycles_cap`] before unwrapping,
/// so this helper takes a non-optional `usize` and the invariant lives at a
/// single call site rather than spanning the pipeline.
///
/// Exits 1 if `count > cap`, 0 if within budget. Always short-circuits.
pub fn run_check_max_cycles(count: usize, cap: usize) -> ! {
    if count > cap {
        eprintln!("cycle count {count} exceeds max-cycles {cap}");
        std::process::exit(1);
    }
    eprintln!("cycle count {count} within max-cycles {cap}");
    std::process::exit(0);
}

/// Enforce the `--check-cyclic-files` baseline.
///
/// Compares the configured `[cycles] cyclic-files` list against the
/// freshly-computed cyclic-files set; exits 0 if identical, 1 with a human
/// diff on stderr if any difference. Always short-circuits.
pub fn run_check_cyclic_files(config: &Config, cyclic_files: &[PathBuf]) -> ! {
    let known: BTreeSet<String> = config
        .cycles
        .cyclic_files
        .iter()
        .map(|s| s.trim().replace('\\', "/"))
        .collect();

    let computed: BTreeSet<String> = cyclic_files
        .iter()
        .map(|p| p.display().to_string().replace('\\', "/"))
        .collect();

    if known == computed {
        eprintln!("cyclic files unchanged ({} files)", computed.len());
        std::process::exit(0);
    }

    let added: Vec<&String> = computed
        .difference(&known)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let removed: Vec<&String> = known
        .difference(&computed)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    eprintln!("cyclic files changed:");
    for path in &added {
        eprintln!("  + {path}");
    }
    for path in &removed {
        eprintln!("  - {path}");
    }
    let all_added_are_prefixed = !added.is_empty()
        && config.source_roots.iter().any(|root| {
            let prefix = root.trim_end_matches('/').to_string() + "/";
            removed
                .iter()
                .all(|r| added.iter().any(|a| a.as_str() == format!("{prefix}{r}")))
        });
    if all_added_are_prefixed {
        eprintln!(
            "  hint: cyclic-files uses pre-0.6.0 source-root-relative paths; run 'oboros --dump-cyclic-files' to regenerate."
        );
    }
    std::process::exit(1);
}
