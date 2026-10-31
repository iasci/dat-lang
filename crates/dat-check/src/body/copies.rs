//! E0202 (§02): a `let` that starts as a copy of a variable or an element
//! of one, is changed, and is never read. Code written with references in
//! mind changes the copy and expects the original to change.

use super::Body;
use crate::codes;
use dat_diag::{Applicability, Span};
use dat_ir as ir;

/// A `let` that holds a copy of something the code could change directly.
pub(super) struct CopyOf {
    pub local: ir::LocalId,
    pub name: String,
    /// The original, as written: `user.tags`, `grid[0]`, `xs`.
    pub source: String,
    /// `for (let x of xs)`: each element of `xs`, and the `let` keyword,
    /// which `mut` replaces when `xs` is a list the loop can change.
    pub loop_let: Option<Option<Span>>,
    /// Where it's changed: the variable's name at the root of each change.
    pub changes: Vec<Span>,
    /// It's read, passed on or reassigned somewhere, so the copy is used.
    pub used: bool,
}

impl Body<'_, '_> {
    /// Notes that the `let` local `local` starts as a copy of `source`.
    pub(super) fn note_copy(&mut self, local: ir::LocalId, name: &str, source: String, loop_let: Option<Option<Span>>) {
        self.f.copies.push(CopyOf { local, name: name.to_string(), source, loop_let, changes: Vec::new(), used: false });
    }

    /// A read of `local`, or a use that may read it.
    pub(super) fn copy_used(&mut self, local: ir::LocalId) {
        if let Some(c) = self.f.copies.iter_mut().find(|c| c.local == local) {
            c.used = true;
        }
    }

    /// A change of `local`, in place, at `root`.
    pub(super) fn copy_changed(&mut self, local: ir::LocalId, root: Span) {
        if let Some(c) = self.f.copies.iter_mut().find(|c| c.local == local) {
            c.changes.push(root);
        }
    }

    /// E0202 for each copy of the function just checked that's changed
    /// and never used. The fix changes the original instead.
    pub(super) fn report_copies(&mut self) {
        for c in std::mem::take(&mut self.f.copies) {
            if c.used || c.changes.is_empty() {
                continue;
            }
            let first = c.changes[0];
            let mut d = self.err(codes::COPY_UNUSED, first, format!("`{}` is modified but never used afterwards", c.name));
            match c.loop_let {
                Some(kw) => {
                    d = d.with_label(format!("it's a copy of an element of `{}`; a `mut` loop variable changes the elements", c.source));
                    if let Some(kw) = kw {
                        d = d.with_fix("loop with a `mut` variable", Applicability::Review, vec![self.edit(kw, "mut")]);
                    }
                }
                None => {
                    d = d.with_label(format!("it's a copy of `{}`; change `{}` directly", c.source, c.source));
                    let edits = c.changes.iter().map(|s| self.edit(*s, c.source.clone())).collect();
                    d = d.with_fix(format!("change `{}`", c.source), Applicability::Review, edits);
                }
            }
            self.push(d);
        }
    }
}
