//! Checking and lowering one function or test body.

use crate::Ty;
use crate::codes;
use crate::module::{Module, Ret};
use dat_diag::{Applicability, Diagnostic, Edit, Span};
use dat_ir as ir;
use dat_syntax::ast::{self, AssignOp, BinOp, ExprId, ExprKind, StmtId, StmtKind, UnOp};

/// An expression after checking: its IR and its type.
struct TE {
    e: ir::Expr,
    ty: Ty,
}

fn mk(kind: ir::ExprKind, ty: Ty) -> TE {
    TE { e: ir::Expr { kind, ty: ty.ir() }, ty }
}

fn poison() -> TE {
    mk(ir::ExprKind::Int(0), Ty::Poison)
}

/// A `never` value where `want` is expected: it never finishes, so it has
/// any type (§01). Other values pass through unchanged.
fn coerce(v: TE, want: Ty) -> TE {
    if v.ty == Ty::Never && !matches!(want, Ty::Never | Ty::Poison) {
        return mk(ir::ExprKind::Diverge(Box::new(v.e)), want);
    }
    v
}

/// The common type two operands are checked at when one of them is `never`.
fn meet(a: Ty, b: Ty, default: Ty) -> Ty {
    match (a, b) {
        (Ty::Never, Ty::Never) => default,
        (Ty::Never, t) | (t, Ty::Never) => t,
        (t, _) => t,
    }
}

