//! Module-level checking: collecting declarations, checking each body once,
//! inferring return types on demand and marking recursive functions.

use crate::body::Body;
use crate::codes;
use crate::{Options, Ty};
use dat_diag::{Diagnostic, FileId, SourceMap, Span};
use dat_ir as ir;
use dat_syntax::ast::{self, Ast, ItemKind};
use std::collections::HashMap;

pub(crate) enum Ret {
    Known(Ty),
    /// Not declared; inferred from the body when first needed.
    Infer,
    /// Being inferred right now: a call to it is recursion through an inferred type.
    InProgress,
}

pub(crate) struct Sig {
    pub name: String,
    pub params: Vec<(String, Ty)>,
    pub ret: Ret,
    /// The function or test declaration, by item index.
    pub item: usize,
    pub is_test: bool,
}

pub(crate) struct Module<'a> {
    pub sources: &'a SourceMap,
    pub file: FileId,
    pub ast: &'a Ast,
    pub diags: &'a mut Vec<Diagnostic>,
    pub sigs: Vec<Sig>,
    pub by_name: HashMap<String, ir::FuncId>,
    pub bodies: Vec<Option<ir::Function>>,
}

/// Checks a parsed file and lowers it. Diagnostics are appended to `diags`.
pub fn check(sources: &SourceMap, ast: &Ast, opts: Options, diags: &mut Vec<Diagnostic>) -> Option<ir::Program> {
    let mut m = Module { sources, file: ast.file, ast, diags, sigs: Vec::new(), by_name: HashMap::new(), bodies: Vec::new() };
    m.collect();
    for id in 0..m.sigs.len() {
        m.check_function(ir::FuncId(id as u32));
    }
    let main = m.by_name.get("main").copied();
    // A `main` with a syntax error is dropped by the parser; don't also claim it's missing.
    let had_errors = m.diags.iter().any(|d| d.severity == dat_diag::Severity::Error);
    m.check_exports();
    match main {
        Some(id) => m.check_main(id),
        None if opts.require_main && !had_errors => {
            let end = m.src().text.len() as u32;
            let sep = if m.src().text.ends_with('\n') { "\n" } else { "\n\n" };
            let d = Diagnostic::error(codes::NO_MAIN, m.file, Span::new(0, 0), "this program has no `main` function")
                .with_label("`dat run` and `dat build` start at `main`")
                .with_fix(
                    "add a `main` function at the end",
                    dat_diag::Applicability::Review,
                    vec![dat_diag::Edit {
                        file: m.file,
                        span: Span::new(end, end),
                        before: String::new(),
                        after: format!("{sep}function main(): void {{\n}}\n"),
                    }],
                );
            m.diags.push(d);
        }
        None => {}
    }
    m.finish(main)
}

