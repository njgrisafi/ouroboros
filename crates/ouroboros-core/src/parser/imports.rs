use rustpython_parser::ast::{
    Arguments, Constant, Expr, ExprCall, Keyword, MatchCase, Stmt, Suite, Visitor, WithItem,
};
use rustpython_parser::text_size::TextSize;

use super::{ExtractOptions, ImportKind, ImportedName, RawImport, StringImportsMode};

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

/// Walk a parsed module and extract imports (statement-level and, when
/// enabled, string-literal module-path candidates).
///
/// When `options.include_local` is `false`, only top-level imports are
/// collected; imports (and string candidates) nested inside functions,
/// classes, or control-flow blocks are ignored. When `true`, all nesting
/// levels are collected.
///
/// String-import candidates (`options.string_imports`) respect the same
/// nesting gate: with `include_local` off, only module-level string literals
/// are scanned. Note that default arguments and decorators of a module-level
/// `def` are evaluated at module level, so strings there count as
/// module-level too.
pub(crate) fn collect_imports(
    suite: Suite,
    source: &str,
    options: &ExtractOptions,
) -> Vec<RawImport> {
    let mut collector = ImportCollector {
        line_map: LineMap::new(source),
        options,
        depth: 0,
        imports: Vec::new(),
    };
    for stmt in suite {
        collector.visit_stmt(stmt);
    }
    collector.imports
}

/// A single-pass AST visitor that collects both `import`/`from` statements
/// and (optionally) string-literal module-path candidates.
///
/// `depth` is the 1-indexed statement nesting depth: top-level statements are
/// visited at depth 1, so the `include_local` gate is `depth == 1 ||
/// include_local`. Expressions inside a statement observe that statement's
/// depth, which is why default arguments and decorators of a module-level
/// `def` count as module-level.
struct ImportCollector<'a> {
    line_map: LineMap,
    options: &'a ExtractOptions,
    depth: usize,
    imports: Vec<RawImport>,
}

impl ImportCollector<'_> {
    /// Whether facts at the current statement depth are collected.
    fn at_included_depth(&self) -> bool {
        self.depth == 1 || self.options.include_local
    }

    fn line_for(&self, offset: u32) -> u32 {
        self.line_map.line_for_offset(offset as usize)
    }

    /// Validate and record a string literal as a module-path candidate.
    ///
    /// In `CallSites` mode the dot requirement is dropped (`min_dots = 0`):
    /// exact resolution in the resolver is the precision mechanism there, and
    /// `import_module("plugins")` on a single-segment module is legitimate.
    fn push_string_candidate(&mut self, value: &str, start: TextSize) {
        let min_dots = match self.options.string_imports_mode {
            StringImportsMode::All => self.options.string_imports_min_dots,
            StringImportsMode::CallSites => 0,
        };
        if let Some(candidate) = string_import_candidate(value, min_dots) {
            let line = self.line_for(u32::from(start));
            self.imports.push(RawImport {
                kind: ImportKind::StringImport,
                module: None,
                names: vec![ImportedName {
                    name: candidate,
                    asname: None,
                }],
                level: 0,
                line,
            });
        }
    }
}

/// Whether a call is `import_module(...)` (bare or `importlib.import_module`)
/// or `__import__(...)` — the two stdlib dynamic-import entry points.
fn is_dynamic_import_call(call: &ExprCall) -> bool {
    match call.func.as_ref() {
        Expr::Name(name) => matches!(&*name.id, "import_module" | "__import__"),
        Expr::Attribute(attr) => &*attr.attr == "import_module",
        _ => false,
    }
}

