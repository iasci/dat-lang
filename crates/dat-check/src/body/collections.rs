//! Lists, sets, maps and tuples: literals, indexing, places, and calls of
//! their methods.

use super::methods::{self, Recv};
use super::{Body, NO_LOC, TE, VarKind, coerce, mk, poison};
use crate::Ty;
use crate::codes;
use dat_diag::{Applicability, Span};
use dat_ir as ir;
use dat_syntax::ast::{self, ExprId, ExprKind};

/// A place as the checker builds it: the IR place, its type, and the
/// variable at its root.
pub(super) struct PlaceOf {
    pub place: ir::Place,
    pub ty: Ty,
    pub root_name: String,
    pub root_span: Span,
}

impl Body<'_, '_> {
    /// Checks `id` where a value of type `want` is expected. A literal takes
    /// its type from `want`: `[]` and `{}` have no type of their own, a list
    /// literal is a set or a tuple only where one is expected, and an arrow
    /// function's parameters get their types from the function type. The
    /// caller still checks that the result has the type it wants.
    pub(super) fn expr_want(&mut self, id: ExprId, want: Option<Ty>) -> TE {
        let ast = self.ast();
        // A type that already has an error says nothing about `[]` or `{}`.
        if want == Some(Ty::Poison) && matches!(&ast.expr(id).kind, ExprKind::List(x) if x.is_empty()) {
            return poison();
        }
        if want == Some(Ty::Poison) && matches!(&ast.expr(id).kind, ExprKind::Map(x) if x.is_empty()) {
            return poison();
        }
        let want = want.filter(|w| !matches!(w, Ty::Poison | Ty::Void | Ty::Never));
        match &ast.expr(id).kind {
            ExprKind::Paren(inner) => self.expr_want(*inner, want),
            ExprKind::List(items) => self.list_literal(id, items, want),
            ExprKind::Map(entries) => self.map_literal(id, entries, want),
            ExprKind::Arrow(a) => {
                let (params, ret) = match want.and_then(|w| self.m.types.func_sig(w.ir())) {
                    Some((ps, r)) if ps.len() == a.params.len() => (Some(ps.iter().map(|p| Ty::of(*p)).collect()), Some(Ty::of(r))),
                    Some((ps, _)) => {
                        let (n, takes) = (a.params.len(), ps.len());
                        let plural = if takes == 1 { "" } else { "s" };
                        let d = self
                            .err(
                                codes::MISMATCH,
                                a.head,
                                format!("expected a function `{}`, found one with {n} parameters", self.m.tn(want.unwrap())),
                            )
                            .with_label(format!("the function passed here takes {takes} parameter{plural}"));
                        self.push(d);
                        return poison();
                    }
                    None => (None, None),
                };
                self.arrow_fn(id, a, params, ret)
            }
            ExprKind::Cond { cond, then, els } if want.is_some() => {
                let c = self.condition(*cond);
                let t = self.expr_want(*then, want);
                let e = self.expr_want(*els, want);
                if [c.ty, t.ty, e.ty].contains(&Ty::Poison) {
                    return poison();
                }
                let w = want.unwrap();
                let (t, e) = (coerce(t, w), coerce(e, w));
                let ty = if t.ty == Ty::Never { e.ty } else { t.ty };
                if (t.ty != ty && t.ty != Ty::Never) || (e.ty != ty && e.ty != Ty::Never) {
                    self.unsupported(self.espan(id), "conditional expressions whose branches have different types");
                    return poison();
                }
                mk(ir::ExprKind::Cond { cond: Box::new(c.e), then: Box::new(t.e), els: Box::new(e.e) }, ty)
            }
            _ => {
                if matches!(ast.expr(id).kind, ExprKind::Call { .. }) {
                    self.call_want = want;
                }
                let v = self.expr(id);
                self.call_want = None;
                match want {
                    Some(w) => coerce(v, w),
                    None => v,
                }
            }
        }
    }