impl<'a> Module<'a> {
    pub fn src(&self) -> &'a dat_diag::SourceFile {
        self.sources.get(self.file)
    }

    pub fn text(&self, span: Span) -> &'a str {
        self.src().slice(span)
    }

    pub fn loc(&self, span: Span) -> ir::Loc {
        let (line, col) = self.src().line_col(span.start);
        ir::Loc { line, col }
    }

    pub fn error(&mut self, code: &'static str, span: Span, message: impl Into<String>) -> &mut Diagnostic {
        self.diags.push(Diagnostic::error(code, self.file, span, message));
        self.diags.last_mut().unwrap()
    }

    pub fn unsupported(&mut self, span: Span, what: &str) {
        self.error(codes::UNSUPPORTED, span, format!("not supported yet: {what}"));
    }

    fn collect(&mut self) {
        let ast = self.ast;
        let mut test_names: HashMap<&str, Span> = HashMap::new();
        for (i, item) in ast.items.iter().enumerate() {
            match &item.kind {
                ItemKind::Function(f) => {
                    let id = ir::FuncId(self.sigs.len() as u32);
                    if self.by_name.contains_key(&f.name.name) {
                        self.error(codes::REDECLARED, f.name.span, format!("`{}` is already declared", f.name.name)).label =
                            Some("a module can declare each name once".into());
                    } else {
                        self.by_name.insert(f.name.name.clone(), id);
                    }
                    let params = f
                        .params
                        .iter()
                        .map(|p| {
                            let ty = match &p.ty {
                                Some(t) => match self.resolve_type(t) {
                                    t @ (Ty::Void | Ty::Never) => {
                                        let span = p.ty.as_ref().unwrap().span;
                                        self.error(codes::VOID_VALUE, span, format!("a parameter can't have type {}", t.name())).label =
                                            Some("no value has this type, so the function could never be called".into());
                                        Ty::Poison
                                    }
                                    t => t,
                                },
                                None => {
                                    self.error(codes::MISMATCH, p.name.span, format!("parameter `{}` needs a type", p.name.name)).label =
                                        Some("parameter types are required".into());
                                    Ty::Poison
                                }
                            };
                            (p.name.name.clone(), ty)
                        })
                        .collect();
                    let ret = match &f.ret {
                        Some(t) => Ret::Known(self.resolve_type(t)),
                        None => Ret::Infer,
                    };
                    self.sigs.push(Sig { name: f.name.name.clone(), params, ret, item: i, is_test: false });
                    self.bodies.push(None);
                }
                ItemKind::Test(t) => {
                    if let Some(&first) = test_names.get(t.name.as_str()) {
                        let (line, _) = self.src().line_col(first.start);
                        self.error(
                            codes::DUPLICATE_TEST,
                            t.name_span,
                            format!("a test named {:?} already exists on line {line}", t.name),
                        )
                        .label = Some("test names must be unique within a module".into());
                    } else {
                        test_names.insert(&t.name, t.name_span);
                    }
                    self.sigs.push(Sig {
                        name: format!("test {:?}", t.name),
                        params: Vec::new(),
                        ret: Ret::Known(Ty::Void),
                        item: i,
                        is_test: true,
                    });
                    self.bodies.push(None);
                }
                ItemKind::Unsupported { what } => self.unsupported(item.span, what),
            }
        }
    }

    pub fn resolve_type(&mut self, t: &ast::TypeExpr) -> Ty {
        match &t.kind {
            ast::TypeKind::Named(name) => match name.as_str() {
                "int" => Ty::Int,
                "bool" => Ty::Bool,
                "string" => Ty::Str,
                "void" => Ty::Void,
                "never" => Ty::Never,
                "float" => {
                    self.unsupported(t.span, "floats");
                    Ty::Poison
                }
                "null" => {
                    self.unsupported(t.span, "nullable types");
                    Ty::Poison
                }
                other => match ir::ErrorType::from_name(other) {
                    Some(e) => Ty::Error(e),
                    None => {
                        self.unsupported(t.span, "user-defined types");
                        Ty::Poison
                    }
                },
            },
            ast::TypeKind::Unsupported(what) => {
                self.unsupported(t.span, what);
                Ty::Poison
            }
        }
    }

    /// Checks and lowers a function or test body, once.
    pub fn check_function(&mut self, id: ir::FuncId) {
        let i = id.0 as usize;
        if self.bodies[i].is_some() || matches!(self.sigs[i].ret, Ret::InProgress) {
            return;
        }
        let ast = self.ast;
        let item = &ast.items[self.sigs[i].item];
        let declared = match self.sigs[i].ret {
            Ret::Known(t) => Some(t),
            _ => None,
        };
        if declared.is_none() {
            self.sigs[i].ret = Ret::InProgress;
        }
        let f = match &item.kind {
            ItemKind::Function(f) => {
                if let Some(g) = f.generics {
                    self.unsupported(g, "generic functions");
                }
                if let Some(t) = &f.throws {
                    self.unsupported(t.span, "declared errors (`throws`)");
                }
                for p in &f.params {
                    if p.is_mut {
                        self.unsupported(p.span, "`mut` parameters");
                    }
                    if let Some(d) = p.default {
                        self.unsupported(ast.expr(d).span, "default parameter values");
                    }
                }
                let params = self.sigs[i].params.clone();
                let mut body = Body::new(self, id, declared, f.name.name.clone());
                for (p, (name, ty)) in f.params.iter().zip(params) {
                    body.declare_param(&name, ty, p.name.span);
                }
                body.finish_function(&f.body, f.name.span)
            }
            ItemKind::Test(t) => {
                let name = self.sigs[i].name.clone();
                let body = Body::new(self, id, Some(Ty::Void), name);
                body.finish_function(&t.body, t.name_span)
            }
            ItemKind::Unsupported { .. } => unreachable!(),
        };
        let (func, ret) = f;
        self.sigs[i].ret = Ret::Known(ret);
        self.bodies[i] = Some(func);
    }

    /// §06: exported functions declare their return type, so a module's
    /// interface can be read without its bodies. The fix writes the inferred one.
    fn check_exports(&mut self) {
        let ast = self.ast;
        for i in 0..self.sigs.len() {
            let ItemKind::Function(f) = &ast.items[self.sigs[i].item].kind else { continue };
            if !f.exported || f.ret.is_some() {
                continue;
            }
            let Ret::Known(t) = self.sigs[i].ret else { continue };
            let text = &self.src().text;
            let close = text[..f.body.span.start as usize].rfind(')').map_or(f.body.span.start, |p| p as u32 + 1);
            let mut d = Diagnostic::error(
                codes::EXPORT_NEEDS_TYPE,
                self.file,
                f.name.span,
                format!("exported function `{}` must declare its return type", f.name.name),
            )
            .with_label("importers read a module's interface without its bodies");
            if t != Ty::Poison {
                d = d.with_fix(
                    format!("declare `: {}`", t.name()),
                    dat_diag::Applicability::Safe,
                    vec![dat_diag::Edit {
                        file: self.file,
                        span: Span::new(close, close),
                        before: String::new(),
                        after: format!(": {}", t.name()),
                    }],
                );
            }
            self.diags.push(d);
        }
    }

    fn check_main(&mut self, id: ir::FuncId) {
        let ast = self.ast;
        let (item, ret) = (
            self.sigs[id.0 as usize].item,
            match self.sigs[id.0 as usize].ret {
                Ret::Known(t) => Some(t),
                _ => None,
            },
        );
        let ItemKind::Function(f) = &ast.items[item].kind else { return };
        if !f.params.is_empty() {
            let span = f.params[0].span.to(f.params.last().unwrap().span);
            self.error(codes::BAD_MAIN, span, "`main` takes no parameters").label = Some("the program starts here".into());
        }
        if let Some(t) = ret
            && t != Ty::Void
            && t != Ty::Poison
        {
            let span = f.ret.as_ref().map_or(f.name.span, |r| r.span);
            let edits = match &f.ret {
                Some(r) => vec![dat_diag::edit(self.sources, self.file, r.span, "void")],
                None => Vec::new(),
            };
            let mut d = Diagnostic::error(codes::BAD_MAIN, self.file, span, format!("`main` must return void, not {}", t.name()));
            if !edits.is_empty() {
                d = d.with_fix("return void", dat_diag::Applicability::Review, edits);
            }
            self.diags.push(d);
        }
    }

    fn finish(self, main: Option<ir::FuncId>) -> Option<ir::Program> {
        if self.bodies.iter().any(Option::is_none) {
            return None;
        }
        let file = self.src().path.clone();
        let mut functions: Vec<ir::Function> = self.bodies.into_iter().map(Option::unwrap).collect();
        mark_recursive(&mut functions);
        let tests = self
            .sigs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_test)
            .map(|(i, s)| {
                let ItemKind::Test(t) = &self.ast.items[s.item].kind else { unreachable!() };
                ir::Test { name: t.name.clone(), func: ir::FuncId(i as u32), loc: functions[i].loc }
            })
            .collect();
        Some(ir::Program { file, functions, tests, main })
    }
}

