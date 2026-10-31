//! Lists, sets, maps, tuples, places and function values.
//!
//! Every operation calls its one implementation in dat-rt, with `Value` as
//! the element type. A callback runs through the machine, so dat-rt's
//! methods get closures that call back into it; the collection they work
//! on is owned by the node, never borrowed from the machine's stack.

use crate::value::{Closure, Value};
use crate::{Code, Compiler, F, Flow, Machine, R, node, run, thrown};
use dat_ir::{self as ir, ExprKind, Method, Type};
use dat_rt::{DatError, List, Loc, Map, Set, Str};
use std::rc::Rc;

/// A compiled step of a place: its operand, if it has one, and where it throws.
pub(crate) enum StepCode {
    Index(F<Value>, Loc),
    Key(F<Value>, Loc),
    Pos(usize),
}

/// A compiled place: its root local and its steps.
pub(crate) struct PlaceCode {
    root: usize,
    steps: Box<[StepCode]>,
}

impl PlaceCode {
    /// The values of the step operands, in order.
    pub(crate) fn operands(&self, c: &Code, m: &mut Machine, b: usize) -> R<Vec<Value>> {
        let mut ops = Vec::with_capacity(self.steps.len());
        for s in self.steps.iter() {
            match s {
                StepCode::Index(f, _) | StepCode::Key(f, _) => ops.push(f(c, m, b)?),
                StepCode::Pos(_) => ops.push(Value::Void),
            }
        }
        Ok(ops)
    }

    /// The value after the first `n` steps, with the operands `ops`.
    fn reach<'m>(&self, m: &'m mut Machine, b: usize, ops: &[Value], n: usize) -> R<&'m mut Value> {
        let mut v = &mut m.stack[b + self.root];
        for (s, op) in self.steps[..n].iter().zip(ops) {
            v = match s {
                StepCode::Index(_, at) => v.list_mut().try_get_mut(op.int()).map_err(|e| thrown(e, *at))?,
                StepCode::Key(_, at) => v.map_mut().try_get_mut(op).map_err(|e| thrown(e, *at))?,
                StepCode::Pos(n) => match v {
                    Value::Tuple(t) => &mut Rc::make_mut(t)[*n],
                    _ => unreachable!("type checker guarantees a tuple"),
                },
            };
        }
        Ok(v)
    }

    fn get<'m>(&self, m: &'m mut Machine, b: usize, ops: &[Value]) -> R<&'m mut Value> {
        self.reach(m, b, ops, self.steps.len())
    }

    /// Takes the value out of the place, leaving a placeholder.
    pub(crate) fn take(&self, m: &mut Machine, b: usize, ops: &[Value]) -> R<Value> {
        Ok(std::mem::take(self.get(m, b, ops)?))
    }

    /// Puts a taken value back. The place was reached once, and nothing
    /// could change the way to it since, so it's reached again.
    pub(crate) fn put(&self, m: &mut Machine, b: usize, ops: &[Value], v: Value) {
        if let Ok(slot) = self.get(m, b, ops) {
            *slot = v;
        }
    }
}

/// An argument of a call: a value, or a place passed `mut`.
pub(crate) enum ArgCode {
    Value(F<Value>),
    Ref(PlaceCode),
}