    /// `[...]`: a list, or a set or tuple where one is expected.
    fn list_literal(&mut self, id: ExprId, items: &[ExprId], want: Option<Ty>) -> TE {
        let span = self.espan(id);
        let kind = want.and_then(|w| self.m.types.kind(w.ir()).cloned());
        match kind {
            Some(ir::Kind::Tuple(types)) => {
                if let Some(&s) = items.iter().find(|&&i| matches!(self.ast().expr(i).kind, ExprKind::Spread(_))) {
                    let d = self.err(codes::MISMATCH, self.espan(s), "a tuple literal can't spread values").with_label(format!(
                        "a tuple `{}` has exactly {} values",
                        self.m.tn(want.unwrap()),
                        types.len()
                    ));
                    self.push(d);
                    return poison();
                }
                if items.len() != types.len() {
                    for &i in items {
                        self.expr(i);
                    }
                    let d = self.err(
                        codes::MISMATCH,
                        span,
                        format!("expected a tuple `{}` of {} values, found {}", self.m.tn(want.unwrap()), types.len(), items.len()),
                    );
                    self.push(d);
                    return poison();
                }
                let mut out = Vec::new();
                let mut bad = false;
                for (&i, t) in items.iter().zip(&types) {
                    let v = self.expr_want(i, Some(Ty::of(*t)));
                    let before = self.m.diags.len();
                    self.expect_type(&v, Ty::of(*t), self.espan(i));
                    bad |= v.ty == Ty::Poison || self.m.diags.len() > before;
                    out.push(v.e);
                }
                if bad {
                    return poison();
                }
                mk(ir::ExprKind::TupleLit(out), want.unwrap())
            }
            Some(ir::Kind::List(elem)) => match self.elements(items, Some(Ty::of(elem))) {
                Some((elems, _)) => mk(ir::ExprKind::ListLit(elems), want.unwrap()),
                None => poison(),
            },
            Some(ir::Kind::Set(elem)) => match self.elements(items, Some(Ty::of(elem))) {
                Some((elems, _)) => mk(ir::ExprKind::SetLit(elems), want.unwrap()),
                None => poison(),
            },
            _ => {
                if items.is_empty() {
                    let d = self
                        .err(codes::CANT_INFER, span, "can't infer the element type")
                        .with_label("an empty list has no elements to tell what type they have");
                    self.push(d);
                    return poison();
                }
                match self.elements(items, None) {
                    Some((elems, elem)) => {
                        let t = self.m.types.list(elem.ir());
                        mk(ir::ExprKind::ListLit(elems), Ty::of(t))
                    }
                    None => poison(),
                }
            }
        }
    }

