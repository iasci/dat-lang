//! Module-level checking: collecting declarations, checking each body once,
//! inferring return types on demand and marking recursive functions.

use crate::body::Body;
use crate::codes;
use crate::stdlib;
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
    /// How many of the last parameters have default values.
    pub defaults: usize,
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
    /// Each `return` with a value in `main`, and the value: when `main`
    /// doesn't return void, its fix rewrites them.
    pub main_returns: Vec<(Span, ast::ExprId)>,
    /// The names imports declare.
    pub imports: HashMap<String, Imported>,
    /// Each named import's path and the `}` that ends its names, for fixes
    /// that import one more name.
    pub named_imports: Vec<(String, Span)>,
    /// Where a fix inserts a new import: after the last one, or before the
    /// first declaration.
    pub import_at: u32,
    /// Each E1401 whose fix imports a name: the diagnostic's index, the
    /// module and the name.
    pub import_fixes: Vec<(usize, &'static stdlib::StdModule, String)>,
}

/// What an imported name refers to.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Imported {
    /// A member of a standard library module.
    Member(&'static stdlib::StdModule, &'static stdlib::Member),
    /// `import * as math from "@dat/math"`.
    Namespace(&'static stdlib::StdModule),
    /// From a module this milestone can't read, already reported, so its
    /// uses aren't reported again.
    Unknown,
}

