mod output;
mod report;

use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use indicatif::{ProgressBar, ProgressStyle};
use ouroboros_core::config::Config;
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

        /// Project source root for reading import lines. If provided, the report
        /// shows actual import statements in the diff view.
        #[arg(long)]
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

    /// Do not record import edges to ancestor package __init__.py files
    /// (importing `a.b.c` normally also depends on `a` and `a.b`).
    #[arg(long = "no-include-ancestor-init")]
    no_include_ancestor_init: bool,

    /// Show detailed intermediate output (discovery, imports, graph).
    #[arg(long, short)]
    verbose: bool,
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
        source_root,
    }) = cli.command.as_ref()
    {
        report::run(input, output, source_root.as_deref());
        return;
    }

    let cwd = std::env::current_dir().expect("failed to determine current directory");

    // Use --config if provided, otherwise discover oboros.toml by walking upward.
    let config_path = match cli.config {
        Some(p) => Some(p),
        None => find_config(&cwd),
    };

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
    }
    for root in &discovery_result.roots {
        for file in &root.files {
            let abs_path = root.path.join(&file.rel_path);
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
    let resolve_result = resolver::resolve_all(&discovery_result, &index, &config);

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

    spinner.set_message("Detecting cycles...");
    let all_cycles = graph::dependency_cycles(&graph_result.graph);
    let size_filtered = cycles::filter_cycles_by_size(all_cycles, &config.cycles);
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
        }
    }

    let cycles = if cli.package {
        cycles::filter_cycles_by_package(filter_result.kept)
    } else {
        filter_result.kept
    };
    let suppressed_count = filter_result.suppressed.len();

    spinner.finish_and_clear();

    if cli.dump_ignores {
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
            if suppressed_count > 0 {
                println!("({} cycles suppressed by ignore list)", suppressed_count);
            }
            if cli.package {
                println!("(filtered to intra-package cycles)");
            }

            let cycle_data = output::order_cycles(&cycles);

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
                    &graph_result.graph,
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
                    &graph_result.graph,
                    &graph_result.edge_metadata,
                    &config.source_roots,
                )
            };
            let has_trace_impacts = traced_has_impacts(&traced);
            let report = output::build_json_report(
                &cycles,
                suppressed_count,
                &graph_result.edge_metadata,
                traced,
                unknown_paths,
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