    /// The elements of a list or set literal, checked against `want`, or
    /// with their type inferred from them: one type, or ints and floats,
    /// which make floats. `None` after an error.
    fn elements(&mut self, items: &[ExprId], want: Option<Ty>) -> Option<(Vec<ir::Elem>, Ty)> {
        let ast = self.ast();
        let mut bad = false;
        // `[[1], []]` and `[[], [1]]`: an empty literal takes the type of the
        // other elements, so it's checked after them. That's safe: it has
        // no effect, and the elements still run in order.
        let empty = |i: ExprId| match &ast.expr(i).kind {
            ExprKind::List(x) => x.is_empty(),
            ExprKind::Map(x) => x.is_empty(),
            _ => false,
        };
        let mut slots: Vec<Option<(bool, TE, ExprId)>> = Vec::new();
        for &i in items {
            match ast.expr(i).kind {
                ExprKind::Spread(inner) => {
                    let v = self.expr(inner);
                    bad |= v.ty == Ty::Poison;
                    slots.push(Some((true, v, inner)));
                }
                _ if want.is_none() && empty(i) => slots.push(None),
                _ => {
                    let v = self.expr_want(i, want);
                    bad |= v.ty == Ty::Poison;
                    slots.push(Some((false, v, i)));
                }
            }
        }
        let other =
            want.or_else(|| slots.iter().flatten().find(|(spread, v, _)| !spread && matches!(v.ty, Ty::Of(_))).map(|(_, v, _)| v.ty));
        let mut checked: Vec<(bool, TE, ExprId)> = Vec::new();
        for (slot, &i) in slots.into_iter().zip(items) {
            match slot {
                Some(c) => checked.push(c),
                None => {
                    let v = self.expr_want(i, other);
                    bad |= v.ty == Ty::Poison;
                    checked.push((false, v, i));
                }
            }
        }
        if bad {
            return None;
        }
        // The type of one element, or of the elements a spread gives.
        let elem_of = |b: &Self, spread: bool, v: &TE| -> Option<Ty> {
            if !spread {
                return Some(v.ty);
            }
            if v.ty == Ty::Str {
                return Some(Ty::Str);
            }
            let types = &b.m.types;
            types.list_elem(v.ty.ir()).or_else(|| types.set_elem(v.ty.ir())).map(Ty::of)
        };
        for (spread, v, at) in &checked {
            if *spread && elem_of(self, true, v).is_none() && v.ty != Ty::Never {
                let d = self
                    .err(codes::BAD_OPERANDS, self.espan(*at), format!("can't spread {}", self.m.an(v.ty)))
                    .with_label("`...` takes a list, a set or a string");
                self.push(d);
                return None;
            }
        }
        let elem = match want {
            Some(w) => w,
            None => {
                let tys: Vec<Ty> = checked.iter().filter_map(|(s, v, _)| elem_of(self, *s, v)).filter(|t| *t != Ty::Never).collect();
                let first = tys.first().copied().unwrap_or(Ty::Never);
                if tys.iter().all(|t| *t == first) {
                    first
                } else if tys.iter().all(|t| t.is_number()) {
                    Ty::Float
                } else {
                    let span = self.espan(items[0]).to(self.espan(*items.last().unwrap()));
                    self.unsupported(span, "lists whose elements have different types");
                    return None;
                }
            }
        };
        if elem == Ty::Never || matches!(elem, Ty::Void) {
            let span = self.espan(items[0]);
            let d = self.err(codes::VOID_VALUE, span, format!("a list can't hold {}", self.m.tn(elem)));
            self.push(d);
            return None;
        }
        let mut out = Vec::new();
        for (spread, v, at) in checked {
            if spread {
                let got = elem_of(self, true, &v);
                if got.is_some_and(|g| g != elem) {
                    let d = self.err(
                        codes::MISMATCH,
                        self.espan(at),
                        format!("expected elements of type {}, found {}", self.m.tn(elem), self.m.an(v.ty)),
                    );
                    self.push(d);
                    bad = true;
                }
                out.push(ir::Elem::Spread(v.e));
            } else {
                let v = coerce(v, elem);
                let before = self.m.diags.len();
                self.expect_type(&v, elem, self.espan(at));
                bad |= self.m.diags.len() > before;
                out.push(ir::Elem::One(v.e));
            }
        }
        if bad { None } else { Some((out, elem)) }
    }