/// Checks a parsed file and lowers it. Diagnostics are appended to `diags`.
pub fn check(sources: &SourceMap, ast: &Ast, opts: Options, diags: &mut Vec<Diagnostic>) -> Option<ir::Program> {
    let mut m = Module {
        sources,
        file: ast.file,
        ast,
        diags,
        sigs: Vec::new(),
        by_name: HashMap::new(),
        bodies: Vec::new(),
        main_returns: Vec::new(),
        imports: HashMap::new(),
        named_imports: Vec::new(),
        import_at: 0,
        import_fixes: Vec::new(),
    };
    m.collect();
    for id in 0..m.sigs.len() {
        m.check_function(ir::FuncId(id as u32));
    }
    m.merge_import_fixes();
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
        self.import_at = self.first_declaration_line();
        for item in &ast.items {
            if let ItemKind::Import(imp) = &item.kind {
                self.import(imp, item.span);
            }
        }
        for (i, item) in ast.items.iter().enumerate() {
            match &item.kind {
                ItemKind::Import(_) => {}
                ItemKind::Function(f) => {
                    let id = ir::FuncId(self.sigs.len() as u32);
                    if self.by_name.contains_key(&f.name.name) || self.imports.contains_key(&f.name.name) {
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
                    let defaults = self.default_count(f);
                    self.sigs.push(Sig { name: f.name.name.clone(), params, defaults, ret, item: i, is_test: false });
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
                        defaults: 0,
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

    /// Where the first declaration's line starts: after the comments that
    /// open the file.
    fn first_declaration_line(&self) -> u32 {
        let text = &self.src().text;
        let start = self.ast.items.first().map_or(text.len() as u32, |i| i.span.start);
        // After a byte order mark, if there's one: nothing goes before it.
        let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
        text[..start as usize].rfind('\n').map_or(bom, |i| i as u32 + 1)
    }

    /// Checks an import and declares its names (§17). Only the standard
    /// library can be read in this milestone.
    fn import(&mut self, imp: &ast::Import, span: Span) {
        let text = &self.src().text;
        let line_end = text[span.end as usize..].find('\n').map_or(text.len(), |i| span.end as usize + i + 1) as u32;
        self.import_at = self.import_at.max(line_end);
        let path = imp.path.as_str();
        let mut module = stdlib::by_path(path);
        if !path.starts_with("./") && !path.starts_with("../") && !path.starts_with('@') {
            module = stdlib::MODULES.iter().find(|m| m.name == path);
            let mut d = Diagnostic::error(codes::IMPORT_PATH, self.file, imp.path_span, "an import path starts with `./`, `../` or `@`")
                .with_label("file paths start with `./` or `../`, and the standard library with `@dat/`");
            if let Some(m) = module {
                let edit = dat_diag::edit(self.sources, self.file, imp.path_span, format!("{:?}", m.path()));
                d = d.with_fix(format!("import `{}` from the standard library", m.path()), dat_diag::Applicability::Safe, vec![edit]);
            }
            self.diags.push(d);
        } else if let Some(name) = path.strip_prefix("@std/").or_else(|| path.strip_prefix("@dat/").filter(|_| module.is_none())) {
            // `@std/` isn't an owner anyone can use (§17), so it means
            // `@dat/`; and a misspelled module, such as `@dat/maths`.
            let exact = stdlib::MODULES.iter().find(|m| m.name == name);
            module = exact.or_else(|| stdlib::similar_module(name));
            match module {
                Some(m) => {
                    let message = if path.starts_with("@std/") {
                        "the standard library's paths start with `@dat/`".to_string()
                    } else {
                        format!("the standard library has no module `{path}`")
                    };
                    let applicability = if exact.is_some() { dat_diag::Applicability::Safe } else { dat_diag::Applicability::Review };
                    let edit = dat_diag::edit(self.sources, self.file, imp.path_span, format!("{:?}", m.path()));
                    let d = Diagnostic::error(codes::IMPORT_PATH, self.file, imp.path_span, message).with_fix(
                        format!("import `{}`", m.path()),
                        applicability,
                        vec![edit],
                    );
                    self.diags.push(d);
                }
                None if path.starts_with("@dat/") => self.unsupported(imp.path_span, &format!("the standard library module `{path}`")),
                None => {
                    let d =
                        Diagnostic::error(codes::IMPORT_PATH, self.file, imp.path_span, "the standard library's paths start with `@dat/`");
                    self.diags.push(d);
                }
            }
        } else if let Some(m) = module.filter(|m| !m.supported()) {
            self.unsupported(imp.path_span, &format!("the standard library module `{}`", m.path()));
        } else if module.is_none() {
            let what = if path.starts_with("@dat/") {
                format!("the standard library module `{path}`")
            } else if path.starts_with('@') {
                "packages".to_string()
            } else {
                "imports of files".to_string()
            };
            self.unsupported(imp.path_span, &what);
        }
        let readable = module.filter(|m| m.supported());
        let declare = |m: &mut Self, name: &ast::Ident, what: Imported| {
            if m.imports.contains_key(&name.name) {
                m.error(codes::REDECLARED, name.span, format!("`{}` is already declared", name.name)).label =
                    Some("each import needs its own name".into());
                return;
            }
            m.imports.insert(name.name.clone(), what);
        };
        match &imp.kind {
            ast::ImportKind::Namespace(ns) => declare(self, ns, readable.map_or(Imported::Unknown, Imported::Namespace)),
            ast::ImportKind::Named { names, close } => {
                if let Some(m) = module {
                    self.named_imports.push((m.path(), *close));
                }
                for n in names {
                    let member = module.and_then(|m| m.member(&n.name.name));
                    let what = match (readable, member, module) {
                        (Some(m), Some(x), _) => Imported::Member(m, x),
                        (_, None, Some(m)) => {
                            let mut d = Diagnostic::error(
                                codes::NOT_EXPORTED,
                                self.file,
                                n.name.span,
                                format!("`{}` isn't exported by `{}`", n.name.name, m.path()),
                            );
                            if let Some(similar) = m.similar(&n.name.name) {
                                let edit = dat_diag::edit(self.sources, self.file, n.name.span, similar);
                                d = d.with_fix(format!("import `{similar}`"), dat_diag::Applicability::Review, vec![edit]);
                            }
                            self.diags.push(d);
                            Imported::Unknown
                        }
                        _ => Imported::Unknown,
                    };
                    declare(self, n.local(), what);
                }
            }
        }
    }

    /// The edit that imports `names` from `m`: added to an import of `m`, or
    /// as a new import.
    pub fn import_edit(&self, m: &stdlib::StdModule, names: &[&str]) -> dat_diag::Edit {
        let path = m.path();
        let text = &self.src().text;
        let list = names.join(", ");
        let insert = |at: u32, after: String| dat_diag::edit(self.sources, self.file, Span::new(at, at), after);
        if let Some((_, close)) = self.named_imports.iter().find(|(p, _)| *p == path) {
            let end = text[..close.start as usize].trim_end().len() as u32;
            let after = match text[..end as usize].chars().last() {
                Some('{' | ',') => format!(" {list}"),
                _ => format!(", {list}"),
            };
            return insert(end, after);
        }
        let has_imports = self.ast.items.iter().any(|i| matches!(i.kind, ItemKind::Import(_)));
        let blank = if has_imports { "" } else { "\n" };
        insert(self.import_at, format!("import {{ {list} }} from {path:?};\n{blank}"))
    }

    /// Fixes that each import one name from the same module would insert
    /// at the same place, so applied together they'd conflict. The first
    /// use's fix imports them all instead, and the others point to it.
    fn merge_import_fixes(&mut self) {
        let fixes = std::mem::take(&mut self.import_fixes);
        for (k, &(first, m, _)) in fixes.iter().enumerate() {
            if fixes[..k].iter().any(|f| std::ptr::eq(f.1, m)) {
                continue;
            }
            let group: Vec<&(usize, &stdlib::StdModule, String)> = fixes.iter().filter(|f| std::ptr::eq(f.1, m)).collect();
            if group.len() < 2 {
                continue;
            }
            let mut names: Vec<&str> = Vec::new();
            for f in &group {
                if !names.contains(&f.2.as_str()) {
                    names.push(&f.2);
                }
            }
            let edit = self.import_edit(m, &names);
            let (line, _) = self.src().line_col(self.diags[first].span.start);
            let shown: Vec<String> = names.iter().map(|n| format!("`{n}`")).collect();
            let fix = &mut self.diags[first].fixes[0];
            fix.title = format!("import {} from `{}`", shown.join(", "), m.path());
            fix.edits = vec![edit];
            for f in &group[1..] {
                let d = &mut self.diags[f.0];
                d.fixes.clear();
                d.label = Some(format!("`{}` comes from `{}`; the fix on line {line} imports it", f.2, m.path()));
            }
        }
    }

    /// How many parameters at the end have defaults. A parameter without
    /// one after one with one can never be left out, so it's an error (§06).
    fn default_count(&mut self, f: &ast::Function) -> usize {
        let first = f.params.iter().position(|p| p.default.is_some()).unwrap_or(f.params.len());
        for p in &f.params[first..] {
            if p.default.is_none() {
                let d = Diagnostic::error(
                    codes::DEFAULT_ORDER,
                    self.file,
                    p.span,
                    format!("`{}` needs a default value, because a parameter before it has one", p.name.name),
                )
                .with_label("parameters with defaults go after the ones without");
                self.diags.push(d);
            }
        }
        f.params.len() - first
    }

    pub fn resolve_type(&mut self, t: &ast::TypeExpr) -> Ty {
        match &t.kind {
            ast::TypeKind::Named(name) => match name.as_str() {
                "int" => Ty::Int,
                "float" => Ty::Float,
                "bool" => Ty::Bool,
                "string" => Ty::Str,
                "void" => Ty::Void,
                "never" => Ty::Never,
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
                }
                let params = self.sigs[i].params.clone();
                let first_default = params.len() - self.sigs[i].defaults;
                let mut body = Body::new(self, id, declared, f.name.name.clone());
                for (j, (p, (name, ty))) in f.params.iter().zip(params).enumerate() {
                    // A default sees the parameters before it.
                    if let (Some(d), true) = (p.default, j >= first_default) {
                        let later = f.params[j..].iter().map(|p| p.name.name.clone()).collect();
                        body.default_value(d, ty, later);
                    }
                    body.declare_param(&name, ty, p.name.span);
                }
                body.finish_function(&f.body, f.name.span)
            }
            ItemKind::Test(t) => {
                let name = self.sigs[i].name.clone();
                let body = Body::new(self, id, Some(Ty::Void), name);
                body.finish_function(&t.body, t.name_span)
            }
            ItemKind::Unsupported { .. } | ItemKind::Import(_) => unreachable!(),
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
            let mut edits = match &f.ret {
                Some(r) => vec![dat_diag::edit(self.sources, self.file, r.span, "void")],
                None => Vec::new(),
            };
            // A returned value goes too; one that may do something stays, as a statement.
            for &(stmt, value) in &self.main_returns {
                let e = ast.expr(value);
                let pure = matches!(
                    e.kind,
                    ast::ExprKind::Int(_)
                        | ast::ExprKind::Float(_)
                        | ast::ExprKind::Str(_)
                        | ast::ExprKind::Bool(_)
                        | ast::ExprKind::Null
                        | ast::ExprKind::Name(_)
                );
                let after = if pure { "return;".to_string() } else { format!("{}; return;", self.text(e.span)) };
                edits.push(dat_diag::edit(self.sources, self.file, stmt, &after));
            }
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
            // Defaults run in the frame of the function they belong to.
            for d in &f.defaults {
                calls_in_expr(d, &mut out);
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
        Builtin { args, .. } => args.iter().for_each(|a| calls_in_expr(a, out)),
        Int(_) | Float(_) | Bool(_) | Str(_) | Local(_) => {}
        IntArith { lhs, rhs, .. } | FloatArith { lhs, rhs, .. } | Compare { lhs, rhs, .. } | And(lhs, rhs) | Or(lhs, rhs) => {
            calls_in_expr(lhs, out);
            calls_in_expr(rhs, out);
        }
        IntNeg { arg, .. } | FloatNeg(arg) | Not(arg) | ToStr(arg) | ErrorMessage(arg) | Print(arg) | Diverge(arg) => {
            calls_in_expr(arg, out)
        }
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
