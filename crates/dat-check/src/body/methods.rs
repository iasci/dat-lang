//! The methods of lists, sets and maps (§10, §11, §16): one table of
//! signatures in a small type language, and one routine that fills in the
//! type variables from the receiver and the arguments.

use super::{Body, TE, mk, poison};
use crate::Ty;
use crate::codes;
use dat_diag::Span;
use dat_ir::{self as ir, Method};
use dat_syntax::ast::{self, ExprId, ExprKind};

/// A type in a signature. `T` is a list's or set's element type, `K` and
/// `V` a map's key and value types, and `U` is free: an argument, usually
/// a callback's result, decides it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum S {
    T,
    U,
    K,
    V,
    Int,
    Bool,
    Str,
    Void,
    List(&'static S),
    Set(&'static S),
    Map(&'static S, &'static S),
    Pair(&'static S, &'static S),
    /// A callback with these parameters and result. `indexed`: it may also
    /// take the element's index, as an `int` after them.
    Fn(&'static [S], &'static S, bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Recv {
    List,
    Set,
    Map,
}

/// What a method needs of a type variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Needs {
    Nothing,
    EquatableT,
    HashableT,
    ComparableT,
    ComparableU,
    HashableU,
}

/// A value for an optional last parameter.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Opt {
    Int(i64),
    Str(&'static str),
}

#[derive(Debug)]
pub(crate) struct Sig {
    pub name: &'static str,
    pub recv: Recv,
    pub op: Method,
    pub params: &'static [S],
    /// Values for the last parameters when a call leaves them out.
    pub optional: &'static [Opt],
    pub ret: S,
    pub needs: Needs,
}

const fn sig(name: &'static str, recv: Recv, op: Method, params: &'static [S], ret: S) -> Sig {
    Sig { name, recv, op, params, optional: &[], ret, needs: Needs::Nothing }
}

impl Sig {
    const fn needs(mut self, needs: Needs) -> Sig {
        self.needs = needs;
        self
    }

    const fn optional(mut self, optional: &'static [Opt]) -> Sig {
        self.optional = optional;
        self
    }
}

use Method as M;
use Recv::{List as L, Map as Mp, Set as St};
use S::*;

const LIST_T: S = List(&T);
const LIST_U: S = List(&U);
const SET_T: S = Set(&T);
const PRED: S = Fn(&[T], &Bool, true);

/// Every method of a list, set or map that M2 implements. A name may have
/// several entries with different numbers of parameters, as `sort()` and
/// `sort(compare)`; a call uses the first that takes its arguments.
pub(crate) const METHODS: &[Sig] = &[
    sig("isEmpty", L, M::ListIsEmpty, &[], Bool),
    sig("includes", L, M::ListIncludes, &[T], Bool).needs(Needs::EquatableT),
    sig("indexOf", L, M::ListIndexOf, &[T], Int).needs(Needs::EquatableT),
    sig("lastIndexOf", L, M::ListLastIndexOf, &[T], Int).needs(Needs::EquatableT),
    sig("at", L, M::ListAt, &[Int], T),
    sig("slice", L, M::ListSlice, &[Int, Int], LIST_T).optional(&[Opt::Int(0), Opt::Int(i64::MAX)]),
    sig("concat", L, M::ListConcat, &[LIST_T], LIST_T),
    sig("join", L, M::ListJoin, &[Str], Str).optional(&[Opt::Str(",")]),
    sig("map", L, M::ListMap, &[Fn(&[T], &U, true)], LIST_U),
    sig("filter", L, M::ListFilter, &[PRED], LIST_T),
    sig("flatMap", L, M::ListFlatMap, &[Fn(&[T], &LIST_U, true)], LIST_U),
    sig("reduce", L, M::ListReduce, &[Fn(&[U, T], &U, false), U], U),
    sig("forEach", L, M::ListForEach, &[Fn(&[T], &Void, true)], Void),
    sig("find", L, M::ListFind, &[PRED, T], T),
    sig("findIndex", L, M::ListFindIndex, &[PRED], Int),
    sig("some", L, M::ListSome, &[PRED], Bool),
    sig("every", L, M::ListEvery, &[PRED], Bool),
    sig("count", L, M::ListCount, &[PRED], Int),
    sig("entries", L, M::ListEntries, &[], List(&Pair(&Int, &T))),
    sig("toSorted", L, M::ListToSorted, &[], LIST_T).needs(Needs::ComparableT),
    sig("toSorted", L, M::ListToSortedWith, &[Fn(&[T, T], &Int, false)], LIST_T),
    sig("toSortedBy", L, M::ListToSortedBy, &[Fn(&[T], &U, false)], LIST_T).needs(Needs::ComparableU),
    sig("toReversed", L, M::ListToReversed, &[], LIST_T),
    sig("unique", L, M::ListUnique, &[], LIST_T).needs(Needs::HashableT),
    sig("zip", L, M::ListZip, &[LIST_U], List(&Pair(&T, &U))),
    sig("groupBy", L, M::ListGroupBy, &[Fn(&[T], &U, false)], Map(&U, &LIST_T)).needs(Needs::HashableU),
    sig("pop", L, M::ListPop, &[], T),
    sig("shift", L, M::ListShift, &[], T),
    sig("insert", L, M::ListInsert, &[Int, T], Void),
    sig("removeAt", L, M::ListRemoveAt, &[Int], T),
    sig("remove", L, M::ListRemove, &[T], Bool).needs(Needs::EquatableT),
    sig("sort", L, M::ListSort, &[], Void).needs(Needs::ComparableT),
    sig("sort", L, M::ListSortWith, &[Fn(&[T, T], &Int, false)], Void),
    sig("sortBy", L, M::ListSortBy, &[Fn(&[T], &U, false)], Void).needs(Needs::ComparableU),
    sig("reverse", L, M::ListReverse, &[], Void),
    sig("clear", L, M::ListClear, &[], Void),
    sig("isEmpty", St, M::SetIsEmpty, &[], Bool),
    sig("has", St, M::SetHas, &[T], Bool),
    sig("union", St, M::SetUnion, &[SET_T], SET_T),
    sig("intersection", St, M::SetIntersection, &[SET_T], SET_T),
    sig("difference", St, M::SetDifference, &[SET_T], SET_T),
    sig("symmetricDifference", St, M::SetSymmetricDifference, &[SET_T], SET_T),
    sig("isSubsetOf", St, M::SetIsSubsetOf, &[SET_T], Bool),
    sig("isSupersetOf", St, M::SetIsSupersetOf, &[SET_T], Bool),
    sig("isDisjointFrom", St, M::SetIsDisjointFrom, &[SET_T], Bool),
    sig("add", St, M::SetAdd, &[T], Void),
    sig("delete", St, M::SetDelete, &[T], Bool),
    sig("clear", St, M::SetClear, &[], Void),
    sig("isEmpty", Mp, M::MapIsEmpty, &[], Bool),
    sig("get", Mp, M::MapGet, &[K, V], V),
    sig("has", Mp, M::MapHas, &[K], Bool),
    sig("keys", Mp, M::MapKeys, &[], List(&K)),
    sig("values", Mp, M::MapValues, &[], List(&V)),
    sig("entries", Mp, M::MapEntries, &[], List(&Pair(&K, &V))),
    sig("set", Mp, M::MapSet, &[K, V], Void),
    sig("delete", Mp, M::MapDelete, &[K], Bool),
    sig("clear", Mp, M::MapClear, &[], Void),
];

/// Members that later milestones implement, with what they need.
pub(crate) fn later(recv: Recv, name: &str) -> Option<&'static str> {
    match (recv, name) {
        (Recv::List, "parallelMap") => Some("`parallelMap`, which comes with concurrency"),
        _ => None,
    }
}

/// The names that change their receiver, whatever its type.
pub(crate) fn mutating_name(name: &str) -> bool {
    METHODS.iter().any(|s| s.name == name && s.op.mutates()) || matches!(name, "push" | "unshift")
}

/// Every member name of a collection kind, for suggesting a similar one.
pub(crate) fn names(recv: Recv) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = METHODS.iter().filter(|s| s.recv == recv).map(|s| s.name).collect();
    out.push(if recv == Recv::List { "length" } else { "size" });
    if recv == Recv::List {
        out.extend(["push", "unshift", "flat", "toString"]);
    } else {
        out.push("toString");
    }
    out.dedup();
    out
}