    /// `{ ... }`: a map.
    fn map_literal(&mut self, id: ExprId, entries: &[ast::MapEntry], want: Option<Ty>) -> TE {
        let span = self.espan(id);
        let (want_k, want_v) = match want.and_then(|w| self.m.types.map_types(w.ir())) {
            Some((k, v)) => (Some(Ty::of(k)), Some(Ty::of(v))),
            None => (None, None),
        };
        if entries.is_empty() && want_k.is_none() {
            let d =
                self.err(codes::CANT_INFER, span, "can't infer the key and value types").with_label("an empty map has no entries to tell");
            self.push(d);
            return poison();
        }
        let (mut kt, mut vt) = (want_k, want_v);
        let mut out = Vec::new();
        let mut bad = false;
        for en in entries {
            match en {
                ast::MapEntry::Spread(x) => {
                    let v = self.expr_want(*x, want);
                    if v.ty == Ty::Poison {
                        bad = true;
                        continue;
                    }
                    match self.m.types.map_types(v.ty.ir()) {
                        Some((k, val)) if kt.is_none_or(|t| t == Ty::of(k)) && vt.is_none_or(|t| t == Ty::of(val)) => {
                            kt = Some(Ty::of(k));
                            vt = Some(Ty::of(val));
                            out.push(ir::Entry::Spread(v.e));
                        }
                        _ => {
                            let what = match (kt, vt) {
                                (Some(k), Some(v)) => format!("a map<{}, {}>", self.m.tn(k), self.m.tn(v)),
                                _ => "a map".to_string(),
                            };
                            let d =
                                self.err(codes::MISMATCH, self.espan(*x), format!("expected {what} to spread, found {}", self.m.an(v.ty)));
                            self.push(d);
                            bad = true;
                        }
                    }
                }
                ast::MapEntry::Named(_, value) | ast::MapEntry::Keyed(_, value) => {
                    let (key, key_span) = match en {
                        ast::MapEntry::Named(name, _) => (mk(ir::ExprKind::Str(name.name.clone()), Ty::Str), name.span),
                        ast::MapEntry::Keyed(k, _) => (self.expr_want(*k, kt), self.espan(*k)),
                        ast::MapEntry::Spread(_) => unreachable!(),
                    };
                    let key = match kt {
                        Some(t) => {
                            let k = coerce(key, t);
                            let before = self.m.diags.len();
                            self.expect_type(&k, t, key_span);
                            bad |= self.m.diags.len() > before;
                            k
                        }
                        None => {
                            if key.ty != Ty::Poison && !self.m.types.hashable(key.ty.ir()) {
                                let note = crate::module::hashable_note(&self.m.types, key.ty.ir());
                                let d = self
                                    .err(
                                        codes::NOT_HASHABLE,
                                        key_span,
                                        format!("a map's keys must be hashable, and `{}` isn't", self.m.tn(key.ty)),
                                    )
                                    .with_label(note);
                                self.push(d);
                                bad = true;
                            }
                            kt = Some(key.ty);
                            key
                        }
                    };
                    let v = self.expr_want(*value, vt);
                    let v = match vt {
                        Some(t) => {
                            let v = coerce(v, t);
                            let before = self.m.diags.len();
                            self.expect_type(&v, t, self.espan(*value));
                            bad |= self.m.diags.len() > before;
                            v
                        }
                        None => {
                            vt = Some(v.ty);
                            v
                        }
                    };
                    bad |= key.ty == Ty::Poison || v.ty == Ty::Poison;
                    out.push(ir::Entry::Pair(key.e, v.e));
                }
            }
        }
        if bad {
            return poison();
        }
        let (k, v) = (kt.unwrap(), vt.unwrap());
        if matches!(v, Ty::Void | Ty::Never) {
            let d = self.err(codes::VOID_VALUE, span, format!("a map can't hold {}", self.m.tn(v)));
            self.push(d);
            return poison();
        }
        let t = self.m.types.map(k.ir(), v.ir());
        mk(ir::ExprKind::MapLit(out), Ty::of(t))
    }

    /// `base[index]` on a list, a map or a tuple. `None` for other types.
    pub(super) fn collection_index(&mut self, b: TE, index: ExprId, span: Span) -> Option<TE> {
        let types = &self.m.types;
        match types.kind(b.ty.ir()).cloned()? {
            ir::Kind::List(t) => {
                let i = self.expr_want(index, Some(Ty::Int));
                self.expect_type(&i, Ty::Int, self.espan(index));
                if i.ty != Ty::Int {
                    return Some(poison());
                }
                let loc = self.loc(span);
                Some(mk(ir::ExprKind::Index { base: Box::new(b.e), index: Box::new(i.e), loc }, Ty::of(t)))
            }
            ir::Kind::Map(k, v) => {
                let key = self.expr_want(index, Some(Ty::of(k)));
                self.expect_type(&key, Ty::of(k), self.espan(index));
                if key.ty != Ty::of(k) {
                    return Some(poison());
                }
                let loc = self.loc(span);
                Some(mk(ir::ExprKind::Index { base: Box::new(b.e), index: Box::new(key.e), loc }, Ty::of(v)))
            }
            ir::Kind::Tuple(items) => {
                let n = self.tuple_position(&items, index, b.ty)?;
                Some(mk(ir::ExprKind::TupleGet(Box::new(b.e), n), Ty::of(items[n as usize])))
            }
            ir::Kind::Set(_) => {
                self.expr(index);
                let d = self
                    .err(codes::BAD_OPERANDS, span, "a set has no index access")
                    .with_label("check membership with `has`, or loop over it with `for...of`");
                self.push(d);
                Some(poison())
            }
            ir::Kind::Func(..) => None,
        }
    }

