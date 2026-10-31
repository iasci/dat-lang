//! The one walk over the IR's children, so a new node is added in one place.

use crate::{AssertCond, Block, Elem, Entry, Expr, ExprKind, Iter, Place, Step, Stmt};

/// Calls `f` on each direct child expression of `e`, in evaluation order.
pub fn each_child(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    use ExprKind::*;
    match &e.kind {
        Int(_) | Float(_) | Bool(_) | Str(_) | Local(_) | Move(_) | FuncRef(_) | Closure { .. } => {}
        Call { args, .. } | Builtin { args, .. } | Method { args, .. } | Concat(args) | TupleLit(args) => args.iter().for_each(f),
        CallValue { callee, args, .. } => {
            f(callee);
            args.iter().for_each(f);
        }
        Ref(place) => place_parts(place, f),
        Mutate { place, args, .. } => {
            place_parts(place, f);
            args.iter().for_each(f);
        }
        ListLit(elems) | SetLit(elems) => elems.iter().for_each(|el| match el {
            Elem::One(x) | Elem::Spread(x) => f(x),
        }),
        MapLit(entries) => entries.iter().for_each(|en| match en {
            Entry::Pair(k, v) => {
                f(k);
                f(v);
            }
            Entry::Spread(x) => f(x),
        }),
        Index { base: lhs, index: rhs, .. }
        | IntArith { lhs, rhs, .. }
        | FloatArith { lhs, rhs, .. }
        | Compare { lhs, rhs, .. }
        | And(lhs, rhs)
        | Or(lhs, rhs) => {
            f(lhs);
            f(rhs);
        }
        TupleGet(a, _)
        | IntNeg { arg: a, .. }
        | FloatNeg(a)
        | Not(a)
        | ToStr(a)
        | ErrorMessage(a)
        | Print(a)
        | Diverge(a)
        | NewError { message: a, .. }
        | Fail { message: a, .. } => f(a),
        Cond { cond, then, els } => {
            f(cond);
            f(then);
            f(els);
        }
        Assert(a) => {
            match &a.cond {
                AssertCond::Plain(c) => f(c),
                AssertCond::Compare { lhs, rhs, .. } => {
                    f(lhs);
                    f(rhs);
                }
            }
            if let Some(m) = &a.message {
                f(m);
            }
        }
    }
}

/// The index and key operands of a place.
fn place_parts(place: &Place, f: &mut dyn FnMut(&Expr)) {
    for s in &place.steps {
        match s {
            Step::Index(x, _) | Step::Key(x, _) => f(x),
            Step::Pos(_) => {}
        }
    }
}

/// A direct part of a statement.
pub enum Part<'a> {
    Expr(&'a Expr),
    Block(&'a Block),
}

/// Calls `f` on each expression and block directly in `s`, in order.
pub fn each_part(s: &Stmt, f: &mut dyn FnMut(Part)) {
    let mut expr = |e: &Expr| f(Part::Expr(e));
    match s {
        Stmt::Let(_, e) | Stmt::Set(_, e, _) | Stmt::Expr(e) | Stmt::Throw(e, _) | Stmt::Return(Some(e)) => expr(e),
        Stmt::Store(place, e) => {
            place_parts(place, &mut expr);
            expr(e);
        }
        Stmt::If(c, a, b) => {
            expr(c);
            f(Part::Block(a));
            f(Part::Block(b));
        }
        Stmt::Loop { cond, body, step } => {
            if let Some(c) = cond {
                expr(c);
            }
            f(Part::Block(body));
            f(Part::Block(step));
        }
        Stmt::Try { body, catches } => {
            f(Part::Block(body));
            catches.iter().for_each(|c| f(Part::Block(&c.body)));
        }
        Stmt::ForOf { iter, body, .. } => {
            match iter {
                Iter::Range { start, end, step, .. } => {
                    expr(start);
                    expr(end);
                    expr(step);
                }
                Iter::Each(e) => expr(e),
            }
            f(Part::Block(body));
        }
        Stmt::ForMut { list, body, .. } => {
            place_parts(list, &mut expr);
            f(Part::Block(body));
        }
        Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
    }
}
