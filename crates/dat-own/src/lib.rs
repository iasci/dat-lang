//! The liveness pass. It runs on the typed IR after lowering and marks
//! each read of a variable that is its last use, before the variable is
//! assigned again or goes out of scope, as a [`ExprKind::Move`]. Both run
//! modes hand the value over there instead of sharing it, so a list that
//! nothing else holds is changed in place and §02's patterns never copy.
//!
//! It walks each function backward over the IR's structured regions,
//! keeping the set of variables that are read later. A loop is walked
//! until that set stops changing, then once more to mark. No control-flow
//! graph is needed.
//!
//! Inside a `try`, an operation that throws can go on in a `catch` or
//! after the `try`, so what those read stays alive for the whole body.
//! A `mut` parameter or loop variable is never moved: its value belongs to
//! the caller or the list.

#![forbid(unsafe_code)]

mod verify;

pub use verify::verify;

use dat_ir::{Block, Expr, ExprKind, Iter, LocalId, Place, Program, Step, Stmt, Type};

/// Marks the last uses in every function of `p`.
pub fn run(p: &mut Program) {
    for f in &mut p.functions {
        let movable: Vec<bool> = f.locals.iter().map(|l| !l.by_ref && shares(l.ty)).collect();
        let mut pass = Pass { movable, pinned: Vec::new(), loops: Vec::new(), mark: false };
        let n = f.locals.len();
        let mut body = std::mem::take(&mut f.body);
        // Walk once to settle the loops' sets, then again to mark.
        pass.block(&mut body, Live::new(n));
        pass.mark = true;
        pass.block(&mut body, Live::new(n));
        f.body = body;
    }
}

/// Whether moving a value of type `t` saves a copy: it shares memory.
fn shares(t: Type) -> bool {
    matches!(t, Type::Str | Type::Error(_) | Type::Of(_))
}

/// A set of locals, as bits.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Live(Vec<u64>);

impl Live {
    fn new(n: usize) -> Live {
        Live(vec![0; n.div_ceil(64)])
    }

    fn has(&self, l: LocalId) -> bool {
        self.0[l.0 as usize / 64] & (1 << (l.0 % 64)) != 0
    }

    fn add(&mut self, l: LocalId) {
        self.0[l.0 as usize / 64] |= 1 << (l.0 % 64);
    }

    fn remove(&mut self, l: LocalId) {
        self.0[l.0 as usize / 64] &= !(1 << (l.0 % 64));
    }

    fn union(&mut self, other: &Live) {
        for (a, b) in self.0.iter_mut().zip(&other.0) {
            *a |= b;
        }
    }

    fn empty_like(&self) -> Live {
        Live(vec![0; self.0.len()])
    }
}

struct Pass {
    movable: Vec<bool>,
    /// For each `try` the code is in: what its `catch` clauses and the code
    /// after it read, which stays alive through its body.
    pinned: Vec<Live>,
    /// For each loop the code is in: what's alive where `break` and
    /// `continue` go.
    loops: Vec<(Live, Live)>,
    /// The last walk: reads that are last uses become moves.
    mark: bool,
}

impl Pass {
    fn alive(&self, live: &Live, l: LocalId) -> bool {
        live.has(l) || self.pinned.iter().any(|p| p.has(l))
    }

    /// What's alive before `b`, given what's alive after it.
    fn block(&mut self, b: &mut Block, mut live: Live) -> Live {
        for i in (0..b.len()).rev() {
            if self.mark {
                self.take_and_restore(b, i);
            }
            live = self.stmt(&mut b[i], live);
        }
        live
    }