impl Visitor for ImportCollector<'_> {
    fn visit_stmt(&mut self, node: Stmt) {
        self.depth += 1;
        if self.at_included_depth() {
            match &node {
                Stmt::Import(import_stmt) => {
                    let line = self.line_for(u32::from(import_stmt.range.start()));
                    let names = import_stmt
                        .names
                        .iter()
                        .map(|alias| ImportedName {
                            name: alias.name.to_string(),
                            asname: alias.asname.as_ref().map(|id| id.to_string()),
                        })
                        .collect();
                    self.imports.push(RawImport {
                        kind: ImportKind::Import,
                        module: None,
                        names,
                        level: 0,
                        line,
                    });
                }
                Stmt::ImportFrom(import_from) => {
                    let line = self.line_for(u32::from(import_from.range.start()));
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
                    self.imports.push(RawImport {
                        kind: ImportKind::ImportFrom,
                        module,
                        names,
                        level,
                        line,
                    });
                }
                _ => {}
            }
        }
        // Recursing below this statement can only yield facts when local
        // imports are included, or — at module level — when string scanning
        // needs this statement's own expressions (decorators, defaults, RHS).
        // Otherwise skip the subtree: nested statements are excluded by the
        // depth gate and expressions have nothing to collect.
        if self.options.include_local || (self.depth == 1 && self.options.string_imports) {
            self.generic_visit_stmt(node);
        }
        self.depth -= 1;
    }

    fn visit_expr(&mut self, node: Expr) {
        // Expressions can never contain import statements, so when string
        // imports are disabled there is nothing to find below this point.
        if !self.options.string_imports {
            return;
        }
        match self.options.string_imports_mode {
            StringImportsMode::All => {
                if self.at_included_depth()
                    && let Expr::Constant(constant) = &node
                    && let Constant::Str(value) = &constant.value
                {
                    self.push_string_candidate(value, constant.range.start());
                }
            }
            StringImportsMode::CallSites => {
                if self.at_included_depth()
                    && let Expr::Call(call) = &node
                    && is_dynamic_import_call(call)
                    && let Some(Expr::Constant(constant)) = call.args.first()
                    && let Constant::Str(value) = &constant.value
                {
                    self.push_string_candidate(value, constant.range.start());
                }
            }
        }
        match node {
            // f-string literal fragments are not candidates (a middle fragment
            // like `a.b` in `f"{x}.a.b.{y}"` is not a module path), but
            // interpolations may contain candidates of their own.
            Expr::JoinedStr(joined) => {
                for value in joined.values {
                    if matches!(value, Expr::FormattedValue(_)) {
                        self.visit_expr(value);
                    }
                }
            }
            other => self.generic_visit_expr(other),
        }
    }

    // The crate's `generic_visit_*` bodies for arguments, keywords, with-items,
    // and match cases are empty, so defaults, `with` context expressions,
    // keyword arguments, and match guards would never be scanned for string
    // candidates. Override them to recurse manually.

    fn visit_arguments(&mut self, node: Arguments) {
        for arg in node
            .posonlyargs
            .into_iter()
            .chain(node.args)
            .chain(node.kwonlyargs)
        {
            if let Some(default) = arg.default {
                self.visit_expr(*default);
            }
        }
    }

    fn visit_keyword(&mut self, node: Keyword) {
        self.visit_expr(node.value);
    }

    fn visit_withitem(&mut self, node: WithItem) {
        self.visit_expr(node.context_expr);
        if let Some(optional_vars) = node.optional_vars {
            self.visit_expr(*optional_vars);
        }
    }

    fn visit_match_case(&mut self, node: MatchCase) {
        self.visit_pattern(node.pattern);
        if let Some(guard) = node.guard {
            self.visit_expr(*guard);
        }
        for stmt in node.body {
            self.visit_stmt(stmt);
        }
    }
}

