use clap::Parser;
use ouroboros_core::config::Config;
use ouroboros_core::cycles;
use ouroboros_core::discovery;
use ouroboros_core::graph;
use ouroboros_core::parser;
use ouroboros_core::resolver;
use std::path::{Path, PathBuf};

/// Ouroboros — detect circular imports in Python projects.
#[derive(Parser)]
#[command(name = "oboros", version)]
struct Cli {
    /// Path to the config file [default: oboros.toml, discovered by walking up from cwd]
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,
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

    let (config, project_root) = match config_path {
        Some(path) => {
            println!("found config: {}", path.display());
            let contents =
                std::fs::read_to_string(&path).expect("failed to read config file");
            let cfg = Config::from_toml(&contents).expect("failed to parse config file");
            let root = path
                .parent()
                .expect("config file must have a parent directory")
                .to_path_buf();
            (cfg, root)
        }
        None => {
            println!("no oboros.toml found, using defaults");
            (Config::default(), cwd.clone())
        }
    };

    println!("{config:#?}");

    // Discover Python files in the configured source roots.
    let discovery_result = match discovery::discover(&config, &project_root) {
        Ok(result) => {
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
            result
        }
        Err(e) => {
            eprintln!("discovery error: {e}");
            std::process::exit(1);
        }
    };

    // Extract imports from each discovered file.
    println!("\n--- imports ---");
    for root in &discovery_result.roots {
        for file in &root.files {
            let abs_path = root.path.join(&file.rel_path);
            let source = match std::fs::read_to_string(&abs_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!(
                        "  warning: could not read {}: {e}",
                        abs_path.display()
                    );
                    continue;
                }
            };

            match parser::extract_imports(&source, config.parse.local_imports) {
                Ok(imports) if imports.is_empty() => {}
                Ok(imports) => {
                    println!("\n  {}:", file.module_name);
                    for imp in &imports {
                        let module_part = imp
                            .module
                            .as_deref()
                            .unwrap_or("");
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
                Err(e) => {
                    eprintln!(
                        "  warning: parse error in {}: {e}",
                        file.module_name
                    );
                }
            }
        }
    }

    // Build first-party module index and resolve imports.
    let index = resolver::ModuleIndex::from_discovery(&discovery_result);
    let resolve_result = resolver::resolve_all(&discovery_result, &index, &config);

    // Print resolved first-party dependency edges.
    println!("\n--- resolved first-party dependencies ({}) ---", resolve_result.deps.len());
    for dep in &resolve_result.deps {
        println!("  {} -> {}", dep.source, dep.target);
    }

    // Print unresolved imports (stdlib/third-party).
    println!(
        "\n--- unresolved imports ({}) ---",
        resolve_result.unresolved.len()
    );
    for imp in &resolve_result.unresolved {
        println!("  {} -> {}", imp.source, imp.import_path);
    }

    // Build and print the first-party file dependency graph.
    let dep_graph = graph::build_file_dependency_graph(&discovery_result, &resolve_result);

    println!("\n--- dependency graph ---\n");
    let mut nodes: Vec<_> = dep_graph.keys().collect();
    nodes.sort();
    for node in nodes {
        println!("{}", node.display());
        for dep in &dep_graph[node] {
            println!("  -> {}", dep.display());
        }
    }

    // Detect dependency cycles and filter by configured SCC size bounds.
    let all_cycles = graph::dependency_cycles(&dep_graph);
    let cycles = cycles::filter_cycles_by_size(all_cycles, &config.cycles);

    println!("\n--- dependency cycles ({}) ---", cycles.len());
    for (i, cycle) in cycles.iter().enumerate() {
        println!("\ncycle {} ({} files)", i + 1, cycle.len());
        for path in cycle {
            println!("  {}", path.display());
        }
    }
}
