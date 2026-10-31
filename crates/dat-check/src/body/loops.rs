//! `for...of` loops (§07) and destructuring (§02).

use super::{Body, TE, VarKind, coerce, mk};
use crate::Ty;
use crate::codes;
use dat_diag::Span;
use dat_ir as ir;
use dat_syntax::ast::{self, ExprId, ExprKind};

impl Body<'_, '_> {
    /// `for (const x of xs) { ... }`, and its `let` and `mut` forms.
    pub(super) fn for_of(
        &mut self,
        binding: ast::Binding,
        pattern: &ast::Pattern,
        iter: ExprId,
        body: &ast::Block,
        span: Span,
        out: &mut ir::Block,
    ) {
        let header = Span::new(span.start, body.span.start);
        if binding == ast::Binding::Mut {
            self.for_mut(pattern, iter, body, span, header, out);
            return;
        }
        let kind = if binding == ast::Binding::Const { VarKind::Const } else { VarKind::Let };
        self.push_scope((span, "for"));
        let (iterate, elem) = match self.range_call(iter) {
            Some(Some(r)) => (r, Ty::Int),
            Some(None) => {
                self.loop_body(pattern, Ty::Poison, kind, body, span, iter);
                self.pop_scope();
                return;
            }
            None => {
                let v = self.expr(iter);
                let elem = self.element_type(v.ty, self.espan(iter));
                (ir::Iter::Each(v.e), elem)
            }
        };
        let (var, body) = self.loop_body(pattern, elem, kind, body, span, iter);
        self.pop_scope();
        if elem != Ty::Poison {
            out.push(ir::Stmt::ForOf { var, iter: iterate, body });
        }
    }

    /// The loop variable and the body, after any destructuring of each element.
    fn loop_body(
        &mut self,
        pattern: &ast::Pattern,
        elem: Ty,
        kind: VarKind,
        body: &ast::Block,
        span: Span,
        iter: ExprId,
    ) -> (ir::LocalId, ir::Block) {
        let mut prologue = Vec::new();
        let var = match pattern {
            ast::Pattern::Name(n) => {
                let var = self.declare(n, elem, kind, n.span, None, None, None);
                if kind == VarKind::Let && matches!(elem, Ty::Of(_)) {
                    // `for (let x of xs)`: `mut` changes the elements, when `xs` is a list that can change.
                    let header = self.text(Span::new(span.start, n.span.start));
                    let kw = header.rfind("let").map(|i| Span::new(span.start + i as u32, span.start + i as u32 + 3));
                    let list = self.ast().expr(iter);
                    let changeable = matches!(&list.kind, ExprKind::Name(x) if self.var_kind(x).is_some_and(VarKind::changeable))
                        && self.m.types.list_elem(self.lookup_ty(iter)).is_some();
                    let source = self.text(list.span).to_string();
                    self.note_copy(var, &n.name, source, Some(kw.filter(|_| changeable)));
                }
                var
            }
            _ => {
                let t = self.temp(elem);
                let read = mk(ir::ExprKind::Local(t), elem);
                self.destructure(pattern, read, kind, span, &mut prologue);
                t
            }
        };
        self.f.loops += 1;
        let mut b = self.block(body, (span, "for"));
        self.f.loops -= 1;
        prologue.append(&mut b);
        (var, prologue)
    }

    /// `range(...)` as a loop's collection, which builds no list: the
    /// iteration, or `Some(None)` after an error. `None` if it isn't one.
    fn range_call(&mut self, iter: ExprId) -> Option<Option<ir::Iter>> {
        let ast = self.ast();
        let ExprKind::Call { callee, args } = &ast.expr(iter).kind else { return None };
        let ExprKind::Name(n) = &ast.expr(*callee).kind else { return None };
        if n != "range" || self.lookup(n).is_some() || self.m.by_name.contains_key(n.as_str()) || self.m.imports.contains_key(n.as_str()) {
            return None;
        }
        let span = self.espan(iter);
        let Some((start, end, step)) = self.range_args(args, span) else { return Some(None) };
        Some(Some(ir::Iter::Range { start, end, step, loc: self.loc(span) }))
    }

