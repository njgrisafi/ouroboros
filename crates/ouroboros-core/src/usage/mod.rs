pub mod walk;

pub use walk::{InitUse, UseContext, scan_init_time_uses};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitUseEdge {
    pub source: String,
    pub target: String,
    pub use_line: u32,
    pub context: UseContext,
}

pub fn collect_init_use_edges(
    discovery: &crate::discovery::DiscoveryResult,
    index: &crate::resolver::index::ModuleIndex,
    project_root: &std::path::Path,
) -> Vec<InitUseEdge> {
    let mut edges = Vec::new();

    for root in &discovery.roots {
        for file in &root.files {
            let abs_path = project_root.join(&file.rel_path);
            let source = match std::fs::read_to_string(&abs_path) {
                Ok(source) => source,
                Err(_) => continue,
            };
            let source_is_package = file
                .rel_path
                .file_name()
                .is_some_and(|name| name == "__init__.py");
            let uses = scan_init_time_uses(&source, &file.module_name, index, source_is_package);

            for init_use in uses {
                edges.push(InitUseEdge {
                    source: file.module_name.clone(),
                    target: init_use.target_module,
                    use_line: init_use.line,
                    context: init_use.context,
                });
            }
        }
    }

    edges.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.target.cmp(&b.target))
            .then(a.use_line.cmp(&b.use_line))
            .then(use_context_rank(&a.context).cmp(&use_context_rank(&b.context)))
    });
    edges.dedup();
    edges
}

pub(crate) const fn use_context_rank(context: &UseContext) -> u8 {
    match context {
        UseContext::ModuleBody => 0,
        UseContext::ClassBody => 1,
        UseContext::Decorator => 2,
        UseContext::BaseClass => 3,
        UseContext::DefaultArg => 4,
        UseContext::Comprehension => 5,
        UseContext::ControlFlow => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{DiscoveryResult, PythonFile, SourceRoot};
    use crate::resolver::index::ModuleIndex;
    use std::path::PathBuf;

    fn make_project(files: &[(&str, &str, &str)]) -> (tempfile::TempDir, DiscoveryResult) {
        let tmp = tempfile::tempdir().unwrap();
        let python_files = files
            .iter()
            .map(|(path, module, source)| {
                let full = tmp.path().join(path);
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(&full, source).unwrap();
                PythonFile {
                    rel_path: PathBuf::from(path),
                    module_name: module.to_string(),
                }
            })
            .collect();

        (
            tmp,
            DiscoveryResult {
                roots: vec![SourceRoot {
                    path: PathBuf::from("/fake/root"),
                    files: python_files,
                }],
            },
        )
    }

    #[test]
    fn collect_edges_for_models_views_serializers_cycle() {
        let (tmp, discovery) = make_project(&[
            (
                "models.py",
                "models",
                "import views\nREGISTERED_VIEW = views.UserView\n",
            ),
            (
                "views.py",
                "views",
                "import serializers\nclass UserView:\n    serializer_class = serializers.UserSerializer\n",
            ),
            (
                "serializers.py",
                "serializers",
                "import models\nclass UserSerializer:\n    class Meta:\n        model = models.User\n",
            ),
        ]);
        let index = ModuleIndex::from_discovery(&discovery);

        let edges = collect_init_use_edges(&discovery, &index, tmp.path());

        assert!(edges.iter().any(|edge| {
            edge.source == "models"
                && edge.target == "views"
                && edge.context == UseContext::ModuleBody
        }));
        assert!(edges.iter().any(|edge| {
            edge.source == "views"
                && edge.target == "serializers"
                && edge.context == UseContext::ClassBody
        }));
        assert!(edges.iter().any(|edge| {
            edge.source == "serializers"
                && edge.target == "models"
                && edge.context == UseContext::ClassBody
        }));
    }
}