    /// §02's take-and-restore: `let v = m.get(k, fallback);` (or `m[k]`,
    /// or `xs[i]`), then statements that leave `m` and `k` alone, then
    /// `m[k] = v;` with `v`'s last use. The read takes the value out of
    /// the entry instead of sharing it, so changing `v` doesn't copy it,
    /// and the store puts it back in the same place. The statements in
    /// between can't leave the block early, and `m` can't be seen after an
    /// error here, so nothing ever sees the placeholder the take leaves.
    fn take_and_restore(&self, b: &mut Block, i: usize) {
        let Stmt::Let(v, read) = &b[i] else { return };
        let v = *v;
        let (root, key) = match &read.kind {
            ExprKind::Method { op: dat_ir::Method::MapGet, args, .. } => match (&args[0].kind, &args[1]) {
                (ExprKind::Local(m), k) => (*m, k.clone()),
                _ => return,
            },
            ExprKind::Index { base, index, .. } => match &base.kind {
                ExprKind::Local(m) => (*m, (**index).clone()),
                _ => return,
            },
            _ => return,
        };
        let key = &key;
        if !self.movable[root.0 as usize] || self.pinned.iter().any(|p| p.has(root)) || !pure_key(key) {
            return;
        }
        let mut key_locals = Vec::new();
        locals_of(key, &mut key_locals);
        for j in i + 1..b.len() {
            let restores = |s: &Stmt| match s {
                Stmt::Store(place, value) => {
                    place.root == root
                        && place.steps.len() == 1
                        && matches!(&value.kind, ExprKind::Move(x) if *x == v)
                        && matches!(&place.steps[0], dat_ir::Step::Key(k, _) | dat_ir::Step::Index(k, _) if same_key(k, key))
                }
                _ => false,
            };
            if restores(&b[j]) {
                let by_key = matches!(&b[j], Stmt::Store(p, _) if matches!(p.steps[0], dat_ir::Step::Key(..)));
                let stmt = std::mem::replace(&mut b[i], Stmt::Break);
                let Stmt::Let(v, read) = stmt else { unreachable!() };
                let ty = read.ty;
                let taken = match read.kind {
                    ExprKind::Method { mut args, loc, .. } => {
                        let fallback = args.pop().expect("a fallback");
                        let key = args.pop().expect("a key");
                        ExprKind::Mutate {
                            op: dat_ir::Method::MapTake,
                            place: Place { root, steps: Vec::new(), ty: args[0].ty },
                            args: vec![key, fallback],
                            loc,
                        }
                    }
                    ExprKind::Index { index, loc, .. } => {
                        let step = if by_key { dat_ir::Step::Key(*index, loc) } else { dat_ir::Step::Index(*index, loc) };
                        ExprKind::Mutate { op: dat_ir::Method::Take, place: Place { root, steps: vec![step], ty }, args: Vec::new(), loc }
                    }
                    _ => unreachable!(),
                };
                b[i] = Stmt::Let(v, Expr { kind: taken, ty });
                return;
            }
            if leaves_or_touches(&b[j], root, &key_locals) {
                return;
            }
        }
    }