    /// The arguments of `range`: start, end and step, with the defaults
    /// filled in. `None` after an error.
    pub(super) fn range_args(&mut self, args: &[ast::Arg], span: Span) -> Option<(ir::Expr, ir::Expr, ir::Expr)> {
        if args.is_empty() || args.len() > 3 {
            self.check_args(Some((span, args)));
            self.arity("range", span, 1, 3, args.len());
            return None;
        }
        let mut vals = Vec::new();
        let mut bad = false;
        for a in args {
            let v = self.expr_want(a.expr, Some(Ty::Int));
            let before = self.m.diags.len();
            self.expect_type(&v, Ty::Int, self.espan(a.expr));
            bad |= v.ty == Ty::Poison || self.m.diags.len() > before;
            vals.push(v.e);
        }
        if bad {
            return None;
        }
        let int = |n: i64| mk(ir::ExprKind::Int(n), Ty::Int).e;
        let mut vals = vals.into_iter();
        Some(match args.len() {
            1 => (int(0), vals.next().unwrap(), int(1)),
            2 => (vals.next().unwrap(), vals.next().unwrap(), int(1)),
            _ => (vals.next().unwrap(), vals.next().unwrap(), vals.next().unwrap()),
        })
    }

    /// The type of the variable `e` names, as an IR type.
    fn lookup_ty(&self, e: ExprId) -> ir::Type {
        match &self.ast().expr(e).kind {
            ExprKind::Name(n) => self.lookup(n).map_or(ir::Type::Void, |v| v.ty.ir()),
            _ => ir::Type::Void,
        }
    }

    /// What `for...of` gets from each step through a value of type `t`.
    fn element_type(&mut self, t: Ty, span: Span) -> Ty {
        if t == Ty::Str {
            return Ty::Str;
        }
        let types = &mut self.m.types;
        match types.kind(t.ir()).cloned() {
            Some(ir::Kind::List(e) | ir::Kind::Set(e)) => Ty::of(e),
            Some(ir::Kind::Map(k, v)) => Ty::of(types.tuple(vec![k, v])),
            _ if t == Ty::Poison => Ty::Poison,
            _ => {
                let d = self
                    .err(codes::BAD_OPERANDS, span, format!("`for...of` can't go through {}", self.m.an(t)))
                    .with_label("it goes through a list, a set, a map, a string or `range(...)`");
                self.push(d);
                Ty::Poison
            }
        }
    }

    /// `for (mut x of xs)`: `x` is each element of the list itself, and the
    /// body can't use `xs` (E0703).
    fn for_mut(&mut self, pattern: &ast::Pattern, iter: ExprId, body: &ast::Block, span: Span, header: Span, out: &mut ir::Block) {
        self.push_scope((span, "for"));
        let place = self.changeable_place(iter, "modified");
        let elem = match &place {
            Some(p) => match self.m.types.list_elem(p.ty.ir()) {
                Some(e) => Ty::of(e),
                None => {
                    if p.ty != Ty::Poison {
                        let d = self
                            .err(
                                codes::BAD_OPERANDS,
                                self.espan(iter),
                                format!("`for (mut ...)` goes through a list, not {}", self.m.an(p.ty)),
                            )
                            .with_label("its variable is each element of the list in turn");
                        self.push(d);
                    }
                    Ty::Poison
                }
            },
            None => Ty::Poison,
        };
        let ast::Pattern::Name(n) = pattern else {
            let d = self
                .err(codes::BAD_OPERANDS, pattern.span(), "a `mut` loop variable can't be destructured")
                .with_label("use `const` to destructure each element");
            self.push(d);
            self.pop_scope();
            return;
        };
        if let Some(p) = &place {
            self.copy_used(p.place.root);
        }
        let var = self.declare(n, elem, VarKind::MutLoop, n.span, None, None, None);
        let fix_span = Span::new(header.start, header.end);
        if let Some(p) = &place {
            self.loop_lists.push((p.place.root, self.level(), fix_span));
        }
        self.f.loops += 1;
        let before = self.m.diags.len();
        let changes_before = self.f.changed.len();
        let b = self.block(body, (span, "for"));
        self.f.loops -= 1;
        if place.is_some() {
            self.loop_lists.pop();
        }
        // E0703's fix makes the variable `const`; a body that changes it
        // needs `mut`, so then there's no fix.
        if self.f.changed[changes_before..].contains(&var) {
            for d in &mut self.m.diags[before..] {
                if d.code == codes::IN_USE_BY_LOOP {
                    d.fixes.clear();
                }
            }
        }
        self.pop_scope();
        if let Some(p) = place
            && elem != Ty::Poison
        {
            let loc = self.loc(self.espan(iter));
            out.push(ir::Stmt::ForMut { var, list: p.place, body: b, loc });
        }
    }