/// Marks the functions that are part of a call cycle, which must check the
/// stack on entry. Tarjan's strongly connected components.
fn mark_recursive(functions: &mut [ir::Function]) {
    let n = functions.len();
    let edges: Vec<Vec<usize>> = functions
        .iter()
        .map(|f| {
            let mut out = Vec::new();
            for s in &f.body {
                calls_in_stmt(s, &mut out);
            }
            out
        })
        .collect();
    struct State {
        index: Vec<Option<u32>>,
        low: Vec<u32>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        next: u32,
        recursive: Vec<bool>,
    }
    fn visit(v: usize, edges: &[Vec<usize>], s: &mut State) {
        s.index[v] = Some(s.next);
        s.low[v] = s.next;
        s.next += 1;
        s.stack.push(v);
        s.on_stack[v] = true;
        for &w in &edges[v] {
            match s.index[w] {
                None => {
                    visit(w, edges, s);
                    s.low[v] = s.low[v].min(s.low[w]);
                }
                Some(iw) if s.on_stack[w] => s.low[v] = s.low[v].min(iw),
                _ => {}
            }
        }
        if Some(s.low[v]) == s.index[v] {
            let mut comp = Vec::new();
            loop {
                let w = s.stack.pop().unwrap();
                s.on_stack[w] = false;
                comp.push(w);
                if w == v {
                    break;
                }
            }
            let cyclic = comp.len() > 1 || edges[v].contains(&v);
            for w in comp {
                s.recursive[w] = cyclic;
            }
        }
    }
    let mut s =
        State { index: vec![None; n], low: vec![0; n], on_stack: vec![false; n], stack: Vec::new(), next: 0, recursive: vec![false; n] };
    for v in 0..n {
        if s.index[v].is_none() {
            visit(v, &edges, &mut s);
        }
    }
    for (f, r) in functions.iter_mut().zip(s.recursive) {
        f.recursive = r;
    }
}