    fn stmt(&mut self, s: &mut Stmt, mut live: Live) -> Live {
        match s {
            Stmt::Let(l, e) | Stmt::Set(l, e, _) => {
                // `s += t` appends to `s` in place: `s` is used, not handed over.
                let appends = e.appended_to(*l).is_some();
                live.remove(*l);
                if appends {
                    live.add(*l);
                }
                self.expr(e, &mut live);
                live
            }
            Stmt::Store(place, e) => {
                live.add(place.root);
                self.expr(e, &mut live);
                self.place(place, &mut live);
                live
            }
            Stmt::Expr(e) => {
                self.walk(e, &mut live, false);
                live
            }
            Stmt::If(c, a, b) => {
                let mut l = self.block(a, live.clone());
                let r = self.block(b, live);
                l.union(&r);
                self.expr(c, &mut l);
                l
            }
            Stmt::Loop { cond, body, step } => {
                let out = live;
                // What's alive at the top of the loop, where `cond` runs.
                let mut head = out.clone();
                loop {
                    let mark = std::mem::replace(&mut self.mark, false);
                    let next = self.loop_head(cond, body, step, &out, &head);
                    self.mark = mark;
                    if next == head {
                        break;
                    }
                    head = next;
                }
                self.loop_head(cond, body, step, &out, &head)
            }
            Stmt::Break => self.loops.last().map(|(b, _)| b.clone()).unwrap_or_else(|| live.empty_like()),
            Stmt::Continue => self.loops.last().map(|(_, c)| c.clone()).unwrap_or_else(|| live.empty_like()),
            Stmt::Return(e) => {
                let mut live = live.empty_like();
                if let Some(e) = e {
                    self.expr(e, &mut live);
                }
                live
            }
            Stmt::Throw(e, _) => {
                let mut live = live.empty_like();
                self.expr(e, &mut live);
                live
            }
            Stmt::Try { body, catches } => {
                let mut pinned = live.clone();
                for c in catches.iter_mut() {
                    let mut l = self.block(&mut c.body, live.clone());
                    if let Some(b) = c.bind {
                        l.remove(b);
                    }
                    pinned.union(&l);
                }
                self.pinned.push(pinned.clone());
                let mut l = self.block(body, live);
                self.pinned.pop();
                l.union(&pinned);
                l
            }
            Stmt::ForOf { var, iter, body } => {
                let out = live;
                let mut head = out.clone();
                loop {
                    let mark = std::mem::replace(&mut self.mark, false);
                    let next = self.for_head(*var, body, &out, &head);
                    self.mark = mark;
                    if next == head {
                        break;
                    }
                    head = next;
                }
                let mut l = self.for_head(*var, body, &out, &head);
                match iter {
                    Iter::Range { start, end, step, .. } => {
                        self.expr(step, &mut l);
                        self.expr(end, &mut l);
                        self.expr(start, &mut l);
                    }
                    Iter::Each(e) => self.expr(e, &mut l),
                }
                l
            }
            Stmt::ForMut { var, list, body, .. } => {
                let mut out = live;
                out.add(list.root);
                let mut head = out.clone();
                loop {
                    let mark = std::mem::replace(&mut self.mark, false);
                    let next = self.for_head(*var, body, &out, &head);
                    self.mark = mark;
                    if next == head {
                        break;
                    }
                    head = next;
                }
                let mut l = self.for_head(*var, body, &out, &head);
                self.place(list, &mut l);
                l
            }
        }
    }

    /// One walk of a `while` or `for (;;)` loop whose head has `head`
    /// alive, after which `out` is alive: what's alive at its head.
    fn loop_head(&mut self, cond: &mut Option<Expr>, body: &mut Block, step: &mut Block, out: &Live, head: &Live) -> Live {
        let step_in = self.block(step, head.clone());
        self.loops.push((out.clone(), step_in.clone()));
        let mut l = self.block(body, step_in);
        self.loops.pop();
        if let Some(c) = cond {
            l.union(out);
            self.expr(c, &mut l);
        }
        let mut next = head.clone();
        next.union(&l);
        next
    }

    /// One walk of a `for...of` body: each turn assigns `var` first.
    fn for_head(&mut self, var: LocalId, body: &mut Block, out: &Live, head: &Live) -> Live {
        self.loops.push((out.clone(), head.clone()));
        let mut l = self.block(body, head.clone());
        self.loops.pop();
        l.remove(var);
        l.union(out);
        let mut next = head.clone();
        next.union(&l);
        next
    }

    /// The index and key operands of a place, which run first.
    fn place(&mut self, p: &mut Place, live: &mut Live) {
        for s in p.steps.iter_mut().rev() {
            if let Step::Index(x, _) | Step::Key(x, _) = s {
                self.expr(x, live);
            }
        }
    }

    /// Walks `e`, whose value its parent consumes, backward.
    fn expr(&mut self, e: &mut Expr, live: &mut Live) {
        self.walk(e, live, true);
    }