/// The type variables of one call.
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct Bind {
    pub t: Option<ir::Type>,
    pub u: Option<ir::Type>,
    pub k: Option<ir::Type>,
    pub v: Option<ir::Type>,
}

impl Bind {
    fn get(&self, s: &S) -> Option<ir::Type> {
        match s {
            T => self.t,
            U => self.u,
            K => self.k,
            V => self.v,
            _ => None,
        }
    }
}

/// `s` with its variables filled in, if they all are bound.
fn subst(types: &mut ir::Types, s: &S, b: &Bind) -> Option<ir::Type> {
    Some(match s {
        T | U | K | V => b.get(s)?,
        Int => ir::Type::Int,
        Bool => ir::Type::Bool,
        Str => ir::Type::Str,
        Void => ir::Type::Void,
        List(e) => {
            let e = subst(types, e, b)?;
            types.list(e)
        }
        Set(e) => {
            let e = subst(types, e, b)?;
            types.set(e)
        }
        Map(k, v) => {
            let (k, v) = (subst(types, k, b)?, subst(types, v, b)?);
            types.map(k, v)
        }
        Pair(a, c) => {
            let (a, c) = (subst(types, a, b)?, subst(types, c, b)?);
            types.tuple(vec![a, c])
        }
        Fn(params, ret, _) => {
            let params: Option<Vec<ir::Type>> = params.iter().map(|p| subst(types, p, b)).collect();
            let ret = subst(types, ret, b)?;
            types.func(params?, ret)
        }
    })
}