/// A built-in type whose members are written on its name, as in `int.MAX`.
fn is_type_name(n: &str) -> bool {
    matches!(n, "int" | "float" | "string" | "bool")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VarKind {
    Let,
    Const,
    Param,
    Catch,
}

struct Var {
    name: String,
    local: ir::LocalId,
    ty: Ty,
    kind: VarKind,
    /// The whole declaration statement.
    decl: Span,
    /// The `const` keyword, for the fix that turns it into `let`.
    keyword: Option<Span>,
}

/// A declaration whose block has ended. If a later use can't find the name,
/// the fix moves the declaration above `owner`, the statement that held the block.
struct Closed {
    name: String,
    decl: Span,
    owner: Span,
    owner_kind: &'static str,
    depth: usize,
}

pub(crate) struct Body<'m, 'a> {
    m: &'m mut Module<'a>,
    func: ir::FuncId,
    name: String,
    /// `None` while the return type is being inferred.
    ret: Option<Ty>,
    returns: Vec<(Ty, Span)>,
    locals: Vec<ir::Local>,
    params: u32,
    scopes: Vec<Vec<Var>>,
    /// For each open scope, the statement being checked in it.
    owners: Vec<(Span, &'static str)>,
    closed: Vec<Closed>,
    loops: u32,
    /// The innermost statement, and whether it's a simple one (`let`, assignment
    /// or expression) that a fix may split.
    stmt: Span,
    stmt_simple: bool,
}

impl<'m, 'a> Body<'m, 'a> {
    pub fn new(m: &'m mut Module<'a>, func: ir::FuncId, ret: Option<Ty>, name: String) -> Body<'m, 'a> {
        Body {
            m,
            func,
            name,
            ret,
            returns: Vec::new(),
            locals: Vec::new(),
            params: 0,
            scopes: vec![Vec::new()],
            owners: vec![(Span::default(), "function")],
            closed: Vec::new(),
            loops: 0,
            stmt: Span::default(),
            stmt_simple: false,
        }
    }

    // ---- helpers ----

    fn ast(&self) -> &'a ast::Ast {
        self.m.ast
    }

    /// Whether this is the module's `main` function.
    fn is_main(&self) -> bool {
        self.m.by_name.get("main") == Some(&self.func)
    }

    /// A `main` declared to return a value: E0661 reports it, so its
    /// returns and its end aren't reported again.
    fn is_bad_main(&self, ret: Option<Ty>) -> bool {
        ret.is_some_and(|t| !matches!(t, Ty::Void | Ty::Poison)) && self.is_main()
    }

    fn text(&self, span: Span) -> &'a str {
        self.m.text(span)
    }

    fn espan(&self, e: ExprId) -> Span {
        self.ast().expr(e).span
    }

    fn push(&mut self, d: Diagnostic) {
        self.m.diags.push(d);
    }

    fn err(&self, code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic::error(code, self.m.file, span, message)
    }

    fn edit(&self, span: Span, after: impl Into<String>) -> Edit {
        dat_diag::edit(self.m.sources, self.m.file, span, after)
    }

    fn insert(&self, at: u32, text: impl Into<String>) -> Edit {
        self.edit(Span::new(at, at), text)
    }

    fn unsupported(&mut self, span: Span, what: &str) {
        self.m.unsupported(span, what);
    }

    fn loc(&self, span: Span) -> ir::Loc {
        self.m.loc(span)
    }

    fn src_text(&self) -> &'a str {
        &self.m.src().text
    }

    fn line_start(&self, offset: u32) -> u32 {
        self.src_text()[..offset as usize].rfind('\n').map_or(0, |i| i as u32 + 1)
    }

    fn indent_at(&self, offset: u32) -> &'a str {
        let start = self.line_start(offset) as usize;
        let line = &self.src_text()[start..];
        &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
    }

    /// True when nothing but whitespace precedes `offset` on its line.
    fn starts_line(&self, offset: u32) -> bool {
        self.src_text()[self.line_start(offset) as usize..offset as usize].trim().is_empty()
    }

    /// The span of the whole line holding `span`, line break included, if the
    /// line holds nothing else apart from a trailing comment.
    fn whole_line(&self, span: Span) -> Option<Span> {
        let text = self.src_text();
        if !self.starts_line(span.start) {
            return None;
        }
        let rest = &text[span.end as usize..];
        let eol = rest.find('\n').map_or(text.len(), |i| span.end as usize + i + 1);
        let tail = text[span.end as usize..eol].trim();
        if !tail.is_empty() && !tail.starts_with("//") {
            return None;
        }
        Some(Span::new(self.line_start(span.start), eol as u32))
    }

    // ---- scopes ----

    fn push_scope(&mut self, owner: (Span, &'static str)) {
        self.scopes.push(Vec::new());
        self.owners.push(owner);
    }

    fn pop_scope(&mut self) {
        let vars = self.scopes.pop().unwrap();
        self.owners.pop();
        let depth = self.scopes.len() - 1;
        let (owner, owner_kind) = self.owners[depth];
        for c in &mut self.closed {
            if c.depth > depth {
                c.depth = depth;
                c.owner = owner;
                c.owner_kind = owner_kind;
            }
        }
        for v in vars {
            if matches!(v.kind, VarKind::Let | VarKind::Const) {
                self.closed.push(Closed { name: v.name, decl: v.decl, owner, owner_kind, depth });
            }
        }
    }

    fn lookup(&self, name: &str) -> Option<&Var> {
        self.scopes.iter().rev().flat_map(|s| s.iter().rev()).find(|v| v.name == name)
    }

    fn new_local(&mut self, name: &str, ty: Ty) -> ir::LocalId {
        self.locals.push(ir::Local { name: name.to_string(), ty: ty.ir(), reassigned: false });
        ir::LocalId(self.locals.len() as u32 - 1)
    }

    pub fn declare_param(&mut self, name: &str, ty: Ty, span: Span) {
        if self.scopes[0].iter().any(|v| v.name == name) {
            let d =
                self.err(codes::REDECLARED, span, format!("`{name}` is already declared")).with_label("each parameter needs its own name");
            self.push(d);
        }
        let local = self.new_local(name, ty);
        self.params += 1;
        self.scopes[0].push(Var { name: name.to_string(), local, ty, kind: VarKind::Param, decl: span, keyword: None });
    }

    fn declare(
        &mut self,
        name: &ast::Ident,
        ty: Ty,
        kind: VarKind,
        decl: Span,
        keyword: Option<Span>,
        init: Option<ExprId>,
    ) -> ir::LocalId {
        // The parameters and the function body share one scope, so a `let` can't
        // redeclare a parameter.
        if self.scopes.len() == 2 && self.scopes[0].iter().any(|v| v.name == name.name) {
            let d = self
                .err(codes::REDECLARED, name.span, format!("{} is already declared", name.name))
                .with_label("it's a parameter of this function; parameters are constants, so copy it into a variable with another name");
            self.push(d);
        }
        let scope = self.scopes.last().unwrap();
        if scope.iter().any(|v| v.name == name.name) {
            let mut d = self
                .err(codes::REDECLARED, name.span, format!("{} is already declared", name.name))
                .with_label("declared earlier in this scope");
            if let Some(init) = init {
                let value = self.text(self.espan(init));
                let stmt = Span::new(decl.start, decl.end);
                d = d.with_fix(
                    format!("assign to the existing `{}`", name.name),
                    Applicability::Review,
                    vec![self.edit(stmt, format!("{} = {value};", name.name))],
                );
            }
            self.push(d);
        }
        let local = self.new_local(&name.name, ty);
        self.scopes.last_mut().unwrap().push(Var { name: name.name.clone(), local, ty, kind, decl, keyword });
        local
    }

    // ---- function ----

    /// Checks the body and returns the lowered function and its return type.
    pub fn finish_function(mut self, block: &ast::Block, name_span: Span) -> (ir::Function, Ty) {
        let body = self.block(block, (block.span, "function"));
        let ret = match self.ret {
            Some(t) => t,
            None => self.infer_return(),
        };
        let bad_main = self.is_bad_main(Some(ret));
        if !matches!(ret, Ty::Void | Ty::Never | Ty::Poison) && !terminates(&body) && !bad_main {
            self.missing_return(block, ret);
        }
        if ret == Ty::Never && !terminates(&body) && !bad_main {
            self.never_reaches_end(block);
        }
        let item = &self.ast().items[self.m.sigs[self.func.0 as usize].item];
        let func = ir::Function {
            name: self.name.clone(),
            params: self.params,
            ret: ret.ir(),
            locals: std::mem::take(&mut self.locals),
            body,
            recursive: false,
            loc: self.loc(Span::new(item.span.start, name_span.end)),
        };
        (func, ret)
    }

    fn infer_return(&mut self) -> Ty {
        let mut ty = None;
        for (t, span) in std::mem::take(&mut self.returns) {
            match ty {
                None => ty = Some(t),
                Some(first) if first != t && t != Ty::Poison && first != Ty::Poison => {
                    let d = self
                        .err(
                            codes::MISMATCH,
                            span,
                            format!("this returns {}, but an earlier `return` gives {}", t.a_name(), first.a_name()),
                        )
                        .with_label("every `return` must give the same type");
                    self.push(d);
                }
                _ => {}
            }
        }
        ty.unwrap_or(Ty::Void)
    }

    /// §06: every path through a `never` function ends in `throw` or a `never` call.
    fn never_reaches_end(&mut self, block: &ast::Block) {
        let close = Span::new(block.span.end - 1, block.span.end);
        let name = self.name.clone();
        let edit = if self.starts_line(close.start) {
            let indent = self.indent_at(close.start);
            self.insert(self.line_start(close.start), format!("{indent}  fail(_);\n"))
        } else {
            self.insert(close.start, " fail(_); ")
        };
        let d = self
            .err(codes::NEVER_RETURNS, close, format!("`{name}` returns never, but it can reach its end"))
            .with_label("a `never` function must end every path with `throw` or a call such as `fail(...)`")
            .with_fix(format!("fail at the end of `{name}`"), Applicability::Review, vec![edit]);
        self.push(d);
    }

    fn missing_return(&mut self, block: &ast::Block, ret: Ty) {
        let close = Span::new(block.span.end - 1, block.span.end);
        let name = self.name.clone();
        let edit = if self.starts_line(close.start) {
            let indent = self.indent_at(close.start);
            self.insert(self.line_start(close.start), format!("{indent}  return _;\n"))
        } else {
            self.insert(close.start, " return _; ")
        };
        let d = self
            .err(codes::MISSING_RETURN, close, format!("missing return: `{name}` can reach its end without returning {}", ret.a_name()))
            .with_label(format!("`{name}` returns {}", ret.name()))
            .with_fix(format!("return a value at the end of `{name}`"), Applicability::Review, vec![edit]);
        self.push(d);
    }

    // ---- statements ----

    fn block(&mut self, block: &ast::Block, owner: (Span, &'static str)) -> ir::Block {
        self.push_scope(owner);
        let mut out = Vec::new();
        for &s in &block.stmts {
            let stmt = self.ast().stmt(s);
            let kind = match stmt.kind {
                StmtKind::If { .. } => "if",
                StmtKind::For { .. } => "for",
                StmtKind::While { .. } => "while",
                StmtKind::Try { .. } => "try",
                _ => "statement",
            };
            *self.owners.last_mut().unwrap() = (stmt.span, kind);
            self.stmt(s, &mut out);
        }
        self.pop_scope();
        out
    }

    fn stmt(&mut self, id: StmtId, out: &mut ir::Block) {
        let ast = self.ast();
        let stmt = ast.stmt(id);
        self.stmt = stmt.span;
        self.stmt_simple = matches!(stmt.kind, StmtKind::Let { .. } | StmtKind::Assign { .. } | StmtKind::Expr(_));
        match &stmt.kind {
            StmtKind::Let { is_const, name, ty, init } => {
                let annotated = ty.as_ref().map(|t| self.m.resolve_type(t));
                if let (Some(t), Some(a @ (Ty::Void | Ty::Never))) = (ty.as_ref(), annotated) {
                    let d = self
                        .err(codes::VOID_VALUE, t.span, format!("a variable can't have type {}", a.name()))
                        .with_label("no value has this type");
                    self.push(d);
                    if let Some(init) = *init {
                        self.expr(init);
                    }
                    self.declare(name, Ty::Poison, VarKind::Let, stmt.span, None, None);
                    return;
                }
                let Some(init) = *init else {
                    self.no_value(stmt.span, *is_const, name, ty.as_ref(), annotated);
                    self.declare(name, Ty::Poison, VarKind::Let, stmt.span, None, None);
                    return;
                };
                let value = self.expr(init);
                let mut ty = value.ty;
                if let Some(want) = annotated {
                    self.expect_type(&value, want, self.espan(init));
                    ty = want;
                }
                let value = coerce(value, ty);
                if matches!(ty, Ty::Void | Ty::Never) {
                    let d = self
                        .err(codes::VOID_VALUE, self.espan(init), format!("a variable can't hold {}", ty.name()))
                        .with_label(format!("this expression has type {}", ty.name()));
                    self.push(d);
                    ty = Ty::Poison;
                }
                let keyword = if *is_const { Some(Span::new(stmt.span.start, stmt.span.start + 5)) } else { None };
                let kind = if *is_const { VarKind::Const } else { VarKind::Let };
                let local = self.declare(name, ty, kind, stmt.span, keyword, Some(init));
                out.push(ir::Stmt::Let(local, value.e));
            }
            StmtKind::Assign { target, op, value } => {
                if let Some(s) = self.assign(*target, *op, *value, stmt.span) {
                    out.push(s);
                }
            }
            StmtKind::IncDec { target, inc, prefix } => {
                if *prefix {
                    let t = self.text(self.espan(*target));
                    let op = if *inc { "++" } else { "--" };
                    let span = Span::new(stmt.span.start, self.espan(*target).end);
                    let d = self
                        .err(codes::INCDEC_PREFIX, span, format!("no prefix form: write `{t}{op}`"))
                        .with_label(format!("`{op}` goes after the variable"))
                        .with_fix(format!("use `{t}{op}`"), Applicability::Safe, vec![self.edit(span, format!("{t}{op}"))]);
                    self.push(d);
                    return;
                }
                if let Some(s) = self.incdec(*target, *inc) {
                    out.push(s);
                }
            }
            StmtKind::Expr(e) => {
                let v = self.expr(*e);
                out.push(ir::Stmt::Expr(v.e));
            }
            StmtKind::If { cond, then, els } => {
                let s = self.if_stmt(*cond, then, els.as_ref(), stmt.span);
                out.push(s);
            }
            StmtKind::For { init, cond, update, body } => {
                self.push_scope((stmt.span, "for"));
                if let Some(init) = init {
                    self.stmt(*init, out);
                }
                let cond = cond.map(|c| self.condition(c).e);
                let mut step = Vec::new();
                if let Some(u) = update {
                    self.stmt(*u, &mut step);
                }
                self.loops += 1;
                let body = self.block(body, (stmt.span, "for"));
                self.loops -= 1;
                self.pop_scope();
                out.push(ir::Stmt::Loop { cond, body, step });
            }
            StmtKind::While { cond, body } => {
                let cond = self.condition(*cond);
                self.loops += 1;
                let body = self.block(body, (stmt.span, "while"));
                self.loops -= 1;
                out.push(ir::Stmt::Loop { cond: Some(cond.e), body, step: Vec::new() });
            }
            StmtKind::Break | StmtKind::Continue => {
                let is_break = matches!(stmt.kind, StmtKind::Break);
                if self.loops == 0 {
                    let word = if is_break { "break" } else { "continue" };
                    let d = self
                        .err(codes::OUTSIDE_LOOP, stmt.span, format!("`{word}` outside a loop"))
                        .with_label("there's no loop to leave here");
                    self.push(d);
                    return;
                }
                out.push(if is_break { ir::Stmt::Break } else { ir::Stmt::Continue });
            }
            StmtKind::Return(value) => {
                if let Some(s) = self.return_stmt(*value, stmt.span) {
                    out.push(s);
                }
            }
            StmtKind::Throw(value) => {
                let v = self.expr(*value);
                match v.ty {
                    Ty::Error(e) if e.is_runtime() && e != ir::ErrorType::Any => {
                        out.push(ir::Stmt::Throw(v.e, self.loc(stmt.span)));
                    }
                    Ty::Error(ir::ErrorType::Any) => out.push(ir::Stmt::Throw(v.e, self.loc(stmt.span))),
                    Ty::Error(_) => self.unsupported(self.espan(*value), "declared errors (`Error` and `ValueError` need `throws`)"),
                    Ty::Poison => {}
                    t => {
                        let d = self
                            .err(
                                codes::THROW_NON_ERROR,
                                self.espan(*value),
                                format!("only errors can be thrown, and this is {}", t.a_name()),
                            )
                            .with_label("throw a built-in error such as `RangeError { message: ... }`");
                        self.push(d);
                    }
                }
            }
            StmtKind::Try { body, catches, finally } => {
                if let Some(f) = finally {
                    self.unsupported(f.span, "`finally` blocks");
                }
                let s = self.try_stmt(body, catches, stmt.span);
                out.push(s);
            }
            StmtKind::Unsupported { what } => self.unsupported(stmt.span, what),
        }
    }

    /// `let x: int;`, `let x;` and `const x: string;` (§02).
    fn no_value(&mut self, span: Span, is_const: bool, name: &ast::Ident, ty: Option<&ast::TypeExpr>, annotated: Option<Ty>) {
        let kw = if is_const { "const" } else { "let" };
        let n = &name.name;
        let d = match ty {
            _ if is_const => {
                let t = ty.map(|t| format!(": {}", self.text(t.span))).unwrap_or_default();
                self.err(codes::CONST_NO_VALUE, span, "const must be initialized").with_fix(
                    "give it a value",
                    Applicability::Review,
                    vec![self.edit(span, format!("const {n}{t} = _;"))],
                )
            }
            None => self
                .err(codes::NO_TYPE_NO_VALUE, span, "no type and no value")
                .with_label(format!("dat can't tell what type `{n}` has"))
                .with_fix("give it a value", Applicability::Review, vec![self.edit(span, format!("let {n} = _;"))]),
            Some(_) if annotated == Some(Ty::Poison) => return,
            Some(t) => {
                let t = self.text(t.span);
                self.err(codes::LET_NO_VALUE, span, "a non-nullable variable needs a value")
                    .with_label(format!("only a nullable `{kw}` starts as null"))
                    .with_fix("give it a value", Applicability::Option, vec![self.edit(span, format!("let {n}: {t} = _;"))])
                    .with_fix("make it nullable", Applicability::Option, vec![self.edit(span, format!("let {n}: {t}?;"))])
            }
        };
        self.push(d);
    }

    fn if_stmt(&mut self, cond: ExprId, then: &ast::Block, els: Option<&ast::Else>, span: Span) -> ir::Stmt {
        let c = self.condition(cond);
        let t = self.block(then, (span, "if"));
        let e = match els {
            None => Vec::new(),
            Some(ast::Else::Block(b)) => self.block(b, (span, "if")),
            Some(ast::Else::If(s)) => {
                let StmtKind::If { cond, then, els } = &self.ast().stmt(*s).kind else { unreachable!() };
                vec![self.if_stmt(*cond, then, els.as_ref(), span)]
            }
        };
        ir::Stmt::If(c.e, t, e)
    }

    /// A condition must be a `bool`: there's no truthiness.
    fn condition(&mut self, cond: ExprId) -> TE {
        let c = coerce(self.expr(cond), Ty::Bool);
        if c.ty != Ty::Bool && c.ty != Ty::Poison && c.ty != Ty::Never {
            self.not_bool(cond, c.ty);
            return poison();
        }
        c
    }

    fn not_bool(&mut self, cond: ExprId, ty: Ty) {
        let span = self.espan(cond);
        let src = self.text(span);
        let mut d = self.err(codes::NOT_BOOL, span, "condition must be bool").with_label(format!("`{src}` is {}", ty.a_name()));
        let fix = match ty {
            Ty::Int => Some(("compare with 0", format!("{src} != 0"))),
            Ty::Str => Some(("compare with the empty string", format!("{src} != \"\""))),
            _ => None,
        };
        if let Some((title, after)) = fix {
            d = d.with_fix(title, Applicability::Safe, vec![self.edit(span, after)]);
        }
        self.push(d);
    }

    fn assign(&mut self, target: ExprId, op: AssignOp, value: ExprId, span: Span) -> Option<ir::Stmt> {
        let ast = self.ast();
        let ExprKind::Name(name) = &ast.expr(target).kind else {
            self.expr(value);
            self.unsupported(self.espan(target), "assignments to fields and elements");
            return None;
        };
        let rhs = self.expr(value);
        let (local, ty) = self.assignable(name, self.espan(target), "reassigned")?;
        let rhs = coerce(rhs, ty);
        let loc = self.loc(span);
        let current = mk(ir::ExprKind::Local(local), ty);
        let new = match op {
            AssignOp::Set => {
                self.expect_type(&rhs, ty, self.espan(value));
                rhs
            }
            AssignOp::Add if ty == Ty::Str && rhs.ty == Ty::Str => mk(ir::ExprKind::Concat(vec![current.e, rhs.e]), Ty::Str),
            AssignOp::Add | AssignOp::Sub | AssignOp::Mul | AssignOp::Rem if ty == Ty::Int && rhs.ty == Ty::Int => {
                let op = match op {
                    AssignOp::Add => ir::IntOp::Add,
                    AssignOp::Sub => ir::IntOp::Sub,
                    AssignOp::Mul => ir::IntOp::Mul,
                    _ => ir::IntOp::Rem,
                };
                mk(ir::ExprKind::IntArith { op, lhs: Box::new(current.e), rhs: Box::new(rhs.e), loc }, Ty::Int)
            }
            AssignOp::Div => {
                self.unsupported(span, "`/` and floats");
                return None;
            }
            _ if ty == Ty::Poison || rhs.ty == Ty::Poison => return None,
            _ => {
                let d = self
                    .err(codes::BAD_OPERANDS, span, format!("can't apply `{}` to {} and {}", op.as_str(), ty.a_name(), rhs.ty.a_name()))
                    .with_label(format!("`{name}` is {}", ty.a_name()));
                self.push(d);
                return None;
            }
        };
        self.locals[local.0 as usize].reassigned = true;
        Some(ir::Stmt::Set(local, new.e))
    }

    /// Finds a variable that a statement may change, or reports why it can't.
    fn assignable(&mut self, name: &str, span: Span, verb: &str) -> Option<(ir::LocalId, Ty)> {
        let Some(var) = self.lookup(name) else {
            self.undefined(name, span);
            return None;
        };
        let (local, ty, kind, decl, keyword) = (var.local, var.ty, var.kind, var.decl, var.keyword);
        match kind {
            VarKind::Let => Some((local, ty)),
            VarKind::Const => {
                let mut d = self
                    .err(codes::CONST_ASSIGN, span, format!("`{name}` is const and can't be {verb}"))
                    .with_label("declared with `const`");
                if let Some(kw) = keyword {
                    let decl_text = self.text(decl);
                    let fixed = format!("let{}", &decl_text[5..]);
                    d = d.with_fix("declare it with `let`", Applicability::Review, vec![self.edit(kw, "let")]);
                    let _ = fixed;
                }
                self.push(d);
                None
            }
            VarKind::Param => {
                let d = self
                    .err(codes::CONST_ASSIGN, span, format!("`{name}` is a parameter and can't be {verb}"))
                    .with_label("parameters are constants; copy the value into a `let` to change it");
                self.push(d);
                None
            }
            VarKind::Catch => {
                let d = self.err(codes::CONST_ASSIGN, span, format!("`{name}` is the caught error and can't be {verb}"));
                self.push(d);
                None
            }
        }
    }

    fn incdec(&mut self, target: ExprId, inc: bool) -> Option<ir::Stmt> {
        let ast = self.ast();
        let span = self.espan(target);
        let ExprKind::Name(name) = &ast.expr(target).kind else {
            self.unsupported(span, "`++` and `--` on fields and elements");
            return None;
        };
        let (local, ty) = self.assignable(name, span, if inc { "incremented" } else { "decremented" })?;
        let op = if inc { "++" } else { "--" };
        if ty != Ty::Int {
            if ty != Ty::Poison {
                let d = self.err(codes::BAD_OPERANDS, span, format!("`{op}` needs an int, and `{name}` is {}", ty.a_name()));
                self.push(d);
            }
            return None;
        }
        self.locals[local.0 as usize].reassigned = true;
        let one = mk(ir::ExprKind::Int(1), Ty::Int);
        let cur = mk(ir::ExprKind::Local(local), Ty::Int);
        let op = if inc { ir::IntOp::Add } else { ir::IntOp::Sub };
        let e = mk(ir::ExprKind::IntArith { op, lhs: Box::new(cur.e), rhs: Box::new(one.e), loc: self.loc(span) }, Ty::Int);
        Some(ir::Stmt::Set(local, e.e))
    }

    fn return_stmt(&mut self, value: Option<ExprId>, span: Span) -> Option<ir::Stmt> {
        let v = value.map(|e| self.expr(e));
        if let Some(e) = value
            && self.is_main()
        {
            self.m.main_returns.push((span, e));
        }
        if self.is_bad_main(self.ret) {
            return None;
        }
        match (self.ret, v) {
            (None, v) => {
                let ty = v.as_ref().map_or(Ty::Void, |v| v.ty);
                if ty != Ty::Never {
                    self.returns.push((ty, span));
                }
                Some(ir::Stmt::Return(v.map(|v| v.e)))
            }
            (Some(Ty::Never), v) if v.as_ref().is_none_or(|v| v.ty != Ty::Never && v.ty != Ty::Poison) => {
                let name = self.name.clone();
                let d = self
                    .err(codes::NEVER_RETURNS, span, format!("`{name}` returns never, so it can't `return`"))
                    .with_label("throw an error instead, or change the return type");
                self.push(d);
                // Keep the `return`, so the end of the function isn't reported too.
                Some(ir::Stmt::Return(None))
            }
            (Some(Ty::Void), Some(v)) if v.ty != Ty::Poison && v.ty != Ty::Never => {
                let name = self.name.clone();
                let d = self
                    .err(
                        codes::RETURN_IN_VOID,
                        self.espan(value.unwrap()),
                        format!("`{name}` returns void, so `return` can't have a value"),
                    )
                    .with_fix("return without a value", Applicability::Review, vec![self.edit(span, "return;")]);
                self.push(d);
                None
            }
            (Some(Ty::Void), v) => Some(ir::Stmt::Return(v.filter(|v| v.ty == Ty::Never).map(|v| v.e))),
            (Some(want), None) => {
                if want != Ty::Poison {
                    let name = self.name.clone();
                    let d = self
                        .err(codes::RETURN_NO_VALUE, span, format!("`{name}` returns {}, so `return` needs a value", want.a_name()))
                        .with_fix("return a value", Applicability::Review, vec![self.edit(span, "return _;")]);
                    self.push(d);
                }
                None
            }
            (Some(want), Some(v)) => {
                self.expect_type(&v, want, self.espan(value.unwrap()));
                Some(ir::Stmt::Return(Some(coerce(v, want).e)))
            }
        }
    }

    fn try_stmt(&mut self, body: &ast::Block, catches: &[ast::Catch], span: Span) -> ir::Stmt {
        let body = self.block(body, (span, "try"));
        let mut out = Vec::new();
        for (i, c) in catches.iter().enumerate() {
            let (filter, ty) = match &c.ty {
                None => (ir::CatchFilter::All, Ty::Error(ir::ErrorType::Any)),
                Some(t) => match self.m.resolve_type(t) {
                    Ty::Error(ir::ErrorType::Error) => (ir::CatchFilter::AllAsError, Ty::Error(ir::ErrorType::Error)),
                    Ty::Error(e) if e.is_runtime() => (ir::CatchFilter::Is(e), Ty::Error(e)),
                    Ty::Error(_) => {
                        self.unsupported(t.span, "declared errors such as `ValueError`");
                        (ir::CatchFilter::All, Ty::Poison)
                    }
                    Ty::Poison => (ir::CatchFilter::All, Ty::Poison),
                    other => {
                        let d = self
                            .err(codes::CATCH_NON_ERROR, t.span, format!("`{}` isn't an error type", other.name()))
                            .with_label("catch an error type such as `RangeError`, or `Error` for every error");
                        self.push(d);
                        (ir::CatchFilter::All, Ty::Poison)
                    }
                },
            };
            if filter
                != ir::CatchFilter::Is(match filter {
                    ir::CatchFilter::Is(e) => e,
                    _ => ir::ErrorType::Any,
                })
                && i + 1 < catches.len()
            {
                let d = self
                    .err(codes::CATCH_ALL_NOT_LAST, c.span, "a clause that catches every error must be the last one")
                    .with_label("the clauses after it could never run");
                self.push(d);
            }
            self.push_scope((span, "try"));
            let bind = c.name.as_ref().map(|n| self.declare(n, ty, VarKind::Catch, n.span, None, None));
            let body = self.block(&c.body, (span, "try"));
            self.pop_scope();
            out.push(ir::Catch { filter, bind, body });
        }
        ir::Stmt::Try { body, catches: out }
    }

    // ---- expressions ----

    fn expect_type(&mut self, v: &TE, want: Ty, span: Span) {
        if v.ty == want || matches!(v.ty, Ty::Poison | Ty::Never) || want == Ty::Poison {
            return;
        }
        let mut d = self
            .err(codes::MISMATCH, span, format!("expected {}, found {}", want.a_name(), v.ty.a_name()))
            .with_label(format!("this is {}", v.ty.a_name()));
        let src = self.text(span);
        if want == Ty::Str && matches!(v.ty, Ty::Int | Ty::Bool) {
            d = d.with_fix("convert it with a template string", Applicability::Safe, vec![self.edit(span, format!("`${{{src}}}`"))]);
        }
        self.push(d);
    }

    fn undefined(&mut self, name: &str, span: Span) {
        let mut d = self.err(codes::UNDEFINED, span, format!("{name} is not defined"));
        // A declaration in a block that has ended: move it above that block.
        let candidate = self.closed.iter().rev().find(|c| c.name == name && c.owner.end <= span.start);
        if let Some(c) = candidate {
            let (decl, owner, kind) = (c.decl, c.owner, c.owner_kind);
            let decl_text = self.text(decl).to_string();
            d = d.with_label(format!("`{name}` is declared inside the {kind} block above, and that block has ended"));
            if let Some(line) = self.whole_line(decl)
                && self.starts_line(owner.start)
            {
                let indent = self.indent_at(owner.start);
                d = d.with_fix(
                    format!("move the declaration above the {kind}"),
                    Applicability::Review,
                    vec![self.insert(self.line_start(owner.start), format!("{indent}{decl_text}\n")), self.edit(line, "")],
                );
            }
        } else if let Some(similar) = self.similar_name(name) {
            d = d.with_fix(format!("use `{similar}`"), Applicability::Review, vec![self.edit(span, similar.clone())]);
        }
        self.push(d);
    }

    fn similar_name(&self, name: &str) -> Option<String> {
        if name.len() < 3 {
            return None;
        }
        let vars = self.scopes.iter().flatten().map(|v| v.name.as_str());
        let funcs = self.m.by_name.keys().map(String::as_str);
        let builtins = ["print", "assert", "fail"].into_iter();
        vars.chain(funcs)
            .chain(builtins)
            .filter(|c| *c != name)
            .map(|c| (edit_distance(name, c), c))
            .filter(|(d, _)| *d <= 2)
            .min()
            .map(|(_, c)| c.to_string())
    }

    fn expr(&mut self, id: ExprId) -> TE {
        let ast = self.ast();
        let e = ast.expr(id);
        let span = e.span;
        match &e.kind {
            ExprKind::Int(Some(v)) if *v <= i64::MAX as u64 => mk(ir::ExprKind::Int(*v as i64), Ty::Int),
            ExprKind::Int(_) => {
                self.unsupported(span, "integer literals outside the int range");
                poison()
            }
            ExprKind::Float(_) => {
                self.unsupported(span, "floats");
                poison()
            }
            ExprKind::Str(s) => mk(ir::ExprKind::Str(s.clone()), Ty::Str),
            ExprKind::Template(parts) => self.template(parts),
            ExprKind::Bool(b) => mk(ir::ExprKind::Bool(*b), Ty::Bool),
            ExprKind::Null => {
                self.unsupported(span, "`null` and nullable types");
                poison()
            }
            ExprKind::Name(name) => self.name(name, span),
            ExprKind::Unary { op: UnOp::Neg, expr } => {
                if let ExprKind::Int(Some(v)) = ast.expr(*expr).kind
                    && v <= i64::MAX as u64
                {
                    return mk(ir::ExprKind::Int(-(v as i64)), Ty::Int);
                }
                let v = coerce(self.expr(*expr), Ty::Int);
                match v.ty {
                    Ty::Int => mk(ir::ExprKind::IntNeg { arg: Box::new(v.e), loc: self.loc(span) }, Ty::Int),
                    Ty::Poison => poison(),
                    t => self.bad_unary("-", t, span),
                }
            }
            ExprKind::Unary { op: UnOp::Not, expr } => {
                let v = coerce(self.expr(*expr), Ty::Bool);
                match v.ty {
                    Ty::Bool => mk(ir::ExprKind::Not(Box::new(v.e)), Ty::Bool),
                    Ty::Poison => poison(),
                    t => self.bad_unary("!", t, span),
                }
            }
            ExprKind::Binary { op: BinOp::Add, .. } => self.add_chain(id),
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, *lhs, *rhs, span),
            ExprKind::Call { callee, args } => self.call(id, *callee, args),
            ExprKind::Field { base, name, optional } => {
                if *optional {
                    self.expr(*base);
                    self.unsupported(span, "`?.` and nullable types");
                    return poison();
                }
                if let ExprKind::Name(n) = &ast.expr(*base).kind
                    && n == "Math"
                    && self.lookup("Math").is_none()
                {
                    self.unsupported(span, "`Math` functions");
                    return poison();
                }
                if let ExprKind::Name(n) = &ast.expr(*base).kind
                    && is_type_name(n)
                    && self.lookup(n).is_none()
                {
                    // `int.MAX` and similar, unless a variable has the type's name.
                    self.unsupported(span, "type members");
                    return poison();
                }
                let b = self.expr(*base);
                match (b.ty, name.name.as_str()) {
                    (Ty::Error(_), "message") => mk(ir::ExprKind::ErrorMessage(Box::new(b.e)), Ty::Str),
                    (Ty::Poison, _) => poison(),
                    (Ty::Error(e), field) => {
                        let d = self
                            .err(codes::UNKNOWN_FIELD, name.span, format!("{} has no field `{field}`", e.name()))
                            .with_label("built-in errors have one field, `message`");
                        self.push(d);
                        poison()
                    }
                    _ => {
                        self.unsupported(span, "fields and methods");
                        poison()
                    }
                }
            }
            ExprKind::Index { base, .. } => {
                self.expr(*base);
                self.unsupported(span, "indexing");
                poison()
            }
            ExprKind::StructLit { name, fields } => self.struct_lit(name, fields, span),
            ExprKind::List(_) => {
                self.unsupported(span, "lists");
                poison()
            }
            ExprKind::Paren(inner) => self.expr(*inner),
            ExprKind::Cond { cond, then, els } => {
                let c = self.condition(*cond);
                let t = self.expr(*then);
                let e = self.expr(*els);
                if t.ty == Ty::Poison || e.ty == Ty::Poison || c.ty == Ty::Poison {
                    return poison();
                }
                let want = meet(t.ty, e.ty, Ty::Never);
                let (t, e) = (coerce(t, want), coerce(e, want));
                if want == Ty::Never {
                    return mk(ir::ExprKind::Cond { cond: Box::new(c.e), then: Box::new(t.e), els: Box::new(e.e) }, Ty::Never);
                }
                if t.ty != e.ty || !matches!(t.ty, Ty::Int | Ty::Bool | Ty::Str) {
                    self.unsupported(span, "conditional expressions whose branches have different types");
                    return poison();
                }
                let ty = t.ty;
                mk(ir::ExprKind::Cond { cond: Box::new(c.e), then: Box::new(t.e), els: Box::new(e.e) }, ty)
            }
            ExprKind::IncDec { target, inc, prefix } => {
                self.incdec_expr(span, *target, *inc, *prefix);
                poison()
            }
            ExprKind::Unsupported { what } => {
                self.unsupported(span, what);
                poison()
            }
        }
    }

    fn bad_unary(&mut self, op: &str, ty: Ty, span: Span) -> TE {
        let want = if op == "-" { "an int" } else { "a bool" };
        let d = self.err(codes::BAD_OPERANDS, span, format!("`{op}` needs {want}, not {}", ty.a_name()));
        self.push(d);
        poison()
    }

    fn incdec_expr(&mut self, span: Span, target: ExprId, inc: bool, prefix: bool) {
        let t = self.text(self.espan(target));
        let op = if inc { "++" } else { "--" };
        let mut d = self
            .err(codes::INCDEC_EXPR, span, format!("{op} is a statement"))
            .with_label(format!("`{op}` can't be used inside an expression"));
        if self.stmt_simple {
            let edits = if prefix {
                vec![self.insert(self.stmt.start, format!("{t}{op}; ")), self.edit(span, t)]
            } else {
                vec![self.edit(span, t), self.insert(self.stmt.end, format!(" {t}{op};"))]
            };
            d = d.with_fix(format!("move `{t}{op}` into its own statement"), Applicability::Safe, edits);
        }
        self.push(d);
    }

    fn name(&mut self, name: &str, span: Span) -> TE {
        if let Some(v) = self.lookup(name) {
            let (local, ty) = (v.local, v.ty);
            return mk(ir::ExprKind::Local(local), ty);
        }
        if self.m.by_name.contains_key(name) || matches!(name, "print" | "assert" | "fail" | "range") {
            self.unsupported(span, "function values");
            return poison();
        }
        if ir::ErrorType::from_name(name).is_some() {
            let d = self.err(codes::MISMATCH, span, format!("`{name}` is a type, not a value")).with_fix(
                format!("create {name} value"),
                Applicability::Review,
                vec![self.edit(span, format!("{name} {{ message: _ }}"))],
            );
            self.push(d);
            return poison();
        }
        self.undefined(name, span);
        poison()
    }

    fn template(&mut self, parts: &[ast::TemplatePart]) -> TE {
        let mut out = Vec::new();
        let mut bad = false;
        for p in parts {
            match p {
                ast::TemplatePart::Text(t) if t.is_empty() => {}
                ast::TemplatePart::Text(t) => out.push(mk(ir::ExprKind::Str(t.clone()), Ty::Str).e),
                ast::TemplatePart::Expr(e) => {
                    let v = coerce(self.expr(*e), Ty::Str);
                    match v.ty {
                        Ty::Str => out.push(v.e),
                        t if t.is_printable() => out.push(mk(ir::ExprKind::ToStr(Box::new(v.e)), Ty::Str).e),
                        Ty::Poison => bad = true,
                        t => {
                            let d = self.err(codes::MISMATCH, self.espan(*e), format!("{} can't be put into a string", t.a_name()));
                            self.push(d);
                            bad = true;
                        }
                    }
                }
            }
        }
        if bad {
            return poison();
        }
        concat(out)
    }

    /// `a + b + c`: checked as one left-to-right chain, so mixing strings and
    /// ints is reported once, with a template string for the whole chain.
    fn add_chain(&mut self, id: ExprId) -> TE {
        let ast = self.ast();
        let mut operands = Vec::new();
        let mut cur = id;
        while let ExprKind::Binary { op: BinOp::Add, lhs, rhs } = ast.expr(cur).kind {
            operands.push(rhs);
            cur = lhs;
        }
        operands.push(cur);
        operands.reverse();
        let checked: Vec<TE> = operands.iter().map(|&o| self.expr(o)).collect();
        let tys: Vec<Ty> = checked.iter().map(|c| c.ty).collect();
        let whole = self.espan(id);
        let loc = self.loc(whole);
        let mut iter = checked.into_iter().enumerate();
        let (_, mut acc) = iter.next().unwrap();
        for (i, v) in iter {
            let want = meet(acc.ty, v.ty, Ty::Int);
            let (acc_in, v) = (coerce(acc, want), coerce(v, want));
            acc = acc_in;
            acc = match (acc.ty, v.ty) {
                (Ty::Poison, _) | (_, Ty::Poison) => return poison(),
                (Ty::Int, Ty::Int) => {
                    mk(ir::ExprKind::IntArith { op: ir::IntOp::Add, lhs: Box::new(acc.e), rhs: Box::new(v.e), loc }, Ty::Int)
                }
                (Ty::Str, Ty::Str) => concat(vec![acc.e, v.e]),
                (Ty::Str, other) | (other, Ty::Str) if other.is_printable() => {
                    self.string_plus(&operands, &tys, i, acc.ty, v.ty, whole);
                    return poison();
                }
                (a, b) => {
                    let d = self.err(codes::BAD_OPERANDS, whole, format!("can't add {} and {}", a.name(), b.name()));
                    self.push(d);
                    return poison();
                }
            };
        }
        acc
    }

    /// E0301, with a template string as the fix. The chain's first `at`
    /// operands have already been combined into one value of type `prefix`.
    ///
    /// Two ints next to each other after a string, as in `"Total: " + count
    /// + 1`, may mean the sum or "Total: 31", so both are options, the sum
    /// first as the likelier intent, and `dat fix` applies neither.
    fn string_plus(&mut self, operands: &[ExprId], tys: &[Ty], at: usize, prefix: Ty, next: Ty, whole: Span) {
        let ast = self.ast();
        let (bad, bad_ty) = if prefix == Ty::Str {
            (self.espan(operands[at]), next)
        } else {
            (self.espan(operands[0]).to(self.espan(operands[at - 1])), prefix)
        };
        let (a, b) = if prefix == Ty::Str { ("string", bad_ty.name()) } else { (bad_ty.name(), "string") };
        let piece = |e: ExprId, out: &mut String| match &ast.expr(e).kind {
            ExprKind::Str(s) => out.push_str(&escape_template(s)),
            ExprKind::Template(_) => {
                let raw = self.text(ast.expr(e).span);
                out.push_str(&raw[1..raw.len() - 1]);
            }
            _ => {
                out.push_str("${");
                out.push_str(self.text(ast.expr(e).span));
                out.push('}');
            }
        };
        let first_str = tys.iter().position(|t| *t == Ty::Str).unwrap_or(at);
        // The end of the run of ints that starts at `i`, after the first string.
        let run_end = |i: usize| {
            let mut j = i;
            while i > first_str && j + 1 < tys.len() && tys[i] == Ty::Int && tys[j + 1] == Ty::Int {
                j += 1;
            }
            j
        };
        let template = |sum_runs: bool| {
            let mut out = String::from("`");
            let mut i = 0;
            if prefix != Ty::Str {
                out.push_str(&format!("${{{}}}", self.text(bad)));
                i = at;
            }
            while i < operands.len() {
                let j = if sum_runs { run_end(i) } else { i };
                if j > i {
                    out.push_str(&format!("${{{}}}", self.text(self.espan(operands[i]).to(self.espan(operands[j])))));
                } else {
                    piece(operands[i], &mut out);
                }
                i = j + 1;
            }
            out.push('`');
            out
        };
        let ambiguous = (first_str + 1..tys.len()).any(|i| run_end(i) > i);
        let d = self.err(codes::STRING_PLUS, whole, format!("can't add {a} and {b}")).with_label(format!(
            "`{}` is {}",
            self.text(bad),
            bad_ty.a_name()
        ));
        let d = if ambiguous {
            d.with_fix("add the numbers first", Applicability::Option, vec![self.edit(whole, template(true))]).with_fix(
                "join the values as text",
                Applicability::Option,
                vec![self.edit(whole, template(false))],
            )
        } else {
            d.with_fix("use a template string", Applicability::Safe, vec![self.edit(whole, template(false))])
        };
        self.push(d);
    }

    fn binary(&mut self, op: BinOp, lhs: ExprId, rhs: ExprId, span: Span) -> TE {
        match op {
            BinOp::Nullish => {
                self.unsupported(span, "`??` and nullable types");
                return poison();
            }
            BinOp::In => {
                self.unsupported(span, "`in` and collections");
                return poison();
            }
            BinOp::Div => {
                self.unsupported(span, "`/` (it returns a float) and floats");
                return poison();
            }
            _ => {}
        }
        let l = self.expr(lhs);
        let r = self.expr(rhs);
        if l.ty == Ty::Poison || r.ty == Ty::Poison {
            return poison();
        }
        let default = if matches!(op, BinOp::And | BinOp::Or) { Ty::Bool } else { Ty::Int };
        let want = meet(l.ty, r.ty, default);
        let (l, r) = (coerce(l, want), coerce(r, want));
        let loc = self.loc(span);
        match op {
            BinOp::Sub | BinOp::Mul | BinOp::Rem if l.ty == Ty::Int && r.ty == Ty::Int => {
                let op = match op {
                    BinOp::Sub => ir::IntOp::Sub,
                    BinOp::Mul => ir::IntOp::Mul,
                    _ => ir::IntOp::Rem,
                };
                mk(ir::ExprKind::IntArith { op, lhs: Box::new(l.e), rhs: Box::new(r.e), loc }, Ty::Int)
            }
            BinOp::And | BinOp::Or if l.ty == Ty::Bool && r.ty == Ty::Bool => {
                let kind = if op == BinOp::And {
                    ir::ExprKind::And(Box::new(l.e), Box::new(r.e))
                } else {
                    ir::ExprKind::Or(Box::new(l.e), Box::new(r.e))
                };
                mk(kind, Ty::Bool)
            }
            _ if op.is_comparison() => match self.compare(op, l, r, span) {
                Some((op, l, r)) => mk(ir::ExprKind::Compare { op, lhs: Box::new(l.e), rhs: Box::new(r.e) }, Ty::Bool),
                None => poison(),
            },
            _ => {
                let d = self.err(
                    codes::BAD_OPERANDS,
                    span,
                    format!("can't apply `{}` to {} and {}", op.as_str(), l.ty.a_name(), r.ty.a_name()),
                );
                self.push(d);
                poison()
            }
        }
    }

    fn compare(&mut self, op: BinOp, l: TE, r: TE, span: Span) -> Option<(ir::CmpOp, TE, TE)> {
        let cmp = match op {
            BinOp::Lt => ir::CmpOp::Lt,
            BinOp::Le => ir::CmpOp::Le,
            BinOp::Gt => ir::CmpOp::Gt,
            BinOp::Ge => ir::CmpOp::Ge,
            BinOp::Eq => ir::CmpOp::Eq,
            _ => ir::CmpOp::Ne,
        };
        let want = meet(l.ty, r.ty, Ty::Int);
        let (l, r) = (coerce(l, want), coerce(r, want));
        let ordered = matches!(cmp, ir::CmpOp::Lt | ir::CmpOp::Le | ir::CmpOp::Gt | ir::CmpOp::Ge);
        let ok = l.ty == r.ty && if ordered { matches!(l.ty, Ty::Int | Ty::Str) } else { matches!(l.ty, Ty::Int | Ty::Bool | Ty::Str) };
        if ok {
            return Some((cmp, l, r));
        }
        if l.ty == Ty::Poison || r.ty == Ty::Poison {
            return None;
        }
        if l.ty == r.ty && matches!(l.ty, Ty::Error(_)) {
            self.unsupported(span, "comparing errors");
            return None;
        }
        let d =
            self.err(codes::BAD_OPERANDS, span, format!("can't compare {} and {} with `{}`", l.ty.a_name(), r.ty.a_name(), op.as_str()));
        self.push(d);
        None
    }

    fn call(&mut self, id: ExprId, callee: ExprId, args: &[ast::Arg]) -> TE {
        let ast = self.ast();
        let span = self.espan(id);
        for a in args {
            if a.is_mut {
                self.unsupported(span, "`mut` arguments");
                return poison();
            }
        }
        match &ast.expr(callee).kind {
            ExprKind::Name(name) if self.lookup(name).is_none() => {
                if let Some(&f) = self.m.by_name.get(name.as_str()) {
                    return self.call_function(f, span, args);
                }
                match name.as_str() {
                    "print" => return self.print(args),
                    "assert" => return self.assert(span, args),
                    "fail" => return self.fail(span, args),
                    "range" => {
                        self.unsupported(span, "`range` and lists");
                        return poison();
                    }
                    _ => {}
                }
                for a in args {
                    self.expr(a.expr);
                }
                self.undefined(name, self.espan(callee));
                poison()
            }
            ExprKind::Field { base, name, optional: false } if name.name == "toString" => {
                if let ExprKind::Name(n) = &ast.expr(*base).kind
                    && n == "Math"
                {
                    self.unsupported(span, "`Math` functions");
                    return poison();
                }
                let b = self.expr(*base);
                if !args.is_empty() {
                    let d = self.err(codes::ARITY, span, format!("`toString` takes no arguments, but {} were given", args.len()));
                    self.push(d);
                    return poison();
                }
                match b.ty {
                    Ty::Str => b,
                    t if t.is_printable() => mk(ir::ExprKind::ToStr(Box::new(b.e)), Ty::Str),
                    Ty::Poison => poison(),
                    t => {
                        let d = self.err(codes::BAD_OPERANDS, span, format!("{} has no `toString`", t.a_name()));
                        self.push(d);
                        poison()
                    }
                }
            }
            ExprKind::Field { base, .. } => {
                if let ExprKind::Name(n) = &ast.expr(*base).kind
                    && n == "Math"
                {
                    self.unsupported(span, "`Math` functions");
                    return poison();
                }
                if let ExprKind::Name(n) = &ast.expr(*base).kind
                    && is_type_name(n)
                    && self.lookup(n).is_none()
                {
                    self.unsupported(span, "type members");
                    return poison();
                }
                self.unsupported(span, "methods");
                poison()
            }
            _ => {
                self.unsupported(span, "calls of function values");
                poison()
            }
        }
    }

    fn call_function(&mut self, f: ir::FuncId, span: Span, args: &[ast::Arg]) -> TE {
        let checked: Vec<TE> = args.iter().map(|a| self.expr(a.expr)).collect();
        if matches!(self.m.sigs[f.0 as usize].ret, Ret::Infer) {
            self.m.check_function(f);
        }
        let sig = &self.m.sigs[f.0 as usize];
        let name = sig.name.clone();
        let params = sig.params.clone();
        let ret = match sig.ret {
            Ret::Known(t) => t,
            Ret::InProgress | Ret::Infer => {
                let d = self
                    .err(codes::UNSUPPORTED, span, format!("`{name}` calls itself, so it needs a declared return type"))
                    .with_label("inferring the return type of a recursive function isn't supported yet");
                self.push(d);
                return poison();
            }
        };
        if params.len() != args.len() {
            let n = params.len();
            let d = self.err(
                codes::ARITY,
                span,
                format!(
                    "`{name}` takes {n} argument{}, but {} {} given",
                    if n == 1 { "" } else { "s" },
                    args.len(),
                    if args.len() == 1 { "was" } else { "were" }
                ),
            );
            self.push(d);
            return poison();
        }
        let mut bad = false;
        let mut lowered = Vec::with_capacity(checked.len());
        for ((v, (_, want)), a) in checked.into_iter().zip(&params).zip(args) {
            if v.ty == Ty::Poison {
                bad = true;
            }
            let before = self.m.diags.len();
            self.expect_type(&v, *want, self.espan(a.expr));
            bad |= self.m.diags.len() > before;
            lowered.push(coerce(v, *want).e);
        }
        if bad || ret == Ty::Poison {
            return poison();
        }
        let loc = self.loc(span);
        mk(ir::ExprKind::Call { func: f, args: lowered, loc }, ret)
    }

    /// Converts a value for `print` or a template: strings as they are, other values with `toString()`.
    fn stringify(&mut self, v: TE, span: Span) -> Option<ir::Expr> {
        let v = coerce(v, Ty::Str);
        match v.ty {
            Ty::Str => Some(v.e),
            t if t.is_printable() => Some(mk(ir::ExprKind::ToStr(Box::new(v.e)), Ty::Str).e),
            Ty::Poison => None,
            t => {
                let d = self.err(codes::MISMATCH, span, format!("{} can't be printed", t.a_name()));
                self.push(d);
                None
            }
        }
    }

    fn print(&mut self, args: &[ast::Arg]) -> TE {
        let mut parts = Vec::new();
        let mut bad = false;
        for (i, a) in args.iter().enumerate() {
            let v = self.expr(a.expr);
            if i > 0 {
                parts.push(mk(ir::ExprKind::Str(" ".into()), Ty::Str).e);
            }
            match self.stringify(v, self.espan(a.expr)) {
                Some(e) => parts.push(e),
                None => bad = true,
            }
        }
        if bad {
            return poison();
        }
        let joined = if parts.is_empty() { mk(ir::ExprKind::Str(String::new()), Ty::Str) } else { concat(parts) };
        mk(ir::ExprKind::Print(Box::new(joined.e)), Ty::Void)
    }

    fn assert(&mut self, span: Span, args: &[ast::Arg]) -> TE {
        let ast = self.ast();
        if args.is_empty() || args.len() > 2 {
            for a in args {
                self.expr(a.expr);
            }
            let d = self.err(
                codes::ARITY,
                span,
                format!("`assert` takes a condition and an optional message, but {} arguments were given", args.len()),
            );
            self.push(d);
            return poison();
        }
        let cond_id = args[0].expr;
        let cond_span = self.espan(cond_id);
        let cond = match &ast.expr(cond_id).kind {
            ExprKind::Binary { op, lhs, rhs } if op.is_comparison() => {
                let l = self.expr(*lhs);
                let r = self.expr(*rhs);
                if l.ty == Ty::Poison || r.ty == Ty::Poison {
                    None
                } else {
                    self.compare(*op, l, r, cond_span).map(|(op, l, r)| ir::AssertCond::Compare { op, lhs: l.e, rhs: r.e })
                }
            }
            _ => {
                let c = coerce(self.expr(cond_id), Ty::Bool);
                if c.ty == Ty::Bool {
                    Some(ir::AssertCond::Plain(c.e))
                } else {
                    if c.ty != Ty::Poison {
                        self.not_bool(cond_id, c.ty);
                    }
                    None
                }
            }
        };
        let message = match args.get(1) {
            Some(a) => {
                let m = coerce(self.expr(a.expr), Ty::Str);
                self.expect_type(&m, Ty::Str, self.espan(a.expr));
                if m.ty != Ty::Str {
                    return poison();
                }
                Some(m.e)
            }
            None => None,
        };
        let Some(cond) = cond else { return poison() };
        let a =
            ir::Assert { cond, message, src: self.text(span).to_string(), cond_src: self.text(cond_span).to_string(), loc: self.loc(span) };
        mk(ir::ExprKind::Assert(Box::new(a)), Ty::Void)
    }

    fn fail(&mut self, span: Span, args: &[ast::Arg]) -> TE {
        if args.len() != 1 {
            for a in args {
                self.expr(a.expr);
            }
            let d = self.err(codes::ARITY, span, format!("`fail` takes 1 argument, but {} were given", args.len()));
            self.push(d);
            return poison();
        }
        let m = coerce(self.expr(args[0].expr), Ty::Str);
        self.expect_type(&m, Ty::Str, self.espan(args[0].expr));
        if m.ty != Ty::Str {
            return poison();
        }
        mk(ir::ExprKind::Fail { message: Box::new(m.e), loc: self.loc(span) }, Ty::Never)
    }

    fn struct_lit(&mut self, name: &ast::Ident, fields: &[(ast::Ident, ExprId)], span: Span) -> TE {
        let Some(kind) = ir::ErrorType::from_name(&name.name) else {
            for (_, v) in fields {
                self.expr(*v);
            }
            self.unsupported(span, "structs");
            return poison();
        };
        let mut message = None;
        let mut bad = false;
        for (f, v) in fields {
            let value = self.expr(*v);
            if f.name != "message" {
                let d = self
                    .err(codes::UNKNOWN_FIELD, f.span, format!("{} has no field `{}`", name.name, f.name))
                    .with_label("built-in errors have one field, `message`");
                self.push(d);
                bad = true;
                continue;
            }
            let value = coerce(value, Ty::Str);
            self.expect_type(&value, Ty::Str, self.espan(*v));
            if value.ty != Ty::Str {
                bad = true;
            }
            message = Some(value);
        }
        if bad {
            return poison();
        }
        let Some(message) = message else {
            let close = Span::new(span.end - 1, span.end);
            let d = self.err(codes::MISSING_FIELD, span, format!("{} needs a `message`", name.name)).with_fix(
                "add the message",
                Applicability::Review,
                vec![self.insert(close.start, "message: _ ")],
            );
            self.push(d);
            return poison();
        };
        mk(ir::ExprKind::NewError { kind, message: Box::new(message.e) }, Ty::Error(kind))
    }
}

