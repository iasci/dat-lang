//! The debug check of the liveness pass's marks (`DAT_DEBUG_CHECKS=1`): no
//! variable is used after it was moved, and no map or list is used while a
//! value taken out of it isn't back. Both run modes execute the same marks,
//! so checking the IR once checks both.
//!
//! It walks each function forward, keeping the variables that may have
//! been moved on some path and the take-and-restores that are open. Unlike
//! the pass, it follows both arms of a branch and every path an error can
//! take: inside a `try`, any point of the body can go on in a `catch`.

use dat_ir::{Block, Expr, ExprKind, Function, Iter, LocalId, Method, Program, Step, Stmt};

/// What's wrong with `p`'s marks, one line each; empty when nothing is.
pub fn verify(p: &Program) -> Vec<String> {
    let mut problems = Vec::new();
    for f in &p.functions {
        let mut v = Verify { f, problems: Vec::new(), loops: Vec::new(), tries: Vec::new() };
        v.block(&f.body, Some(State::default()));
        for m in v.problems {
            let line = format!("in `{}`: {m}", f.name);
            if !problems.contains(&line) {
                problems.push(line);
            }
        }
    }
    problems
}

/// What may have happened on some path to a point.
#[derive(Clone, Default, PartialEq)]
struct State {
    moved: Vec<LocalId>,
    /// `(root, v)`: `let v` took a value out of `root`, not yet put back.
    taken: Vec<(LocalId, LocalId)>,
}

impl State {
    fn join(&mut self, other: &State) {
        for l in &other.moved {
            if !self.moved.contains(l) {
                self.moved.push(*l);
            }
        }
        for t in &other.taken {
            if !self.taken.contains(t) {
                self.taken.push(*t);
            }
        }
    }
}

fn join(a: Option<State>, b: Option<State>) -> Option<State> {
    match (a, b) {
        (Some(mut a), Some(b)) => {
            a.join(&b);
            Some(a)
        }
        (a, b) => a.or(b),
    }
}

struct Verify<'f> {
    f: &'f Function,
    problems: Vec<String>,
    /// For each loop the code is in: the states at its `break`s and `continue`s.
    loops: Vec<(Option<State>, Option<State>)>,
    /// For each `try` the code is in: every state its body passed through.
    tries: Vec<Option<State>>,
}