    /// A tuple position: an integer literal within the tuple (E0101).
    fn tuple_position(&mut self, items: &[ir::Type], index: ExprId, ty: Ty) -> Option<u32> {
        let ast = self.ast();
        let mut inner = index;
        while let ExprKind::Paren(i) = ast.expr(inner).kind {
            inner = i;
        }
        let span = self.espan(index);
        match ast.expr(inner).kind {
            ExprKind::Int(Some(n)) if (n as usize) < items.len() => Some(n as u32),
            ExprKind::Int(_) => {
                let d = self.err(codes::TUPLE_INDEX, span, format!("a tuple `{}` has positions 0 to {}", self.m.tn(ty), items.len() - 1));
                self.push(d);
                None
            }
            _ => {
                self.expr(index);
                let mut d =
                    self.err(codes::TUPLE_INDEX, span, "tuple index must be a constant").with_label("each position has its own type");
                for i in 0..items.len() {
                    d = d.with_fix(format!("use position {i}"), Applicability::Option, vec![self.edit(span, i.to_string())]);
                }
                self.push(d);
                None
            }
        }
    }

    /// `xs.length`, `s.size` and `m.size`. `None` for other members.
    pub(super) fn collection_field(&mut self, b: &TE, name: &str) -> Option<ir::Method> {
        let (recv, _) = self.receiver(b.ty)?;
        match (recv, name) {
            (Recv::List, "length") => Some(ir::Method::ListLength),
            (Recv::Set, "size") => Some(ir::Method::SetSize),
            (Recv::Map, "size") => Some(ir::Method::MapSize),
            _ => None,
        }
    }

    /// The place `target` names: a variable, or an element, key or tuple
    /// position below one. Reports and returns `None` when it isn't one.
    /// It doesn't check that the variable can change.
    pub(super) fn place_of(&mut self, target: ExprId) -> Option<PlaceOf> {
        let ast = self.ast();
        let span = self.espan(target);
        match &ast.expr(target).kind {
            ExprKind::Paren(inner) => self.place_of(*inner),
            ExprKind::Name(name) => {
                let Some(var) = self.lookup(name) else {
                    // Not a variable: `assignable` says what it is.
                    self.assignable(name, span, "modified");
                    return None;
                };
                let (local, ty, level) = (var.local, var.ty, var.level);
                if ty == Ty::Poison {
                    return None;
                }
                if level == self.level() && !self.check_in_use(local, level, name, span) {
                    return None;
                }
                let local = if level < self.level() { self.capture_use(name, span)? } else { local };
                self.copy_changed(local, span);
                self.f.changed.push(local);
                Some(PlaceOf {
                    place: ir::Place { root: local, steps: Vec::new(), ty: ty.ir() },
                    ty,
                    root_name: name.clone(),
                    root_span: span,
                })
            }
            ExprKind::Index { base, index } => {
                let mut p = self.place_of(*base)?;
                let kind = self.m.types.kind(p.ty.ir()).cloned();
                let loc = self.loc(span);
                let (step, ty) = match kind {
                    Some(ir::Kind::List(t)) => {
                        let i = self.expr_want(*index, Some(Ty::Int));
                        self.expect_type(&i, Ty::Int, self.espan(*index));
                        if i.ty != Ty::Int {
                            return None;
                        }
                        (ir::Step::Index(i.e, loc), Ty::of(t))
                    }
                    Some(ir::Kind::Map(k, v)) => {
                        let key = self.expr_want(*index, Some(Ty::of(k)));
                        self.expect_type(&key, Ty::of(k), self.espan(*index));
                        if key.ty != Ty::of(k) {
                            return None;
                        }
                        (ir::Step::Key(key.e, loc), Ty::of(v))
                    }
                    Some(ir::Kind::Tuple(items)) => {
                        let n = self.tuple_position(&items, *index, p.ty)?;
                        (ir::Step::Pos(n), Ty::of(items[n as usize]))
                    }
                    _ if p.ty == Ty::Str => {
                        self.expr(*index);
                        let d = self
                            .err(codes::BAD_OPERANDS, span, "a string's characters can't be assigned")
                            .with_label("strings are immutable; build a new one, for example with `slice` and `+`");
                        self.push(d);
                        return None;
                    }
                    _ => {
                        self.expr(*index);
                        let d = self.err(codes::BAD_OPERANDS, span, format!("{} can't be indexed", self.m.an(p.ty)));
                        self.push(d);
                        return None;
                    }
                };
                p.place.steps.push(step);
                p.place.ty = ty.ir();
                p.ty = ty;
                Some(p)
            }
            ExprKind::Field { name, .. } => {
                let v = self.expr(target);
                // `xs.length = 0`: a length follows from the elements.
                if let ir::ExprKind::Method { op: ir::Method::ListLength | ir::Method::SetSize | ir::Method::MapSize, .. }
                | ir::ExprKind::Builtin { op: ir::Builtin::StrLength, .. } = v.e.kind
                {
                    let d = self
                        .err(codes::BAD_OPERANDS, span, format!("`{}` can't be changed", self.text(span)))
                        .with_label(format!("the `{}` follows from the elements; remove or add elements to change it", name.name));
                    self.push(d);
                } else if v.ty != Ty::Poison {
                    self.unsupported(span, "changing fields of structs");
                }
                None
            }
            _ => {
                let v = self.expr(target);
                if v.ty != Ty::Poison {
                    let d = self
                        .err(codes::BAD_OPERANDS, span, format!("`{}` isn't a variable, so it can't be changed", self.text(span)))
                        .with_label("only a variable, or an element or key of one, can change");
                    self.push(d);
                }
                None
            }
        }
    }

