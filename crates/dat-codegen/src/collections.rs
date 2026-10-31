//! Lists, sets, maps, tuples, places and function values in generated Rust.
//!
//! Every collection operation calls its one implementation in dat-rt.
//! Callbacks are passed to dat-rt's methods as Rust closures, so the
//! compiler can inline them; they return `Ok` because a dat error unwinds,
//! and `dat_rt::done` takes the result. A place's index and key operands
//! are worked out into locals first, so the `&mut` borrow of the place
//! never overlaps them.

use crate::{Ctx, Gen, changes, is_copy};
use dat_ir::{self as ir, ExprKind, Method, Type};
use std::fmt::Write as _;

/// The argument of a lent parameter, from [`Gen::lent_arg`].
pub(crate) enum LentArg {
    /// A `dat_rt::FnRefN` as it is.
    Ready(String),
    /// A Rust closure or function with `N` parameters, to borrow.
    Closure(usize, String),
    /// A shared function value made for the call, to lend.
    Shared(String),
}

impl LentArg {
    /// The argument, made right where it's passed.
    pub(crate) fn inline(self) -> String {
        match self {
            LentArg::Ready(s) => s,
            LentArg::Closure(n, v) => format!("dat_rt::FnRef{n}::new(&{v})"),
            LentArg::Shared(v) => format!("({v}).lend()"),
        }
    }

    /// The argument, made first into the local `name`, written to `s`.
    pub(crate) fn hoisted(self, name: &str, s: &mut String) -> String {
        match self {
            LentArg::Ready(v) => {
                let _ = write!(s, "let {name} = {v}; ");
                name.to_string()
            }
            LentArg::Closure(n, v) => {
                let _ = write!(s, "let {name} = {v}; ");
                format!("dat_rt::FnRef{n}::new(&{name})")
            }
            LentArg::Shared(v) => {
                let _ = write!(s, "let {name} = {v}; ");
                format!("{name}.lend()")
            }
        }
    }
}