impl Compiler<'_> {
    pub(crate) fn place(&self, p: &ir::Place) -> PlaceCode {
        let steps = p
            .steps
            .iter()
            .map(|s| match s {
                ir::Step::Index(x, loc) => StepCode::Index(self.value(x), self.loc(*loc)),
                ir::Step::Key(x, loc) => StepCode::Key(self.value(x), self.loc(*loc)),
                ir::Step::Pos(n) => StepCode::Pos(*n as usize),
            })
            .collect();
        PlaceCode { root: p.root.0 as usize, steps }
    }

    /// A call with `mut` arguments. The arguments are worked out in order,
    /// a `mut` one as its place's operands; then each `mut` value moves out
    /// of its place into the callee, and back when the call ends, even by
    /// an error. Nothing else can reach a place during the call.
    pub(crate) fn call_refs<T: 'static>(
        &self,
        func: ir::FuncId,
        args: &[ir::Expr],
        loc: ir::Loc,
        out: impl Fn(Value) -> T + 'static,
    ) -> F<T> {
        let id = func.0 as usize;
        let codes: Box<[ArgCode]> = args
            .iter()
            .map(|a| match &a.kind {
                ExprKind::Ref(p) => ArgCode::Ref(self.place(p)),
                _ => ArgCode::Value(self.value(a)),
            })
            .collect();
        let at = self.loc(loc);
        node(move |c, m, b| {
            if dat_rt::stack_is_low() {
                return Err(thrown(DatError::stack_overflow(), at));
            }
            let mut vals = Vec::with_capacity(codes.len());
            let mut ops = Vec::new();
            for a in codes.iter() {
                match a {
                    ArgCode::Value(f) => vals.push(f(c, m, b)?),
                    ArgCode::Ref(p) => {
                        ops.push(p.operands(c, m, b)?);
                        vals.push(Value::Void);
                    }
                }
            }
            let refs: Vec<&PlaceCode> = codes
                .iter()
                .filter_map(|a| match a {
                    ArgCode::Ref(p) => Some(p),
                    ArgCode::Value(_) => None,
                })
                .collect();
            let slots: Vec<usize> = codes.iter().enumerate().filter(|(_, a)| matches!(a, ArgCode::Ref(_))).map(|(i, _)| i).collect();
            for (k, p) in refs.iter().enumerate() {
                match p.take(m, b, &ops[k]) {
                    Ok(v) => vals[slots[k]] = v,
                    Err(err) => {
                        for j in 0..k {
                            refs[j].put(m, b, &ops[j], std::mem::take(&mut vals[slots[j]]));
                        }
                        return Err(err);
                    }
                }
            }
            let base = m.stack.len();
            m.stack.extend(vals);
            let f = &c.funcs[id];
            let given = codes.len();
            let result = if given < f.params { f.run_defaulted(c, m, base, given) } else { f.run(c, m, base) };
            let back = std::mem::take(&mut m.out);
            for ((p, v), o) in refs.iter().zip(back).zip(&ops) {
                p.put(m, b, o, v);
            }
            match result {
                Ok(v) => Ok(out(v)),
                Err(err) => Err(pending(err, at)),
            }
        })
    }

    /// `place = value`; a last `Key` step inserts.
    pub(crate) fn store_place(&self, p: &ir::Place, value: &ir::Expr) -> F<Flow> {
        let place = self.place(p);
        let value = self.value(value);
        let insert = matches!(p.steps.last(), Some(ir::Step::Key(..)));
        node(move |c, m, b| {
            let ops = place.operands(c, m, b)?;
            let v = value(c, m, b)?;
            if insert {
                let n = place.steps.len() - 1;
                place.reach(m, b, &ops, n)?.map_mut().insert(ops[n].clone(), v);
            } else {
                *place.get(m, b, &ops)? = v;
            }
            Ok(Flow::Normal)
        })
    }

    /// The expressions only the general path compiles: collections, places
    /// and function values. Every one gives a `Value`.
    pub(crate) fn general(&self, e: &ir::Expr) -> F<Value> {
        match &e.kind {
            ExprKind::ListLit(elems) => {
                let elems = self.elems(elems);
                node(move |c, m, b| {
                    let mut l = List::with_capacity(elems.len());
                    for el in elems.iter() {
                        match el {
                            Elem::One(f) => l.push(f(c, m, b)?),
                            Elem::Spread(f) => spread(&f(c, m, b)?, |v| l.push(v)),
                        }
                    }
                    Ok(Value::List(l))
                })
            }
            ExprKind::SetLit(elems) => {
                let elems = self.elems(elems);
                node(move |c, m, b| {
                    let mut s = Set::new();
                    for el in elems.iter() {
                        match el {
                            Elem::One(f) => s.add(f(c, m, b)?),
                            Elem::Spread(f) => spread(&f(c, m, b)?, |v| s.add(v)),
                        }
                    }
                    Ok(Value::Set(s))
                })
            }
            ExprKind::MapLit(entries) => {
                // A pair's key and value, or a spread map and `None`.
                type EntryCode = (F<Value>, Option<F<Value>>);
                let entries: Box<[EntryCode]> = entries
                    .iter()
                    .map(|en| match en {
                        ir::Entry::Pair(k, v) => (self.value(k), Some(self.value(v))),
                        ir::Entry::Spread(x) => (self.value(x), None),
                    })
                    .collect();
                node(move |c, m, b| {
                    let mut map = Map::new();
                    for (k, v) in entries.iter() {
                        match v {
                            Some(v) => {
                                let key = k(c, m, b)?;
                                map.insert(key, v(c, m, b)?);
                            }
                            None => map.extend_from(k(c, m, b)?.map()),
                        }
                    }
                    Ok(Value::Map(map))
                })
            }
            ExprKind::TupleLit(items) => {
                let items: Box<[F<Value>]> = items.iter().map(|x| self.value(x)).collect();
                node(move |c, m, b| {
                    let mut out = Vec::with_capacity(items.len());
                    for f in items.iter() {
                        out.push(f(c, m, b)?);
                    }
                    Ok(Value::Tuple(Rc::new(out)))
                })
            }
            ExprKind::Index { base, index, loc } => {
                let (base_f, index_f, at) = (self.value(base), self.value(index), self.loc(*loc));
                if self.p.types.map_types(base.ty).is_some() {
                    node(move |c, m, b| {
                        let v = base_f(c, m, b)?;
                        let k = index_f(c, m, b)?;
                        v.map().try_get(&k).cloned().map_err(|e| thrown(e, at))
                    })
                } else {
                    node(move |c, m, b| {
                        let v = base_f(c, m, b)?;
                        let i = index_f(c, m, b)?.int();
                        v.list().try_get(i).cloned().map_err(|e| thrown(e, at))
                    })
                }
            }
            ExprKind::TupleGet(base, n) => {
                let (base, n) = (self.value(base), *n as usize);
                node(move |c, m, b| Ok(base(c, m, b)?.tuple()[n].clone()))
            }
            ExprKind::FuncRef(f) => {
                let v = Value::Func(Rc::new(Closure { func: f.0 as usize, captures: Box::new([]) }));
                node(move |_, _, _| Ok(v.clone()))
            }
            ExprKind::Closure { func, captures } => {
                let func = func.0 as usize;
                let slots: Box<[usize]> = captures.iter().map(|l| l.0 as usize).collect();
                node(move |_, m, b| {
                    let captures = slots.iter().map(|s| m.stack[b + s].clone()).collect();
                    Ok(Value::Func(Rc::new(Closure { func, captures })))
                })
            }
            ExprKind::CallValue { callee, args, loc } => {
                let callee = self.value(callee);
                let args: Box<[F<Value>]> = args.iter().map(|a| self.value(a)).collect();
                let at = self.loc(*loc);
                node(move |c, m, b| {
                    let f = callee(c, m, b)?;
                    let mut vals = Vec::with_capacity(args.len());
                    for a in args.iter() {
                        vals.push(a(c, m, b)?);
                    }
                    call_value(c, m, f.func(), vals).map_err(|e| pending(e, at))
                })
            }
            ExprKind::Method { op, args, loc } => self.method(*op, args, self.loc(*loc)),
            ExprKind::Mutate { op, place, args, loc } => self.at(Some(*loc), self.mutate(*op, place, args, self.loc(*loc))),
            _ => unreachable!("not a general expression: {e:?}"),
        }
    }

    fn elems(&self, elems: &[ir::Elem]) -> Box<[Elem]> {
        elems
            .iter()
            .map(|e| match e {
                ir::Elem::One(x) => Elem::One(self.value(x)),
                ir::Elem::Spread(x) => Elem::Spread(self.value(x)),
            })
            .collect()
    }

    /// Whether a callback value takes the element's index as well.
    fn indexed(&self, cb: &ir::Expr) -> bool {
        self.p.types.func_sig(cb.ty).is_some_and(|(p, _)| p.len() == 2)
    }

    /// A method that doesn't change its receiver, `args[0]`.
    fn method(&self, op: Method, args: &[ir::Expr], at: Loc) -> F<Value> {
        use Method::*;
        let a: Vec<F<Value>> = args.iter().map(|x| self.value(x)).collect();
        let mut a = a.into_iter();
        let mut next = || a.next().expect("an argument");
        // One argument, then an operation on it.
        macro_rules! one {
            (|$x:ident| $body:expr) => {{
                let f0 = next();
                node(move |c, m, b| {
                    let $x = f0(c, m, b)?;
                    Ok($body)
                })
            }};
        }
        macro_rules! two {
            (|$x:ident, $y:ident| $body:expr) => {{
                let (f0, f1) = (next(), next());
                node(move |c, m, b| {
                    let $x = f0(c, m, b)?;
                    let $y = f1(c, m, b)?;
                    Ok($body)
                })
            }};
        }
        macro_rules! three {
            (|$x:ident, $y:ident, $z:ident| $body:expr) => {{
                let (f0, f1, f2) = (next(), next(), next());
                node(move |c, m, b| {
                    let $x = f0(c, m, b)?;
                    let $y = f1(c, m, b)?;
                    let $z = f2(c, m, b)?;
                    $body
                })
            }};
        }
        // The same, for operations that call back into the machine `$m`.
        macro_rules! two_cb {
            ($c:ident, $m:ident, |$x:ident, $y:ident| $body:expr) => {{
                let (f0, f1) = (next(), next());
                node(move |$c, $m, b| {
                    let $x = f0($c, $m, b)?;
                    let $y = f1($c, $m, b)?;
                    Ok($body)
                })
            }};
        }
        macro_rules! three_cb {
            ($c:ident, $m:ident, |$x:ident, $y:ident, $z:ident| $body:expr) => {{
                let (f0, f1, f2) = (next(), next(), next());
                node(move |$c, $m, b| {
                    let $x = f0($c, $m, b)?;
                    let $y = f1($c, $m, b)?;
                    let $z = f2($c, $m, b)?;
                    $body
                })
            }};
        }
        let err = move |e: Box<DatError>| thrown(e, at);
        match op {
            InList => two!(|item, l| Value::Bool(l.list().includes(&item))),
            InSet => two!(|item, s| Value::Bool(s.set().has(&item))),
            InMap => two!(|key, mp| Value::Bool(mp.map().has(&key))),
            ListLength => one!(|l| Value::Int(l.list().len())),
            ListIsEmpty => one!(|l| Value::Bool(l.list().is_empty())),
            ListIncludes => two!(|l, x| Value::Bool(l.list().includes(&x))),
            ListIndexOf => two!(|l, x| Value::Int(l.list().index_of(&x))),
            ListLastIndexOf => two!(|l, x| Value::Int(l.list().last_index_of(&x))),
            ListAt => {
                let (f0, f1) = (next(), next());
                node(move |c, m, b| {
                    let l = f0(c, m, b)?;
                    let i = f1(c, m, b)?.int();
                    l.list().try_at(i).cloned().map_err(err)
                })
            }
            ListSlice => three!(|l, s, e| Ok(Value::List(l.list().slice(s.int(), e.int())))),
            ListConcat => two!(|l, o| Value::List(l.list().concat(o.list()))),
            ListJoin => two!(|l, s| Value::Str(l.list().join(s.str().as_str()))),
            ListEntries => {
                one!(|l| Value::List(l.list().iter().enumerate().map(|(i, x)| tuple2(Value::Int(i as i64), x.clone())).collect()))
            }
            ListToSorted => one!(|l| Value::List(l.list().to_sorted())),
            ListToReversed => one!(|l| Value::List(l.list().to_reversed())),
            ListUnique => one!(|l| Value::List(l.list().unique())),
            ListZip => two!(|l, o| Value::List(l.list().iter().zip(o.list().iter()).map(|(x, y)| tuple2(x.clone(), y.clone())).collect())),
            ListFlat => one!(|l| {
                let mut out = List::new();
                for x in l.list().iter() {
                    out.push_all(x.list());
                }
                Value::List(out)
            }),
            SetSize => one!(|s| Value::Int(s.set().len())),
            SetIsEmpty => one!(|s| Value::Bool(s.set().is_empty())),
            SetHas => two!(|s, x| Value::Bool(s.set().has(&x))),
            SetUnion => two!(|s, o| Value::Set(s.set().union(o.set()))),
            SetIntersection => two!(|s, o| Value::Set(s.set().intersection(o.set()))),
            SetDifference => two!(|s, o| Value::Set(s.set().difference(o.set()))),
            SetSymmetricDifference => two!(|s, o| Value::Set(s.set().symmetric_difference(o.set()))),
            SetIsSubsetOf => two!(|s, o| Value::Bool(s.set().is_subset_of(o.set()))),
            SetIsSupersetOf => two!(|s, o| Value::Bool(s.set().is_superset_of(o.set()))),
            SetIsDisjointFrom => two!(|s, o| Value::Bool(s.set().is_disjoint_from(o.set()))),
            MapSize => one!(|mp| Value::Int(mp.map().len())),
            MapIsEmpty => one!(|mp| Value::Bool(mp.map().is_empty())),
            MapGet => three!(|mp, k, fallback| Ok(mp.map().get_or(&k, fallback))),
            MapHas => two!(|mp, k| Value::Bool(mp.map().has(&k))),
            MapKeys => one!(|mp| Value::List(mp.map().keys())),
            MapValues => one!(|mp| Value::List(mp.map().values())),
            MapEntries => one!(|mp| Value::List(mp.map().iter().map(|(k, v)| tuple2(k.clone(), v.clone())).collect())),
            StrSplit => two!(|s, sep| Value::List(
                dat_rt::text::split(s.str().as_str(), sep.str().as_str()).iter().map(|x| Value::Str(x.clone())).collect()
            )),
            StrChars => one!(|s| Value::List(chars(s.str()))),
            Range => three!(|s, e, st| dat_rt::list::try_range(s.int(), e.int(), st.int())
                .map(|l| Value::List(l.iter().map(|n| Value::Int(*n)).collect()))
                .map_err(err)),
            MaxInts | MinInts | MaxFloats | MinFloats => {
                let f0 = next();
                node(move |c, m, b| {
                    let l = f0(c, m, b)?;
                    let l = l.list();
                    let r = match op {
                        MaxInts | MinInts => {
                            let ints: List<i64> = l.iter().map(Value::int).collect();
                            if op == MaxInts { dat_rt::num::try_max_int(&ints) } else { dat_rt::num::try_min_int(&ints) }.map(Value::Int)
                        }
                        _ => {
                            let floats: List<f64> = l.iter().map(Value::float).collect();
                            if op == MaxFloats { dat_rt::num::try_max_float(&floats) } else { dat_rt::num::try_min_float(&floats) }
                                .map(Value::Float)
                        }
                    };
                    r.map_err(err)
                })
            }
            ListCheckLen => two!(|l, n| {
                dat_rt::list::try_check_len(l.list().len(), n.int()).map_err(err)?;
                Value::Void
            }),
            // The callback methods: an error from a callback passes through
            // with its trace, and this call is where it leaves the caller.
            ListMap | ListFilter | ListFlatMap | ListForEach | ListFindIndex | ListSome | ListEvery | ListCount => {
                let indexed = self.indexed(&args[1]);
                let (f0, f1) = (next(), next());
                node(move |c, m, b| {
                    let l = f0(c, m, b)?;
                    let cb = f1(c, m, b)?;
                    let cb = cb.func();
                    let call = |m: &mut Machine, x: &Value, i: i64| {
                        let vals = if indexed { vec![x.clone(), Value::Int(i)] } else { vec![x.clone()] };
                        call_value(c, m, cb, vals)
                    };
                    let l = l.list();
                    let r = match op {
                        ListMap => l.try_map(|x, i| call(m, x, i)).map(Value::List),
                        ListFilter => l.try_filter(|x, i| call(m, x, i).map(|v| v.bool())).map(Value::List),
                        ListFlatMap => l.try_flat_map(|x, i| call(m, x, i).map(|v| v.list().clone())).map(Value::List),
                        ListForEach => l.try_for_each(|x, i| call(m, x, i).map(drop)).map(|()| Value::Void),
                        ListFindIndex => l.try_find_index(|x, i| call(m, x, i).map(|v| v.bool())).map(Value::Int),
                        ListSome => l.try_some(|x, i| call(m, x, i).map(|v| v.bool())).map(Value::Bool),
                        ListEvery => l.try_every(|x, i| call(m, x, i).map(|v| v.bool())).map(Value::Bool),
                        _ => l.try_count(|x, i| call(m, x, i).map(|v| v.bool())).map(Value::Int),
                    };
                    r.map_err(|e| pending(e, at))
                })
            }
            ListFind => {
                let indexed = self.indexed(&args[1]);
                three_cb!(c, m, |l, cb, fallback| {
                    let cb = cb.func();
                    l.list()
                        .try_find(
                            |x, i| {
                                let vals = if indexed { vec![x.clone(), Value::Int(i)] } else { vec![x.clone()] };
                                call_value(c, m, cb, vals).map(|v| v.bool())
                            },
                            fallback,
                        )
                        .map_err(|e| pending(e, at))
                })
            }
            ListReduce => three_cb!(c, m, |l, cb, init| {
                let cb = cb.func();
                l.list().try_reduce(init, |acc, x| call_value(c, m, cb, vec![acc, x.clone()])).map_err(|e| pending(e, at))
            }),
            ListToSortedWith => two_cb!(c, m, |l, cb| {
                let cb = cb.func();
                Value::List(
                    l.list()
                        .try_to_sorted_by(|x, y| call_value(c, m, cb, vec![x.clone(), y.clone()]).map(|v| v.int()))
                        .map_err(|e| pending(e, at))?,
                )
            }),
            ListToSortedBy => two_cb!(c, m, |l, cb| {
                let cb = cb.func();
                Value::List(l.list().try_to_sorted_by_key(|x| call_value(c, m, cb, vec![x.clone()])).map_err(|e| pending(e, at))?)
            }),
            ListGroupBy => two_cb!(c, m, |l, cb| {
                let cb = cb.func();
                let groups = l.list().try_group_by(|x| call_value(c, m, cb, vec![x.clone()])).map_err(|e| pending(e, at))?;
                Value::Map(groups.iter().map(|(k, v)| (k.clone(), Value::List(v.clone()))).collect())
            }),
            other => unreachable!("not a method without a place: {other:?}"),
        }
    }

    /// A method that changes the collection at `place`.
    fn mutate(&self, op: Method, place: &ir::Place, args: &[ir::Expr], at: Loc) -> F<Value> {
        use Method::*;
        let place = self.place(place);
        let args: Box<[F<Value>]> = args.iter().map(|x| self.value(x)).collect();
        let err = move |e: Box<DatError>| thrown(e, at);
        node(move |c, m, b| {
            let ops = place.operands(c, m, b)?;
            let mut vals = Vec::with_capacity(args.len());
            for a in args.iter() {
                vals.push(a(c, m, b)?);
            }
            let mut vals = vals.into_iter();
            if op == Take {
                return place.take(m, b, &ops);
            }
            if matches!(op, ListSortWith | ListSortBy) {
                // The callback runs on the machine, so the list leaves its
                // place while it's sorted; nothing else can reach it then.
                let cb = vals.next().expect("a callback");
                let cb = cb.func();
                let mut taken = place.take(m, b, &ops)?;
                let l = taken.list_mut();
                let r = if op == ListSortWith {
                    l.try_sort_by(|x, y| call_value(c, m, cb, vec![x.clone(), y.clone()]).map(|v| v.int()))
                } else {
                    l.try_sort_by_key(|x| call_value(c, m, cb, vec![x.clone()]))
                };
                place.put(m, b, &ops, taken);
                return r.map(|()| Value::Void).map_err(|e| pending(e, at));
            }
            let target = place.get(m, b, &ops)?;
            let mut arg = || vals.next().expect("an argument");
            Ok(match op {
                ListPush => {
                    let l = target.list_mut();
                    for v in vals.by_ref() {
                        l.push(v);
                    }
                    Value::Void
                }
                ListPushAll => {
                    target.list_mut().push_all(arg().list());
                    Value::Void
                }
                ListUnshift => {
                    target.list_mut().unshift_all(arg().list());
                    Value::Void
                }
                ListPop => target.list_mut().try_pop().map_err(err)?,
                ListShift => target.list_mut().try_shift().map_err(err)?,
                ListInsert => {
                    let i = arg().int();
                    target.list_mut().try_insert(i, arg()).map_err(err)?;
                    Value::Void
                }
                ListRemoveAt => target.list_mut().try_remove_at(arg().int()).map_err(err)?,
                ListRemove => Value::Bool(target.list_mut().remove(&arg())),
                ListSort => {
                    target.list_mut().sort();
                    Value::Void
                }
                ListReverse => {
                    target.list_mut().reverse();
                    Value::Void
                }
                ListClear => {
                    target.list_mut().clear();
                    Value::Void
                }
                SetAdd => {
                    target.set_mut().add(arg());
                    Value::Void
                }
                SetDelete => Value::Bool(target.set_mut().delete(&arg())),
                SetClear => {
                    target.set_mut().clear();
                    Value::Void
                }
                MapSet => {
                    let k = arg();
                    target.map_mut().insert(k, arg());
                    Value::Void
                }
                MapDelete => Value::Bool(target.map_mut().delete(&arg())),
                MapClear => {
                    target.map_mut().clear();
                    Value::Void
                }
                MapTake => {
                    let k = arg();
                    let fallback = arg();
                    target.map_mut().take(&k).unwrap_or(fallback)
                }
                other => unreachable!("not a method that changes its receiver: {other:?}"),
            })
        })
    }

    /// `for (const x of ...)`, going through the collection as it was when
    /// the loop started.
    pub(crate) fn for_of(&self, var: ir::LocalId, iter: &ir::Iter, body: &[ir::Stmt]) -> F<Flow> {
        let slot = var.0 as usize;
        let body = self.block(body);
        // One step of the loop: `None` to go on, or how the loop ends.
        fn step(body: &[F<Flow>], c: &Code, m: &mut Machine, b: usize) -> R<Option<Flow>> {
            Ok(match run(body, c, m, b)? {
                Flow::Normal | Flow::Continue => None,
                Flow::Break => Some(Flow::Normal),
                Flow::Return => Some(Flow::Return),
            })
        }
        match iter {
            ir::Iter::Range { start, end, step: by, loc } => {
                let (s, e, st, at) = (self.int(start), self.int(end), self.int(by), self.loc(*loc));
                node(move |c, m, b| {
                    let (s, e) = (s(c, m, b)?, e(c, m, b)?);
                    let st = st(c, m, b)?;
                    for i in dat_rt::list::try_range_iter(s, e, st).map_err(|err| thrown(err, at))? {
                        m.stack[b + slot] = Value::Int(i);
                        if let Some(flow) = step(&body, c, m, b)? {
                            return Ok(flow);
                        }
                    }
                    Ok(Flow::Normal)
                })
            }
            ir::Iter::Each(e) => {
                let ty = e.ty;
                let coll = self.value(e);
                let is_map = self.p.types.map_types(ty).is_some();
                let is_set = self.p.types.set_elem(ty).is_some();
                node(move |c, m, b| {
                    let v = coll(c, m, b)?;
                    macro_rules! each {
                        ($iter:expr, |$x:pat_param| $value:expr) => {
                            for $x in $iter {
                                m.stack[b + slot] = $value;
                                if let Some(flow) = step(&body, c, m, b)? {
                                    return Ok(flow);
                                }
                            }
                        };
                    }
                    if ty == Type::Str {
                        each!(chars(v.str()).iter(), |x| x.clone());
                    } else if is_map {
                        each!(v.map().iter(), |(k, x)| tuple2(k.clone(), x.clone()));
                    } else if is_set {
                        each!(v.set().iter(), |x| x.clone());
                    } else {
                        each!(v.list().iter(), |x| x.clone());
                    }
                    Ok(Flow::Normal)
                })
            }
        }
    }

    /// `for (mut x of xs)`: each element moves into `x` for its turn of the
    /// body and back after it. The list leaves its place for the loop; the
    /// body can't reach it (E0703), so nothing sees it gone.
    pub(crate) fn for_mut(&self, var: ir::LocalId, list: &ir::Place, body: &[ir::Stmt]) -> F<Flow> {
        let slot = var.0 as usize;
        let place = self.place(list);
        let body = self.block(body);
        node(move |c, m, b| {
            let ops = place.operands(c, m, b)?;
            let mut taken = place.take(m, b, &ops)?;
            let n = taken.list().len() as usize;
            let mut result = Ok(Flow::Normal);
            for i in 0..n {
                let items = taken.list_mut().slice_mut();
                m.stack[b + slot] = std::mem::take(&mut items[i]);
                let r = run(&body, c, m, b);
                taken.list_mut().slice_mut()[i] = std::mem::take(&mut m.stack[b + slot]);
                match r {
                    Ok(Flow::Normal | Flow::Continue) => {}
                    Ok(Flow::Break) => break,
                    other => {
                        result = other;
                        break;
                    }
                }
            }
            place.put(m, b, &ops, taken);
            result
        })
    }
}