fn calls_in_stmt(s: &ir::Stmt, out: &mut Vec<usize>) {
    use ir::Stmt::*;
    match s {
        Let(_, e) | Set(_, e) | Expr(e) | Throw(e, _) | Return(Some(e)) => calls_in_expr(e, out),
        If(c, a, b) => {
            calls_in_expr(c, out);
            a.iter().chain(b).for_each(|s| calls_in_stmt(s, out));
        }
        Loop { cond, body, step } => {
            if let Some(c) = cond {
                calls_in_expr(c, out);
            }
            body.iter().chain(step).for_each(|s| calls_in_stmt(s, out));
        }
        Try { body, catches } => {
            body.iter().for_each(|s| calls_in_stmt(s, out));
            catches.iter().flat_map(|c| &c.body).for_each(|s| calls_in_stmt(s, out));
        }
        Return(None) | Break | Continue => {}
    }
}

fn calls_in_expr(e: &ir::Expr, out: &mut Vec<usize>) {
    use ir::ExprKind::*;
    match &e.kind {
        Call { func, args, .. } => {
            out.push(func.0 as usize);
            args.iter().for_each(|a| calls_in_expr(a, out));
        }
        Int(_) | Bool(_) | Str(_) | Local(_) => {}
        IntArith { lhs, rhs, .. } | Compare { lhs, rhs, .. } | And(lhs, rhs) | Or(lhs, rhs) => {
            calls_in_expr(lhs, out);
            calls_in_expr(rhs, out);
        }
        IntNeg { arg, .. } | Not(arg) | ToStr(arg) | ErrorMessage(arg) | Print(arg) | Diverge(arg) => calls_in_expr(arg, out),
        NewError { message, .. } | Fail { message, .. } => calls_in_expr(message, out),
        Cond { cond, then, els } => {
            calls_in_expr(cond, out);
            calls_in_expr(then, out);
            calls_in_expr(els, out);
        }
        Concat(parts) => parts.iter().for_each(|p| calls_in_expr(p, out)),
        Assert(a) => {
            match &a.cond {
                ir::AssertCond::Plain(c) => calls_in_expr(c, out),
                ir::AssertCond::Compare { lhs, rhs, .. } => {
                    calls_in_expr(lhs, out);
                    calls_in_expr(rhs, out);
                }
            }
            if let Some(m) = &a.message {
                calls_in_expr(m, out);
            }
        }
    }
}