impl Gen<'_> {
    /// An expression that can be borrowed with `&`: a local or a part of
    /// one without copying it, or else the value itself.
    pub(crate) fn reff(&mut self, f: &ir::Function, e: &ir::Expr) -> String {
        match &e.kind {
            ExprKind::Local(l) | ExprKind::Move(l) => self.lvalue(f, *l),
            ExprKind::Index { base, index, loc } if changes(index) => {
                // `xs[xs.pop()]` reads the list as it was before the index.
                let b = self.expr(f, base);
                let i = self.expr(f, index);
                let (m, l) = self.site(*loc);
                let ty = if self.p.types.map_types(base.ty).is_some() { "Map" } else { "List" };
                let amp = if ty == "Map" { "&" } else { "" };
                format!("({{ let __b = {b}; let __i = {i}; {m}dat_rt::{ty}::get(&__b, {amp}__i, {l}).clone() }})")
            }
            ExprKind::Index { base, index, loc } => {
                if let ExprKind::Local(x) = base.kind
                    && let Some((e, kind)) = self.edit_of(x)
                {
                    let i = self.expr(f, index);
                    let (m, l) = self.site(*loc);
                    return format!("(*{m}dat_rt::{kind}::get(&{e}, {i}, {l}))");
                }
                let b = self.reff(f, base);
                let i = self.expr(f, index);
                let (m, l) = self.site(*loc);
                if self.p.types.map_types(base.ty).is_some() {
                    format!("(*{m}dat_rt::Map::get(&{b}, &{i}, {l}))")
                } else {
                    format!("(*{m}dat_rt::List::get(&{b}, {i}, {l}))")
                }
            }
            ExprKind::TupleGet(base, n) => format!("{}.{n}", self.reff(f, base)),
            _ => format!("({})", self.expr(f, e)),
        }
    }

    /// Moves the place's index and key operands into locals named with
    /// `prefix`, written to `s`, and returns the place that reads them.
    pub(crate) fn hoisted(&mut self, f: &ir::Function, place: &ir::Place, s: &mut String, prefix: &str) -> ir::Place {
        let mut p = place.clone();
        for (i, step) in p.steps.iter_mut().enumerate() {
            if let ir::Step::Index(x, _) | ir::Step::Key(x, _) = step {
                let v = self.expr(f, x);
                let name = format!("{prefix}{i}");
                let _ = write!(s, "let {name} = {v}; ");
                // A local with no slot: the name stands for itself.
                *x = ir::Expr { kind: ExprKind::Str(name), ty: Type::Void };
            }
        }
        p
    }

    /// A step operand after [`Self::hoisted`]: the name of its local.
    fn operand(&mut self, f: &ir::Function, x: &ir::Expr) -> String {
        match (&x.kind, x.ty) {
            (ExprKind::Str(name), Type::Void) => name.clone(),
            _ => self.expr(f, x),
        }
    }

    /// A `&mut` to the value at `place`, whose operands are hoisted. A list
    /// element out of range or a missing key throws there.
    pub(crate) fn place_mut(&mut self, f: &ir::Function, place: &ir::Place) -> String {
        self.place_mut_to(f, place, place.steps.len())
    }

    /// A `&mut` to the value after the first `n` steps of `place`. For a
    /// list a loop changes through a `ListMut`, that's the `ListMut`, or
    /// an element it hands out.
    fn place_mut_to(&mut self, f: &ir::Function, place: &ir::Place, n: usize) -> String {
        if let Some((e, kind)) = self.edit_of(place.root) {
            let e = format!("&mut {e}");
            return match place.steps[..n].split_first() {
                None => e,
                Some((ir::Step::Index(i, loc), rest)) => {
                    let i = self.operand(f, i);
                    let (m, l) = self.site(*loc);
                    self.steps_mut(f, format!("{m}dat_rt::{kind}::get_mut({e}, {i}, {l})"), rest)
                }
                Some(_) => unreachable!("a list's first step is an index"),
            };
        }
        let root = self.root_mut(f, place.root);
        self.steps_mut(f, root, &place.steps[..n])
    }

    /// The `ListMut` or `ListSlice` a loop changes the list `l` through,
    /// if any, and which of the two it is.
    fn edit_of(&self, l: ir::LocalId) -> Option<(String, &'static str)> {
        self.edits.iter().find(|e| e.0 == l).map(|e| (e.1.clone(), e.2))
    }

    /// A `&mut` to the local `l`.
    pub(crate) fn root_mut(&self, f: &ir::Function, l: ir::LocalId) -> String {
        let name = self.local(f, l);
        if self.holds_ref(f, l) { format!("&mut *{name}") } else { format!("&mut {name}") }
    }

    /// A `&mut` to the value `steps` past the `&mut` `s`.
    fn steps_mut(&mut self, f: &ir::Function, mut s: String, steps: &[ir::Step]) -> String {
        for step in steps {
            s = match step {
                ir::Step::Index(i, loc) => {
                    let i = self.operand(f, i);
                    let (m, l) = self.site(*loc);
                    format!("{m}dat_rt::List::get_mut({s}, {i}, {l})")
                }
                ir::Step::Key(k, loc) => {
                    let k = self.operand(f, k);
                    let (m, l) = self.site(*loc);
                    format!("{m}dat_rt::Map::get_mut({s}, &{k}, {l})")
                }
                ir::Step::Pos(n) => format!("&mut ({s}).{n}"),
            };
        }
        s
    }

    /// The `mut` arguments of a call where two share a variable, as
    /// `swap(mut xs[0], mut xs[2])`, which the checker proved are different
    /// parts. Each is reached once, in order, so a missing element throws
    /// where it would alone; then each variable is split where its places
    /// part, and argument `i` is lent as `__r{i}`. `places` holds each
    /// argument's index, its place with operands hoisted, and the place.
    pub(crate) fn disjoint_refs(&mut self, f: &ir::Function, places: &[(usize, ir::Place, &ir::Place)], s: &mut String) {
        let throws = |p: &ir::Place| p.steps.iter().any(|s| !matches!(s, ir::Step::Pos(_)));
        for (_, p, _) in places.iter().filter(|(_, p, _)| throws(p)) {
            let r = self.place_mut(f, p);
            let _ = write!(s, "let _ = {r}; ");
        }
        let mut roots: Vec<ir::LocalId> = Vec::new();
        for (_, p, _) in places {
            if !roots.contains(&p.root) {
                roots.push(p.root);
            }
        }
        let mut n = 0;
        for root in roots {
            let group: Vec<_> = places.iter().filter(|(_, p, _)| p.root == root).collect();
            let base = self.root_mut(f, root);
            self.split(f, &group, 0, base, &mut n, s);
        }
    }

    /// Lends each place of `group`, whose first `depth` steps are the same
    /// and reach the `&mut` `base`.
    fn split(
        &mut self,
        f: &ir::Function,
        group: &[&(usize, ir::Place, &ir::Place)],
        depth: usize,
        base: String,
        n: &mut usize,
        s: &mut String,
    ) {
        if let [(i, p, _)] = group {
            let r = self.steps_mut(f, base, &p.steps[depth..]);
            let _ = write!(s, "let __r{i} = {r}; ");
            return;
        }
        // The places by their step here, in order.
        let mut parts: Vec<Vec<&(usize, ir::Place, &ir::Place)>> = Vec::new();
        for &x in group {
            let step = &x.2.steps[depth];
            match parts.iter_mut().find(|g| g[0].2.steps[depth].same_part(step) == Some(true)) {
                Some(g) => g.push(x),
                None => parts.push(vec![x]),
            }
        }
        let first = &group[0].1;
        if let [one] = parts.as_slice() {
            let base = self.steps_mut(f, base, &first.steps[depth..=depth]);
            return self.split(f, one, depth + 1, base, n, s);
        }
        let g = format!("__g{n}");
        *n += 1;
        // A place that ends here is lent as it is.
        let names: Vec<String> = parts
            .iter()
            .enumerate()
            .map(|(j, part)| match part.as_slice() {
                [(i, p, _)] if p.steps.len() == depth + 1 => format!("__r{i}"),
                _ => format!("{g}_{j}"),
            })
            .collect();
        match &first.steps[depth] {
            ir::Step::Pos(_) => {
                let _ = write!(s, "let {g} = {base}; ");
                for (name, part) in names.iter().zip(&parts) {
                    if let ir::Step::Pos(k) = part[0].1.steps[depth] {
                        let _ = write!(s, "let {name} = &mut {g}.{k}; ");
                    }
                }
            }
            ir::Step::Index(..) | ir::Step::Key(..) => {
                let is_key = matches!(first.steps[depth], ir::Step::Key(..));
                let ops: Vec<String> = parts
                    .iter()
                    .map(|part| match &part[0].1.steps[depth] {
                        ir::Step::Index(x, _) | ir::Step::Key(x, _) => {
                            let x = self.operand(f, x);
                            if is_key { format!("&{x}") } else { x }
                        }
                        ir::Step::Pos(_) => unreachable!("one variable's places have the same kind of step here"),
                    })
                    .collect();
                let ty = if is_key { "Map" } else { "List" };
                let _ = write!(s, "let [{}] = dat_rt::{ty}::disjoint_mut({base}, [{}]); ", names.join(", "), ops.join(", "));
            }
        }
        for (name, part) in names.into_iter().zip(&parts) {
            if name.starts_with("__g") {
                self.split(f, part, depth + 1, name, n, s);
            }
        }
    }

    /// `place = value`. A last `Key` step inserts the key if it's missing.
    pub(crate) fn store(&mut self, f: &ir::Function, place: &ir::Place, value: &ir::Expr, ind: &str, out: &mut String) {
        let mut s = String::new();
        let p = self.hoisted(f, place, &mut s, "__p");
        let v = self.expr(f, value);
        let _ = write!(s, "let __v = {v}; ");
        match p.steps.last() {
            Some(ir::Step::Key(k, _)) => {
                let k = self.operand(f, k);
                let map = self.place_mut_to(f, &p, p.steps.len() - 1);
                let _ = write!(s, "dat_rt::Map::insert({map}, {k}, __v);");
            }
            _ => {
                let target = self.place_mut(f, &p);
                let _ = write!(s, "*{target} = __v;");
            }
        }
        let _ = writeln!(out, "{ind}{{ {s} }}");
    }

    /// A function value: the dat function `func`, with an arrow function's
    /// copies of what it captures, which each call passes on.
    pub(crate) fn func_value(&mut self, _f: &ir::Function, ty: Type, func: ir::FuncId, captures: &[String]) -> String {
        let (params, _) = self.p.types.func_sig(ty).expect("a function value has a function type");
        let n = params.len();
        let name = crate::fn_name(self.p, func);
        let args = self.lent_to(func, (0..n).map(|i| format!("__p{i}")).collect());
        let typed: Vec<String> = params.iter().enumerate().map(|(i, t)| format!("__p{i}: {}", self.ty(*t))).collect();
        let mut s = String::from("{ ");
        let mut call_args = args;
        for (i, c) in captures.iter().enumerate() {
            let _ = write!(s, "let __c{i} = {c}; ");
            call_args.push(format!("__c{i}.clone()"));
        }
        let _ = write!(s, "dat_rt::Func{n}::new(move |{}| {name}({})) }}", typed.join(", "), call_args.join(", "));
        s
    }

    /// `args`, shared function values for `func`'s parameters in order,
    /// with each lent where `func` takes it lent.
    fn lent_to(&self, func: ir::FuncId, args: Vec<String>) -> Vec<String> {
        args.into_iter()
            .enumerate()
            .map(|(k, a)| if self.lent.local(func, ir::LocalId(k as u32)) { format!("({a}).lend()") } else { a })
            .collect()
    }

    /// The argument `a` of a lent parameter. An arrow function is made as a
    /// plain Rust closure, which the call borrows: it copies what it
    /// captures when it's made, as a shared one would, and nothing is
    /// allocated.
    pub(crate) fn lent_arg(&mut self, f: &ir::Function, a: &ir::Expr) -> LentArg {
        let n = self.p.types.func_sig(a.ty).expect("a lent argument is a function value").0.len();
        match &a.kind {
            ExprKind::Local(l) | ExprKind::Move(l) if self.is_lent(*l) => LentArg::Ready(self.lvalue(f, *l)),
            ExprKind::Local(l) | ExprKind::Move(l) => LentArg::Ready(format!("{}.lend()", self.lvalue(f, *l))),
            ExprKind::FuncRef(func) if (0..n).all(|k| !self.lent.local(*func, ir::LocalId(k as u32))) => {
                LentArg::Closure(n, crate::fn_name(self.p, *func))
            }
            ExprKind::FuncRef(func) => LentArg::Closure(n, self.closure(f, a.ty, *func, &[])),
            ExprKind::Closure { func, captures } => LentArg::Closure(n, self.closure(f, a.ty, *func, captures)),
            _ => LentArg::Shared(self.expr(f, a)),
        }
    }

    /// The dat function `func` as a Rust closure of the function type `ty`,
    /// with the copies of `captures` an arrow function takes. A copy is lent
    /// where the arrow takes it lent.
    fn closure(&mut self, f: &ir::Function, ty: Type, func: ir::FuncId, captures: &[ir::LocalId]) -> String {
        let (params, _) = self.p.types.func_sig(ty).expect("a function value has a function type");
        let name = crate::fn_name(self.p, func);
        let mut call_args = self.lent_to(func, (0..params.len()).map(|i| format!("__p{i}")).collect());
        let typed: Vec<String> = params.iter().enumerate().map(|(i, t)| format!("__p{i}: {}", self.ty(*t))).collect();
        if captures.is_empty() {
            return format!("|{}| {name}({})", typed.join(", "), call_args.join(", "));
        }
        let slots = self.p.func(func).captures.clone();
        let mut s = String::from("{ ");
        for (i, (c, slot)) in captures.iter().zip(slots).enumerate() {
            let v = if self.lent.local(func, slot) && !self.is_lent(*c) {
                format!("{}.lend()", self.lvalue(f, *c))
            } else {
                self.expr(f, &ir::Expr { kind: ExprKind::Local(*c), ty: f.locals[c.0 as usize].ty })
            };
            let _ = write!(s, "let __c{i} = {v}; ");
            call_args.push(format!("__c{i}.clone()"));
        }
        let _ = write!(s, "move |{}| {name}({}) }}", typed.join(", "), call_args.join(", "));
        s
    }

    /// A callback for a dat-rt method: a Rust closure that takes `params`
    /// (Rust patterns with types) and calls the dat function value `cb`
    /// with `args`, returning `Ok`. Setup code, such as a function value
    /// worked out once, goes into `pre`.
    fn callback(&mut self, f: &ir::Function, cb: &ir::Expr, params: &str, args: &[String], pre: &mut String) -> String {
        let call = self.callback_call(f, cb, args, pre);
        format!("|{params}| Ok({call})")
    }

    /// A callback that can't throw, as a closure that returns its result.
    fn total_callback(&mut self, f: &ir::Function, cb: &ir::Expr, params: &str, args: &[String], pre: &mut String) -> String {
        let call = self.callback_call(f, cb, args, pre);
        format!("|{params}| {call}")
    }

    /// A call of the callback `cb` with `args`, after its captures.
    fn callback_call(&mut self, f: &ir::Function, cb: &ir::Expr, args: &[String], pre: &mut String) -> String {
        match &cb.kind {
            ExprKind::Closure { func, captures } => {
                let mut all = args.to_vec();
                let slots = &self.p.func(*func).captures;
                for (i, (c, slot)) in captures.iter().zip(slots).enumerate() {
                    let local = &f.locals[c.0 as usize];
                    let name = format!("__cap{i}");
                    let v = self.lvalue(f, *c);
                    let _ = write!(pre, "let {name} = &{v}; ");
                    all.push(if is_copy(local.ty) {
                        format!("*{name}")
                    } else if self.lent.local(*func, *slot) && !self.is_lent(*c) {
                        format!("{name}.lend()")
                    } else {
                        format!("{name}.clone()")
                    });
                }
                format!("{}({})", crate::fn_name(self.p, *func), all.join(", "))
            }
            ExprKind::FuncRef(func) => format!("{}({})", crate::fn_name(self.p, *func), self.lent_to(*func, args.to_vec()).join(", ")),
            _ => {
                let v = self.reff(f, cb);
                let _ = write!(pre, "let __f = &{v}; ");
                format!("__f.call({})", args.join(", "))
            }
        }
    }

    /// The expression the arrow or named function `cb` returns, if its
    /// body is that one `return`, and the function.
    fn returned(&self, cb: &ir::Expr) -> Option<(&ir::Function, &ir::Expr)> {
        let id = match &cb.kind {
            ExprKind::Closure { func, .. } | ExprKind::FuncRef(func) => *func,
            _ => return None,
        };
        let g = self.p.func(id);
        match g.body.as_slice() {
            [ir::Stmt::Return(Some(e))] => Some((g, e)),
            _ => None,
        }
    }

    /// Whether the callback `cb` can't throw or print and costs little, so
    /// nothing can tell how often or in what order it's called. Generated
    /// code then sorts in place, and works a sort key out in each
    /// comparison. The copy report keeps the interpreter's way.
    fn total(&self, cb: &ir::Expr) -> bool {
        !self.copies && self.returned(cb).is_some_and(|(_, e)| total(e))
    }

    /// `(a, b) => a - b` on ints: `Some(false)`; `(a, b) => b - a`: `Some(true)`.
    fn difference(&self, cb: &ir::Expr) -> Option<bool> {
        let (g, e) = self.returned(cb).filter(|_| !self.copies)?;
        if g.params != 2 || g.locals[0].ty != Type::Int || g.locals[1].ty != Type::Int {
            return None;
        }
        match &e.kind {
            ExprKind::IntArith { op: ir::IntOp::Sub, lhs, rhs, .. } => match (&lhs.kind, &rhs.kind) {
                (ExprKind::Local(a), ExprKind::Local(b)) if (a.0, b.0) == (0, 1) => Some(false),
                (ExprKind::Local(a), ExprKind::Local(b)) if (a.0, b.0) == (1, 0) => Some(true),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether a callback's function type takes the element's index too.
    fn indexed(&self, cb: &ir::Expr) -> bool {
        self.p.types.func_sig(cb.ty).is_some_and(|(p, _)| p.len() == 2)
    }

    /// The common callback shape: the element, and the index if it takes it.
    fn element_callback(&mut self, f: &ir::Function, cb: &ir::Expr, pre: &mut String) -> String {
        let args = if self.indexed(cb) { vec!["__x.clone()".to_string(), "__i".to_string()] } else { vec!["__x.clone()".to_string()] };
        self.callback(f, cb, "__x, __i", &args, pre)
    }

    /// A method that doesn't change its receiver, `args[0]`.
    pub(crate) fn method(&mut self, f: &ir::Function, op: Method, args: &[ir::Expr], loc: ir::Loc) -> String {
        use Method::*;
        let mut pre = String::new();
        // The receiver, borrowed, and the other arguments, as values.
        let recv = |g: &mut Self, pre: &mut String| -> String {
            // A receiver an argument may change is copied first (§03: left to right).
            let r = if args[1..].iter().any(changes) { format!("({})", g.expr(f, &args[0])) } else { g.reff(f, &args[0]) };
            let _ = write!(pre, "let __r = &{r}; ");
            "__r".to_string()
        };
        let code = match op {
            InList | InSet | InMap => {
                // The item first (§03).
                let item = self.expr(f, &args[0]);
                let c = self.reff(f, &args[1]);
                let ty = match op {
                    InList => "List::includes",
                    InSet => "Set::has",
                    _ => "Map::has",
                };
                return format!("{{ let __item = {item}; dat_rt::{ty}(&{c}, &__item) }}");
            }
            ListLength | ListIsEmpty
                if let ExprKind::Local(x) = args[0].kind
                    && let Some((e, kind)) = self.edit_of(x) =>
            {
                return format!("dat_rt::{kind}::{}(&{e})", if op == ListLength { "len" } else { "is_empty" });
            }
            // `filter` on a list just made, such as `map`'s: it keeps what
            // stays in that list's buffer.
            ListFilter
                if !matches!(args[0].kind, ExprKind::Local(_) | ExprKind::Move(_) | ExprKind::Index { .. } | ExprKind::TupleGet(..)) =>
            {
                let v = self.expr(f, &args[0]);
                let _ = write!(pre, "let __o = {v}; ");
                let c = self.element_callback(f, &args[1], &mut pre);
                let (m, _) = self.site(loc);
                format!("dat_rt::done({m}dat_rt::List::try_filter_owned(__o, {c}))")
            }
            ListLength => format!("dat_rt::List::len(&{})", self.reff(f, &args[0])),
            SetSize => format!("dat_rt::Set::len(&{})", self.reff(f, &args[0])),
            MapSize => format!("dat_rt::Map::len(&{})", self.reff(f, &args[0])),
            Range => {
                let (a, b, c) = (self.expr(f, &args[0]), self.expr(f, &args[1]), self.expr(f, &args[2]));
                let (m, l) = self.site(loc);
                return format!("{m}dat_rt::list::range({a}, {b}, {c}, {l})");
            }
            StrSplit => {
                let (a, b) = (self.str_ref(f, &args[0]), self.str_ref(f, &args[1]));
                return format!("dat_rt::text::split({a}, {b})");
            }
            StrChars => return format!("dat_rt::text::chars({})", self.str_ref(f, &args[0])),
            MaxInts | MinInts | MaxFloats | MinFloats => {
                let r = self.reff(f, &args[0]);
                let name = match op {
                    MaxInts => "max_int",
                    MinInts => "min_int",
                    MaxFloats => "max_float",
                    _ => "min_float",
                };
                let (m, l) = self.site(loc);
                return format!("{m}dat_rt::num::{name}(&{r}, {l})");
            }
            ListCheckLen => {
                let r = self.reff(f, &args[0]);
                let n = self.expr(f, &args[1]);
                let (m, l) = self.site(loc);
                return format!("{m}dat_rt::list::check_len(dat_rt::List::len(&{r}), {n}, {l})");
            }
            _ => {
                let r = recv(self, &mut pre);
                let vals: Vec<String> =
                    args[1..].iter().map(|a| if op.takes_callback() { String::new() } else { self.expr(f, a) }).collect();
                let call = |_: &mut Self, name: &str, extra: &[String]| -> String {
                    let mut all = vec![r.clone()];
                    all.extend(extra.iter().cloned());
                    format!("{name}({})", all.join(", "))
                };
                match op {
                    ListIsEmpty => call(self, "dat_rt::List::is_empty", &[]),
                    ListIncludes => call(self, "dat_rt::List::includes", &[format!("&{}", vals[0])]),
                    ListIndexOf => call(self, "dat_rt::List::index_of", &[format!("&{}", vals[0])]),
                    ListLastIndexOf => call(self, "dat_rt::List::last_index_of", &[format!("&{}", vals[0])]),
                    ListAt => {
                        let (m, l) = self.site(loc);
                        format!("{m}dat_rt::List::at({r}, {}, {l}).clone()", vals[0])
                    }
                    ListSlice => call(self, "dat_rt::List::slice", &[vals[0].clone(), vals[1].clone()]),
                    ListConcat => call(self, "dat_rt::List::concat", &[format!("&{}", vals[0])]),
                    ListJoin => call(self, "dat_rt::List::join", &[format!("{}.as_str()", vals[0])]),
                    ListEntries => call(self, "dat_rt::List::entries", &[]),
                    ListFlat => call(self, "dat_rt::List::flat", &[]),
                    ListToSorted => call(self, "dat_rt::List::to_sorted", &[]),
                    ListToReversed => call(self, "dat_rt::List::to_reversed", &[]),
                    ListUnique => call(self, "dat_rt::List::unique", &[]),
                    ListZip => call(self, "dat_rt::List::zip", &[format!("&{}", vals[0])]),
                    SetIsEmpty => call(self, "dat_rt::Set::is_empty", &[]),
                    SetHas => call(self, "dat_rt::Set::has", &[format!("&{}", vals[0])]),
                    SetUnion => call(self, "dat_rt::Set::union", &[format!("&{}", vals[0])]),
                    SetIntersection => call(self, "dat_rt::Set::intersection", &[format!("&{}", vals[0])]),
                    SetDifference => call(self, "dat_rt::Set::difference", &[format!("&{}", vals[0])]),
                    SetSymmetricDifference => call(self, "dat_rt::Set::symmetric_difference", &[format!("&{}", vals[0])]),
                    SetIsSubsetOf => call(self, "dat_rt::Set::is_subset_of", &[format!("&{}", vals[0])]),
                    SetIsSupersetOf => call(self, "dat_rt::Set::is_superset_of", &[format!("&{}", vals[0])]),
                    SetIsDisjointFrom => call(self, "dat_rt::Set::is_disjoint_from", &[format!("&{}", vals[0])]),
                    MapIsEmpty => call(self, "dat_rt::Map::is_empty", &[]),
                    MapGet => call(self, "dat_rt::Map::get_or", &[format!("&{}", vals[0]), vals[1].clone()]),
                    MapHas => call(self, "dat_rt::Map::has", &[format!("&{}", vals[0])]),
                    MapKeys => call(self, "dat_rt::Map::keys", &[]),
                    MapValues => call(self, "dat_rt::Map::values", &[]),
                    MapEntries => call(self, "dat_rt::Map::entries", &[]),
                    ListToSortedWith if self.difference(&args[1]).is_some() => {
                        let desc = self.difference(&args[1]).unwrap_or_default();
                        let c = self.callback(f, &args[1], "__a, __b", &["__a.clone()".into(), "__b.clone()".into()], &mut pre);
                        let (m, _) = self.site(loc);
                        format!(
                            "{{ let mut __o = (*{r}).clone(); if !dat_rt::List::sort_by_difference(&mut __o, {desc}) {{ __o = dat_rt::done({m}dat_rt::List::try_to_sorted_by({r}, {c})); }} __o }}"
                        )
                    }
                    ListToSortedWith | ListToSortedBy if self.total(&args[1]) => {
                        let (params, call_args, rt): (&str, Vec<String>, &str) = if op == ListToSortedWith {
                            ("__a, __b", vec!["__a.clone()".into(), "__b.clone()".into()], "sort_by_total")
                        } else {
                            ("__x", vec!["__x.clone()".into()], "sort_by_total_key")
                        };
                        let c = self.total_callback(f, &args[1], params, &call_args, &mut pre);
                        format!("{{ let mut __o = (*{r}).clone(); dat_rt::List::{rt}(&mut __o, {c}); __o }}")
                    }
                    _ => {
                        // A method with a callback: a trace site, since the callback may throw.
                        let cb = &args[1];
                        let (m, _) = self.site(loc);
                        let rt = match op {
                            ListMap => "try_map",
                            ListFilter => "try_filter",
                            ListFlatMap => "try_flat_map",
                            ListForEach => "try_for_each",
                            ListFindIndex => "try_find_index",
                            ListSome => "try_some",
                            ListEvery => "try_every",
                            ListCount => "try_count",
                            _ => "",
                        };
                        let body = if !rt.is_empty() {
                            let c = self.element_callback(f, cb, &mut pre);
                            format!("{m}dat_rt::List::{rt}({r}, {c})")
                        } else {
                            match op {
                                ListReduce => {
                                    let init = self.expr(f, &args[2]);
                                    let c = self.callback(f, cb, "__acc, __x", &["__acc".into(), "__x.clone()".into()], &mut pre);
                                    format!("{m}dat_rt::List::try_reduce({r}, {init}, {c})")
                                }
                                ListFind => {
                                    // The callback first, as the interpreter evaluates them.
                                    let c = self.element_callback(f, cb, &mut pre);
                                    let fallback = self.expr(f, &args[2]);
                                    let _ = write!(pre, "let __fallback = {fallback}; ");
                                    format!("{m}dat_rt::List::try_find({r}, {c}, __fallback)")
                                }
                                ListToSortedWith => {
                                    let c = self.callback(f, cb, "__a, __b", &["__a.clone()".into(), "__b.clone()".into()], &mut pre);
                                    format!("{m}dat_rt::List::try_to_sorted_by({r}, {c})")
                                }
                                ListToSortedBy => {
                                    let c = self.callback(f, cb, "__x", &["__x.clone()".into()], &mut pre);
                                    format!("{m}dat_rt::List::try_to_sorted_by_key({r}, {c})")
                                }
                                ListGroupBy => {
                                    let c = self.callback(f, cb, "__x", &["__x.clone()".into()], &mut pre);
                                    format!("{m}dat_rt::List::try_group_by({r}, {c})")
                                }
                                other => unreachable!("not a method without a place: {other:?}"),
                            }
                        };
                        format!("dat_rt::done({body})")
                    }
                }
            }
        };
        format!("{{ {pre}{code} }}")
    }

    /// A method that changes the collection at `place`.
    pub(crate) fn mutate(&mut self, f: &ir::Function, op: Method, place: &ir::Place, args: &[ir::Expr], loc: ir::Loc) -> String {
        use Method::*;
        let borrow = std::mem::take(&mut self.borrow_take);
        let mut s = String::from("{ ");
        let p = self.hoisted(f, place, &mut s, "__p");
        let mut vals = Vec::new();
        for (i, a) in args.iter().enumerate() {
            if op.takes_callback() && i == 0 {
                vals.push(String::new());
                continue;
            }
            // A borrowing take makes a constant fallback only when the key
            // is missing: making it can't throw or print, so when is unseen.
            if borrow && op == MapTake && i == 1 && constant(a) {
                vals.push(self.expr(f, a));
                continue;
            }
            let v = self.expr(f, a);
            let _ = write!(s, "let __a{i} = {v}; ");
            vals.push(format!("__a{i}"));
        }
        let target = self.place_mut(f, &p);
        // The list itself, when a loop changes its length through a `ListMut`.
        let list = if p.steps.is_empty() && self.edit_of(p.root).is_some() { "ListMut" } else { "List" };
        let code = match op {
            ListPush => {
                let mut c = String::new();
                for v in &vals {
                    let _ = write!(c, "dat_rt::{list}::push(__t, {v}); ");
                }
                format!("let __t = {target}; {c}")
            }
            ListPushAll => format!("dat_rt::List::push_all({target}, &{})", vals[0]),
            ListUnshift => format!("dat_rt::List::unshift_all({target}, &{})", vals[0]),
            ListPop | ListShift | ListInsert | ListRemoveAt => {
                let name = match op {
                    ListPop => "pop",
                    ListShift => "shift",
                    ListInsert => "insert",
                    _ => "remove_at",
                };
                let (m, l) = self.site(loc);
                let mut all = vec![target];
                all.extend(vals.iter().cloned());
                all.push(l);
                format!("{m}dat_rt::{list}::{name}({})", all.join(", "))
            }
            ListRemove => format!("dat_rt::List::remove({target}, &{})", vals[0]),
            ListSort => format!("dat_rt::List::sort({target})"),
            ListReverse => format!("dat_rt::List::reverse({target})"),
            ListClear => format!("dat_rt::List::clear({target})"),
            ListSortWith if self.difference(&args[0]).is_some() => {
                let desc = self.difference(&args[0]).unwrap_or_default();
                let mut pre = String::new();
                let c = self.callback(f, &args[0], "__a, __b", &["__a.clone()".into(), "__b.clone()".into()], &mut pre);
                let (m, _) = self.site(loc);
                format!(
                    "{pre}let __t = {target}; if !dat_rt::List::sort_by_difference(&mut *__t, {desc}) {{ dat_rt::done({m}dat_rt::List::try_sort_by(__t, {c})) }}"
                )
            }
            ListSortWith | ListSortBy if self.total(&args[0]) => {
                let mut pre = String::new();
                let c = if op == ListSortWith {
                    self.total_callback(f, &args[0], "__a, __b", &["__a.clone()".into(), "__b.clone()".into()], &mut pre)
                } else {
                    self.total_callback(f, &args[0], "__x", &["__x.clone()".into()], &mut pre)
                };
                let rt = if op == ListSortWith { "sort_by_total" } else { "sort_by_total_key" };
                format!("{pre}dat_rt::List::{rt}({target}, {c})")
            }
            ListSortWith | ListSortBy => {
                let mut pre = String::new();
                let c = if op == ListSortWith {
                    self.callback(f, &args[0], "__a, __b", &["__a.clone()".into(), "__b.clone()".into()], &mut pre)
                } else {
                    self.callback(f, &args[0], "__x", &["__x.clone()".into()], &mut pre)
                };
                let (m, _) = self.site(loc);
                let rt = if op == ListSortWith { "try_sort_by" } else { "try_sort_by_key" };
                format!("{pre}dat_rt::done({m}dat_rt::List::{rt}({target}, {c}))")
            }
            SetAdd => format!("dat_rt::Set::add({target}, {})", vals[0]),
            SetDelete => format!("dat_rt::Set::delete({target}, &{})", vals[0]),
            SetClear => format!("dat_rt::Set::clear({target})"),
            MapSet => format!("dat_rt::Map::insert({target}, {}, {})", vals[0], vals[1]),
            MapDelete => format!("dat_rt::Map::delete({target}, &{})", vals[0]),
            MapClear => format!("dat_rt::Map::clear({target})"),
            // A take that borrows its entry (`take.rs`).
            MapTake if borrow => format!("dat_rt::Map::get_or_insert_with({target}, {}, || {})", vals[0], vals[1]),
            Take if borrow => target,
            MapTake => format!("dat_rt::Map::take({target}, &{}).unwrap_or({})", vals[0], vals[1]),
            Take => format!("dat_rt::take({target})"),
            other => unreachable!("not a method that changes its receiver: {other:?}"),
        };
        let _ = write!(s, "{code} }}");
        s
    }

    pub(crate) fn list_lit(&mut self, f: &ir::Function, elems: &[ir::Elem]) -> String {
        if elems.iter().all(|e| matches!(e, ir::Elem::One(_))) {
            let vals: Vec<String> = elems
                .iter()
                .map(|e| match e {
                    ir::Elem::One(x) => self.expr(f, x),
                    ir::Elem::Spread(_) => unreachable!(),
                })
                .collect();
            if vals.is_empty() {
                return "dat_rt::List::new()".to_string();
            }
            return format!("dat_rt::List::from_vec(Vec::from([{}]))", vals.join(", "));
        }
        let mut s = format!("{{ let mut __l = dat_rt::List::with_capacity({}); ", elems.len());
        for e in elems {
            match e {
                ir::Elem::One(x) => {
                    let v = self.expr(f, x);
                    let _ = write!(s, "dat_rt::List::push(&mut __l, {v}); ");
                }
                ir::Elem::Spread(x) => {
                    let v = self.spread_source(f, x);
                    let _ = write!(s, "for __x in {v} {{ dat_rt::List::push(&mut __l, __x.clone()); }} ");
                }
            }
        }
        s.push_str("__l }");
        s
    }

    pub(crate) fn set_lit(&mut self, f: &ir::Function, elems: &[ir::Elem]) -> String {
        let mut s = String::from("{ let mut __s = dat_rt::Set::new(); ");
        for e in elems {
            match e {
                ir::Elem::One(x) => {
                    let v = self.expr(f, x);
                    let _ = write!(s, "dat_rt::Set::add(&mut __s, {v}); ");
                }
                ir::Elem::Spread(x) => {
                    let v = self.spread_source(f, x);
                    let _ = write!(s, "for __x in {v} {{ dat_rt::Set::add(&mut __s, __x.clone()); }} ");
                }
            }
        }
        s.push_str("__s }");
        s
    }

    /// What `...x` goes through: a list's or set's elements, or a string's characters.
    fn spread_source(&mut self, f: &ir::Function, x: &ir::Expr) -> String {
        if x.ty == Type::Str {
            return format!("dat_rt::text::chars({}).iter()", self.str_ref(f, x));
        }
        let r = self.reff(f, x);
        if self.p.types.set_elem(x.ty).is_some() { format!("dat_rt::Set::iter(&{r})") } else { format!("dat_rt::List::iter(&{r})") }
    }

    pub(crate) fn map_lit(&mut self, f: &ir::Function, entries: &[ir::Entry]) -> String {
        let mut s = String::from("{ let mut __m = dat_rt::Map::new(); ");
        for en in entries {
            match en {
                ir::Entry::Pair(k, v) => {
                    let (k, v) = (self.expr(f, k), self.expr(f, v));
                    let _ = write!(s, "dat_rt::Map::insert(&mut __m, {k}, {v}); ");
                }
                ir::Entry::Spread(x) => {
                    let r = self.reff(f, x);
                    let _ = write!(s, "dat_rt::Map::extend_from(&mut __m, &{r}); ");
                }
            }
        }
        s.push_str("__m }");
        s
    }

    /// `for (const x of ...)`. The loop goes through the collection as it was
    /// when it started: a copy that shares the elements.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn for_of(
        &mut self,
        f: &ir::Function,
        var: ir::LocalId,
        iter: &ir::Iter,
        body: &[ir::Stmt],
        ctx: Ctx,
        depth: usize,
        loops: &mut u32,
        out: &mut String,
    ) {
        let ind = "    ".repeat(depth);
        *loops += 1;
        let id = *loops;
        let inner = Ctx { in_closure: ctx.in_closure, lp: Some((id, false)), any_loop: true };
        let name = self.local(f, var);
        let ty = self.ty(f.locals[var.0 as usize].ty);
        let m = if f.locals[var.0 as usize].reassigned || !is_copy(f.locals[var.0 as usize].ty) { "mut " } else { "" };
        let _ = writeln!(out, "{ind}{{");
        let head = match iter {
            ir::Iter::Range { start, end, step, loc } => {
                let (a, b, c) = (self.expr(f, start), self.expr(f, end), self.expr(f, step));
                let (mk, l) = self.site(*loc);
                format!("for __x in {mk}dat_rt::list::range_iter({a}, {b}, {c}, {l}) {{\n{ind}        let {m}{name}: {ty} = __x;")
            }
            // A literal of values goes through them as they are, with no list.
            ir::Iter::Each(e) if matches!(&e.kind, ExprKind::ListLit(elems) if !elems.is_empty() && elems.iter().all(|x| matches!(x, ir::Elem::One(_)))) =>
            {
                let ExprKind::ListLit(elems) = &e.kind else { unreachable!() };
                let vals: Vec<String> = elems
                    .iter()
                    .map(|x| match x {
                        ir::Elem::One(x) => self.expr(f, x),
                        ir::Elem::Spread(_) => unreachable!(),
                    })
                    .collect();
                format!("for __x in [{}] {{\n{ind}        let {m}{name}: {ty} = __x;", vals.join(", "))
            }
            ir::Iter::Each(e) => {
                let v = self.expr(f, e);
                let _ = writeln!(out, "{ind}    let __it{id} = {v};");
                if e.ty == Type::Str {
                    format!("for __x in dat_rt::text::chars(__it{id}.as_str()).iter() {{\n{ind}        let {m}{name}: {ty} = __x.clone();")
                } else if self.p.types.map_types(e.ty).is_some() {
                    format!(
                        "for (__k, __v) in dat_rt::Map::iter(&__it{id}) {{\n{ind}        let {m}{name}: {ty} = (__k.clone(), __v.clone());"
                    )
                } else if self.p.types.set_elem(e.ty).is_some() {
                    format!("for __x in dat_rt::Set::iter(&__it{id}) {{\n{ind}        let {m}{name}: {ty} = __x.clone();")
                } else {
                    format!("for __x in dat_rt::List::iter(&__it{id}) {{\n{ind}        let {m}{name}: {ty} = __x.clone();")
                }
            }
        };
        let _ = writeln!(out, "{ind}    'l{id}: {head}");
        out.push_str(&self.block(f, body, inner, depth + 2, loops));
        let _ = writeln!(out, "{ind}    }}");
        let _ = writeln!(out, "{ind}}}");
    }

    /// `for (mut x of xs)`: `x` is a `&mut` to each element in turn.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn for_mut(
        &mut self,
        f: &ir::Function,
        var: ir::LocalId,
        list: &ir::Place,
        body: &[ir::Stmt],
        ctx: Ctx,
        depth: usize,
        loops: &mut u32,
        out: &mut String,
    ) {
        let ind = "    ".repeat(depth);
        *loops += 1;
        let id = *loops;
        let inner = Ctx { in_closure: ctx.in_closure, lp: Some((id, false)), any_loop: true };
        let mut s = String::new();
        let p = self.hoisted(f, list, &mut s, "__p");
        let target = self.place_mut(f, &p);
        let name = self.local(f, var);
        let _ = writeln!(out, "{ind}{{ {s}");
        let _ = writeln!(out, "{ind}    'l{id}: for {name} in dat_rt::List::slice_mut({target}).iter_mut() {{");
        out.push_str(&self.block(f, body, inner, depth + 2, loops));
        let _ = writeln!(out, "{ind}    }}");
        let _ = writeln!(out, "{ind}}}");
    }
}

