pub mod build;
pub mod scc;

pub use build::{EdgeMetadata, FileDependencyGraph, FileGraphResult, build_file_dependency_graph};
pub use scc::{FileCycle, dependency_cycles, strongly_connected_components};