    /// A place whose variable can change: a `let`, a `mut` parameter or a
    /// `mut` loop variable (E0201).
    pub(super) fn changeable_place(&mut self, target: ExprId, verb: &str) -> Option<PlaceOf> {
        let p = self.place_of(target)?;
        if self.var_kind(&p.root_name).is_some_and(|k| !k.changeable()) {
            self.assignable(&p.root_name.clone(), p.root_span, verb);
            return None;
        }
        Some(p)
    }

    /// The value at a place, read through its steps.
    pub(super) fn read_place(&mut self, p: &ir::Place) -> ir::Expr {
        let root_ty = self.f.locals[p.root.0 as usize].ty;
        let mut e = ir::Expr { kind: ir::ExprKind::Local(p.root), ty: root_ty };
        for step in &p.steps {
            let types = &self.m.types;
            let (kind, ty) = match step {
                ir::Step::Index(i, loc) => {
                    let t = types.list_elem(e.ty).unwrap();
                    (ir::ExprKind::Index { base: Box::new(e), index: Box::new(i.clone()), loc: *loc }, t)
                }
                ir::Step::Key(k, loc) => {
                    let t = types.map_types(e.ty).unwrap().1;
                    (ir::ExprKind::Index { base: Box::new(e), index: Box::new(k.clone()), loc: *loc }, t)
                }
                ir::Step::Pos(n) => {
                    let t = types.tuple_items(e.ty).unwrap()[*n as usize];
                    (ir::ExprKind::TupleGet(Box::new(e), *n), t)
                }
            };
            e = ir::Expr { kind, ty };
        }
        e
    }

    /// Moves the index and key operands of a place into locals of their
    /// own, so the place can be read and then written with each operand
    /// worked out once, as `xs[f()] += 1` needs.
    pub(super) fn hoist_steps(&mut self, p: &mut ir::Place, out: &mut ir::Block) {
        for step in &mut p.steps {
            if let ir::Step::Index(x, _) | ir::Step::Key(x, _) = step
                && !matches!(x.kind, ir::ExprKind::Local(_) | ir::ExprKind::Int(_) | ir::ExprKind::Str(_) | ir::ExprKind::Bool(_))
            {
                let t = self.temp(Ty::of(x.ty));
                let ty = x.ty;
                let value = std::mem::replace(x, ir::Expr { kind: ir::ExprKind::Local(t), ty });
                out.push(ir::Stmt::Let(t, value));
            }
        }
    }

