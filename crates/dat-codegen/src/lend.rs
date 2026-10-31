//! Which function values are lent rather than shared.
//!
//! A function value is a shared `dat_rt::FuncN`, which allocates when it's
//! made. A parameter of function type that the function only calls, passes
//! to a built-in method as its callback, or passes on to another lent
//! parameter can't outlive the call, so it's lent instead: a
//! `dat_rt::FnRefN` that borrows the caller's function, and an arrow
//! function passed to it is never allocated. Any other use, such as storing
//! it, returning it or printing it, keeps it shared. It's a choice of the
//! generated Rust only: dat code behaves the same either way.
//!
//! An arrow function that's only lent, to a built-in method or to a lent
//! parameter, lends on the function values it captures by the same rule.
//! An arrow's own parameters and a parameter with a default stay shared: a
//! function value's type fixes how its parameters are passed, and a default
//! is a value of its own.
//!
//! Lent parameters pass each other on, recursion included, so the rule is a
//! fixed point: every candidate starts lent, and a use that needs a value of
//! its own makes it shared, until nothing changes.

use dat_ir::{self as ir, ExprKind, Stmt};

pub(crate) struct Lent {
    /// For each function, which of its locals are lent function values.
    locals: Vec<Vec<bool>>,
}

impl Lent {
    pub(crate) fn new(p: &ir::Program) -> Lent {
        let n = p.functions.len();
        let mut arrow = vec![false; n];
        for f in &p.functions {
            each_expr(f, &mut |e| {
                if let ExprKind::Closure { func, .. } = e.kind {
                    arrow[func.0 as usize] = true;
                }
            });
        }
        let mut locals: Vec<Vec<bool>> = p
            .functions
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let first_default = f.params as usize - f.defaults.len();
                f.locals
                    .iter()
                    .enumerate()
                    .map(|(l, local)| {
                        let param = !arrow[i] && l < first_default;
                        let capture = f.captures.contains(&ir::LocalId(l as u32));
                        p.types.func_sig(local.ty).is_some() && !local.by_ref && (param || capture)
                    })
                    .collect()
            })
            .collect();
        // Arrow functions that are only lent so far.
        let mut lent_arrow = arrow;
        loop {
            let mut w = Walk { p, lent: &locals, lent_arrow: &lent_arrow, func: 0, shared: Vec::new(), stored: Vec::new() };
            for (i, f) in p.functions.iter().enumerate() {
                w.func = i;
                for d in &f.defaults {
                    w.value(d);
                }
                w.block(&f.body);
            }
            let (shared, stored) = (w.shared, w.stored);
            let mut changed = false;
            for (f, l) in shared {
                changed |= std::mem::replace(&mut locals[f][l.0 as usize], false);
            }
            for a in stored {
                changed |= std::mem::replace(&mut lent_arrow[a.0 as usize], false);
                for c in &p.func(a).captures {
                    changed |= std::mem::replace(&mut locals[a.0 as usize][c.0 as usize], false);
                }
            }
            if !changed {
                return Lent { locals };
            }
        }
    }

    /// Whether the local `l` of the function `f` is a lent function value.
    pub(crate) fn local(&self, f: ir::FuncId, l: ir::LocalId) -> bool {
        self.locals[f.0 as usize].get(l.0 as usize).copied().unwrap_or(false)
    }
}

/// One pass over the program, from the state the last pass left.
struct Walk<'a> {
    p: &'a ir::Program,
    lent: &'a [Vec<bool>],
    lent_arrow: &'a [bool],
    /// The function being walked.
    func: usize,
    /// Lent locals, by function, that a use needs as a value of their own.
    shared: Vec<(usize, ir::LocalId)>,
    /// Arrow functions made into a value of their own.
    stored: Vec<ir::FuncId>,
}