/// An expression with no effects that can't throw: a literal, or a
/// collection literal of them.
fn constant(e: &ir::Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::Bool(_) | ExprKind::Str(_) => true,
        ExprKind::ListLit(elems) | ExprKind::SetLit(elems) => elems.iter().all(|el| matches!(el, ir::Elem::One(x) if constant(x))),
        ExprKind::MapLit(entries) => entries.iter().all(|en| matches!(en, ir::Entry::Pair(k, v) if constant(k) && constant(v))),
        ExprKind::TupleLit(items) => items.iter().all(constant),
        _ => false,
    }
}

/// An expression that can't throw or print and costs little: locals,
/// constants, comparisons, logic, float arithmetic, tuple positions,
/// lengths, and `%` by a constant that can't fail. Integer `+`, `-` and
/// `*` can overflow, so they can throw.
fn total(e: &ir::Expr) -> bool {
    let here = match &e.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Local(_)
        | ExprKind::Move(_)
        | ExprKind::Compare { .. }
        | ExprKind::Not(_)
        | ExprKind::And(..)
        | ExprKind::Or(..)
        | ExprKind::Cond { .. }
        | ExprKind::FloatArith { .. }
        | ExprKind::FloatNeg(_)
        | ExprKind::TupleGet(..) => true,
        ExprKind::IntArith { op: ir::IntOp::Rem, rhs, .. } => matches!(rhs.kind, ExprKind::Int(n) if n != 0 && n != -1),
        ExprKind::Method { op: Method::ListLength | Method::ListIsEmpty | Method::SetSize | Method::MapSize, .. } => true,
        _ => false,
    };
    let mut all = here;
    ir::each_child(e, &mut |c| all &= total(c));
    all
}
