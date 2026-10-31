//! Arrow functions, closures and function values (§06).

use super::{Body, Frame, TE, VarKind, coerce, mk, poison, terminates};
use crate::Ty;
use crate::codes;
use crate::module::{MAX_ARITY, Ret};
use dat_diag::{Applicability, Span};
use dat_ir as ir;
use dat_syntax::ast::{self, BinOp, ExprId, ExprKind, StmtKind};

/// A `forEach` call whose callback, if it uses a `let` variable, can be a
/// loop instead (E0603's fix): the arrow function's level, and the edit.
pub(super) struct ForEachFix {
    pub level: usize,
    pub span: Span,
    pub after: String,
}

impl Body<'_, '_> {
    /// An arrow function, lowered to a function of its own and used as a
    /// closure value. `params` and `ret` are the types the context gives.
    pub(super) fn arrow_fn(&mut self, id: ExprId, a: &ast::Arrow, params: Option<Vec<Ty>>, ret: Option<Ty>) -> TE {
        let span = self.espan(id);
        let mut ptys = Vec::new();
        let mut bad = false;
        if a.params.len() > MAX_ARITY {
            self.unsupported(span, &format!("arrow functions with more than {MAX_ARITY} parameters"));
            bad = true;
        }
        for (i, p) in a.params.iter().enumerate() {
            if p.is_mut {
                self.unsupported(p.span, "`mut` parameters of arrow functions");
                bad = true;
            }
            if p.default.is_some() {
                self.unsupported(p.span, "default values in arrow functions");
                bad = true;
            }
            let given = params.as_ref().and_then(|ps| ps.get(i).copied()).filter(|t| *t != Ty::Poison);
            let t = match &p.ty {
                Some(t) => {
                    let t = self.m.resolve_type(t);
                    if let Some(g) = given
                        && g != t
                        && t != Ty::Poison
                    {
                        let d = self.err(
                            codes::MISMATCH,
                            p.span,
                            format!("this parameter is {}, but the callback gets {}", self.m.an(t), self.m.an(g)),
                        );
                        self.push(d);
                        bad = true;
                    }
                    t
                }
                None => match given {
                    Some(g) => g,
                    None => {
                        if params.is_none() {
                            self.cant_infer_param(a, i);
                        }
                        bad = true;
                        Ty::Poison
                    }
                },
            };
            if matches!(t, Ty::Void | Ty::Never) {
                let d = self.err(codes::VOID_VALUE, p.span, format!("a parameter can't have type {}", self.m.tn(t)));
                self.push(d);
                bad = true;
            }
            ptys.push(t);
        }
        let declared = a.ret.as_ref().map(|t| self.m.resolve_type(t));
        if let (Some(d), Some(r)) = (declared, ret)
            && d != r
            && d != Ty::Poison
            && r != Ty::Poison
        {
            let e = self.err(codes::MISMATCH, a.ret.as_ref().unwrap().span, format!("expected {}, found {}", self.m.an(r), self.m.an(d)));
            self.push(e);
            bad = true;
        }
        let want_ret = declared.or(ret).filter(|t| *t != Ty::Poison);
        // The id is taken now, so arrow functions inside this one come after it.
        let index = self.m.arrows.len();
        let func = ir::FuncId((self.m.sigs.len() + index) as u32);
        self.m.arrows.push(placeholder());
        let frame = Frame::new(func, "<arrow>".to_string(), want_ret, self.scopes.len());
        let enclosing = std::mem::replace(&mut self.f, frame);
        self.outer.push(enclosing);
        self.push_scope((span, "function"));
        for (p, t) in a.params.iter().zip(&ptys) {
            self.declare_param(&p.name.name, *t, p.name.span, false);
        }
        let (body, ret) = match &a.body {
            ast::ArrowBody::Expr(e) => {
                let v = self.expr_want(*e, want_ret);
                match want_ret {
                    Some(Ty::Void) => (vec![ir::Stmt::Expr(v.e)], Ty::Void),
                    Some(r) => {
                        let v = coerce(v, r);
                        self.expect_type(&v, r, self.espan(*e));
                        (vec![ir::Stmt::Return(Some(v.e))], r)
                    }
                    None if matches!(v.ty, Ty::Void | Ty::Never) => (vec![ir::Stmt::Expr(v.e)], Ty::Void),
                    None => (vec![ir::Stmt::Return(Some(v.e))], v.ty),
                }
            }
            ast::ArrowBody::Block(b) => {
                let last = std::mem::replace(&mut self.last_stmt, b.stmts.last().copied());
                let body = self.block(b, (b.span, "function"));
                self.last_stmt = last;
                let ret = match self.f.ret {
                    Some(t) => t,
                    None => self.infer_return(),
                };
                if !matches!(ret, Ty::Void | Ty::Never | Ty::Poison) && !terminates(&body) && !self.f.syntax_error {
                    self.missing_return(b, ret);
                }
                (body, ret)
            }
        };
        self.report_copies();
        self.pop_scope();
        let frame = std::mem::replace(&mut self.f, self.outer.pop().expect("an enclosing function"));
        bad |= ret == Ty::Poison || matches!(ret, Ty::Never);
        let (sources, locals): (Vec<ir::LocalId>, Vec<ir::LocalId>) = frame.captures.iter().copied().unzip();
        self.m.arrows[index] = ir::Function {
            name: "<arrow>".to_string(),
            params: frame.params,
            defaults: Vec::new(),
            ret: ret.ir(),
            locals: frame.locals,
            captures: locals,
            body,
            recursive: false,
            loc: self.loc(a.head),
        };
        if bad {
            return poison();
        }
        let ty = self.m.types.func(ptys.iter().map(|t| t.ir()).collect(), ret.ir());
        mk(ir::ExprKind::Closure { func, captures: sources }, Ty::of(ty))
    }