    /// Walks `e` backward: its children run left to right, so the last one
    /// is walked first. Only where the value is consumed, as an argument,
    /// an assignment's value or a list element, does a last use move it;
    /// where it's only looked at, as a method's receiver, it stays put.
    fn walk(&mut self, e: &mut Expr, live: &mut Live, consumed: bool) {
        match &mut e.kind {
            ExprKind::Local(l) => {
                let l = *l;
                if self.mark && consumed && self.movable[l.0 as usize] && !self.alive(live, l) {
                    e.kind = ExprKind::Move(l);
                }
                live.add(l);
            }
            ExprKind::Move(l) => live.add(*l),
            ExprKind::Closure { captures, .. } => {
                for c in captures.iter() {
                    live.add(*c);
                }
            }
            ExprKind::Ref(place) => {
                live.add(place.root);
                self.place(place, live);
            }
            ExprKind::Mutate { op, place, args, .. } => {
                live.add(place.root);
                let callback = op.takes_callback();
                for (i, a) in args.iter_mut().enumerate().rev() {
                    self.walk(a, live, !(callback && i == 0));
                }
                self.place(place, live);
            }
            _ => {
                // What generated code borrows while it works out the other
                // children stays alive until the node is done: a method's
                // receiver, an index's base, a called function value, a
                // string a built-in reads, and a `mut` argument.
                match &e.kind {
                    ExprKind::Method { op, args, .. }
                        if !matches!(op, dat_ir::Method::InList | dat_ir::Method::InSet | dat_ir::Method::InMap) =>
                    {
                        reads(&args[0], live);
                    }
                    ExprKind::Index { base, .. } => reads(base, live),
                    ExprKind::CallValue { callee, .. } => reads(callee, live),
                    ExprKind::Builtin { args, .. } => args.iter().filter(|a| a.ty == Type::Str).for_each(|a| reads(a, live)),
                    ExprKind::Call { args, .. } => {
                        for a in args {
                            if let ExprKind::Ref(p) = &a.kind {
                                live.add(p.root);
                            }
                        }
                    }
                    _ => {}
                }
                let consumes = matches!(
                    e.kind,
                    ExprKind::Call { .. }
                        | ExprKind::CallValue { .. }
                        | ExprKind::ListLit(_)
                        | ExprKind::SetLit(_)
                        | ExprKind::MapLit(_)
                        | ExprKind::TupleLit(_)
                );
                let callee = matches!(e.kind, ExprKind::CallValue { .. });
                let mut kids: Vec<(&mut Expr, bool)> = Vec::new();
                children_mut(e, &mut kids);
                let n = kids.len();
                for (i, (k, spread)) in kids.into_iter().enumerate().rev() {
                    // A function value is called, and a spread list read, in place.
                    self.walk(k, live, consumes && !spread && !(callee && i == 0) && n > 0);
                }
            }
        }
    }
}

/// Adds every local `e` reads to `live`.
fn reads(e: &Expr, live: &mut Live) {
    match &e.kind {
        ExprKind::Local(l) | ExprKind::Move(l) => live.add(*l),
        ExprKind::Ref(p) | ExprKind::Mutate { place: p, .. } => live.add(p.root),
        ExprKind::Closure { captures, .. } => captures.iter().for_each(|c| live.add(*c)),
        _ => {}
    }
    dat_ir::each_child(e, &mut |c| reads(c, live));
}

/// The locals a key reads.
fn locals_of(e: &Expr, out: &mut Vec<LocalId>) {
    if let ExprKind::Local(l) | ExprKind::Move(l) = e.kind {
        out.push(l);
    }
    dat_ir::each_child(e, &mut |c| locals_of(c, out));
}

/// A key a take can work out twice and get the same value, as long as
/// the locals it reads keep theirs: locals, constants, and arithmetic,
/// joins and tuple positions of them, such as `i % 2` or `team + "!"`.
/// A key that threw at the take never reaches the put-back.
fn pure_key(k: &Expr) -> bool {
    let here = matches!(
        k.kind,
        ExprKind::Local(_)
            | ExprKind::Str(_)
            | ExprKind::Int(_)
            | ExprKind::Bool(_)
            | ExprKind::IntArith { .. }
            | ExprKind::IntNeg { .. }
            | ExprKind::TupleGet(..)
            | ExprKind::Concat(_)
            | ExprKind::ToStr(_)
    );
    let mut all = here;
    dat_ir::each_child(k, &mut |c| all &= pure_key(c));
    all
}

