use rustpython_parser::ast::{ExceptHandler, Stmt, Suite};

use super::{ImportKind, ImportedName, RawImport};

/// Maps byte offsets in source to 1-indexed line numbers.
struct LineMap {
    newline_offsets: Vec<usize>,
}

impl LineMap {
    fn new(source: &str) -> Self {
        let newline_offsets = source
            .as_bytes()
            .iter()
            .enumerate()
            .filter_map(|(offset, &byte)| (byte == b'\n').then_some(offset))
            .collect();

        Self { newline_offsets }
    }

    fn line_for_offset(&self, offset: usize) -> u32 {
        self.newline_offsets
            .partition_point(|&newline| newline < offset) as u32
            + 1
    }
}

/// Walk a parsed module body and extract import statements.
///
/// When `include_local` is `false` (the default), only top-level statements
/// are inspected. Imports nested inside functions, classes, or control-flow
/// blocks are intentionally ignored.
///
/// When `include_local` is `true`, the walker recurses into all nested
/// statement bodies (functions, classes, if/for/while/with/try blocks) so
/// that function-scoped ("local") imports are also collected.
pub(crate) fn collect_imports(body: &Suite, source: &str, include_local: bool) -> Vec<RawImport> {
    let mut imports = Vec::new();
    let line_map = LineMap::new(source);
    collect_imports_recursive(body, &line_map, include_local, &mut imports);
    imports
}

fn collect_imports_recursive(
    body: &[Stmt],
    line_map: &LineMap,
    include_local: bool,
    imports: &mut Vec<RawImport>,
) {
    for stmt in body {
        match stmt {
            Stmt::Import(import_stmt) => {
                let offset = u32::from(import_stmt.range.start()) as usize;
                let line = line_map.line_for_offset(offset);

                let names = import_stmt
                    .names
                    .iter()
                    .map(|alias| ImportedName {
                        name: alias.name.to_string(),
                        asname: alias.asname.as_ref().map(|id| id.to_string()),
                    })
                    .collect();

                imports.push(RawImport {
                    kind: ImportKind::Import,
                    module: None,
                    names,
                    level: 0,
                    line,
                });
            }
            Stmt::ImportFrom(import_from) => {
                let offset = u32::from(import_from.range.start()) as usize;
                let line = line_map.line_for_offset(offset);

                let module = import_from.module.as_ref().map(|id| id.to_string());

                let level = import_from.level.as_ref().map(|l| l.to_u32()).unwrap_or(0);

                let names = import_from
                    .names
                    .iter()
                    .map(|alias| ImportedName {
                        name: alias.name.to_string(),
                        asname: alias.asname.as_ref().map(|id| id.to_string()),
                    })
                    .collect();

                imports.push(RawImport {
                    kind: ImportKind::ImportFrom,
                    module,
                    names,
                    level,
                    line,
                });
            }
            _ if include_local => {
                for nested_body in nested_bodies(stmt) {
                    collect_imports_recursive(nested_body, line_map, true, imports);
                }
            }
            _ => {}
        }
    }
}

