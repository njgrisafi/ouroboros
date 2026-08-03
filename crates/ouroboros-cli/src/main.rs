mod output;
mod report;

use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use indicatif::{ProgressBar, ProgressStyle};
use ouroboros_core::config::Config;
use ouroboros_core::config_edit;
use ouroboros_core::cycles;
use ouroboros_core::discovery;
use ouroboros_core::graph;
use ouroboros_core::parser;
use ouroboros_core::resolver;
use std::path::{Path, PathBuf};

#[derive(Clone, Default, ValueEnum)]
enum OutputFormat {
    #[default]
    Human,
    Json,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate an HTML report from a JSON cycle report.
    Report {
        /// Path to the JSON report file (produced by --format json).
        input: PathBuf,

        /// Output HTML file path.
        #[arg(long, short, default_value = "report.html")]
        output: PathBuf,

        /// Project root for reading import lines. If provided, the report
        /// shows actual import statements in the diff view.
        #[arg(long)]
        root: Option<PathBuf>,

        /// Deprecated: use --root instead.
        #[arg(long, hide = true)]
        source_root: Option<PathBuf>,
    },
}

/// Ouroboros — detect circular imports in Python projects.
#[derive(Parser)]
#[command(name = "oboros", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    #[arg(long)]
    dump_ignores: bool,

    #[arg(long)]
    strict: bool,

    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,

    /// Only report cycles where all files belong to the same top-level package.
    #[arg(long)]
    package: bool,

    /// Report cycles that impact the given file or directory path(s), relative to a
    /// source root (e.g. `app/mod.py` or `app/sub/`). Repeatable and/or comma-separated.
    /// When omitted, output is identical to today.
    #[arg(
        long = "trace",
        short = 't',
        value_name = "PATH",
        value_delimiter = ','
    )]
    traces: Vec<String>,

    /// Exclude paths (files or directories) from analysis seeds. Excluded files
    /// reachable via imports from non-excluded files are still reported.
    /// Repeatable and/or comma-separated. Unioned with `exclude` in config.
    #[arg(long = "exclude", value_name = "PATH", value_delimiter = ',')]
    excludes: Vec<String>,

    /// Do not record import edges to ancestor package __init__.py files
    /// (importing `a.b.c` normally also depends on `a` and `a.b`).
    #[arg(long = "no-include-ancestor-init")]
    no_include_ancestor_init: bool,

    /// Show detailed intermediate output (discovery, imports, graph).
    #[arg(long, short)]
    verbose: bool,

    /// Print the sorted set of files participating in any cycle as a pasteable
    /// TOML fragment (human) or JSON object (--format json), then exit.
    #[arg(long)]
    dump_cyclic_files: bool,

    /// Write the dump output directly into oboros.toml instead of printing to stdout. Requires --dump-cyclic-files or --dump-ignores.
    #[arg(long)]
    write: bool,

    /// Compare the configured [cycles] known-cyclic-files list against the
    /// freshly-computed cyclic-files set; exit 0 if identical, 1 if any
    /// difference (with a human diff on stderr). Independent of --format.
    #[arg(long)]
    check_cyclic_files: bool,

    /// Include the cyclic-files set as an optional top-level `cyclic_files`
    /// array in the JSON report. No-op in human mode.
    #[arg(long)]
    show_cyclic_files: bool,

    /// When set, files that become cyclic only via a derived ancestor-__init__.py edge
    /// are excluded from the known-cyclic-files baseline. Overrides
    /// [cycles] ignore-derived-ancestor-init in config. Baseline-only; does not affect
    /// the normal cycle report.
    #[arg(long = "ignore-derived-ancestor-init")]
    ignore_derived_ancestor_init: bool,
}

/// Walk upward from `start` looking for `oboros.toml`.
/// Returns the path to the file if found, or `None`.
pub(crate) fn find_config(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join("oboros.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn make_spinner(verbose: bool) -> ProgressBar {
    if !verbose {
        return ProgressBar::hidden();
    }
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .expect("invalid spinner template"),
    );
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}

fn print_file_impacts(file: &output::JsonTraceFile) {
    println!(
        "    impacted by {} cycle{}:",
        file.impacts.len(),
        if file.impacts.len() == 1 { "" } else { "s" }
    );
    for impact in &file.impacts {
        if impact.relationship == "member" {
            println!("      cycle {} (member)", impact.cycle_index);
        } else {
            let chain = build_reachable_chain(&impact.path, &impact.entry);
            println!(
                "      cycle {} (reachable via {})",
                impact.cycle_index, chain
            );
        }
    }
}