    /// `x.name(args)` where `name` changes its receiver, as `xs.push(1)`.
    /// `None` when `base` isn't a collection, so the caller goes on.
    pub(super) fn mutating_call(&mut self, base: ExprId, name: &ast::Ident, args: &[ast::Arg], span: Span) -> Option<TE> {
        let ast = self.ast();
        let m = name.name.as_str();
        let place_like = |mut e: ExprId| loop {
            match &ast.expr(e).kind {
                ExprKind::Paren(i) => e = *i,
                ExprKind::Index { base, .. } => e = *base,
                ExprKind::Name(_) => return true,
                _ => return false,
            }
        };
        if !place_like(base) {
            // A temporary, such as `f().push(1)`: report it if it's a collection.
            let b = self.expr(base);
            self.check_args(Some((span, args)));
            if b.ty == Ty::Poison {
                return Some(poison());
            }
            if let Some((recv, _)) = self.receiver(b.ty)
                && (Self::sigs(recv, m).iter().any(|s| s.op.mutates()) || (recv == Recv::List && matches!(m, "push" | "unshift")))
            {
                let d = self
                    .err(
                        codes::BAD_OPERANDS,
                        self.espan(base),
                        format!("`{m}` changes its list, set or map, and this one isn't stored anywhere"),
                    )
                    .with_label("call it on a variable, or an element or key of one");
                self.push(d);
                return Some(poison());
            }
            self.unknown_member(b.ty, name, Some(span));
            return Some(poison());
        }
        let p = self.place_of(base);
        let Some(p) = p else {
            self.check_args(Some((span, args)));
            return Some(poison());
        };
        let Some((recv, bind)) = self.receiver(p.ty) else {
            self.check_args(Some((span, args)));
            self.unknown_member(p.ty, name, Some(span));
            return Some(poison());
        };
        let sigs: Vec<_> = Self::sigs(recv, m).into_iter().filter(|s| s.op.mutates()).collect();
        let is_push = recv == Recv::List && matches!(m, "push" | "unshift");
        if sigs.is_empty() && !is_push {
            self.check_args(Some((span, args)));
            self.unknown_member(p.ty, name, Some(span));
            return Some(poison());
        }
        // Only now: a const list gets E0201 for `push`, not for a misspelled method.
        let var = self.lookup(&p.root_name).map(|v| v.kind);
        if var.is_some_and(|k| !k.changeable()) {
            self.check_args(Some((span, args)));
            self.assignable(&p.root_name.clone(), p.root_span, "modified");
            return Some(poison());
        }
        let loc = self.loc(span);
        if is_push {
            let elem = Ty::of(bind.t.unwrap());
            let items: Vec<ExprId> = args.iter().map(|a| a.expr).collect();
            let (elems, _) = match self.elements(&items, Some(elem)) {
                Some(x) => x,
                None => return Some(poison()),
            };
            let spread = elems.iter().any(|e| matches!(e, ir::Elem::Spread(_)));
            let list_ty = p.ty;
            let (op, args) = if m == "push" && !spread {
                if elems.is_empty() {
                    self.arity(m, span, 1, usize::MAX, 0);
                    return Some(poison());
                }
                (ir::Method::ListPush, elems.into_iter().map(|e| if let ir::Elem::One(x) = e { x } else { unreachable!() }).collect())
            } else {
                let op = if m == "push" { ir::Method::ListPushAll } else { ir::Method::ListUnshift };
                (op, vec![mk(ir::ExprKind::ListLit(elems), list_ty).e])
            };
            return Some(mk(ir::ExprKind::Mutate { op, place: p.place, args, loc }, Ty::Void));
        }
        let (op, lowered, ret) = match self.method_args(&sigs, bind, m, args, span) {
            Some(x) => x,
            None => return Some(poison()),
        };
        Some(mk(ir::ExprKind::Mutate { op, place: p.place, args: lowered, loc }, ret))
    }