/// Return all nested statement bodies for a given statement.
///
/// This covers functions, classes, control-flow, with-blocks, try/except,
/// and match/case — every AST node that can contain nested import statements.
fn nested_bodies(stmt: &Stmt) -> Vec<&[Stmt]> {
    match stmt {
        Stmt::FunctionDef(f) => vec![&f.body],
        Stmt::AsyncFunctionDef(f) => vec![&f.body],
        Stmt::ClassDef(c) => vec![&c.body],
        Stmt::For(f) => vec![&f.body, &f.orelse],
        Stmt::AsyncFor(f) => vec![&f.body, &f.orelse],
        Stmt::While(w) => vec![&w.body, &w.orelse],
        Stmt::If(i) => vec![&i.body, &i.orelse],
        Stmt::With(w) => vec![&w.body],
        Stmt::AsyncWith(w) => vec![&w.body],
        Stmt::Try(t) => {
            let mut bodies: Vec<&[Stmt]> = vec![&t.body, &t.orelse, &t.finalbody];
            for handler in &t.handlers {
                let ExceptHandler::ExceptHandler(h) = handler;
                bodies.push(&h.body);
            }
            bodies
        }
        Stmt::TryStar(t) => {
            let mut bodies: Vec<&[Stmt]> = vec![&t.body, &t.orelse, &t.finalbody];
            for handler in &t.handlers {
                let ExceptHandler::ExceptHandler(h) = handler;
                bodies.push(&h.body);
            }
            bodies
        }
        Stmt::Match(m) => m.cases.iter().map(|c| c.body.as_slice()).collect(),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use rustpython_parser::{Parse, ast};

    use super::*;

    fn parse_and_collect(source: &str) -> Vec<RawImport> {
        let suite = ast::Suite::parse(source, "<test>").expect("source should parse");
        collect_imports(&suite, source, false)
    }

    fn parse_and_collect_all(source: &str) -> Vec<RawImport> {
        let suite = ast::Suite::parse(source, "<test>").expect("source should parse");
        collect_imports(&suite, source, true)
    }

    #[test]
    fn simple_import() {
        let imports = parse_and_collect("import os");
        assert_eq!(imports.len(), 1);

        let imp = &imports[0];
        assert!(matches!(imp.kind, ImportKind::Import));
        assert_eq!(imp.module, None);
        assert_eq!(imp.level, 0);
        assert_eq!(imp.names.len(), 1);
        assert_eq!(imp.names[0].name, "os");
        assert_eq!(imp.names[0].asname, None);
    }

    #[test]
    fn import_multiple_names() {
        let imports = parse_and_collect("import os, sys");
        assert_eq!(imports.len(), 1);

        let imp = &imports[0];
        assert!(matches!(imp.kind, ImportKind::Import));
        assert_eq!(imp.names.len(), 2);
        assert_eq!(imp.names[0].name, "os");
        assert_eq!(imp.names[1].name, "sys");
    }

    #[test]
    fn from_import() {
        let imports = parse_and_collect("from os import path");
        assert_eq!(imports.len(), 1);

        let imp = &imports[0];
        assert!(matches!(imp.kind, ImportKind::ImportFrom));
        assert_eq!(imp.module.as_deref(), Some("os"));
        assert_eq!(imp.level, 0);
        assert_eq!(imp.names.len(), 1);
        assert_eq!(imp.names[0].name, "path");
        assert_eq!(imp.names[0].asname, None);
    }

    #[test]
    fn from_import_with_alias() {
        let imports = parse_and_collect("from os import path as p");
        assert_eq!(imports.len(), 1);

        let imp = &imports[0];
        assert!(matches!(imp.kind, ImportKind::ImportFrom));
        assert_eq!(imp.module.as_deref(), Some("os"));
        assert_eq!(imp.names.len(), 1);
        assert_eq!(imp.names[0].name, "path");
        assert_eq!(imp.names[0].asname.as_deref(), Some("p"));
    }

    #[test]
    fn from_dotted_import_multiple() {
        let imports = parse_and_collect("from os.path import join, dirname");
        assert_eq!(imports.len(), 1);

        let imp = &imports[0];
        assert!(matches!(imp.kind, ImportKind::ImportFrom));
        assert_eq!(imp.module.as_deref(), Some("os.path"));
        assert_eq!(imp.names.len(), 2);
        assert_eq!(imp.names[0].name, "join");
        assert_eq!(imp.names[1].name, "dirname");
    }

    #[test]
    fn relative_import_single_dot() {
        let imports = parse_and_collect("from . import sibling");
        assert_eq!(imports.len(), 1);

        let imp = &imports[0];
        assert!(matches!(imp.kind, ImportKind::ImportFrom));
        assert_eq!(imp.module, None);
        assert_eq!(imp.level, 1);
        assert_eq!(imp.names[0].name, "sibling");
    }

    #[test]
    fn relative_import_double_dot() {
        let imports = parse_and_collect("from ..pkg import thing");
        assert_eq!(imports.len(), 1);

        let imp = &imports[0];
        assert!(matches!(imp.kind, ImportKind::ImportFrom));
        assert_eq!(imp.module.as_deref(), Some("pkg"));
        assert_eq!(imp.level, 2);
        assert_eq!(imp.names[0].name, "thing");
    }

    #[test]
    fn relative_import_triple_dot() {
        let imports = parse_and_collect("from ...deep import x");
        assert_eq!(imports.len(), 1);

        let imp = &imports[0];
        assert!(matches!(imp.kind, ImportKind::ImportFrom));
        assert_eq!(imp.module.as_deref(), Some("deep"));
        assert_eq!(imp.level, 3);
        assert_eq!(imp.names[0].name, "x");
    }

    #[test]
    fn star_import() {
        let imports = parse_and_collect("from x import *");
        assert_eq!(imports.len(), 1);

        let imp = &imports[0];
        assert!(matches!(imp.kind, ImportKind::ImportFrom));
        assert_eq!(imp.module.as_deref(), Some("x"));
        assert_eq!(imp.names.len(), 1);
        assert_eq!(imp.names[0].name, "*");
    }

    #[test]
    fn empty_file() {
        let imports = parse_and_collect("");
        assert!(imports.is_empty());
    }

    #[test]
    fn no_imports() {
        let imports = parse_and_collect("x = 1\nprint(x)\n");
        assert!(imports.is_empty());
    }

    #[test]
    fn multiple_import_statements() {
        let source = "\
import os
import sys
from pathlib import Path
from . import local
";
        let imports = parse_and_collect(source);
        assert_eq!(imports.len(), 4);

        assert!(matches!(imports[0].kind, ImportKind::Import));
        assert_eq!(imports[0].names[0].name, "os");

        assert!(matches!(imports[1].kind, ImportKind::Import));
        assert_eq!(imports[1].names[0].name, "sys");

        assert!(matches!(imports[2].kind, ImportKind::ImportFrom));
        assert_eq!(imports[2].module.as_deref(), Some("pathlib"));
        assert_eq!(imports[2].names[0].name, "Path");

        assert!(matches!(imports[3].kind, ImportKind::ImportFrom));
        assert_eq!(imports[3].level, 1);
        assert_eq!(imports[3].names[0].name, "local");
    }

    #[test]
    fn import_with_alias() {
        let imports = parse_and_collect("import numpy as np");
        assert_eq!(imports.len(), 1);

        let imp = &imports[0];
        assert!(matches!(imp.kind, ImportKind::Import));
        assert_eq!(imp.names[0].name, "numpy");
        assert_eq!(imp.names[0].asname.as_deref(), Some("np"));
    }

    #[test]
    fn imports_mixed_with_code() {
        let source = "\
import os

x = 1

from sys import argv

def foo():
    pass
";
        let imports = parse_and_collect(source);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].names[0].name, "os");
        assert_eq!(imports[1].names[0].name, "argv");
    }

    #[test]
    fn local_imports_skipped_by_default() {
        let source = "\
import os

def foo():
    from sys import argv
";
        let imports = parse_and_collect(source);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names[0].name, "os");
    }

    #[test]
    fn local_imports_included_when_enabled() {
        let source = "\
import os

def foo():
    from sys import argv
";
        let imports = parse_and_collect_all(source);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].names[0].name, "os");
        assert_eq!(imports[1].names[0].name, "argv");
    }

    #[test]
    fn local_imports_in_class_method() {
        let source = "\
class Foo:
    def bar(self):
        from utils import helper
";
        let top_only = parse_and_collect(source);
        assert!(top_only.is_empty());

        let all = parse_and_collect_all(source);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].module.as_deref(), Some("utils"));
        assert_eq!(all[0].names[0].name, "helper");
    }

    #[test]
    fn local_imports_in_if_block() {
        let source = "\
if True:
    import json
";
        let top_only = parse_and_collect(source);
        assert!(top_only.is_empty());

        let all = parse_and_collect_all(source);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].names[0].name, "json");
    }

    #[test]
    fn local_imports_in_try_except() {
        let source = "\
try:
    from fast_impl import func
except ImportError:
    from slow_impl import func
";
        let top_only = parse_and_collect(source);
        assert!(top_only.is_empty());

        let all = parse_and_collect_all(source);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].module.as_deref(), Some("fast_impl"));
        assert_eq!(all[1].module.as_deref(), Some("slow_impl"));
    }

    #[test]
    fn import_line_numbers() {
        let source = "import os\nfrom sys import argv\nimport json\n";
        let imports = parse_and_collect(source);
        assert_eq!(imports[0].line, 1);
        assert_eq!(imports[1].line, 2);
        assert_eq!(imports[2].line, 3);
    }

    #[test]
    fn import_line_numbers_with_blank_lines() {
        let source = "import os\n\nfrom sys import argv\n";
        let imports = parse_and_collect(source);
        assert_eq!(imports[0].line, 1);
        assert_eq!(imports[1].line, 3);
    }

    #[test]
    fn line_map_counts_only_newlines_before_offset() {
        let source = "x\nimport os\n";
        let line_map = LineMap::new(source);

        assert_eq!(line_map.line_for_offset(0), 1);
        assert_eq!(line_map.line_for_offset(1), 1);
        assert_eq!(line_map.line_for_offset(source.find("import").unwrap()), 2);
    }
}