/// Whether two keys of [`pure_key`]'s kind are the same expression.
fn same_key(a: &Expr, b: &Expr) -> bool {
    use ExprKind::*;
    match (&a.kind, &b.kind) {
        (Local(x) | Move(x), Local(y) | Move(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        (Int(x), Int(y)) => x == y,
        (Bool(x), Bool(y)) => x == y,
        (IntArith { op: o1, lhs: l1, rhs: r1, .. }, IntArith { op: o2, lhs: l2, rhs: r2, .. }) => {
            o1 == o2 && same_key(l1, l2) && same_key(r1, r2)
        }
        (IntNeg { arg: x, .. }, IntNeg { arg: y, .. }) | (ToStr(x), ToStr(y)) => same_key(x, y),
        (TupleGet(x, i), TupleGet(y, j)) => i == j && same_key(x, y),
        (Concat(xs), Concat(ys)) => xs.len() == ys.len() && xs.iter().zip(ys).all(|(x, y)| same_key(x, y)),
        _ => false,
    }
}

/// Whether `s` mentions `root`, assigns a local the key reads, or may
/// leave the block other than at its end: what keeps a take from being
/// put back.
fn leaves_or_touches(s: &Stmt, root: LocalId, key: &[LocalId]) -> bool {
    // Whether `l` is the map or read by the key: the key may change in
    // place, as `k.push(9)` or `change(mut k)`, and then the store goes
    // elsewhere.
    let watched = |l: LocalId| l == root || key.contains(&l);
    fn expr(e: &Expr, root: LocalId, watched: &dyn Fn(LocalId) -> bool) -> bool {
        let here = match &e.kind {
            ExprKind::Local(l) | ExprKind::Move(l) => *l == root,
            ExprKind::Ref(p) | ExprKind::Mutate { place: p, .. } => watched(p.root),
            ExprKind::Closure { captures, .. } => captures.contains(&root),
            _ => false,
        };
        let mut any = here;
        dat_ir::each_child(e, &mut |c| any |= expr(c, root, watched));
        any
    }
    let mut any = match s {
        Stmt::Break | Stmt::Continue | Stmt::Return(_) | Stmt::Throw(..) => true,
        Stmt::Set(l, _, _) | Stmt::Let(l, _) => watched(*l),
        Stmt::Store(p, _) | Stmt::ForMut { list: p, .. } => watched(p.root),
        _ => false,
    };
    dat_ir::each_part(s, &mut |p| match p {
        dat_ir::Part::Expr(e) => any |= expr(e, root, &watched),
        dat_ir::Part::Block(b) => any |= b.iter().any(|s| leaves_or_touches(s, root, key)),
    });
    any
}

/// The direct children of `e`, in evaluation order, for the nodes that
/// [`Pass::walk`] doesn't handle itself, each with whether it's a spread.
fn children_mut<'e>(e: &'e mut Expr, out: &mut Vec<(&'e mut Expr, bool)>) {
    use ExprKind::*;
    let one = |x: &'e mut Expr, out: &mut Vec<(&'e mut Expr, bool)>| out.push((x, false));
    match &mut e.kind {
        Int(_) | Float(_) | Bool(_) | Str(_) | Local(_) | Move(_) | FuncRef(_) | Closure { .. } | Ref(_) | Mutate { .. } => {}
        Call { args, .. } | Builtin { args, .. } | Method { args, .. } | Concat(args) | TupleLit(args) => {
            args.iter_mut().for_each(|a| one(a, out));
        }
        CallValue { callee, args, .. } => {
            one(callee, out);
            args.iter_mut().for_each(|a| one(a, out));
        }
        ListLit(elems) | SetLit(elems) => {
            for el in elems.iter_mut() {
                match el {
                    dat_ir::Elem::One(x) => out.push((x, false)),
                    dat_ir::Elem::Spread(x) => out.push((x, true)),
                }
            }
        }
        MapLit(entries) => {
            for en in entries.iter_mut() {
                match en {
                    dat_ir::Entry::Pair(k, v) => {
                        out.push((k, false));
                        out.push((v, false));
                    }
                    dat_ir::Entry::Spread(x) => out.push((x, true)),
                }
            }
        }
        Index { base: lhs, index: rhs, .. }
        | IntArith { lhs, rhs, .. }
        | FloatArith { lhs, rhs, .. }
        | Compare { lhs, rhs, .. }
        | And(lhs, rhs)
        | Or(lhs, rhs) => {
            one(lhs, out);
            one(rhs, out);
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
        | Fail { message: a, .. } => one(a, out),
        // Only one branch runs, but walking them in a row only keeps alive,
        // in the first, a value both use: it gives up a move, nothing more.
        Cond { cond, then, els } => {
            one(cond, out);
            one(then, out);
            one(els, out);
        }
        Assert(a) => {
            match &mut a.cond {
                dat_ir::AssertCond::Plain(c) => one(c, out),
                dat_ir::AssertCond::Compare { lhs, rhs, .. } => {
                    one(lhs, out);
                    one(rhs, out);
                }
            }
            if let Some(m) = &mut a.message {
                one(m, out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat_ir::{Function, Loc, Local, Types};

    fn local(n: u32) -> Expr {
        Expr { kind: ExprKind::Local(LocalId(n)), ty: Type::Str }
    }

    fn function(locals: usize, body: Block) -> Program {
        let f = Function {
            name: "f".into(),
            params: 0,
            captures: Vec::new(),
            defaults: Vec::new(),
            ret: Type::Void,
            locals: (0..locals).map(|i| Local { name: format!("x{i}"), ty: Type::Str, reassigned: true, by_ref: false }).collect(),
            body,
            recursive: false,
            loc: Loc { line: 1, col: 1 },
        };
        Program { file: "t.dat".into(), functions: vec![f], tests: Vec::new(), main: None, types: Types::default() }
    }

    fn moves(p: &Program) -> String {
        format!("{:?}", p.functions[0].body).matches("Move").count().to_string()
    }

    #[test]
    fn last_use_moves_and_a_loop_keeps_what_the_next_turn_reads() {
        // let b = a; let c = b;  → both reads are last uses.
        let mut p = function(3, vec![Stmt::Let(LocalId(1), local(0)), Stmt::Let(LocalId(2), local(1))]);
        run(&mut p);
        assert_eq!(moves(&p), "2");
        // while (true) { let b = a; } → `a` is read again on the next turn.
        let looped = Stmt::Loop { cond: None, body: vec![Stmt::Let(LocalId(1), local(0))], step: Vec::new() };
        let mut p = function(2, vec![looped]);
        run(&mut p);
        assert_eq!(moves(&p), "0");
        // while (true) { b = a; a = b; } → each read is followed by an assignment.
        let at = Loc { line: 1, col: 1 };
        let swap = vec![Stmt::Set(LocalId(1), local(0), at), Stmt::Set(LocalId(0), local(1), at)];
        let mut p = function(2, vec![Stmt::Loop { cond: None, body: swap, step: Vec::new() }]);
        run(&mut p);
        assert_eq!(moves(&p), "2");
        // print(a): `print` only looks at it.
        let print = Stmt::Expr(Expr { kind: ExprKind::Print(Box::new(local(0))), ty: Type::Void });
        let mut p = function(1, vec![print]);
        run(&mut p);
        assert_eq!(moves(&p), "0");
    }

    #[test]
    fn a_try_keeps_what_its_catch_reads() {
        let body = vec![Stmt::Let(LocalId(1), local(0))];
        let catches = vec![dat_ir::Catch { filter: dat_ir::CatchFilter::All, bind: None, body: vec![Stmt::Let(LocalId(2), local(0))] }];
        let mut p = function(3, vec![Stmt::Try { body, catches }]);
        run(&mut p);
        // Only the read in the `catch` is a last use.
        assert_eq!(moves(&p), "1");
    }
}