    /// `b.name(args)` on a list, set or map, for a method that doesn't
    /// change it. `None` when `b` isn't a collection.
    pub(super) fn collection_call(&mut self, b: TE, name: &ast::Ident, args: &[ast::Arg], span: Span, want: Option<Ty>) -> Option<TE> {
        let (recv, bind) = self.receiver(b.ty)?;
        let m = name.name.as_str();
        if m == "toString" {
            if !args.is_empty() {
                self.check_args(Some((span, args)));
                self.arity("toString", span, 0, 0, args.len());
                return Some(poison());
            }
            return Some(mk(ir::ExprKind::ToStr(Box::new(b.e)), Ty::Str));
        }
        // `flat()` takes a list of lists, which the table can't say.
        if recv == Recv::List && m == "flat" {
            if !args.is_empty() {
                self.check_args(Some((span, args)));
                self.arity("flat", span, 0, 0, args.len());
                return Some(poison());
            }
            let elem = bind.t.unwrap();
            if self.m.types.list_elem(elem).is_none() {
                let d = self
                    .err(codes::BAD_OPERANDS, span, format!("`flat` joins a list of lists, and this is {}", self.m.an(b.ty)))
                    .with_label("each element must be a list");
                self.push(d);
                return Some(poison());
            }
            return Some(mk(ir::ExprKind::Method { op: ir::Method::ListFlat, args: vec![b.e], loc: NO_LOC }, Ty::of(elem)));
        }
        let sigs = Self::sigs(recv, m);
        if sigs.is_empty() {
            self.check_args(Some((span, args)));
            match methods::later(recv, m) {
                Some(what) => self.unsupported(span, what),
                None => self.unknown_member(b.ty, name, Some(span)),
            }
            return Some(poison());
        }
        if recv == Recv::List && m == "find" && args.len() == 1 {
            self.check_args(Some((span, args)));
            self.unsupported(span, "`find` without a fallback, which returns a nullable value; pass the value to use when none is found");
            return Some(poison());
        }
        let mut bind = bind;
        if let Some(w) = want
            && sigs.iter().all(|s| matches!(s.ret, methods::S::U))
        {
            bind.u = Some(w.ir());
        }
        let (op, lowered, ret) = self.method_args(&sigs, bind, m, args, span)?;
        let mut all = vec![b.e];
        all.extend(lowered);
        Some(mk(ir::ExprKind::Method { op, args: all, loc: self.loc(span) }, ret))
    }

    /// `item in xs`, `item in s` or `key in m`, with both sides checked.
    pub(super) fn collection_in(&mut self, l: TE, r: TE, span: Span) -> Option<TE> {
        let (recv, bind) = self.receiver(r.ty)?;
        let (want, op) = match recv {
            Recv::List => (bind.t.unwrap(), ir::Method::InList),
            Recv::Set => (bind.t.unwrap(), ir::Method::InSet),
            Recv::Map => (bind.k.unwrap(), ir::Method::InMap),
        };
        let l = coerce(l, Ty::of(want));
        if l.ty != Ty::of(want) {
            let d = self.err(
                codes::BAD_OPERANDS,
                span,
                format!("`in` {} needs {} on its left, not {}", self.m.an(r.ty), self.m.an(Ty::of(want)), self.m.an(l.ty)),
            );
            self.push(d);
            return Some(poison());
        }
        if recv == Recv::List && !self.m.types.equatable(want) {
            let d = self.err(codes::NOT_COMPARABLE, span, "`in` compares with `==`, and function values can't be compared");
            self.push(d);
            return Some(poison());
        }
        Some(mk(ir::ExprKind::Method { op, args: vec![l.e, r.e], loc: NO_LOC }, Ty::Bool))
    }

    /// `kind` of the variable `name`, if it's one.
    pub(super) fn var_kind(&self, name: &str) -> Option<VarKind> {
        self.lookup(name).map(|v| v.kind)
    }
}
