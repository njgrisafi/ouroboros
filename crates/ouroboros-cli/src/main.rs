mod output;

use clap::Parser;
use clap::ValueEnum;
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

/// Ouroboros — detect circular imports in Python projects.
#[derive(Parser)]
#[command(name = "oboros", version)]
struct Cli {
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
}

/// Walk upward from `start` looking for `oboros.toml`.
/// Returns the path to the file if found, or `None`.
fn find_config(start: &Path) -> Option<PathBuf> {
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

fn main() {
    let cli = Cli::parse();

    let cwd = std::env::current_dir().expect("failed to determine current directory");

    // Use --config if provided, otherwise discover oboros.toml by walking upward.
    let config_path = match cli.config {
        Some(p) => Some(p),
        None => find_config(&cwd),
    };

    let verbose = matches!(cli.format, OutputFormat::Human);

    let (config, project_root) = match config_path {
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

    if verbose {
        println!("{config:#?}");
    }

    // Discover Python files in the configured source roots.
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
        let matched = filter_result.suppressed.iter().any(|c| *c == ignore_paths);
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

            let mut cycle_data: Vec<(Vec<String>, &Vec<PathBuf>)> = cycles
                .iter()
                .map(|cycle| (output::packages_for_cycle(cycle), cycle))
                .collect();

            cycle_data.sort_by(|a, b| {
                let pkg_ord = match (a.0.first(), b.0.first()) {
                    (Some(pa), Some(pb)) => pa.cmp(pb),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                };
                pkg_ord.then_with(|| a.1.len().cmp(&b.1.len()))
            });

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
        }
        OutputFormat::Json => {
            let report =
                output::build_json_report(&cycles, suppressed_count, &graph_result.edge_metadata);
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        }
    }

    if cli.strict && !cycles.is_empty() {
        std::process::exit(1);
    }
}