fn build_reachable_chain(hops: &[output::JsonBranchHop], entry: &str) -> String {
    let mut parts: Vec<String> = hops
        .iter()
        .map(|hop| {
            let lines_str = hop
                .lines
                .iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join(",");
            format!("{}:{}", hop.from, lines_str)
        })
        .collect();
    parts.push(entry.to_string());
    parts.join(" -> ")
}

fn traced_has_impacts(traced: &[output::JsonTrace]) -> bool {
    traced
        .iter()
        .any(|trace| trace.files.iter().any(|file| !file.impacts.is_empty()))
}

fn main() {
    let cli = Cli::parse();

    if let Some(Commands::Report {
        input,
        output,
        root,
        source_root,
    }) = cli.command.as_ref()
    {
        if source_root.is_some() {
            eprintln!("warning: --source-root is deprecated; use --root instead");
        }
        let effective_root = root.as_deref().or(source_root.as_deref());
        report::run(input, output, effective_root);
        return;
    }

    let cwd = std::env::current_dir().expect("failed to determine current directory");

    // Use --config if provided, otherwise discover oboros.toml by walking upward.
    let config_path = match cli.config {
        Some(p) => Some(p),
        None => find_config(&cwd),
    };

    if cli.write {
        if !cli.dump_cyclic_files && !cli.dump_ignores {
            eprintln!("error: --write requires --dump-cyclic-files or --dump-ignores");
            std::process::exit(2);
        }
        if cli.check_cyclic_files {
            eprintln!("error: --write cannot be combined with --check-cyclic-files");
            std::process::exit(2);
        }
        if config_path.is_none() {
            eprintln!("error: --write requires an existing oboros.toml, but none was found");
            std::process::exit(2);
        }
    }
    let write_config_path: Option<PathBuf> = config_path.clone();

    let is_human = matches!(cli.format, OutputFormat::Human);
    let verbose = is_human && cli.verbose;
    let spinner = make_spinner(is_human && !cli.verbose);

    let (mut config, project_root) = match config_path {
        Some(path) => {
            if verbose {
                println!("found config: {}", path.display());
            }
            let contents = std::fs::read_to_string(&path).expect("failed to read config file");
            let cfg = Config::from_toml(&contents).expect("failed to parse config file");
            let root = path
                .parent()
                .expect("config file must have a parent directory")
                .to_path_buf();
            (cfg, root)
        }
        None => {
            if verbose {
                println!("no oboros.toml found, using defaults");
            }
            (Config::default(), cwd.clone())
        }
    };

    // CLI flag overrides config: --no-include-ancestor-init forces the option off.
    if cli.no_include_ancestor_init {
        config.resolve.include_ancestor_init = false;
    }

    if cli.ignore_derived_ancestor_init {
        config.cycles.ignore_derived_ancestor_init = true;
    }

    let mut seen_excludes = std::collections::HashSet::new();
    let merged_excludes: Vec<String> = config
        .exclude
        .iter()
        .chain(cli.excludes.iter())
        .filter(|e| seen_excludes.insert((*e).clone()))
        .cloned()
        .collect();

    if verbose {
        println!("{config:#?}");
    }

    // Discover Python files in the configured source roots.
    spinner.set_message("Discovering Python files...");
    let discovery_result = match discovery::discover(&config, &project_root) {
        Ok(result) => {
            if verbose {
                for root in &result.roots {
                    println!(
                        "\nsource root: {} ({} files)",
                        root.path.display(),
                        root.files.len()
                    );
                    for f in &root.files {
                        println!("  {} -> {}", f.rel_path.display(), f.module_name);
                    }
                }
                println!("\ntotal: {} Python files", result.total_files());
            }
            result
        }
        Err(e) => {
            eprintln!("discovery error: {e}");
            std::process::exit(1);
        }
    };

    // Extract imports from each discovered file.
    spinner.set_message(format!(
        "Extracting imports from {} files...",
        discovery_result.total_files()
    ));
    if verbose {
        println!("\n--- imports ---");
        println!("project root: {}", project_root.display());
    }
    for root in &discovery_result.roots {
        if verbose {
            println!(
                "source root: {} ({} files)",
                root.path.display(),
                root.files.len()
            );
        }
        for file in &root.files {
            let abs_path = project_root.join(&file.rel_path);
            let source = match std::fs::read_to_string(&abs_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("  warning: could not read {}: {e}", abs_path.display());
                    continue;
                }
            };

            match parser::extract_imports(&source, config.parse.local_imports) {
                Ok(imports) if imports.is_empty() => {}
                Ok(imports) => {
                    if verbose {
                        println!("\n  {}:", file.module_name);
                        for imp in &imports {
                            let module_part = imp.module.as_deref().unwrap_or("");
                            let dots = ".".repeat(imp.level as usize);
                            let names: Vec<&str> =
                                imp.names.iter().map(|n| n.name.as_str()).collect();
                            println!(
                                "    {kind} {dots}{module} ({names})",
                                kind = match imp.kind {
                                    parser::ImportKind::Import => "import",
                                    parser::ImportKind::ImportFrom => "from  ",
                                },
                                module = module_part,
                                names = names.join(", "),
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("  warning: parse error in {}: {e}", file.module_name);
                }
            }
        }
    }

    // Build first-party module index and resolve imports.
    spinner.set_message("Resolving imports...");
    let index = resolver::ModuleIndex::from_discovery(&discovery_result);
    let resolve_result = resolver::resolve_all(&discovery_result, &index, &config, &project_root);

    if verbose {
        println!(
            "\n--- resolved first-party dependencies ({}) ---",
            resolve_result.deps.len()
        );
        for dep in &resolve_result.deps {
            println!("  {} -> {}", dep.source, dep.target);
        }

        println!(
            "\n--- unresolved imports ({}) ---",
            resolve_result.unresolved.len()
        );
        for imp in &resolve_result.unresolved {
            println!("  {} -> {}", imp.source, imp.import_path);
        }
    }

    spinner.set_message("Building dependency graph...");
    let graph_result = graph::build_file_dependency_graph(&discovery_result, &resolve_result);

    for (module_name, paths) in &graph_result.module_collisions {
        let path_list: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        eprintln!(
            "warning: module '{}' is defined by multiple files: {}",
            module_name,
            path_list.join(", ")
        );
    }

    if verbose {
        println!("\n--- dependency graph ---\n");
        let mut nodes: Vec<_> = graph_result.graph.keys().collect();
        nodes.sort();
        for node in nodes {
            println!("{}", node.display());
            for dep in &graph_result.graph[node] {
                println!("  -> {}", dep.display());
            }
        }
    }

    let node_paths: std::collections::BTreeSet<std::path::PathBuf> =
        graph_result.graph.keys().cloned().collect();
    let mut excluded: std::collections::HashSet<std::path::PathBuf> =
        std::collections::HashSet::new();
    let mut applied_exclude_patterns: Vec<String> = Vec::new();
    for raw in &merged_excludes {
        match output::resolve_path_to_nodes(&node_paths, raw, &config.source_roots) {
            Some((matched, resolved)) => {
                let display = match matched.kind {
                    ouroboros_core::graph::PathKind::File => resolved.clone(),
                    ouroboros_core::graph::PathKind::Directory => format!("{resolved}/"),
                };
                applied_exclude_patterns.push(display);
                for node in matched.nodes {
                    excluded.insert(node);
                }
            }
            None => {
                eprintln!("warning: exclude path '{raw}' matched no first-party files");
            }
        }
    }
    let effective_graph = if excluded.is_empty() {
        graph_result.graph.clone()
    } else {
        graph::apply_exclusions(&graph_result.graph, &excluded)
    };

    spinner.set_message("Detecting cycles...");
    let all_cycles = graph::dependency_cycles(&effective_graph);
    let size_filtered = cycles::filter_cycles_by_size(all_cycles, &config.cycles);
    // Rebind `size_filtered` to the kept partition so every downstream consumer
    // (report, --strict, cyclic-files baseline) sees only non-dir-ignored cycles.
    let (size_filtered, dir_ignored_cycles) =
        cycles::partition_dir_ignored(size_filtered, &config.cycles.ignore_dirs);

    let cyclic_surface_active =
        cli.dump_cyclic_files || cli.check_cyclic_files || cli.show_cyclic_files;

    // No-op warning: the option has no effect when include-ancestor-init is already disabled.
    if config.cycles.ignore_derived_ancestor_init
        && !config.resolve.include_ancestor_init
        && cyclic_surface_active
    {
        eprintln!(
            "warning: --ignore-derived-ancestor-init / [cycles] ignore-derived-ancestor-init \
             has no effect when include-ancestor-init is disabled"
        );
    }

    let cyclic_files: Vec<std::path::PathBuf> = if config.cycles.ignore_derived_ancestor_init
        && config.resolve.include_ancestor_init
        && cyclic_surface_active
    {
        // Direct-only pass: resolve without ancestor-init edges to compute the baseline.
        // Reuses the same index (edge-independent) and excluded set (path-keyed).
        let mut direct_config = config.clone();
        direct_config.resolve.include_ancestor_init = false;
        let direct_resolve =
            resolver::resolve_all(&discovery_result, &index, &direct_config, &project_root);
        let direct_graph = graph::build_file_dependency_graph(&discovery_result, &direct_resolve);
        let direct_effective = if excluded.is_empty() {
            direct_graph.graph
        } else {
            graph::apply_exclusions(&direct_graph.graph, &excluded)
        };
        let direct_cycles = graph::dependency_cycles(&direct_effective);
        let direct_size_filtered = cycles::filter_cycles_by_size(direct_cycles, &config.cycles);
        let direct_kept =
            cycles::partition_dir_ignored(direct_size_filtered, &config.cycles.ignore_dirs).0;
        cycles::collect_cyclic_files(&direct_kept)
    } else {
        cycles::collect_cyclic_files(&size_filtered)
    };

    let filter_result = cycles::filter_ignored_cycles(size_filtered, &config.cycles.ignore);

    for ignored_entry in &config.cycles.ignore {
        let mut ignore_paths: Vec<std::path::PathBuf> = ignored_entry
            .files
            .iter()
            .map(std::path::PathBuf::from)
            .collect();
        ignore_paths.sort();
        let matched = filter_result.suppressed.contains(&ignore_paths);
        if !matched {
            let files_str = ignore_paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            eprintln!("warning: ignore entry [{files_str}] did not match any detected cycle");
            for root in &config.source_roots {
                let root_prefix = root.trim_end_matches('/').to_string() + "/";
                let prefixed: Vec<std::path::PathBuf> = ignored_entry
                    .files
                    .iter()
                    .map(|f| std::path::PathBuf::from(format!("{root_prefix}{f}")))
                    .collect();
                let mut sorted_prefixed = prefixed.clone();
                sorted_prefixed.sort();
                if filter_result.suppressed.contains(&sorted_prefixed)
                    || filter_result.kept.contains(&sorted_prefixed)
                {
                    let hint = prefixed
                        .iter()
                        .map(|p| format!("\"{}\"", p.display()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    eprintln!(
                        "  hint: ignore entry looks pre-0.6.0 (source-root-relative); rewrite to project-root-relative, e.g. files = [{hint}]"
                    );
                    break;
                }
            }
        }
    }

    let cycles = if cli.package {
        cycles::filter_cycles_by_package(filter_result.kept, &config.source_roots)
    } else {
        filter_result.kept
    };
    let ignore_list_suppressed = filter_result.suppressed.len();
    let suppressed_count = ignore_list_suppressed + dir_ignored_cycles.len();

    spinner.finish_and_clear();

    if cli.check_cyclic_files {
        use std::collections::BTreeSet;

        let known: BTreeSet<String> = config
            .cycles
            .known_cyclic_files
            .iter()
            .map(|s| s.trim().replace('\\', "/"))
            .collect();

        let computed: BTreeSet<String> = cyclic_files
            .iter()
            .map(|p| p.display().to_string().replace('\\', "/"))
            .collect();

        if known == computed {
            eprintln!("cyclic files unchanged ({} files)", computed.len());
            return;
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
                "  hint: known-cyclic-files uses pre-0.6.0 source-root-relative paths; run 'oboros --dump-cyclic-files' to regenerate."
            );
        }
        eprintln!(
            "run `oboros --dump-cyclic-files` to update [cycles] known-cyclic-files in oboros.toml"
        );
        std::process::exit(1);
    } else if cli.dump_cyclic_files {
        let paths: Vec<String> = cyclic_files
            .iter()
            .map(|p| p.display().to_string().replace('\\', "/"))
            .collect();
        if cli.write {
            let Some(config_path) = write_config_path.clone() else {
                eprintln!("error: --write requires an existing oboros.toml, but none was found");
                std::process::exit(2);
            };
            let old_count = config.cycles.known_cyclic_files.len();
            let new_count = paths.len();
            match config_edit::patch_config_file(&config_path, |doc| {
                config_edit::set_known_cyclic_files(doc, &paths);
            }) {
                Ok(result) => {
                    if result.changed {
                        eprintln!(
                            "wrote {}: known-cyclic-files {} entries (was {})",
                            result.path.display(),
                            new_count,
                            old_count
                        );
                    } else {
                        eprintln!("unchanged: known-cyclic-files {new_count} entries");
                    }
                    if matches!(cli.format, OutputFormat::Json) {
                        let out = serde_json::json!({
                            "written": result.changed,
                            "path": result.path.display().to_string(),
                            "known_cyclic_files": new_count,
                        });
                        println!("{out}");
                    }
                }
                Err(e) => {
                    eprintln!("error: failed to write {}: {e}", config_path.display());
                    std::process::exit(1);
                }
            }
            return;
        }
        match cli.format {
            OutputFormat::Human => {
                println!(
                    "# paste under [cycles] in oboros.toml (merge into an existing [cycles] table if you have one)"
                );
                println!("[cycles]");
                if config.cycles.ignore_derived_ancestor_init {
                    println!("ignore-derived-ancestor-init = true");
                }
                if paths.is_empty() {
                    println!("known-cyclic-files = []");
                } else {
                    println!("known-cyclic-files = [");
                    for path in &paths {
                        println!("    \"{path}\",");
                    }
                    println!("]");
                }
            }
            OutputFormat::Json => {
                let report = output::build_dump_cyclic_files_report(
                    &cyclic_files,
                    config.cycles.ignore_derived_ancestor_init,
                );
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            }
        }
        return;
    } else if cli.dump_ignores {
        if cli.write {
            let Some(config_path) = write_config_path.clone() else {
                eprintln!("error: --write requires an existing oboros.toml, but none was found");
                std::process::exit(2);
            };
            let cycle_file_sets: Vec<Vec<String>> = cycles
                .iter()
                .map(|cycle| {
                    let mut files: Vec<String> = cycle
                        .iter()
                        .map(|p| p.display().to_string().replace('\\', "/"))
                        .collect();
                    files.sort();
                    files
                })
                .collect();
            let existing: std::collections::HashSet<Vec<String>> = config
                .cycles
                .ignore
                .iter()
                .map(|entry| {
                    let mut files: Vec<String> =
                        entry.files.iter().map(|f| f.replace('\\', "/")).collect();
                    files.sort();
                    files
                })
                .collect();
            let added_entries = cycle_file_sets
                .iter()
                .filter(|set| !existing.contains(*set))
                .count();
            match config_edit::patch_config_file(&config_path, |doc| {
                config_edit::merge_ignored_cycles(doc, &cycle_file_sets);
            }) {
                Ok(result) => {
                    if result.changed {
                        eprintln!(
                            "wrote {}: added {added_entries} ignore entries",
                            result.path.display()
                        );
                    } else {
                        eprintln!("unchanged: no new ignore entries");
                    }
                    if matches!(cli.format, OutputFormat::Json) {
                        let out = serde_json::json!({
                            "written": result.changed,
                            "path": result.path.display().to_string(),
                            "added_entries": added_entries,
                        });
                        println!("{out}");
                    }
                }
                Err(e) => {
                    eprintln!("error: failed to write {}: {e}", config_path.display());
                    std::process::exit(1);
                }
            }
            return;
        }
        match cli.format {
            OutputFormat::Human => {
                for cycle in &cycles {
                    println!("[[cycles.ignore]]");
                    let mut files: Vec<String> = cycle
                        .iter()
                        .map(|p| format!("\"{}\"", p.display()))
                        .collect();
                    files.sort();
                    println!("files = [{}]", files.join(", "));
                    println!();
                }
            }
            OutputFormat::Json => {
                let report = output::build_dump_ignores_report(&cycles);
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            }
        }
        return;
    }

    match cli.format {
        OutputFormat::Human => {
            println!("\n--- dependency cycles ({}) ---", cycles.len());
            if ignore_list_suppressed > 0 {
                println!(
                    "({} cycles suppressed by ignore list)",
                    ignore_list_suppressed
                );
            }
            if !dir_ignored_cycles.is_empty() {
                println!(
                    "({} cycles ignored by ignore-dirs)",
                    dir_ignored_cycles.len()
                );
            }
            if cli.package {
                println!("(filtered to intra-package cycles)");
            }

            let cycle_data = output::order_cycles(&cycles, &config.source_roots);

            let mut current_packages: Option<&Vec<String>> = None;
            let mut group_count = 0;
            for (idx, (packages, _)) in cycle_data.iter().enumerate() {
                if current_packages != Some(packages) {
                    let remaining = cycle_data[idx..]
                        .iter()
                        .take_while(|(p, _)| p == packages)
                        .count();
                    group_count = remaining;
                    if packages.is_empty() {
                        println!(
                            "\n(root-level) ({} cycle{})",
                            group_count,
                            if group_count == 1 { "" } else { "s" }
                        );
                    } else if packages.len() == 1 {
                        println!(
                            "\npackage: {} ({} cycle{})",
                            packages[0],
                            group_count,
                            if group_count == 1 { "" } else { "s" }
                        );
                    } else {
                        println!(
                            "\n(cross-package: {}) ({} cycle{})",
                            packages.join(", "),
                            group_count,
                            if group_count == 1 { "" } else { "s" }
                        );
                    }
                    current_packages = Some(packages);
                }
                let _ = group_count;
                let (_, cycle) = &cycle_data[idx];
                println!("\ncycle {} ({} files)", idx + 1, cycle.len());
                for path in *cycle {
                    let import_lines =
                        output::collect_import_lines(path, cycle, &graph_result.edge_metadata);

                    if import_lines.is_empty() {
                        println!("  {}", path.display());
                    } else if import_lines.len() == 1 {
                        println!("  {} (import at line {})", path.display(), import_lines[0]);
                    } else {
                        let line_strs: Vec<String> =
                            import_lines.iter().map(|l| l.to_string()).collect();
                        println!(
                            "  {} (imports at lines {})",
                            path.display(),
                            line_strs.join(", ")
                        );
                    }
                }
            }

            let trace_result = if cli.traces.is_empty() {
                None
            } else {
                Some(output::build_traces(
                    &cli.traces,
                    &cycles,
                    &effective_graph,
                    &graph_result.edge_metadata,
                    &config.source_roots,
                ))
            };

            if let Some((ref traced, ref unknown_paths)) = trace_result {
                println!("\n--- cycle impact ---");

                for trace in traced {
                    let is_dir = trace.kind == "directory";

                    if is_dir {
                        let total = trace.files.len();
                        let impacted = trace
                            .files
                            .iter()
                            .filter(|file| !file.impacts.is_empty())
                            .count();
                        println!(
                            "\ntrace: {} (directory, {} of {} files impacted)",
                            trace.path, impacted, total
                        );
                    } else {
                        println!("\ntrace: {} (file)", trace.path);
                    }

                    if is_dir {
                        let impacted_files: Vec<_> = trace
                            .files
                            .iter()
                            .filter(|file| !file.impacts.is_empty())
                            .collect();
                        if impacted_files.is_empty() {
                            println!("  no cycles impact this path");
                        } else {
                            for file in impacted_files {
                                println!("  {}:", file.path);
                                print_file_impacts(file);
                            }
                        }
                    } else if let Some(file) = trace.files.first() {
                        if file.impacts.is_empty() {
                            println!("  not impacted by any cycle");
                        } else {
                            print_file_impacts(file);
                        }
                    }
                }

                if !unknown_paths.is_empty() {
                    println!("\n(unknown paths: {})", unknown_paths.join(", "));
                }
            }

            if cli.strict {
                if let Some((ref traced, _)) = trace_result {
                    if traced_has_impacts(traced) {
                        std::process::exit(1);
                    }
                } else if !cycles.is_empty() {
                    std::process::exit(1);
                }
            }
        }
        OutputFormat::Json => {
            let (traced, unknown_paths) = if cli.traces.is_empty() {
                (vec![], vec![])
            } else {
                output::build_traces(
                    &cli.traces,
                    &cycles,
                    &effective_graph,
                    &graph_result.edge_metadata,
                    &config.source_roots,
                )
            };
            let has_trace_impacts = traced_has_impacts(&traced);
            let report = output::build_json_report(
                &cycles,
                suppressed_count,
                &graph_result.edge_metadata,
                output::JsonReportInput {
                    traced,
                    unknown_paths,
                    excluded: applied_exclude_patterns,
                    cyclic_files: if cli.show_cyclic_files {
                        cyclic_files
                            .iter()
                            .map(|p| p.display().to_string().replace('\\', "/"))
                            .collect()
                    } else {
                        vec![]
                    },
                    source_roots: &config.source_roots,
                },
            );
            println!("{}", serde_json::to_string_pretty(&report).unwrap());

            if cli.strict {
                if cli.traces.is_empty() {
                    if !cycles.is_empty() {
                        std::process::exit(1);
                    }
                } else if has_trace_impacts {
                    std::process::exit(1);
                }
            }
        }
    }
}