    /// E0602: an arrow function's parameter has no type and nothing gives
    /// it one. A parameter used in arithmetic gets `int` and `float` as options.
    fn cant_infer_param(&mut self, a: &ast::Arrow, i: usize) {
        let p = &a.params[i];
        let name = &p.name.name;
        let mut d = self
            .err(codes::PARAM_TYPE_UNKNOWN, p.name.span, format!("can't infer the type of {name}"))
            .with_label("give the parameter a type, or pass the function where a function type is expected");
        let ast = self.ast();
        let body = match &a.body {
            ast::ArrowBody::Block(b) => b.span,
            ast::ArrowBody::Expr(e) => ast.expr(*e).span,
        };
        let in_arithmetic = ast.exprs.iter().any(|e| match e.kind {
            ExprKind::Binary { op: BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem, lhs, rhs } => {
                body.start <= e.span.start
                    && e.span.end <= body.end
                    && [lhs, rhs].iter().any(|&x| matches!(&ast.expr(x).kind, ExprKind::Name(n) if n == name))
            }
            _ => false,
        });
        if in_arithmetic {
            for t in ["int", "float"] {
                d = d.with_fix(
                    format!("declare `{name}: {t}`"),
                    Applicability::Option,
                    vec![self.edit(p.name.span, format!("{name}: {t}"))],
                );
            }
        }
        self.push(d);
    }

    /// A use, from inside an arrow function, of a variable declared outside
    /// it: the closure gets its own copy, made when it's created, through
    /// every arrow function in between (§06). A variable that can change
    /// can't be used (E0603).
    pub(super) fn capture_use(&mut self, name: &str, span: Span) -> Option<ir::LocalId> {
        let var = self.lookup(name).expect("a declared variable");
        let (kind, mut source, level, ty) = (var.kind, var.local, var.level, var.ty);
        if kind.changeable() {
            let what = match kind {
                VarKind::MutParam => "the `mut` parameter",
                VarKind::MutLoop => "the `mut` loop variable",
                _ => "the `let` variable",
            };
            let mut d = self
                .err(codes::CAPTURES_LET, span, format!("closures can't use {what} `{name}`"))
                .with_label("a closure keeps a copy of what it uses, so it may only use what can't change: use a loop, or `reduce`");
            if let Some(fix) = self.foreach_fix.as_ref().filter(|f| f.level + 1 == self.level()) {
                d = d.with_fix("use a `for...of` loop", Applicability::Safe, vec![self.edit(fix.span, fix.after.clone())]);
            }
            self.push(d);
            return None;
        }
        for lvl in level + 1..=self.level() {
            let frame = if lvl == self.level() { &mut self.f } else { &mut self.outer[lvl] };
            if let Some(&(_, mine)) = frame.captures.iter().find(|(s, _)| *s == source) {
                source = mine;
                continue;
            }
            frame.locals.push(ir::Local { name: name.to_string(), ty: ty.ir(), reassigned: false, by_ref: false });
            let mine = ir::LocalId(frame.locals.len() as u32 - 1);
            frame.captures.push((source, mine));
            source = mine;
        }
        Some(source)
    }