    /// E0703: `name`, the list a running `for (mut ...)` loop goes through,
    /// used in the loop's body.
    pub(super) fn check_in_use(&mut self, local: ir::LocalId, level: usize, name: &str, span: Span) -> bool {
        let Some(&(_, _, header)) = self.loop_lists.iter().find(|(l, lv, _)| *l == local && *lv == level) else { return true };
        let text = self.text(header);
        let fixed = text.replacen("mut ", "const ", 1);
        let d = self
            .err(codes::IN_USE_BY_LOOP, span, format!("`{name}` is in use by the loop"))
            .with_label("inside a `for (mut ...)` loop, the list can only be reached through the loop variable")
            .with_fix("loop over a copy with `const`", dat_diag::Applicability::Safe, vec![self.edit(header, fixed)]);
        self.push(d);
        false
    }

    /// `const [a, b] = value;` and the other destructuring declarations.
    pub(super) fn destructure_stmt(
        &mut self,
        is_const: bool,
        pattern: &ast::Pattern,
        ty: Option<&ast::TypeExpr>,
        init: ExprId,
        span: Span,
        out: &mut ir::Block,
    ) {
        let want = ty.map(|t| self.m.resolve_type(t));
        let mut v = self.expr_want(init, want);
        if let Some(w) = want {
            v = coerce(v, w);
            self.expect_type(&v, w, self.espan(init));
            if v.ty != Ty::Poison {
                v.ty = w;
            }
        }
        let kind = if is_const { VarKind::Const } else { VarKind::Let };
        self.destructure(pattern, v, kind, span, out);
    }

