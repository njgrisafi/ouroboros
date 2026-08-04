use std::collections::{HashMap, HashSet};

use rustpython_parser::Parse;
use rustpython_parser::ast::{self, ExceptHandler, Expr, ExprContext, Ranged, Stmt, TextSize};

use crate::parser::{ImportKind, ImportedName, RawImport};
use crate::resolver::BindingTarget;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum UseContext {
    ModuleBody,
    ClassBody,
    Decorator,
    BaseClass,
    DefaultArg,
    Comprehension,
    ControlFlow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitUse {
    pub target_module: String,
    pub line: u32,
    pub context: UseContext,
}

pub fn scan_init_time_uses(
    source: &str,
    source_module: &str,
    index: &crate::resolver::index::ModuleIndex,
    source_is_package: bool,
) -> Vec<InitUse> {
    let Ok(suite) = ast::Suite::parse(source, "<source>") else {
        return Vec::new();
    };

    let line_map = LineMap::new(source);
    let mut bindings =
        seed_top_level_bindings(&suite, source_module, index, source_is_package, &line_map);
    let mut uses = Vec::new();

    for stmt in &suite {
        walk_module_stmt(
            stmt,
            &mut bindings,
            &mut uses,
            &line_map,
            UseContext::ModuleBody,
        );
    }

    uses
}

type Bindings = HashMap<String, Vec<BindingTarget>>;

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

fn seed_top_level_bindings(
    suite: &[Stmt],
    source_module: &str,
    index: &crate::resolver::index::ModuleIndex,
    source_is_package: bool,
    line_map: &LineMap,
) -> Bindings {
    let mut bindings: Bindings = HashMap::new();

    for stmt in suite {
        let Some(raw_import) = raw_import_from_stmt(stmt, line_map) else {
            continue;
        };

        for target in crate::resolver::binding::resolve_binding_target(
            source_module,
            &raw_import,
            index,
            source_is_package,
        ) {
            bindings
                .entry(target.root_name.clone())
                .or_default()
                .push(target);
        }
    }

    bindings
}

fn raw_import_from_stmt(stmt: &Stmt, line_map: &LineMap) -> Option<RawImport> {
    match stmt {
        Stmt::Import(import_stmt) => {
            let offset = offset_to_usize(import_stmt.range.start());
            let names = import_stmt
                .names
                .iter()
                .map(|alias| ImportedName {
                    name: alias.name.to_string(),
                    asname: alias.asname.as_ref().map(|id| id.to_string()),
                })
                .collect();

            Some(RawImport {
                kind: ImportKind::Import,
                module: None,
                names,
                level: 0,
                line: line_map.line_for_offset(offset),
            })
        }
        Stmt::ImportFrom(import_from) => {
            let offset = offset_to_usize(import_from.range.start());
            let names = import_from
                .names
                .iter()
                .map(|alias| ImportedName {
                    name: alias.name.to_string(),
                    asname: alias.asname.as_ref().map(|id| id.to_string()),
                })
                .collect();

            Some(RawImport {
                kind: ImportKind::ImportFrom,
                module: import_from.module.as_ref().map(|id| id.to_string()),
                names,
                level: import_from
                    .level
                    .as_ref()
                    .map(|level| level.to_u32())
                    .unwrap_or(0),
                line: line_map.line_for_offset(offset),
            })
        }
        _ => None,
    }
}

fn walk_module_stmt(
    stmt: &Stmt,
    bindings: &mut Bindings,
    uses: &mut Vec<InitUse>,
    line_map: &LineMap,
    ctx: UseContext,
) {
    match stmt {
        Stmt::Import(_) | Stmt::ImportFrom(_) => {}
        Stmt::Assign(assign) => {
            walk_expr(&assign.value, ctx, bindings, None, uses, line_map);
            shadow_targets(&assign.targets, bindings);
        }
        Stmt::AugAssign(assign) => {
            walk_expr(&assign.value, ctx, bindings, None, uses, line_map);
            shadow_target(&assign.target, bindings);
        }
        Stmt::AnnAssign(assign) => {
            if let Some(value) = &assign.value {
                walk_expr(value, ctx, bindings, None, uses, line_map);
            }
            shadow_target(&assign.target, bindings);
        }
        Stmt::Expr(expr) => walk_expr(&expr.value, ctx, bindings, None, uses, line_map),
        Stmt::FunctionDef(func) => {
            walk_function_init(func, bindings, None, uses, line_map);
            bindings.remove(func.name.as_str());
        }
        Stmt::AsyncFunctionDef(func) => {
            walk_async_function_init(func, bindings, None, uses, line_map);
            bindings.remove(func.name.as_str());
        }
        Stmt::ClassDef(class_def) => {
            walk_class_init(class_def, bindings, None, uses, line_map);
            bindings.remove(class_def.name.as_str());
        }
        Stmt::If(if_stmt) => {
            if is_type_checking_test(&if_stmt.test) {
                return;
            }
            walk_expr(
                &if_stmt.test,
                UseContext::ControlFlow,
                bindings,
                None,
                uses,
                line_map,
            );
            for body_stmt in if_stmt.body.iter().chain(if_stmt.orelse.iter()) {
                walk_module_stmt(body_stmt, bindings, uses, line_map, UseContext::ControlFlow);
            }
        }
        Stmt::For(for_stmt) => {
            walk_expr(
                &for_stmt.iter,
                UseContext::ControlFlow,
                bindings,
                None,
                uses,
                line_map,
            );
            for body_stmt in for_stmt.body.iter().chain(for_stmt.orelse.iter()) {
                walk_module_stmt(body_stmt, bindings, uses, line_map, UseContext::ControlFlow);
            }
            shadow_target(&for_stmt.target, bindings);
        }
        Stmt::AsyncFor(for_stmt) => {
            walk_expr(
                &for_stmt.iter,
                UseContext::ControlFlow,
                bindings,
                None,
                uses,
                line_map,
            );
            for body_stmt in for_stmt.body.iter().chain(for_stmt.orelse.iter()) {
                walk_module_stmt(body_stmt, bindings, uses, line_map, UseContext::ControlFlow);
            }
            shadow_target(&for_stmt.target, bindings);
        }
        Stmt::While(while_stmt) => {
            walk_expr(
                &while_stmt.test,
                UseContext::ControlFlow,
                bindings,
                None,
                uses,
                line_map,
            );
            for body_stmt in while_stmt.body.iter().chain(while_stmt.orelse.iter()) {
                walk_module_stmt(body_stmt, bindings, uses, line_map, UseContext::ControlFlow);
            }
        }
        Stmt::With(with_stmt) => {
            for item in &with_stmt.items {
                walk_expr(
                    &item.context_expr,
                    UseContext::ControlFlow,
                    bindings,
                    None,
                    uses,
                    line_map,
                );
            }
            for body_stmt in &with_stmt.body {
                walk_module_stmt(body_stmt, bindings, uses, line_map, UseContext::ControlFlow);
            }
            for item in &with_stmt.items {
                if let Some(optional_vars) = &item.optional_vars {
                    shadow_target(optional_vars, bindings);
                }
            }
        }
        Stmt::AsyncWith(with_stmt) => {
            for item in &with_stmt.items {
                walk_expr(
                    &item.context_expr,
                    UseContext::ControlFlow,
                    bindings,
                    None,
                    uses,
                    line_map,
                );
            }
            for body_stmt in &with_stmt.body {
                walk_module_stmt(body_stmt, bindings, uses, line_map, UseContext::ControlFlow);
            }
            for item in &with_stmt.items {
                if let Some(optional_vars) = &item.optional_vars {
                    shadow_target(optional_vars, bindings);
                }
            }
        }
        Stmt::Try(try_stmt) => walk_try_stmt(
            &try_stmt.body,
            &try_stmt.handlers,
            &try_stmt.orelse,
            &try_stmt.finalbody,
            bindings,
            uses,
            line_map,
        ),
        Stmt::TryStar(try_stmt) => walk_try_stmt(
            &try_stmt.body,
            &try_stmt.handlers,
            &try_stmt.orelse,
            &try_stmt.finalbody,
            bindings,
            uses,
            line_map,
        ),
        Stmt::Match(match_stmt) => {
            walk_expr(
                &match_stmt.subject,
                UseContext::ControlFlow,
                bindings,
                None,
                uses,
                line_map,
            );
            for case in &match_stmt.cases {
                for body_stmt in &case.body {
                    walk_module_stmt(body_stmt, bindings, uses, line_map, UseContext::ControlFlow);
                }
            }
        }
        Stmt::Delete(delete_stmt) => shadow_targets(&delete_stmt.targets, bindings),
        Stmt::Return(_)
        | Stmt::TypeAlias(_)
        | Stmt::Pass(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Raise(_)
        | Stmt::Assert(_)
        | Stmt::Global(_)
        | Stmt::Nonlocal(_) => {}
    }
}

fn walk_try_stmt(
    body: &[Stmt],
    handlers: &[ExceptHandler],
    orelse: &[Stmt],
    finalbody: &[Stmt],
    bindings: &mut Bindings,
    uses: &mut Vec<InitUse>,
    line_map: &LineMap,
) {
    for body_stmt in body.iter().chain(orelse.iter()).chain(finalbody.iter()) {
        walk_module_stmt(body_stmt, bindings, uses, line_map, UseContext::ControlFlow);
    }
    for handler in handlers {
        let ExceptHandler::ExceptHandler(handler) = handler;
        for body_stmt in &handler.body {
            walk_module_stmt(body_stmt, bindings, uses, line_map, UseContext::ControlFlow);
        }
        if let Some(name) = &handler.name {
            bindings.remove(name.as_str());
        }
    }
}

fn walk_function_init(
    func: &ast::StmtFunctionDef,
    bindings: &Bindings,
    class_shadow: Option<&HashSet<String>>,
    uses: &mut Vec<InitUse>,
    line_map: &LineMap,
) {
    for decorator in &func.decorator_list {
        walk_expr(
            decorator,
            UseContext::Decorator,
            bindings,
            class_shadow,
            uses,
            line_map,
        );
    }
    walk_arg_defaults(&func.args, bindings, class_shadow, uses, line_map);
}

fn walk_async_function_init(
    func: &ast::StmtAsyncFunctionDef,
    bindings: &Bindings,
    class_shadow: Option<&HashSet<String>>,
    uses: &mut Vec<InitUse>,
    line_map: &LineMap,
) {
    for decorator in &func.decorator_list {
        walk_expr(
            decorator,
            UseContext::Decorator,
            bindings,
            class_shadow,
            uses,
            line_map,
        );
    }
    walk_arg_defaults(&func.args, bindings, class_shadow, uses, line_map);
}

fn walk_arg_defaults(
    args: &ast::Arguments,
    bindings: &Bindings,
    class_shadow: Option<&HashSet<String>>,
    uses: &mut Vec<InitUse>,
    line_map: &LineMap,
) {
    for default in args.defaults() {
        walk_expr(
            default,
            UseContext::DefaultArg,
            bindings,
            class_shadow,
            uses,
            line_map,
        );
    }
    for default in args
        .kwonlyargs
        .iter()
        .filter_map(|arg| arg.default.as_deref())
    {
        walk_expr(
            default,
            UseContext::DefaultArg,
            bindings,
            class_shadow,
            uses,
            line_map,
        );
    }
}

fn walk_class_init(
    class_def: &ast::StmtClassDef,
    bindings: &Bindings,
    class_shadow: Option<&HashSet<String>>,
    uses: &mut Vec<InitUse>,
    line_map: &LineMap,
) {
    for decorator in &class_def.decorator_list {
        walk_expr(
            decorator,
            UseContext::Decorator,
            bindings,
            class_shadow,
            uses,
            line_map,
        );
    }
    for base in &class_def.bases {
        walk_expr(
            base,
            UseContext::BaseClass,
            bindings,
            class_shadow,
            uses,
            line_map,
        );
    }
    for keyword in &class_def.keywords {
        walk_expr(
            &keyword.value,
            UseContext::BaseClass,
            bindings,
            class_shadow,
            uses,
            line_map,
        );
    }

    let mut child_shadow = HashSet::new();
    for stmt in &class_def.body {
        walk_class_body_stmt(stmt, bindings, &mut child_shadow, uses, line_map);
    }
}

fn walk_class_body_stmt(
    stmt: &Stmt,
    bindings: &Bindings,
    class_shadow: &mut HashSet<String>,
    uses: &mut Vec<InitUse>,
    line_map: &LineMap,
) {
    match stmt {
        Stmt::Import(_) | Stmt::ImportFrom(_) => {}
        Stmt::Assign(assign) => {
            walk_expr(
                &assign.value,
                UseContext::ClassBody,
                bindings,
                Some(class_shadow),
                uses,
                line_map,
            );
            shadow_targets_in_set(&assign.targets, class_shadow);
        }
        Stmt::AugAssign(assign) => {
            walk_expr(
                &assign.value,
                UseContext::ClassBody,
                bindings,
                Some(class_shadow),
                uses,
                line_map,
            );
            shadow_target_in_set(&assign.target, class_shadow);
        }
        Stmt::AnnAssign(assign) => {
            if let Some(value) = &assign.value {
                walk_expr(
                    value,
                    UseContext::ClassBody,
                    bindings,
                    Some(class_shadow),
                    uses,
                    line_map,
                );
            }
            shadow_target_in_set(&assign.target, class_shadow);
        }
        Stmt::Expr(expr) => walk_expr(
            &expr.value,
            UseContext::ClassBody,
            bindings,
            Some(class_shadow),
            uses,
            line_map,
        ),
        Stmt::FunctionDef(func) => {
            walk_function_init(func, bindings, Some(class_shadow), uses, line_map);
            class_shadow.insert(func.name.to_string());
        }
        Stmt::AsyncFunctionDef(func) => {
            walk_async_function_init(func, bindings, Some(class_shadow), uses, line_map);
            class_shadow.insert(func.name.to_string());
        }
        Stmt::ClassDef(class_def) => {
            walk_class_init(class_def, bindings, Some(class_shadow), uses, line_map);
            class_shadow.insert(class_def.name.to_string());
        }
        Stmt::If(if_stmt) => {
            if is_type_checking_test(&if_stmt.test) {
                return;
            }
            walk_expr(
                &if_stmt.test,
                UseContext::ClassBody,
                bindings,
                Some(class_shadow),
                uses,
                line_map,
            );
            for body_stmt in if_stmt.body.iter().chain(if_stmt.orelse.iter()) {
                walk_class_body_stmt(body_stmt, bindings, class_shadow, uses, line_map);
            }
        }
        Stmt::For(for_stmt) => {
            walk_expr(
                &for_stmt.iter,
                UseContext::ClassBody,
                bindings,
                Some(class_shadow),
                uses,
                line_map,
            );
            for body_stmt in for_stmt.body.iter().chain(for_stmt.orelse.iter()) {
                walk_class_body_stmt(body_stmt, bindings, class_shadow, uses, line_map);
            }
            shadow_target_in_set(&for_stmt.target, class_shadow);
        }
        Stmt::AsyncFor(for_stmt) => {
            walk_expr(
                &for_stmt.iter,
                UseContext::ClassBody,
                bindings,
                Some(class_shadow),
                uses,
                line_map,
            );
            for body_stmt in for_stmt.body.iter().chain(for_stmt.orelse.iter()) {
                walk_class_body_stmt(body_stmt, bindings, class_shadow, uses, line_map);
            }
            shadow_target_in_set(&for_stmt.target, class_shadow);
        }
        Stmt::While(while_stmt) => {
            walk_expr(
                &while_stmt.test,
                UseContext::ClassBody,
                bindings,
                Some(class_shadow),
                uses,
                line_map,
            );
            for body_stmt in while_stmt.body.iter().chain(while_stmt.orelse.iter()) {
                walk_class_body_stmt(body_stmt, bindings, class_shadow, uses, line_map);
            }
        }
        Stmt::With(with_stmt) => {
            for item in &with_stmt.items {
                walk_expr(
                    &item.context_expr,
                    UseContext::ClassBody,
                    bindings,
                    Some(class_shadow),
                    uses,
                    line_map,
                );
            }
            for body_stmt in &with_stmt.body {
                walk_class_body_stmt(body_stmt, bindings, class_shadow, uses, line_map);
            }
            for item in &with_stmt.items {
                if let Some(optional_vars) = &item.optional_vars {
                    shadow_target_in_set(optional_vars, class_shadow);
                }
            }
        }
        Stmt::AsyncWith(with_stmt) => {
            for item in &with_stmt.items {
                walk_expr(
                    &item.context_expr,
                    UseContext::ClassBody,
                    bindings,
                    Some(class_shadow),
                    uses,
                    line_map,
                );
            }
            for body_stmt in &with_stmt.body {
                walk_class_body_stmt(body_stmt, bindings, class_shadow, uses, line_map);
            }
            for item in &with_stmt.items {
                if let Some(optional_vars) = &item.optional_vars {
                    shadow_target_in_set(optional_vars, class_shadow);
                }
            }
        }
        Stmt::Try(try_stmt) => walk_class_try_stmt(
            TryBodies {
                body: &try_stmt.body,
                handlers: &try_stmt.handlers,
                orelse: &try_stmt.orelse,
                finalbody: &try_stmt.finalbody,
            },
            bindings,
            class_shadow,
            uses,
            line_map,
        ),
        Stmt::TryStar(try_stmt) => walk_class_try_stmt(
            TryBodies {
                body: &try_stmt.body,
                handlers: &try_stmt.handlers,
                orelse: &try_stmt.orelse,
                finalbody: &try_stmt.finalbody,
            },
            bindings,
            class_shadow,
            uses,
            line_map,
        ),
        Stmt::Match(match_stmt) => {
            walk_expr(
                &match_stmt.subject,
                UseContext::ClassBody,
                bindings,
                Some(class_shadow),
                uses,
                line_map,
            );
            for case in &match_stmt.cases {
                for body_stmt in &case.body {
                    walk_class_body_stmt(body_stmt, bindings, class_shadow, uses, line_map);
                }
            }
        }
        Stmt::Delete(delete_stmt) => shadow_targets_in_set(&delete_stmt.targets, class_shadow),
        Stmt::Return(_)
        | Stmt::TypeAlias(_)
        | Stmt::Pass(_)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::Raise(_)
        | Stmt::Assert(_)
        | Stmt::Global(_)
        | Stmt::Nonlocal(_) => {}
    }
}

struct TryBodies<'a> {
    body: &'a [Stmt],
    handlers: &'a [ExceptHandler],
    orelse: &'a [Stmt],
    finalbody: &'a [Stmt],
}

fn walk_class_try_stmt(
    try_bodies: TryBodies<'_>,
    bindings: &Bindings,
    class_shadow: &mut HashSet<String>,
    uses: &mut Vec<InitUse>,
    line_map: &LineMap,
) {
    for body_stmt in try_bodies
        .body
        .iter()
        .chain(try_bodies.orelse.iter())
        .chain(try_bodies.finalbody.iter())
    {
        walk_class_body_stmt(body_stmt, bindings, class_shadow, uses, line_map);
    }
    for handler in try_bodies.handlers {
        let ExceptHandler::ExceptHandler(handler) = handler;
        for body_stmt in &handler.body {
            walk_class_body_stmt(body_stmt, bindings, class_shadow, uses, line_map);
        }
        if let Some(name) = &handler.name {
            class_shadow.insert(name.to_string());
        }
    }
}

fn walk_expr(
    expr: &Expr,
    ctx: UseContext,
    bindings: &Bindings,
    class_shadow: Option<&HashSet<String>>,
    uses: &mut Vec<InitUse>,
    line_map: &LineMap,
) {
    match expr {
        Expr::Name(name) => {
            if matches!(name.ctx, ExprContext::Load) {
                emit_chain(
                    &[name.id.to_string()],
                    expr,
                    ctx,
                    bindings,
                    class_shadow,
                    uses,
                    line_map,
                );
            }
        }
        Expr::Attribute(attr) => {
            if matches!(attr.ctx, ExprContext::Load) {
                if let Some(chain) = attr_chain(expr) {
                    emit_chain(&chain, expr, ctx, bindings, class_shadow, uses, line_map);
                } else {
                    walk_expr(&attr.value, ctx, bindings, class_shadow, uses, line_map);
                }
            }
        }
        Expr::Call(call) => {
            walk_expr(
                &call.func,
                ctx.clone(),
                bindings,
                class_shadow,
                uses,
                line_map,
            );
            for arg in &call.args {
                walk_expr(arg, ctx.clone(), bindings, class_shadow, uses, line_map);
            }
            for keyword in &call.keywords {
                walk_expr(
                    &keyword.value,
                    ctx.clone(),
                    bindings,
                    class_shadow,
                    uses,
                    line_map,
                );
            }
        }
        Expr::Subscript(subscript) => {
            walk_expr(
                &subscript.value,
                ctx.clone(),
                bindings,
                class_shadow,
                uses,
                line_map,
            );
            walk_expr(
                &subscript.slice,
                ctx,
                bindings,
                class_shadow,
                uses,
                line_map,
            );
        }
        Expr::BoolOp(bool_op) => {
            for value in &bool_op.values {
                walk_expr(value, ctx.clone(), bindings, class_shadow, uses, line_map);
            }
        }
        Expr::BinOp(bin_op) => {
            walk_expr(
                &bin_op.left,
                ctx.clone(),
                bindings,
                class_shadow,
                uses,
                line_map,
            );
            walk_expr(&bin_op.right, ctx, bindings, class_shadow, uses, line_map);
        }
        Expr::UnaryOp(unary_op) => walk_expr(
            &unary_op.operand,
            ctx,
            bindings,
            class_shadow,
            uses,
            line_map,
        ),
        Expr::Compare(compare) => {
            walk_expr(
                &compare.left,
                ctx.clone(),
                bindings,
                class_shadow,
                uses,
                line_map,
            );
            for comparator in &compare.comparators {
                walk_expr(
                    comparator,
                    ctx.clone(),
                    bindings,
                    class_shadow,
                    uses,
                    line_map,
                );
            }
        }
        Expr::IfExp(if_exp) => {
            walk_expr(
                &if_exp.test,
                ctx.clone(),
                bindings,
                class_shadow,
                uses,
                line_map,
            );
            walk_expr(
                &if_exp.body,
                ctx.clone(),
                bindings,
                class_shadow,
                uses,
                line_map,
            );
            walk_expr(&if_exp.orelse, ctx, bindings, class_shadow, uses, line_map);
        }
        Expr::Dict(dict) => {
            for key in dict.keys.iter().flatten() {
                walk_expr(key, ctx.clone(), bindings, class_shadow, uses, line_map);
            }
            for value in &dict.values {
                walk_expr(value, ctx.clone(), bindings, class_shadow, uses, line_map);
            }
        }
        Expr::Set(set) => walk_exprs(&set.elts, ctx, bindings, class_shadow, uses, line_map),
        Expr::List(list) => walk_exprs(&list.elts, ctx, bindings, class_shadow, uses, line_map),
        Expr::Tuple(tuple) => walk_exprs(&tuple.elts, ctx, bindings, class_shadow, uses, line_map),
        Expr::Starred(starred) => {
            walk_expr(&starred.value, ctx, bindings, class_shadow, uses, line_map)
        }
        Expr::Await(await_expr) => walk_expr(
            &await_expr.value,
            ctx,
            bindings,
            class_shadow,
            uses,
            line_map,
        ),
        Expr::Yield(yield_expr) => {
            if let Some(value) = &yield_expr.value {
                walk_expr(value, ctx, bindings, class_shadow, uses, line_map);
            }
        }
        Expr::YieldFrom(yield_from) => walk_expr(
            &yield_from.value,
            ctx,
            bindings,
            class_shadow,
            uses,
            line_map,
        ),
        Expr::NamedExpr(named_expr) => walk_expr(
            &named_expr.value,
            ctx,
            bindings,
            class_shadow,
            uses,
            line_map,
        ),
        Expr::Lambda(lambda) => {
            walk_arg_defaults(&lambda.args, bindings, class_shadow, uses, line_map)
        }
        Expr::ListComp(comp) => {
            walk_expr(
                &comp.elt,
                UseContext::Comprehension,
                bindings,
                class_shadow,
                uses,
                line_map,
            );
            walk_generators(&comp.generators, bindings, class_shadow, uses, line_map);
        }
        Expr::SetComp(comp) => {
            walk_expr(
                &comp.elt,
                UseContext::Comprehension,
                bindings,
                class_shadow,
                uses,
                line_map,
            );
            walk_generators(&comp.generators, bindings, class_shadow, uses, line_map);
        }
        Expr::DictComp(comp) => {
            walk_expr(
                &comp.key,
                UseContext::Comprehension,
                bindings,
                class_shadow,
                uses,
                line_map,
            );
            walk_expr(
                &comp.value,
                UseContext::Comprehension,
                bindings,
                class_shadow,
                uses,
                line_map,
            );
            walk_generators(&comp.generators, bindings, class_shadow, uses, line_map);
        }
        Expr::GeneratorExp(generator) => {
            if let Some(first) = generator.generators.first() {
                walk_expr(
                    &first.iter,
                    UseContext::Comprehension,
                    bindings,
                    class_shadow,
                    uses,
                    line_map,
                );
            }
        }
        Expr::JoinedStr(joined) => {
            walk_exprs(&joined.values, ctx, bindings, class_shadow, uses, line_map);
        }
        Expr::FormattedValue(formatted) => walk_expr(
            &formatted.value,
            ctx,
            bindings,
            class_shadow,
            uses,
            line_map,
        ),
        Expr::Slice(slice) => {
            for value in [&slice.lower, &slice.upper, &slice.step]
                .into_iter()
                .flatten()
            {
                walk_expr(value, ctx.clone(), bindings, class_shadow, uses, line_map);
            }
        }
        Expr::Constant(_) => {}
    }
}

fn walk_exprs(
    exprs: &[Expr],
    ctx: UseContext,
    bindings: &Bindings,
    class_shadow: Option<&HashSet<String>>,
    uses: &mut Vec<InitUse>,
    line_map: &LineMap,
) {
    for expr in exprs {
        walk_expr(expr, ctx.clone(), bindings, class_shadow, uses, line_map);
    }
}

fn walk_generators(
    generators: &[ast::Comprehension],
    bindings: &Bindings,
    class_shadow: Option<&HashSet<String>>,
    uses: &mut Vec<InitUse>,
    line_map: &LineMap,
) {
    for generator in generators {
        walk_expr(
            &generator.iter,
            UseContext::Comprehension,
            bindings,
            class_shadow,
            uses,
            line_map,
        );
        for condition in &generator.ifs {
            walk_expr(
                condition,
                UseContext::Comprehension,
                bindings,
                class_shadow,
                uses,
                line_map,
            );
        }
    }
}

fn attr_chain(expr: &Expr) -> Option<Vec<String>> {
    match expr {
        Expr::Name(name) => Some(vec![name.id.to_string()]),
        Expr::Attribute(attr) if matches!(attr.ctx, ExprContext::Load) => {
            let mut chain = attr_chain(&attr.value)?;
            chain.push(attr.attr.to_string());
            Some(chain)
        }
        _ => None,
    }
}

fn emit_chain(
    chain: &[String],
    expr: &Expr,
    ctx: UseContext,
    bindings: &Bindings,
    class_shadow: Option<&HashSet<String>>,
    uses: &mut Vec<InitUse>,
    line_map: &LineMap,
) {
    let Some(root) = chain.first() else {
        return;
    };
    if class_shadow.is_some_and(|shadow| shadow.contains(root)) {
        return;
    }
    let Some(targets) = bindings.get(root) else {
        return;
    };

    let best = targets
        .iter()
        .filter(|target| {
            class_shadow.is_none_or(|shadow| !shadow.contains(target.root_name.as_str()))
        })
        .filter(|target| local_prefix_matches(&target.local_prefix, chain))
        .max_by_key(|target| target.local_prefix.split('.').count());

    if let Some(target) = best {
        uses.push(InitUse {
            target_module: target.target_module.clone(),
            line: line_map.line_for_offset(offset_to_usize(expr.range().start())),
            context: ctx,
        });
    }
}

fn local_prefix_matches(local_prefix: &str, chain: &[String]) -> bool {
    let parts: Vec<&str> = local_prefix.split('.').collect();
    parts.len() <= chain.len()
        && parts
            .iter()
            .zip(chain.iter())
            .all(|(prefix, component)| *prefix == component)
}

fn shadow_targets(targets: &[Expr], bindings: &mut Bindings) {
    for target in targets {
        shadow_target(target, bindings);
    }
}

fn shadow_target(target: &Expr, bindings: &mut Bindings) {
    for name in binding_names(target) {
        bindings.remove(name.as_str());
    }
}

fn shadow_targets_in_set(targets: &[Expr], class_shadow: &mut HashSet<String>) {
    for target in targets {
        shadow_target_in_set(target, class_shadow);
    }
}

fn shadow_target_in_set(target: &Expr, class_shadow: &mut HashSet<String>) {
    for name in binding_names(target) {
        class_shadow.insert(name);
    }
}

fn binding_names(target: &Expr) -> Vec<String> {
    match target {
        Expr::Name(name) => vec![name.id.to_string()],
        Expr::Tuple(tuple) => tuple.elts.iter().flat_map(binding_names).collect(),
        Expr::List(list) => list.elts.iter().flat_map(binding_names).collect(),
        Expr::Starred(starred) => binding_names(&starred.value),
        _ => Vec::new(),
    }
}

fn is_type_checking_test(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id.as_str() == "TYPE_CHECKING",
        Expr::Attribute(attr) => attr.attr.as_str() == "TYPE_CHECKING",
        _ => false,
    }
}