impl Verify<'_> {
    fn name(&self, l: LocalId) -> &str {
        &self.f.locals[l.0 as usize].name
    }

    /// A point that can throw: a `catch` can start from here.
    fn may_throw(&mut self, s: &State) {
        for t in &mut self.tries {
            *t = join(t.take(), Some(s.clone()));
        }
    }

    /// A use of `l`.
    fn read(&mut self, s: &State, l: LocalId) {
        if s.moved.contains(&l) {
            self.problems.push(format!("`{}` is used after it was moved", self.name(l)));
        }
        if s.taken.iter().any(|(root, _)| *root == l) {
            self.problems.push(format!("`{}` is used while a value taken out of it isn't back", self.name(l)));
        }
    }

    fn block(&mut self, b: &Block, mut s: Option<State>) -> Option<State> {
        for stmt in b {
            let cur = s?;
            s = self.stmt(stmt, cur);
        }
        s
    }

    fn stmt(&mut self, stmt: &Stmt, mut s: State) -> Option<State> {
        match stmt {
            Stmt::Let(l, e) | Stmt::Set(l, e, _) => {
                let mut s = self.expr(e, s)?;
                if s.taken.iter().any(|(root, _)| root == l) {
                    self.problems.push(format!("`{}` is assigned while a value taken out of it isn't back", self.name(*l)));
                }
                s.moved.retain(|m| m != l);
                if let ExprKind::Mutate { op: Method::Take | Method::MapTake, place, .. } = &e.kind {
                    if self.f.locals[place.root.0 as usize].by_ref {
                        self.problems.push(format!("a value is taken out of the `mut` parameter `{}`", self.name(place.root)));
                    }
                    s.taken.push((place.root, *l));
                }
                Some(s)
            }
            Stmt::Store(place, e) => {
                s = self.steps(&place.steps, s)?;
                let restore = match (&place.steps[..], &e.kind) {
                    ([Step::Index(..) | Step::Key(..)], ExprKind::Move(v)) => s.taken.iter().position(|t| *t == (place.root, *v)),
                    _ => None,
                };
                let mut s = self.expr(e, s)?;
                match restore {
                    Some(i) => {
                        s.taken.remove(i);
                    }
                    None => self.read(&s, place.root),
                }
                self.may_throw(&s);
                Some(s)
            }
            Stmt::Expr(e) => self.expr(e, s),
            Stmt::If(c, a, b) => {
                let s = self.expr(c, s)?;
                let a = self.block(a, Some(s.clone()));
                let b = self.block(b, Some(s));
                join(a, b)
            }
            Stmt::Loop { cond, body, step } => {
                let entry = s;
                let mut head = entry.clone();
                loop {
                    self.loops.push((None, None));
                    let after_cond = match cond {
                        Some(c) => self.expr(c, head.clone()),
                        None => Some(head.clone()),
                    };
                    let end = self.block(body, after_cond.clone());
                    let (breaks, continues) = self.loops.pop().expect("pushed above");
                    let back = self.block(step, join(end, continues));
                    let mut next = entry.clone();
                    if let Some(b) = &back {
                        next.join(b);
                    }
                    if next == head {
                        let out = if cond.is_some() { after_cond } else { None };
                        return join(out, breaks);
                    }
                    head = next;
                }
            }
            Stmt::Break => {
                let lp = self.loops.last_mut().expect("`break` is inside a loop");
                lp.0 = join(lp.0.take(), Some(s));
                None
            }
            Stmt::Continue => {
                let lp = self.loops.last_mut().expect("`continue` is inside a loop");
                lp.1 = join(lp.1.take(), Some(s));
                None
            }
            Stmt::Return(e) => {
                if let Some(e) = e {
                    self.expr(e, s)?;
                }
                None
            }
            Stmt::Throw(e, _) => {
                let s = self.expr(e, s)?;
                self.may_throw(&s);
                None
            }
            Stmt::Try { body, catches } => {
                self.tries.push(Some(s.clone()));
                let end = self.block(body, Some(s));
                let caught = self.tries.pop().expect("pushed above");
                let mut out = end;
                for c in catches {
                    let mut from = caught.clone();
                    if let (Some(st), Some(b)) = (&mut from, c.bind) {
                        st.moved.retain(|m| *m != b);
                    }
                    out = join(out, self.block(&c.body, from));
                }
                out
            }
            Stmt::ForOf { var, iter, body } => {
                let entry = match iter {
                    Iter::Range { start, end, step, .. } => {
                        let s = self.expr(start, s)?;
                        let s = self.expr(end, s)?;
                        self.expr(step, s)?
                    }
                    Iter::Each(e) => self.expr(e, s)?,
                };
                self.each_turn(*var, body, entry)
            }
            Stmt::ForMut { var, list, body, .. } => {
                let s = self.steps(&list.steps, s)?;
                self.read(&s, list.root);
                self.each_turn(*var, body, s)
            }
        }
    }

    /// A `for...of` body, which runs any number of times, each turn with
    /// `var` assigned first.
    fn each_turn(&mut self, var: LocalId, body: &Block, entry: State) -> Option<State> {
        let mut head = entry.clone();
        loop {
            let mut start = head.clone();
            start.moved.retain(|m| *m != var);
            self.loops.push((None, None));
            let end = self.block(body, Some(start));
            let (breaks, continues) = self.loops.pop().expect("pushed above");
            let mut next = entry.clone();
            if let Some(e) = join(end, continues) {
                next.join(&e);
            }
            if next == head {
                return join(Some(head), breaks);
            }
            head = next;
        }
    }

    /// The index and key operands of a place, in order.
    fn steps(&mut self, steps: &[Step], mut s: State) -> Option<State> {
        for st in steps {
            if let Step::Index(x, _) | Step::Key(x, _) = st {
                s = self.expr(x, s)?;
            }
        }
        Some(s)
    }

    /// `e` run from `s`: the state after it, or `None` if it never finishes.
    fn expr(&mut self, e: &Expr, s: State) -> Option<State> {
        let s = match &e.kind {
            ExprKind::Local(l) => {
                self.read(&s, *l);
                s
            }
            ExprKind::Move(l) => {
                self.read(&s, *l);
                if self.f.locals[l.0 as usize].by_ref {
                    self.problems.push(format!("the `mut` variable `{}` is moved", self.name(*l)));
                }
                let mut s = s;
                if !s.moved.contains(l) {
                    s.moved.push(*l);
                }
                s
            }
            ExprKind::Closure { captures, .. } => {
                for c in captures {
                    self.read(&s, *c);
                }
                s
            }
            ExprKind::Ref(place) => {
                let s = self.steps(&place.steps, s)?;
                self.read(&s, place.root);
                s
            }
            ExprKind::Mutate { place, args, .. } => {
                let mut s = self.steps(&place.steps, s)?;
                for a in args {
                    s = self.expr(a, s)?;
                }
                self.read(&s, place.root);
                s
            }
            // Only one arm runs.
            ExprKind::Cond { cond, then, els } => {
                let s = self.expr(cond, s)?;
                let a = self.expr(then, s.clone());
                let b = self.expr(els, s);
                join(a, b)?
            }
            ExprKind::And(lhs, rhs) | ExprKind::Or(lhs, rhs) => {
                let s = self.expr(lhs, s)?;
                let r = self.expr(rhs, s.clone());
                join(Some(s), r)?
            }
            _ => {
                let mut cur = Some(s);
                dat_ir::each_child(e, &mut |c| {
                    if let Some(st) = cur.take() {
                        cur = self.expr(c, st);
                    }
                });
                cur?
            }
        };
        if e.ty == dat_ir::Type::Never {
            self.may_throw(&s);
            return None;
        }
        // Any node can throw, if only by running out of stack in a call.
        self.may_throw(&s);
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dat_ir::{Loc, Local, Type, Types};

    fn program(locals: usize, body: Block) -> Program {
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

    fn e(kind: ExprKind) -> Expr {
        Expr { kind, ty: Type::Str }
    }

    #[test]
    fn a_use_after_a_move_is_found_on_any_path() {
        let at = Loc { line: 1, col: 1 };
        // let a = b (moved); let c = b;
        let p =
            program(3, vec![Stmt::Let(LocalId(0), e(ExprKind::Move(LocalId(1)))), Stmt::Let(LocalId(2), e(ExprKind::Local(LocalId(1))))]);
        assert_eq!(verify(&p), vec!["in `f`: `x1` is used after it was moved"]);
        // Reassigned in between: fine.
        let p = program(
            3,
            vec![
                Stmt::Let(LocalId(0), e(ExprKind::Move(LocalId(1)))),
                Stmt::Set(LocalId(1), e(ExprKind::Str("x".into())), at),
                Stmt::Let(LocalId(2), e(ExprKind::Local(LocalId(1)))),
            ],
        );
        assert!(verify(&p).is_empty());
        // Moved in a `try`, read in its `catch`.
        let catches = vec![dat_ir::Catch {
            filter: dat_ir::CatchFilter::All,
            bind: None,
            body: vec![Stmt::Let(LocalId(2), e(ExprKind::Local(LocalId(1))))],
        }];
        let body = vec![Stmt::Let(LocalId(0), e(ExprKind::Move(LocalId(1))))];
        let p = program(3, vec![Stmt::Try { body, catches }]);
        assert_eq!(verify(&p), vec!["in `f`: `x1` is used after it was moved"]);
        // Moved in a loop that reads it again on the next turn.
        let body = vec![Stmt::Let(LocalId(0), e(ExprKind::Move(LocalId(1))))];
        let p = program(2, vec![Stmt::Loop { cond: None, body, step: Vec::new() }]);
        assert_eq!(verify(&p), vec!["in `f`: `x1` is used after it was moved"]);
    }
}