    /// Binds the names in `pattern` to the parts of `value`, appending the
    /// lowered declarations to `out`. A list too short for the pattern
    /// throws a `RangeError` (§02).
    pub(super) fn destructure(&mut self, pattern: &ast::Pattern, value: TE, kind: VarKind, span: Span, out: &mut ir::Block) {
        match pattern {
            ast::Pattern::Name(n) => {
                let local = self.declare(n, value.ty, kind, n.span, None, None, None);
                if value.ty != Ty::Poison {
                    out.push(ir::Stmt::Let(local, value.e));
                }
            }
            ast::Pattern::Ignore(_) => {
                if !matches!(value.e.kind, ir::ExprKind::Local(_) | ir::ExprKind::Index { .. } | ir::ExprKind::TupleGet(..)) {
                    out.push(ir::Stmt::Expr(value.e));
                }
            }
            ast::Pattern::Unsupported { what, span } => {
                self.unsupported(*span, what);
                self.declare_names(pattern, kind);
            }
            ast::Pattern::List { items, rest, span: pspan } => {
                let kind_of = self.m.types.kind(value.ty.ir()).cloned();
                match kind_of {
                    Some(ir::Kind::List(e)) => self.destructure_list(items, rest.as_ref(), value, Ty::of(e), kind, span, out),
                    Some(ir::Kind::Tuple(types)) => self.destructure_tuple(items, rest.as_ref(), value, &types, kind, *pspan, out),
                    _ => {
                        if value.ty != Ty::Poison {
                            let d = self
                                .err(codes::BAD_OPERANDS, *pspan, format!("{} can't be destructured with `[...]`", self.m.an(value.ty)))
                                .with_label("`[...]` takes a list or a tuple apart");
                            self.push(d);
                        }
                        self.declare_names(pattern, kind);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn destructure_list(
        &mut self,
        items: &[ast::PatItem],
        rest: Option<&ast::Ident>,
        value: TE,
        elem: Ty,
        kind: VarKind,
        span: Span,
        out: &mut ir::Block,
    ) {
        let list_ty = value.ty;
        let tmp = self.temp(list_ty);
        out.push(ir::Stmt::Let(tmp, value.e));
        let loc = self.loc(span);
        let list = || mk(ir::ExprKind::Local(tmp), list_ty).e;
        let int = |n: i64| mk(ir::ExprKind::Int(n), Ty::Int).e;
        let required = items.iter().rposition(|i| i.default.is_none()).map_or(0, |p| p + 1);
        if required > 0 {
            let check = ir::ExprKind::Method { op: ir::Method::ListCheckLen, args: vec![list(), int(required as i64)], loc };
            out.push(ir::Stmt::Expr(mk(check, Ty::Void).e));
        }
        for (i, item) in items.iter().enumerate() {
            let at = mk(ir::ExprKind::Index { base: Box::new(list()), index: Box::new(int(i as i64)), loc }, elem);
            let v = match item.default {
                Some(d) if i >= required => {
                    let dv = coerce(self.expr_want(d, Some(elem)), elem);
                    self.expect_type(&dv, elem, self.espan(d));
                    let len = mk(ir::ExprKind::Method { op: ir::Method::ListLength, args: vec![list()], loc }, Ty::Int);
                    let has = ir::ExprKind::Compare { op: ir::CmpOp::Gt, lhs: Box::new(len.e), rhs: Box::new(int(i as i64)) };
                    let cond = mk(has, Ty::Bool);
                    mk(ir::ExprKind::Cond { cond: Box::new(cond.e), then: Box::new(at.e), els: Box::new(dv.e) }, elem)
                }
                _ => at,
            };
            self.destructure(&item.pattern, v, kind, span, out);
        }
        if let Some(r) = rest {
            let slice = ir::ExprKind::Method { op: ir::Method::ListSlice, args: vec![list(), int(items.len() as i64), int(i64::MAX)], loc };
            let v = mk(slice, list_ty);
            let local = self.declare(r, list_ty, kind, r.span, None, None, None);
            out.push(ir::Stmt::Let(local, v.e));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn destructure_tuple(
        &mut self,
        items: &[ast::PatItem],
        rest: Option<&ast::Ident>,
        value: TE,
        types: &[ir::Type],
        kind: VarKind,
        span: Span,
        out: &mut ir::Block,
    ) {
        if let Some(r) = rest {
            let d =
                self.err(codes::BAD_OPERANDS, r.span, "a tuple can't be destructured with `...rest`").with_label("name each of its values");
            self.push(d);
        }
        if items.len() > types.len() {
            let d =
                self.err(codes::TUPLE_INDEX, span, format!("this pattern takes {} values, and the tuple has {}", items.len(), types.len()));
            self.push(d);
        }
        if let Some(i) = items.iter().find(|i| i.default.is_some()) {
            let d =
                self.err(codes::BAD_OPERANDS, self.espan(i.default.unwrap()), "a tuple always has all its values, so they need no default");
            self.push(d);
        }
        let tuple_ty = value.ty;
        let tmp = self.temp(tuple_ty);
        out.push(ir::Stmt::Let(tmp, value.e));
        for (i, item) in items.iter().enumerate() {
            let Some(&t) = types.get(i) else {
                self.declare_names(&item.pattern, kind);
                continue;
            };
            let v = mk(ir::ExprKind::TupleGet(Box::new(mk(ir::ExprKind::Local(tmp), tuple_ty).e), i as u32), Ty::of(t));
            self.destructure(&item.pattern, v, kind, span, out);
        }
    }

    /// Declares the names of a pattern that couldn't be checked, so their
    /// uses aren't reported as undefined.
    fn declare_names(&mut self, pattern: &ast::Pattern, kind: VarKind) {
        match pattern {
            ast::Pattern::Name(n) => {
                self.declare(n, Ty::Poison, kind, n.span, None, None, None);
            }
            ast::Pattern::List { items, rest, .. } => {
                for i in items {
                    self.declare_names(&i.pattern, kind);
                }
                if let Some(r) = rest {
                    self.declare(r, Ty::Poison, kind, r.span, None, None, None);
                }
            }
            ast::Pattern::Ignore(_) | ast::Pattern::Unsupported { .. } => {}
        }
    }
}