fn offset_to_usize(offset: TextSize) -> usize {
    u32::from(offset) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{DiscoveryResult, PythonFile, SourceRoot};
    use crate::resolver::index::ModuleIndex;
    use std::path::PathBuf;

    fn make_index(modules: &[&str]) -> ModuleIndex {
        let files = modules
            .iter()
            .map(|m| PythonFile {
                rel_path: PathBuf::from(m.replace('.', "/") + ".py"),
                module_name: m.to_string(),
            })
            .collect();

        let result = DiscoveryResult {
            roots: vec![SourceRoot {
                path: PathBuf::from("/fake"),
                files,
            }],
        };

        ModuleIndex::from_discovery(&result)
    }

    #[test]
    fn module_body_use_when_imported_name_is_read() {
        let uses = scan_init_time_uses("import b\nx = b.foo\n", "app", &make_index(&["b"]), false);
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].target_module, "b");
        assert_eq!(uses[0].context, UseContext::ModuleBody);
    }

    #[test]
    fn base_class_use_when_imported_base_is_referenced() {
        let uses = scan_init_time_uses(
            "import b\nclass C(b.Base): pass\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert_eq!(uses[0].context, UseContext::BaseClass);
    }

    #[test]
    fn decorator_use_when_imported_decorator_is_referenced() {
        let uses = scan_init_time_uses(
            "import b\n@b.deco\ndef f(): pass\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert_eq!(uses[0].context, UseContext::Decorator);
    }

    #[test]
    fn default_arg_use_when_imported_default_is_referenced() {
        let uses = scan_init_time_uses(
            "import b\ndef f(x=b.default): pass\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert_eq!(uses[0].context, UseContext::DefaultArg);
    }

    #[test]
    fn function_body_is_deferred() {
        let uses = scan_init_time_uses(
            "import b\ndef f(): return b.foo\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert!(uses.is_empty());
    }

    #[test]
    fn class_body_use_when_imported_name_is_read_in_class_body() {
        let uses = scan_init_time_uses(
            "import b\nclass C:\n    x = b.attr\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert_eq!(uses[0].context, UseContext::ClassBody);
    }

    #[test]
    fn type_checking_block_is_skipped() {
        let uses = scan_init_time_uses(
            "import b\nif TYPE_CHECKING:\n    y = b.T\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert!(uses.is_empty());
    }

    #[test]
    fn annotations_are_skipped() {
        let uses = scan_init_time_uses(
            "import b\ndef f(x: b.T): pass\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert!(uses.is_empty());
    }

    #[test]
    fn lambda_body_is_deferred() {
        let uses = scan_init_time_uses(
            "import b\nf = lambda: b.foo\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert!(uses.is_empty());
    }

    #[test]
    fn list_comprehension_use_has_comprehension_context() {
        let uses = scan_init_time_uses(
            "import b\nx = [b.x for _ in []]\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert_eq!(uses[0].context, UseContext::Comprehension);
    }

    #[test]
    fn generator_body_is_deferred() {
        let uses = scan_init_time_uses(
            "import b\nx = (b.x for _ in [])\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert!(uses.is_empty());
    }

    #[test]
    fn class_body_import_is_not_a_binding() {
        let uses = scan_init_time_uses(
            "class C:\n    import b\n    z = b.q\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert!(uses.is_empty());
    }

    #[test]
    fn top_level_shadow_removes_binding() {
        let uses = scan_init_time_uses(
            "import b\nb = object()\nx = b.foo\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert!(uses.is_empty());
    }

    #[test]
    fn imported_name_without_shadow_emits_use() {
        let uses = scan_init_time_uses("import b\nx = b.foo\n", "app", &make_index(&["b"]), false);
        assert_eq!(uses.len(), 1);
    }

    #[test]
    fn class_body_shadow_suppresses_use() {
        let uses = scan_init_time_uses(
            "import b\nclass C:\n    b = 1\n    y = b.attr\n",
            "app",
            &make_index(&["b"]),
            false,
        );
        assert!(uses.is_empty());
    }

    #[test]
    fn import_alias_targets_original_module() {
        let uses = scan_init_time_uses(
            "import mymod as x\nx.X\n",
            "app",
            &make_index(&["mymod"]),
            false,
        );
        assert_eq!(uses[0].target_module, "mymod");
    }

    #[test]
    fn from_submodule_targets_submodule() {
        let uses = scan_init_time_uses(
            "from a import b\nb.X\n",
            "app",
            &make_index(&["a", "a.b"]),
            false,
        );
        assert_eq!(uses[0].target_module, "a.b");
    }

    #[test]
    fn from_attribute_targets_base_module() {
        let uses = scan_init_time_uses(
            "from a import B\ny = B.attr\n",
            "app",
            &make_index(&["a"]),
            false,
        );
        assert_eq!(uses[0].target_module, "a");
    }

    #[test]
    fn from_alias_targets_submodule() {
        let uses = scan_init_time_uses(
            "from a import b as c\nc.X\n",
            "app",
            &make_index(&["a", "a.b"]),
            false,
        );
        assert_eq!(uses[0].target_module, "a.b");
    }
}