/// Joins string-typed parts into one expression, flattening nested joins.
fn concat(parts: Vec<ir::Expr>) -> TE {
    let mut flat = Vec::new();
    for p in parts {
        match p.kind {
            ir::ExprKind::Concat(inner) => flat.extend(inner),
            _ => flat.push(p),
        }
    }
    if flat.len() == 1 {
        let e = flat.pop().unwrap();
        return TE { e, ty: Ty::Str };
    }
    mk(ir::ExprKind::Concat(flat), Ty::Str)
}

fn escape_template(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '`' => out.push_str("\\`"),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            '$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
            c => out.push(c),
        }
    }
    out
}

/// Every path through `b` ends in `return`, `throw` or a `never` call.
fn terminates(b: &ir::Block) -> bool {
    b.iter().any(stmt_terminates)
}

fn stmt_terminates(s: &ir::Stmt) -> bool {
    match s {
        ir::Stmt::Return(_) | ir::Stmt::Throw(..) => true,
        ir::Stmt::Expr(e) | ir::Stmt::Let(_, e) | ir::Stmt::Set(_, e) => e.ty == ir::Type::Never,
        ir::Stmt::If(_, a, b) => terminates(a) && terminates(b),
        ir::Stmt::Try { body, catches } => terminates(body) && catches.iter().all(|c| terminates(&c.body)),
        ir::Stmt::Loop { cond, body, .. } => {
            matches!(cond.as_ref().map(|c| &c.kind), None | Some(ir::ExprKind::Bool(true))) && !breaks(body)
        }
        ir::Stmt::Break | ir::Stmt::Continue => false,
    }
}

/// Contains a `break` of the loop whose body is `b`.
fn breaks(b: &ir::Block) -> bool {
    b.iter().any(|s| match s {
        ir::Stmt::Break => true,
        ir::Stmt::If(_, a, c) => breaks(a) || breaks(c),
        ir::Stmt::Try { body, catches } => breaks(body) || catches.iter().any(|c| breaks(&c.body)),
        _ => false,
    })
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}
