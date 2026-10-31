//! Lists a loop changes in place.
//!
//! Every change to a `dat_rt::List` checks whether another copy shares its
//! buffer. A loop that changes a list on every pass makes that check, and
//! goes through the list's pointer to its buffer, every time. When the loop
//! does nothing with the list but read and change its elements, ask its
//! length, push, pop and shift, nothing in the loop can make a copy that
//! shares the buffer, so the generated code checks once, before the loop:
//! `dat_rt::List::edit` makes the buffer the list's own, and the loop
//! works through the `dat_rt::ListMut` it returns. A loop that never
//! pushes, pops or shifts the list works through a `dat_rt::ListSlice`
//! instead, whose length stays in a register: a breadth-first search's
//! distances ran 9% faster that way. Any other use of the list in the
//! loop, such as passing it on, printing it or copying it, keeps every
//! check. It's a choice of the generated Rust only: dat code behaves the
//! same either way.

use dat_ir::{self as ir, ExprKind, Method, Stmt};

/// The lists declared before the loop `s` that it changes and otherwise
/// only uses as a `ListMut` can, leaving out those in `skip`, each with
/// whether the loop changes its length. A list whose length stays the same
/// goes through a `ListSlice`, which keeps it in a register.
pub(crate) fn loop_lists(p: &ir::Program, f: &ir::Function, s: &Stmt, skip: &[ir::LocalId]) -> Vec<(ir::LocalId, bool)> {
    let mut u = Uses::default();
    u.stmt(s);
    let mut lists: Vec<ir::LocalId> = u
        .changed
        .iter()
        .copied()
        .filter(|l| !u.other.contains(l) && !u.declared.contains(l) && !skip.contains(l))
        .filter(|l| p.types.list_elem(f.locals[l.0 as usize].ty).is_some())
        .collect();
    lists.sort_by_key(|l| l.0);
    lists.dedup();
    lists.into_iter().map(|l| (l, u.resized.contains(&l))).collect()
}

#[derive(Default)]
struct Uses {
    /// Changed through a `ListMut`.
    changed: Vec<ir::LocalId>,
    /// Pushed to, popped or shifted.
    resized: Vec<ir::LocalId>,
    /// Used some other way.
    other: Vec<ir::LocalId>,
    /// Declared inside the loop.
    declared: Vec<ir::LocalId>,
}

impl Uses {
    fn block(&mut self, b: &[Stmt]) {
        b.iter().for_each(|s| self.stmt(s));
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Let(l, _) => self.declared.push(*l),
            Stmt::Set(l, _, _) => self.other.push(*l),
            Stmt::Store(place, _) => self.place(place, true),
            Stmt::ForOf { var, .. } => self.declared.push(*var),
            Stmt::ForMut { var, list, .. } => {
                self.declared.push(*var);
                self.other.push(list.root);
            }
            _ => {}
        }
        ir::each_part(s, &mut |part| match part {
            ir::Part::Expr(e) => self.expr(e),
            ir::Part::Block(b) => self.block(b),
        });
    }

    /// A place changed in place: through a `ListMut` when its first step is
    /// an index, or at the list itself when `whole` is allowed.
    fn place(&mut self, place: &ir::Place, whole: bool) {
        match place.steps.first() {
            Some(ir::Step::Index(..)) => self.changed.push(place.root),
            None if whole => self.changed.push(place.root),
            _ => self.other.push(place.root),
        }
    }

    fn expr(&mut self, e: &ir::Expr) {
        match &e.kind {
            // `xs[i]`, unless the index changes something (`xs[xs.pop()]`),
            // which reads the list as it was before.
            ExprKind::Index { base, index, .. } if matches!(base.kind, ExprKind::Local(_)) && !crate::changes(index) => {
                return self.expr(index);
            }
            ExprKind::Method { op: Method::ListLength | Method::ListIsEmpty, args, .. } if matches!(args[0].kind, ExprKind::Local(_)) => {
                return;
            }
            ExprKind::Mutate { op, place, .. } => {
                let resizes = matches!(op, Method::ListPush | Method::ListPop | Method::ListShift);
                if resizes && place.steps.is_empty() {
                    self.resized.push(place.root);
                }
                self.place(place, resizes);
            }
            ExprKind::Ref(place) => self.other.push(place.root),
            ExprKind::Local(l) | ExprKind::Move(l) => self.other.push(*l),
            ExprKind::Closure { captures, .. } => self.other.extend(captures),
            _ => {}
        }
        ir::each_child(e, &mut |c| self.expr(c));
    }
}