/// Validate a string literal as a dotted module-path candidate.
///
/// Mirrors ruff's check: the string must have at least `min_dots` dots
/// (`min_dots == 0` disables the dot requirement) and consist of non-empty
/// dot-separated identifier segments. Keywords are allowed as segments —
/// `importlib.import_module("a.class.b")` can import a `class.py` at runtime,
/// so the check is purely syntactic (identifier shape, no keyword list).
fn string_import_candidate(value: &str, min_dots: usize) -> Option<String> {
    if min_dots > 0 && value.bytes().filter(|&b| b == b'.').count() < min_dots {
        return None;
    }
    let valid = value.split('.').all(|segment| {
        let mut chars = segment.chars();
        match chars.next() {
            Some(first) if first == '_' || first.is_alphabetic() => {
                chars.all(|c| c == '_' || c.is_alphanumeric())
            }
            _ => false,
        }
    });
    valid.then(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use rustpython_parser::{Parse, ast};

    use super::*;
    use crate::parser::DEFAULT_STRING_IMPORTS_MIN_DOTS;

    fn parse_and_collect(source: &str) -> Vec<RawImport> {
        let suite = ast::Suite::parse(source, "<test>").expect("source should parse");
        collect_imports(suite, source, &ExtractOptions::default())
    }

    fn parse_and_collect_all(source: &str) -> Vec<RawImport> {
        let suite = ast::Suite::parse(source, "<test>").expect("source should parse");
        let options = ExtractOptions {
            include_local: true,
            ..ExtractOptions::default()
        };
        collect_imports(suite, source, &options)
    }

    /// String-import candidates in `All` mode with the given min-dots.
    fn parse_and_collect_strings(
        source: &str,
        include_local: bool,
        min_dots: usize,
    ) -> Vec<RawImport> {
        let suite = ast::Suite::parse(source, "<test>").expect("source should parse");
        let options = ExtractOptions {
            include_local,
            string_imports: true,
            string_imports_mode: StringImportsMode::All,
            string_imports_min_dots: min_dots,
        };
        collect_imports(suite, source, &options)
    }

    /// String-import candidates in `CallSites` mode.
    fn parse_and_collect_call_sites(source: &str, include_local: bool) -> Vec<RawImport> {
        let suite = ast::Suite::parse(source, "<test>").expect("source should parse");
        let options = ExtractOptions {
            include_local,
            string_imports: true,
            string_imports_mode: StringImportsMode::CallSites,
            string_imports_min_dots: DEFAULT_STRING_IMPORTS_MIN_DOTS,
        };
        collect_imports(suite, source, &options)
    }

    /// String-import candidates with default min-dots, at module level only.
    fn collect_strings(source: &str) -> Vec<RawImport> {
        parse_and_collect_strings(source, false, DEFAULT_STRING_IMPORTS_MIN_DOTS)
            .into_iter()
            .filter(|imp| imp.kind == ImportKind::StringImport)
            .collect()
    }

    /// Call-site candidates at module level, names only.
    fn collect_call_site_names(source: &str) -> Vec<String> {
        parse_and_collect_call_sites(source, false)
            .into_iter()
            .filter(|imp| imp.kind == ImportKind::StringImport)
            .map(|imp| imp.names[0].name.clone())
            .collect()
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

    // --- string imports ---

    #[test]
    fn string_import_basic_candidate() {
        let imports = collect_strings("mod = importlib.import_module(\"a.b.c\")\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].kind, ImportKind::StringImport);
        assert_eq!(imports[0].module, None);
        assert_eq!(imports[0].level, 0);
        assert_eq!(imports[0].names.len(), 1);
        assert_eq!(imports[0].names[0].name, "a.b.c");
        assert_eq!(imports[0].names[0].asname, None);
        assert_eq!(imports[0].line, 1);
    }

    #[test]
    fn string_import_any_string_literal_counts() {
        // Aggressive, like ruff: not gated on importlib/__import__ call sites.
        let source = "SETTINGS_MODULE = \"my.pkg.settings\"\n";
        let imports = collect_strings(source);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names[0].name, "my.pkg.settings");
    }

    #[test]
    fn string_import_min_dots_default_rejects_one_dot() {
        let imports = collect_strings("x = \"foo.bar\"\n");
        assert!(
            imports.is_empty(),
            "default min-dots=2 must reject {imports:?}"
        );
    }

    #[test]
    fn string_import_min_dots_one() {
        let imports: Vec<_> = parse_and_collect_strings("x = \"foo.bar\"\ny = \"baz\"\n", false, 1)
            .into_iter()
            .filter(|imp| imp.kind == ImportKind::StringImport)
            .collect();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names[0].name, "foo.bar");
    }

    #[test]
    fn string_import_min_dots_zero() {
        // Shockingly aggressive (ruff parity): any identifier-looking string.
        let imports: Vec<_> =
            parse_and_collect_strings("if name == \"utils\":\n    pass\n", false, 0)
                .into_iter()
                .filter(|imp| imp.kind == ImportKind::StringImport)
                .collect();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names[0].name, "utils");
    }

    #[test]
    fn string_import_rejects_invalid_module_paths() {
        let source = "\
a = \"hello world\"
b = \"a-b.c\"
c = \"/abs/path\"
d = \"a.2b.c\"
e = \"a..b\"
f = \".a.b\"
g = \"a.b.\"
h = \".\"
i = \"\"
";
        let imports = parse_and_collect_strings(source, false, 0);
        assert!(
            imports
                .iter()
                .all(|imp| imp.kind != ImportKind::StringImport),
            "no invalid path may become a candidate: {imports:?}"
        );
    }

    #[test]
    fn string_import_allows_keyword_segments() {
        // `importlib.import_module("a.class.b")` can import a `class.py` at
        // runtime; the validation is purely syntactic.
        let imports = collect_strings("x = \"a.class.b\"\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names[0].name, "a.class.b");
    }

    #[test]
    fn string_import_allows_raw_string() {
        let imports = collect_strings("x = r\"a.b.c\"\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names[0].name, "a.b.c");
    }

    #[test]
    fn string_import_ignores_byte_string() {
        let imports = collect_strings("x = b\"a.b.c\"\n");
        assert!(imports.is_empty());
    }

    #[test]
    fn string_import_unicode_identifiers() {
        // PEP 3131: Python allows unicode identifiers; pinned behavior.
        let imports = collect_strings("x = \"módulo.util.cosa\"\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names[0].name, "módulo.util.cosa");
    }

    #[test]
    fn string_import_implicit_concatenation() {
        // Pin rustpython's folding behavior for `"a.b" ".c"`.
        let imports = collect_strings("x = \"a.b\" \".c\"\n");
        let names: Vec<&str> = imports.iter().map(|i| i.names[0].name.as_str()).collect();
        assert!(
            names == vec!["a.b.c"] || names.is_empty(),
            "concatenation either folds to one candidate or yields none; got {names:?}"
        );
    }

    #[test]
    fn string_import_ignores_fstring_fragments() {
        let imports = collect_strings("x = f\"{prefix}.a.b\"\n");
        assert!(
            imports.is_empty(),
            "f-string literal fragments must not be candidates: {imports:?}"
        );
    }

    #[test]
    fn string_import_scans_fstring_interpolations() {
        let imports = collect_strings("x = f\"{load('a.b.c')}\"\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names[0].name, "a.b.c");
    }

    #[test]
    fn string_import_docstring_counted() {
        // Ruff parity: docstrings are Stmt::Expr strings and are scanned.
        let source = "\"\"\"See a.b.c for details.\"\"\"\n\"a.b.c\"\n";
        let imports = collect_strings(source);
        assert!(
            imports.iter().any(|imp| imp.names[0].name == "a.b.c"),
            "bare dotted-string expression (docstring position) is a candidate: {imports:?}"
        );
    }

    #[test]
    fn string_import_multiline_docstring_line_is_start_line() {
        let source = "\
CONST = 1
\"a.b.c\"
x = \"d.e.f\"
";
        let imports = collect_strings(source);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].line, 2);
        assert_eq!(imports[1].line, 3);
    }

    #[test]
    fn string_import_type_checking_block_scanned_as_real() {
        // Oboros has no TYPE_CHECKING concept: strings there are real
        // candidates (module-level `if` bodies are nested, so this needs
        // include_local — same gate as statement imports).
        let source = "\
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    TARGET = \"a.b.c\"
";
        let top_level_only = parse_and_collect_strings(source, false, 2);
        assert!(
            top_level_only
                .iter()
                .all(|imp| imp.kind != ImportKind::StringImport)
        );

        let with_local = parse_and_collect_strings(source, true, 2);
        assert!(
            with_local
                .iter()
                .any(|imp| imp.kind == ImportKind::StringImport && imp.names[0].name == "a.b.c")
        );
    }

    // --- local-imports x string-imports truth table ---

    const TRUTH_TABLE_SOURCE: &str = "\
MODULE_LEVEL = \"a.b.c\"

def load():
    return importlib.import_module(\"d.e.f\")
";

    fn string_candidates(imports: &[RawImport]) -> Vec<&str> {
        imports
            .iter()
            .filter(|imp| imp.kind == ImportKind::StringImport)
            .map(|imp| imp.names[0].name.as_str())
            .collect()
    }

    #[test]
    fn truth_table_local_off_strings_on() {
        let imports = parse_and_collect_strings(TRUTH_TABLE_SOURCE, false, 2);
        assert_eq!(string_candidates(&imports), vec!["a.b.c"]);
    }

    #[test]
    fn truth_table_local_on_strings_on() {
        let imports = parse_and_collect_strings(TRUTH_TABLE_SOURCE, true, 2);
        assert_eq!(string_candidates(&imports), vec!["a.b.c", "d.e.f"]);
    }

    #[test]
    fn truth_table_local_on_strings_off() {
        let suite = ast::Suite::parse(TRUTH_TABLE_SOURCE, "<test>").unwrap();
        let options = ExtractOptions {
            include_local: true,
            string_imports: false,
            ..ExtractOptions::default()
        };
        let imports = collect_imports(suite, TRUTH_TABLE_SOURCE, &options);
        assert!(string_candidates(&imports).is_empty());
    }

    #[test]
    fn truth_table_local_off_strings_off() {
        let imports = parse_and_collect(TRUTH_TABLE_SOURCE);
        assert!(string_candidates(&imports).is_empty());
    }

    #[test]
    fn string_import_default_arg_is_module_level() {
        // Defaults evaluate at def time (module import time), so they count
        // as module-level even with include_local off.
        let imports = collect_strings("def f(x=\"a.b.c\"):\n    pass\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names[0].name, "a.b.c");
    }

    #[test]
    fn string_import_with_statement_context_expr() {
        // The crate's generic with-item visitor is empty; ours must recurse.
        let imports = collect_strings("with load(\"a.b.c\") as f:\n    pass\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names[0].name, "a.b.c");
    }

    #[test]
    fn string_import_keyword_argument_value() {
        let imports = collect_strings("register(name=\"a.b.c\")\n");
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names[0].name, "a.b.c");
    }

    #[test]
    fn string_import_match_guard() {
        // The crate's generic match-case visitor is empty; ours must recurse.
        let source = "\
match x:
    case y if y in \"a.b.c\":
        pass
";
        let imports = collect_strings(source);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].names[0].name, "a.b.c");
    }

    // --- call-sites mode ---

    #[test]
    fn call_sites_detects_importlib_attribute_call() {
        let names = collect_call_site_names("mod = importlib.import_module(\"a.b.c\")\n");
        assert_eq!(names, vec!["a.b.c"]);
    }

    #[test]
    fn call_sites_detects_bare_import_module() {
        let source = "from importlib import import_module\nmod = import_module(\"a.b.c\")\n";
        let names = collect_call_site_names(source);
        assert_eq!(names, vec!["a.b.c"]);
    }

    #[test]
    fn call_sites_detects_dunder_import() {
        let names = collect_call_site_names("mod = __import__(\"a.b.c\")\n");
        assert_eq!(names, vec!["a.b.c"]);
    }

    #[test]
    fn call_sites_ignores_registry_strings() {
        // The ruff-style "all" mode flags these; call-sites mode must not.
        let source = "\
SETTINGS_MODULE = \"my.pkg.settings\"
REGISTRY = {\"a.b.c\": 1}
TASK = \"tasks.send_email\"
";
        assert!(collect_call_site_names(source).is_empty());
    }

    #[test]
    fn call_sites_ignores_non_first_args_and_other_calls() {
        let source = "\
load(\"a.b.c\")                      # not a dynamic-import call
import_module(name=\"d.e.f\")         # keyword-only: not scanned
register(\"x\", \"g.h.i\")              # second arg
";
        assert!(collect_call_site_names(source).is_empty());
    }

    #[test]
    fn call_sites_single_segment_allowed() {
        // min-dots is ignored in call-sites mode: import_module("plugins") is
        // a legitimate dynamic import of a single-segment module.
        let names = collect_call_site_names("import_module(\"plugins\")\n");
        assert_eq!(names, vec!["plugins"]);
    }

    #[test]
    fn call_sites_nested_call_respects_local_imports_gate() {
        let source = "\
def load():
    return importlib.import_module(\"a.b.c\")
";
        assert!(collect_call_site_names(source).is_empty());

        let with_local: Vec<_> = parse_and_collect_call_sites(source, true)
            .into_iter()
            .filter(|imp| imp.kind == ImportKind::StringImport)
            .collect();
        assert_eq!(with_local.len(), 1);
        assert_eq!(with_local[0].names[0].name, "a.b.c");
    }

    #[test]
    fn call_sites_variable_first_arg_not_scanned() {
        let source = "name = \"a.b.c\"\nimport_module(name)\n";
        assert!(collect_call_site_names(source).is_empty());
    }

    #[test]
    fn call_sites_fstring_first_arg_not_scanned() {
        let names = collect_call_site_names("import_module(f\"a.b.{suffix}\")\n");
        assert!(names.is_empty());
    }

    #[test]
    fn string_import_class_body_is_local() {
        let source = "\
class C:
    TARGET = \"a.b.c\"
";
        assert!(collect_strings(source).is_empty());
        let with_local: Vec<_> = parse_and_collect_strings(source, true, 2)
            .into_iter()
            .filter(|imp| imp.kind == ImportKind::StringImport)
            .collect();
        assert_eq!(with_local.len(), 1);
    }
}