/// Binds the free variable `U` by matching `s` against `t`. `false` if they
/// can't match, which the caller reports as a type mismatch.
fn unify(types: &ir::Types, s: &S, t: ir::Type, b: &mut Bind) -> bool {
    match s {
        U => match b.u {
            Some(u) => u == t,
            None => {
                b.u = Some(t);
                true
            }
        },
        T | K | V => b.get(s) == Some(t),
        Int => t == ir::Type::Int,
        Bool => t == ir::Type::Bool,
        Str => t == ir::Type::Str,
        Void => t == ir::Type::Void,
        List(e) => types.list_elem(t).is_some_and(|x| unify(types, e, x, b)),
        Set(e) => types.set_elem(t).is_some_and(|x| unify(types, e, x, b)),
        Map(k, v) => types.map_types(t).is_some_and(|(x, y)| unify(types, k, x, b) && unify(types, v, y, b)),
        Pair(a, c) => match types.tuple_items(t) {
            Some([x, y]) => {
                let (x, y) = (*x, *y);
                unify(types, a, x, b) && unify(types, c, y, b)
            }
            _ => false,
        },
        Fn(..) => false,
    }
}

impl Body<'_, '_> {
    /// The kind of collection `t` is, with its type variables bound.
    pub(super) fn receiver(&self, t: Ty) -> Option<(Recv, Bind)> {
        let types = &self.m.types;
        match types.kind(t.ir())? {
            ir::Kind::List(e) => Some((Recv::List, Bind { t: Some(*e), ..Bind::default() })),
            ir::Kind::Set(e) => Some((Recv::Set, Bind { t: Some(*e), ..Bind::default() })),
            ir::Kind::Map(k, v) => Some((Recv::Map, Bind { k: Some(*k), v: Some(*v), ..Bind::default() })),
            _ => None,
        }
    }

