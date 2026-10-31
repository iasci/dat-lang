//! E0657 (§06): one call passes the same value as `mut` twice, or a value
//! and a part of it. Two parts of one variable are fine when they're
//! visibly different, as `swap(mut xs[0], mut xs[2])`. When only the
//! values at run time can tell, as `swap(mut xs[i], mut xs[j])`, the fix
//! passes copies and stores them back after the call.

use super::Body;
use crate::codes;
use dat_diag::{Applicability, Span};
use dat_ir as ir;

/// A `mut` argument of the call being checked.
pub(super) struct MutArg {
    pub place: ir::Place,
    /// The argument, without `mut`.
    pub span: Span,
    /// Its parameter's name.
    pub param: String,
}

/// The overlaps found among one call's `mut` arguments.
#[derive(Default)]
pub(super) struct Overlaps {
    /// An argument is the same value as an earlier one, or a part of it.
    same: bool,
    /// The variables with two arguments that may be one part at run time,
    /// and the first error about them.
    maybe: Vec<ir::LocalId>,
    first_maybe: Option<usize>,
}

/// How two `mut` arguments with the same variable meet.
enum Overlap {
    /// One value, or one inside the other.
    Same,
    /// Indexes or keys that may be equal: `xs[i]` and `xs[j]`.
    Maybe(&'static str),
}

/// How the places `a` and `b`, of one variable, meet; `None` when they're
/// visibly different. A local in `changed` may hold another value by the
/// time the second place is worked out, so it says nothing.
fn overlap(a: &ir::Place, b: &ir::Place, changed: &[ir::LocalId]) -> Option<Overlap> {
    for (x, y) in a.steps.iter().zip(&b.steps) {
        match x.same_part(y) {
            Some(true) if !reads(x, changed) => {}
            Some(false) => return None,
            _ => return Some(Overlap::Maybe(if matches!(x, ir::Step::Key(..)) { "entry" } else { "element" })),
        }
    }
    Some(Overlap::Same)
}

/// Whether the operand of `step` is a local in `locals`.
fn reads(step: &ir::Step, locals: &[ir::LocalId]) -> bool {
    match step {
        ir::Step::Index(x, _) | ir::Step::Key(x, _) => {
            matches!(x.kind, ir::ExprKind::Local(l) | ir::ExprKind::Move(l) if locals.contains(&l))
        }
        ir::Step::Pos(_) => false,
    }
}

/// Whether `e` gives the same value when it's worked out again after the
/// call: literals, and locals not in `changed`, with integer arithmetic.
fn repeatable(e: &ir::Expr, changed: &[ir::LocalId]) -> bool {
    match &e.kind {
        ir::ExprKind::Int(_) | ir::ExprKind::Str(_) | ir::ExprKind::Bool(_) => true,
        ir::ExprKind::Local(l) | ir::ExprKind::Move(l) => !changed.contains(l),
        ir::ExprKind::IntArith { lhs, rhs, .. } => repeatable(lhs, changed) && repeatable(rhs, changed),
        ir::ExprKind::IntNeg { arg, .. } => repeatable(arg, changed),
        _ => false,
    }
}

/// Whether the code in `text`, outside `//` comments, uses `name` as a word.
fn mentions(text: &str, name: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    text.lines().map(|l| l.split_once("//").map_or(l, |(code, _)| code)).any(|code| {
        code.match_indices(name)
            .any(|(i, _)| !code[..i].chars().next_back().is_some_and(word) && !code[i + name.len()..].chars().next().is_some_and(word))
    })
}

impl Body<'_, '_> {
    /// E0657 for `arg` if it overlaps an earlier `mut` argument of its call
    /// in `passed`. `changes_from` is where the call's changes start in
    /// the function's list of them. Returns whether there was one.
    pub(super) fn mut_twice(&mut self, passed: &[MutArg], arg: &MutArg, root_name: &str, changes_from: usize, o: &mut Overlaps) -> bool {
        let changed = &self.f.changed[changes_from..];
        let Some((first, how)) =
            passed.iter().filter(|q| q.place.root == arg.place.root).find_map(|q| Some((q, overlap(&q.place, &arg.place, changed)?)))
        else {
            return false;
        };
        let first = self.text(first.span).to_string();
        let d = self.err(codes::MUT_TWICE, arg.span, format!("`{root_name}` is already passed as `mut` to this call"));
        let d = match how {
            Overlap::Same => {
                o.same = true;
                d.with_label(format!("as `{first}`; a call can change a value through one argument only"))
            }
            Overlap::Maybe(part) => {
                if !o.maybe.contains(&arg.place.root) {
                    o.maybe.push(arg.place.root);
                }
                o.first_maybe.get_or_insert(self.m.diags.len());
                d.with_label(format!("as `{first}`, which may be the same {part}"))
            }
        };
        self.push(d);
        true
    }

    /// The fix for arguments that may be one part at run time: each `mut`
    /// argument of their variables becomes a copy in a `let` before the
    /// statement, stored back after it. Only for a call that's a whole
    /// statement or a `let`'s value, whose indexes and keys give the same
    /// values when worked out again. `call` is the call, and `stmt` and
    /// `value` the statement and value it was in when its check began.
    pub(super) fn mut_twice_fix(
        &mut self,
        passed: &[MutArg],
        o: &Overlaps,
        call: Span,
        stmt: Span,
        value: Option<Span>,
        changes_from: usize,
    ) {
        let Some(at) = o.first_maybe else { return };
        if o.same || value != Some(call) || !self.starts_line(stmt.start) {
            return;
        }
        let text = self.src_text();
        let line_end = text[stmt.end as usize..].find('\n').map_or(text.len(), |i| stmt.end as usize + i);
        let rest = text[stmt.end as usize..line_end].trim();
        if !rest.is_empty() && !rest.starts_with("//") {
            return;
        }
        let changed = &self.f.changed[changes_from..];
        let args: Vec<&MutArg> = passed.iter().filter(|a| o.maybe.contains(&a.place.root)).collect();
        let simple = |a: &&MutArg| {
            a.place.steps.iter().all(|s| match s {
                ir::Step::Index(x, _) | ir::Step::Key(x, _) => repeatable(x, changed),
                ir::Step::Pos(_) => true,
            })
        };
        if !args.iter().all(simple) {
            return;
        }
        let end = self.block_ends.last().map_or(text.len(), |e| *e as usize);
        let after = &text[stmt.start as usize..end];
        let mut names: Vec<String> = Vec::new();
        for a in &args {
            let free = |n: &str| self.lookup(n).is_none() && !names.iter().any(|t| t == n) && !mentions(after, n);
            let name = if free(&a.param) {
                a.param.clone()
            } else {
                (2..).map(|k| format!("{}{k}", a.param)).find(|n| free(n)).unwrap_or_default()
            };
            names.push(name);
        }
        let indent = self.indent_at(stmt.start);
        let mut before = String::new();
        let mut back = String::new();
        let mut edits = Vec::new();
        for (a, name) in args.iter().zip(&names) {
            let place = self.text(a.span);
            before.push_str(&format!("let {name} = {place};\n{indent}"));
            back.push_str(&format!("\n{indent}{place} = {name};"));
            edits.push(self.edit(a.span, name.clone()));
        }
        edits.insert(0, self.insert(stmt.start, before));
        edits.push(self.insert(line_end as u32, back));
        let d = self.m.diags[at].clone().with_fix("pass copies, and store them back after the call", Applicability::Review, edits);
        self.m.diags[at] = d;
    }
}
