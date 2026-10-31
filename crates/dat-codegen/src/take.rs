//! Take-and-restore as a borrow.
//!
//! The liveness pass turns `let v = m.get(k, fallback); ...; m[k] = v;`
//! (or `m[k]`, or `xs[i]`) into a take that moves the value out of the
//! entry and a put-back that stores it again (`dat-own`). Generated code
//! can do better: `v` borrows the entry itself, `&mut` from
//! `Map::get_or_insert_with` (or the element), the statements in between
//! change it in place, and the put-back is gone. That's one lookup instead
//! of a take and an insert, the way hand-written Rust uses the entry API.
//!
//! Nothing between the two touches the map, and the map can't be seen
//! after an error there (the pass checks both), so inserting a missing key
//! at the take instead of at the put-back puts it in the same place. `v`
//! must be used only as the entry: not assigned, not moved before the
//! put-back, and not used after it.

use dat_ir::{self as ir, ExprKind, LocalId, Method, Stmt};

/// The takes in `b` that can borrow their entry: `(take, put-back, v)`,
/// as statement positions. `rest` is what follows `b` in its scope.
pub(crate) fn borrowed_takes(b: &[Stmt], rest: &[Stmt]) -> Vec<(usize, usize, LocalId)> {
    let mut out = Vec::new();
    for (i, s) in b.iter().enumerate() {
        let Stmt::Let(v, e) = s else { continue };
        let ExprKind::Mutate { op: Method::MapTake | Method::Take, place, .. } = &e.kind else { continue };
        let Some(j) = (i + 1..b.len()).find(|&j| puts_back(&b[j], place.root, *v)) else { continue };
        let between_ok = b[i + 1..j].iter().all(|s| !assigns_or_moves(s, *v));
        let after_ok = b[j + 1..].iter().chain(rest).all(|s| !mentions(s, *v));
        if between_ok && after_ok {
            out.push((i, j, *v));
        }
    }
    out
}

/// `root[k] = v`, with `v`'s last use.
fn puts_back(s: &Stmt, root: LocalId, v: LocalId) -> bool {
    matches!(s, Stmt::Store(p, e) if p.root == root && p.steps.len() == 1 && matches!(e.kind, ExprKind::Move(x) if x == v))
}

/// Whether `s` assigns `v` or moves its value.
fn assigns_or_moves(s: &Stmt, v: LocalId) -> bool {
    let mut any = matches!(s, Stmt::Set(l, _, _) | Stmt::Let(l, _) if *l == v);
    ir::each_part(s, &mut |p| match p {
        ir::Part::Expr(e) => any |= moves(e, v),
        ir::Part::Block(b) => any |= b.iter().any(|s| assigns_or_moves(s, v)),
    });
    any
}

fn moves(e: &ir::Expr, v: LocalId) -> bool {
    let mut any = matches!(e.kind, ExprKind::Move(x) if x == v);
    ir::each_child(e, &mut |c| any |= moves(c, v));
    any
}

/// Whether `s` uses `v` at all.
fn mentions(s: &Stmt, v: LocalId) -> bool {
    fn expr(e: &ir::Expr, v: LocalId) -> bool {
        let here = match &e.kind {
            ExprKind::Local(l) | ExprKind::Move(l) => *l == v,
            ExprKind::Ref(p) | ExprKind::Mutate { place: p, .. } => p.root == v,
            ExprKind::Closure { captures, .. } => captures.contains(&v),
            _ => false,
        };
        let mut any = here;
        ir::each_child(e, &mut |c| any |= expr(c, v));
        any
    }
    let mut any = match s {
        Stmt::Set(l, _, _) | Stmt::Let(l, _) => *l == v,
        Stmt::Store(p, _) | Stmt::ForMut { list: p, .. } => p.root == v,
        _ => false,
    };
    ir::each_part(s, &mut |p| match p {
        ir::Part::Expr(e) => any |= expr(e, v),
        ir::Part::Block(b) => any |= b.iter().any(|s| mentions(s, v)),
    });
    any
}