    /// The signatures of `name` on a receiver of kind `recv`.
    pub(super) fn sigs(recv: Recv, name: &str) -> Vec<&'static Sig> {
        METHODS.iter().filter(|s| s.recv == recv && s.name == name).collect()
    }

    /// Checks a call of a collection method whose signatures are `sigs`,
    /// and returns the lowered arguments (without the receiver), the
    /// operation and the result type. `None` after an error.
    pub(super) fn method_args(
        &mut self,
        sigs: &[&'static Sig],
        mut bind: Bind,
        name: &str,
        args: &[ast::Arg],
        span: Span,
    ) -> Option<(ir::Method, Vec<ir::Expr>, Ty)> {
        let Some(sig) = sigs.iter().find(|s| (s.params.len() - s.optional.len()..=s.params.len()).contains(&args.len())) else {
            let min = sigs.iter().map(|s| s.params.len() - s.optional.len()).min().unwrap_or(0);
            let max = sigs.iter().map(|s| s.params.len()).max().unwrap_or(0);
            self.check_args(Some((span, args)));
            self.arity(name, span, min, max, args.len());
            return None;
        };
        if let Some(a) = args.iter().find(|a| matches!(self.ast().expr(a.expr).kind, ExprKind::Spread(_))) {
            self.check_args(Some((span, args)));
            let d = self.err(codes::BAD_OPERANDS, self.espan(a.expr), format!("`{name}` doesn't take `...`"));
            self.push(d);
            return None;
        }
        let mut lowered: Vec<Option<ir::Expr>> = vec![None; args.len()];
        let mut bad = false;
        // Plain arguments first, so a callback's parameter types are known:
        // `reduce`'s initial value decides its accumulator's type. Checking
        // out of order is safe, since creating a closure has no effect; the
        // arguments still run left to right.
        let order: Vec<usize> = (0..args.len())
            .filter(|&i| !matches!(sig.params[i], Fn(..)))
            .chain((0..args.len()).filter(|&i| matches!(sig.params[i], Fn(..))))
            .collect();
        for i in order {
            let a = &args[i];
            let param = sig.params[i];
            if a.is_mut {
                let d = self.err(codes::NOT_MUT, self.espan(a.expr), format!("`{name}` doesn't take `mut` arguments"));
                self.push(d);
            }
            let v = match param {
                Fn(params, ret, indexed) => self.callback(a.expr, params, ret, indexed, &mut bind),
                _ => {
                    let want = subst(&mut self.m.types, &param, &bind).map(Ty::of);
                    let v = self.expr_want(a.expr, want);
                    if let Some(w) = want {
                        let before = self.m.diags.len();
                        self.expect_type(&v, w, self.espan(a.expr));
                        bad |= self.m.diags.len() > before;
                    } else if v.ty != Ty::Poison && !unify(&self.m.types, &param, v.ty.ir(), &mut bind) {
                        let d = self.err(codes::MISMATCH, self.espan(a.expr), format!("`{name}` can't take {}", self.m.an(v.ty)));
                        self.push(d);
                        bad = true;
                    }
                    v
                }
            };
            bad |= v.ty == Ty::Poison;
            lowered[i] = Some(v.e);
        }
        if bad {
            return None;
        }
        let ret = subst(&mut self.m.types, &sig.ret, &bind).map(Ty::of)?;
        if !self.needs(sig, &bind, span) {
            return None;
        }
        let mut out: Vec<ir::Expr> = lowered.into_iter().map(Option::unwrap).collect();
        let first_optional = sig.params.len() - sig.optional.len();
        for o in &sig.optional[args.len() - first_optional..] {
            out.push(match o {
                Opt::Int(n) => mk(ir::ExprKind::Int(*n), Ty::Int).e,
                Opt::Str(s) => mk(ir::ExprKind::Str(s.to_string()), Ty::Str).e,
            });
        }
        Some((sig.op, out, ret))
    }

    /// Checks what `sig` needs of its type variables (§01).
    fn needs(&mut self, sig: &Sig, bind: &Bind, span: Span) -> bool {
        let types = &self.m.types;
        let (ok, what, t) = match sig.needs {
            Needs::Nothing => return true,
            Needs::EquatableT => (types.equatable(bind.t.unwrap()), "compare its elements with `==`", bind.t.unwrap()),
            Needs::HashableT => (types.hashable(bind.t.unwrap()), "hash its elements", bind.t.unwrap()),
            Needs::ComparableT => (types.comparable(bind.t.unwrap()), "order its elements", bind.t.unwrap()),
            Needs::ComparableU => (types.comparable(bind.u.unwrap()), "order by the key", bind.u.unwrap()),
            Needs::HashableU => (types.hashable(bind.u.unwrap()), "hash the key", bind.u.unwrap()),
        };
        if ok {
            return true;
        }
        let shown = self.m.tn(Ty::of(t));
        let (code, label) = match sig.needs {
            Needs::HashableT | Needs::HashableU => (codes::NOT_HASHABLE, crate::module::hashable_note(types, t)),
            _ if !types.equatable(t) => (codes::NOT_COMPARABLE, "function values can't be compared".to_string()),
            _ => (
                codes::NOT_COMPARABLE,
                "comparable types are int, float and string; pass a function that compares two values instead".to_string(),
            ),
        };
        let d = self.err(code, span, format!("`{}` needs to {what}, and `{shown}` can't", sig.name)).with_label(label);
        self.push(d);
        false
    }

    /// A callback argument: an arrow function gets its parameter types from
    /// the signature; any other function value must have a matching type.
    /// Its result binds `U` when that's still free.
    fn callback(&mut self, id: ExprId, params: &'static [S], ret: &'static S, indexed: bool, bind: &mut Bind) -> TE {
        let ast = self.ast();
        let mut inner = id;
        while let ExprKind::Paren(i) = ast.expr(inner).kind {
            inner = i;
        }
        let mut want: Vec<Ty> = params.iter().map(|p| subst(&mut self.m.types, p, bind).map_or(Ty::Poison, Ty::of)).collect();
        let want_ret = subst(&mut self.m.types, ret, bind).map(Ty::of);
        let v = if let ExprKind::Arrow(a) = &ast.expr(inner).kind {
            let n = a.params.len();
            if indexed && n == want.len() + 1 {
                want.push(Ty::Int);
            } else if n != want.len() {
                let takes = if indexed { format!("{} or {}", want.len(), want.len() + 1) } else { want.len().to_string() };
                let plural = if takes == "1" { "" } else { "s" };
                let d = self.err(codes::ARITY, a.head, format!("this callback takes {takes} parameter{plural}, but it declares {n}"));
                self.push(d);
                return poison();
            }
            self.arrow_fn(inner, a, Some(want), want_ret)
        } else {
            let v = self.expr(id);
            if v.ty == Ty::Poison {
                return v;
            }
            let ok = match self.m.types.func_sig(v.ty.ir()) {
                Some((ps, r)) => {
                    let ps: Vec<Ty> = ps.iter().map(|p| Ty::of(*p)).collect();
                    let n = ps.len();
                    let fits = ps[..n.min(want.len())] == want[..n.min(want.len())]
                        && (n == want.len() || (indexed && n == want.len() + 1 && ps[n - 1] == Ty::Int));
                    fits && want_ret.is_none_or(|w| w == Ty::of(r))
                }
                None => false,
            };
            if !ok {
                let shown: Vec<String> = want.iter().map(|t| self.m.tn(*t)).collect();
                let r = want_ret.map_or("_".to_string(), |r| self.m.tn(r));
                let d = self
                    .err(
                        codes::MISMATCH,
                        self.espan(id),
                        format!("expected a function `({}) => {r}`, found {}", shown.join(", "), self.m.an(v.ty)),
                    )
                    .with_label(format!("this is {}", self.m.an(v.ty)));
                self.push(d);
                return poison();
            }
            v
        };
        if v.ty == Ty::Poison {
            return v;
        }
        let (_, r) = self.m.types.func_sig(v.ty.ir()).expect("a callback is a function");
        // `map((x) => print(x))`: what it returns is collected or compared.
        if r == ir::Type::Void && !matches!(ret, Void) {
            let d = self
                .err(codes::VOID_VALUE, self.espan(id), "this callback returns nothing, and its result is needed")
                .with_label("return a value from it; to only do something for each element, use `forEach` or a loop");
            self.push(d);
            return poison();
        }
        if !unify(&self.m.types, ret, r, bind) && want_ret.is_none() {
            let d = self.err(codes::MISMATCH, self.espan(id), format!("this callback can't return {}", self.m.an(Ty::of(r))));
            self.push(d);
            return poison();
        }
        v
    }
}