    /// The edit that turns `xs.forEach((x) => { ... });` into a loop, when
    /// that keeps its meaning: a block body without `return`, one
    /// parameter, and the call is the whole statement.
    pub(super) fn foreach_loop_fix(&self, call: Span, recv: ExprId, arrow: ExprId) -> Option<ForEachFix> {
        let ast = self.ast();
        let ExprKind::Arrow(a) = &ast.expr(arrow).kind else { return None };
        let ast::ArrowBody::Block(b) = &a.body else { return None };
        if a.params.len() != 1 || a.params[0].ty.is_some() || self.stmt.start != call.start || !self.stmt_simple {
            return None;
        }
        let has_return =
            ast.stmts.iter().any(|s| matches!(s.kind, StmtKind::Return(_)) && b.span.start <= s.span.start && s.span.end <= b.span.end);
        if has_return {
            return None;
        }
        let after = format!("for (const {} of {}) {}", a.params[0].name.name, self.text(self.espan(recv)), self.text(b.span));
        Some(ForEachFix { level: self.level(), span: self.stmt, after })
    }

    /// A named function used as a value.
    pub(super) fn function_value(&mut self, f: ir::FuncId, span: Span) -> TE {
        if matches!(self.m.sigs[f.0 as usize].ret, Ret::Infer) {
            self.m.check_function(f);
        }
        let sig = &self.m.sigs[f.0 as usize];
        let name = sig.name.clone();
        let ret = match sig.ret {
            Ret::Known(t) => t,
            _ => {
                let d = self
                    .err(codes::UNSUPPORTED, span, format!("`{name}` is used as a value inside itself, so it needs a declared return type"))
                    .with_label("inferring the return type of a recursive function isn't supported yet");
                self.push(d);
                return poison();
            }
        };
        if sig.params.iter().any(|p| p.2) {
            self.unsupported(span, "functions with `mut` parameters as values");
            return poison();
        }
        if sig.params.len() > MAX_ARITY {
            self.unsupported(span, &format!("functions with more than {MAX_ARITY} parameters as values"));
            return poison();
        }
        let params: Vec<Ty> = sig.params.iter().map(|p| p.1).collect();
        if params.contains(&Ty::Poison) || ret == Ty::Poison {
            return poison();
        }
        if ret == Ty::Never {
            self.unsupported(span, "function values that return never");
            return poison();
        }
        let ty = self.m.types.func(params.iter().map(|t| t.ir()).collect(), ret.ir());
        mk(ir::ExprKind::FuncRef(f), Ty::of(ty))
    }

    /// `callee(args)` where `callee` is a function value.
    pub(super) fn call_value(&mut self, c: TE, callee: ExprId, args: &[ast::Arg], span: Span) -> TE {
        let Some((params, ret)) = self.m.types.func_sig(c.ty.ir()).map(|(p, r)| (p.to_vec(), r)) else {
            self.check_args(Some((span, args)));
            let d = self.err(
                codes::BAD_OPERANDS,
                span,
                format!("`{}` isn't a function: it's {}", self.text(self.espan(callee)), self.m.an(c.ty)),
            );
            self.push(d);
            return poison();
        };
        if args.len() != params.len() {
            self.check_args(Some((span, args)));
            let name = self.text(self.espan(callee)).to_string();
            self.arity(&name, span, params.len(), params.len(), args.len());
            return poison();
        }
        let mut lowered = Vec::new();
        let mut bad = false;
        for (a, p) in args.iter().zip(&params) {
            if a.is_mut {
                let d = self.err(codes::NOT_MUT, self.espan(a.expr), "a function value's parameters aren't `mut`");
                self.push(d);
                bad = true;
            }
            let want = Ty::of(*p);
            let v = coerce(self.expr_want(a.expr, Some(want)), want);
            let before = self.m.diags.len();
            self.expect_type(&v, want, self.espan(a.expr));
            bad |= v.ty == Ty::Poison || self.m.diags.len() > before;
            lowered.push(v.e);
        }
        if bad {
            return poison();
        }
        let loc = self.loc(span);
        mk(ir::ExprKind::CallValue { callee: Box::new(c.e), args: lowered, loc }, Ty::of(ret))
    }
}

/// What an arrow function's slot holds while its body is checked.
fn placeholder() -> ir::Function {
    ir::Function {
        name: String::new(),
        params: 0,
        captures: Vec::new(),
        defaults: Vec::new(),
        ret: ir::Type::Void,
        locals: Vec::new(),
        body: Vec::new(),
        recursive: false,
        loc: ir::Loc { line: 0, col: 0 },
    }
}