enum Elem {
    One(F<Value>),
    Spread(F<Value>),
}

/// The elements a spread gives: a list's, a set's, or a string's characters.
fn spread(v: &Value, mut add: impl FnMut(Value)) {
    match v {
        Value::List(l) => l.iter().for_each(|x| add(x.clone())),
        Value::Set(s) => s.iter().for_each(|x| add(x.clone())),
        Value::Str(s) => dat_rt::text::chars(s.as_str()).iter().for_each(|x| add(Value::Str(x.clone()))),
        _ => unreachable!("type checker guarantees a list, a set or a string"),
    }
}

fn chars(s: &Str) -> List<Value> {
    dat_rt::text::chars(s.as_str()).iter().map(|x| Value::Str(x.clone())).collect()
}

fn tuple2(a: Value, b: Value) -> Value {
    Value::Tuple(Rc::new(vec![a, b]))
}

/// An error that leaves a call or a callback method: the call is where it
/// leaves the caller's frame, unless it was thrown right here.
pub(crate) fn pending(mut err: Box<DatError>, at: Loc) -> Box<DatError> {
    if err.trace.pending.is_none() {
        err.trace.pending = Some(at);
    }
    err
}

/// Calls a function value with `args`, then its captures.
pub(crate) fn call_value(c: &Code, m: &mut Machine, f: &Closure, args: Vec<Value>) -> R<Value> {
    if dat_rt::stack_is_low() {
        return Err(DatError::stack_overflow());
    }
    let base = m.stack.len();
    m.stack.extend(args);
    c.funcs[f.func].run_with(c, m, base, &f.captures)
}