impl Walk<'_> {
    fn block(&mut self, b: &[Stmt]) {
        for s in b {
            if let Stmt::Store(place, _) | Stmt::ForMut { list: place, .. } = s {
                self.share(place.root);
            }
            ir::each_part(s, &mut |part| match part {
                ir::Part::Expr(e) => self.value(e),
                ir::Part::Block(b) => self.block(b),
            });
        }
    }

    /// `e`, whose value is used as it is: a function value there needs one
    /// of its own.
    fn value(&mut self, e: &ir::Expr) {
        match &e.kind {
            ExprKind::Local(l) | ExprKind::Move(l) => self.share(*l),
            ExprKind::Closure { func, captures } => {
                self.stored.push(*func);
                self.captures(*func, captures);
            }
            ExprKind::Ref(place) => {
                self.share(place.root);
                ir::each_child(e, &mut |c| self.value(c));
            }
            ExprKind::Mutate { op, place, args, .. } => {
                self.share(place.root);
                self.operands(place);
                for (i, a) in args.iter().enumerate() {
                    if op.takes_callback() && i == 0 { self.lent_arg(a) } else { self.value(a) }
                }
            }
            ExprKind::Method { op, args, .. } if op.takes_callback() => {
                for (i, a) in args.iter().enumerate() {
                    if i == 1 { self.lent_arg(a) } else { self.value(a) }
                }
            }
            ExprKind::CallValue { callee, args, .. } => {
                if !matches!(callee.kind, ExprKind::Local(_) | ExprKind::Move(_)) {
                    self.value(callee);
                }
                args.iter().for_each(|a| self.value(a));
            }
            ExprKind::Call { func, args, .. } => {
                for (k, a) in args.iter().enumerate() {
                    if self.lent[func.0 as usize][k] { self.lent_arg(a) } else { self.value(a) }
                }
            }
            _ => ir::each_child(e, &mut |c| self.value(c)),
        }
    }

    /// `a`, which is only lent for a call: a built-in method's callback, or
    /// the argument of a lent parameter. A shared function value is lent with
    /// `lend()`.
    fn lent_arg(&mut self, a: &ir::Expr) {
        match &a.kind {
            ExprKind::Local(_) | ExprKind::Move(_) | ExprKind::FuncRef(_) => {}
            ExprKind::Closure { func, captures } => self.captures(*func, captures),
            _ => self.value(a),
        }
    }

    /// The locals `captures` that the arrow function `arrow` copies.
    fn captures(&mut self, arrow: ir::FuncId, captures: &[ir::LocalId]) {
        let slots = &self.p.func(arrow).captures;
        for (c, slot) in captures.iter().zip(slots) {
            if !(self.lent_arrow[arrow.0 as usize] && self.lent[arrow.0 as usize][slot.0 as usize]) {
                self.share(*c);
            }
        }
    }

    fn operands(&mut self, place: &ir::Place) {
        for s in &place.steps {
            if let ir::Step::Index(x, _) | ir::Step::Key(x, _) = s {
                self.value(x);
            }
        }
    }

    fn share(&mut self, l: ir::LocalId) {
        if self.lent[self.func].get(l.0 as usize).copied().unwrap_or(false) {
            self.shared.push((self.func, l));
        }
    }
}

/// Calls `visit` on every expression of `f`, its defaults included.
fn each_expr(f: &ir::Function, visit: &mut dyn FnMut(&ir::Expr)) {
    fn expr(e: &ir::Expr, visit: &mut dyn FnMut(&ir::Expr)) {
        visit(e);
        ir::each_child(e, &mut |c| expr(c, visit));
    }
    fn block(b: &[Stmt], visit: &mut dyn FnMut(&ir::Expr)) {
        for s in b {
            ir::each_part(s, &mut |part| match part {
                ir::Part::Expr(e) => expr(e, visit),
                ir::Part::Block(b) => block(b, visit),
            });
        }
    }
    for d in &f.defaults {
        expr(d, visit);
    }
    block(&f.body, visit);
}
