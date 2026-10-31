//! The interpreter: compiles the typed IR to closures once, then runs them.
//!
//! Each IR node becomes one closure that does the node's work and calls the
//! closures of its children, so a running program never looks at the IR.
//! A closure returns what its node produces: an `int` expression an `i64`,
//! a `bool` expression a `bool`, so most values never go through `Value`.
//! Common shapes, such as a local compared with a constant, get closures of
//! their own.
//!
//! Every observable operation goes through dat-rt, as generated code does.
//! Errors propagate as `Err(Box<DatError>)`; each call they leave records its
//! frame, by the same rule the generated code's frame guards follow.

#![forbid(unsafe_code)]

mod collections;
mod value;

use dat_ir::{self as ir, Builtin, CmpOp, ExprKind, FloatOp, IntOp, Stmt, Type};
use dat_rt::testing::TestInfo;
use dat_rt::{DatError, ErrorKind, Join, Loc, Str, Trace};
use dat_rt::{num, text};
use std::cmp::Ordering;
use std::convert::identity;
use std::rc::Rc;
use std::time::Instant;
use value::Value;

/// The interpreter's stack. Its frames are larger than compiled code's, so
/// it gets more room; both modes guarantee at least 10,000 nested calls.
pub const STACK_SIZE: usize = 512 << 20;

/// How a statement finished. `return` leaves its value in [`Machine::ret`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    Normal,
    Return,
    Break,
    Continue,
}

type R<T> = Result<T, Box<DatError>>;

/// A compiled node. It runs with the program, the machine, and the stack
/// index of the running call's first local.
type F<T> = Box<dyn Fn(&Code, &mut Machine, usize) -> R<T>>;

type Block = Box<[F<Flow>]>;

/// Boxes a closure as a node, which also gives the closure its argument types.
fn node<T>(f: impl Fn(&Code, &mut Machine, usize) -> R<T> + 'static) -> F<T> {
    Box::new(f)
}

/// What running changes.
struct Machine {
    /// The locals of every active call, each call's after its caller's.
    stack: Vec<Value>,
    /// The value of the last `return`, until its call takes it.
    ret: Value,
    /// The values of the `mut` parameters of the call that just finished,
    /// which its caller puts back where they came from.
    out: Vec<Value>,
}

impl Machine {
    fn new() -> Machine {
        Machine { stack: Vec::with_capacity(1 << 16), ret: Value::Void, out: Vec::new() }
    }
}

/// A compiled program.
struct Code {
    file: &'static str,
    funcs: Vec<Func>,
}

struct Func {
    name: &'static str,
    /// The declaration, where a trace places this frame if the error
    /// brought no location in it.
    at: Loc,
    locals: usize,
    body: Block,
    params: usize,
    /// The default values of the last parameters, which see the ones before them.
    defaults: Box<[F<Value>]>,
    /// The locals an arrow function's captured copies go in.
    captures: Box<[usize]>,
    /// The `mut` parameters, whose values go back to the caller.
    by_ref: Box<[usize]>,
}

impl Func {
    /// Runs a call whose arguments are already on the stack, from `base`.
    #[inline]
    fn run(&self, c: &Code, m: &mut Machine, base: usize) -> R<Value> {
        m.stack.resize(base + self.locals, Value::Void);
        let result = run(&self.body, c, m, base);
        self.finish(m, base, result)
    }

    /// Runs a call of a closure: the arguments are on the stack from
    /// `base`, and its captured copies go in their locals.
    pub(crate) fn run_with(&self, c: &Code, m: &mut Machine, base: usize, captures: &[Value]) -> R<Value> {
        m.stack.resize(base + self.locals, Value::Void);
        for (slot, v) in self.captures.iter().zip(captures) {
            m.stack[base + slot] = v.clone();
        }
        let result = run(&self.body, c, m, base);
        self.finish(m, base, result)
    }

    /// Runs a call that gave the first `given` arguments: the defaults of
    /// the others are worked out in this call's frame, so an error in one
    /// is traced to this function.
    fn run_defaulted(&self, c: &Code, m: &mut Machine, base: usize, given: usize) -> R<Value> {
        m.stack.resize(base + self.locals, Value::Void);
        let first = self.params - self.defaults.len();
        let mut result = Ok(Flow::Normal);
        for i in given..self.params {
            match self.defaults[i - first](c, m, base) {
                Ok(v) => m.stack[base + i] = v,
                Err(e) => {
                    result = Err(e);
                    break;
                }
            }
        }
        if result.is_ok() {
            result = run(&self.body, c, m, base);
        }
        self.finish(m, base, result)
    }

    #[inline]
    fn finish(&self, m: &mut Machine, base: usize, result: R<Flow>) -> R<Value> {
        if !self.by_ref.is_empty() {
            m.out.clear();
            for &i in self.by_ref.iter() {
                let v = std::mem::take(&mut m.stack[base + i]);
                m.out.push(v);
            }
        }
        m.stack.truncate(base);
        match result {
            Ok(Flow::Return) => Ok(std::mem::take(&mut m.ret)),
            Ok(_) => Ok(Value::Void),
            Err(mut e) => {
                e.trace.leave(self.name, self.at);
                Err(e)
            }
        }
    }
}

/// Runs a block's statements until one doesn't finish normally.
#[inline]
fn run(b: &[F<Flow>], c: &Code, m: &mut Machine, base: usize) -> R<Flow> {
    for s in b {
        match s(c, m, base)? {
            Flow::Normal => {}
            other => return Ok(other),
        }
    }
    Ok(Flow::Normal)
}

/// `err`, thrown at `at`.
#[cold]
#[inline(never)]
fn thrown(mut err: Box<DatError>, at: Loc) -> Box<DatError> {
    err.trace = Trace::at(at);
    err
}

/// `DAT_FAULT=interp-plus-one`'s `+`: wrong, to prove the differential harness notices.
fn add_plus_one(a: i64, b: i64) -> R<i64> {
    dat_rt::int::try_add(a, b).map(|v| v.wrapping_add(1))
}

impl Code {
    fn compile(program: &ir::Program) -> Code {
        let leak = |s: &str| -> &'static str { Box::leak(s.into()) };
        let file = leak(&program.file);
        let fault_plus_one = std::env::var("DAT_FAULT").is_ok_and(|v| v == "interp-plus-one");
        let funcs = program
            .functions
            .iter()
            .map(|f| {
                let cx = Compiler { p: program, file, func: leak(&f.name), fault_plus_one, copies: dat_rt::copies::on() };
                Func {
                    name: cx.func,
                    at: cx.loc(f.loc),
                    locals: f.locals.len(),
                    body: cx.block(&f.body),
                    params: f.params as usize,
                    defaults: f.defaults.iter().map(|d| cx.value(d)).collect(),
                    captures: f.captures.iter().map(|l| l.0 as usize).collect(),
                    by_ref: (0..f.params as usize).filter(|&i| f.locals[i].by_ref).collect(),
                }
            })
            .collect();
        Code { file, funcs }
    }
}

/// Compiles one function.
struct Compiler<'p> {
    p: &'p ir::Program,
    file: &'static str,
    func: &'static str,
    fault_plus_one: bool,
    /// `--report-copies`: each change says where it is while it runs.
    copies: bool,
}

impl Compiler<'_> {
    fn loc(&self, l: ir::Loc) -> Loc {
        Loc { file: self.file, func: self.func, line: l.line, col: l.col }
    }

    /// `f`, which changes something at `loc`, saying so to the copy
    /// report while it runs. The same IR nodes say so in generated code.
    fn at<T: 'static>(&self, loc: Option<ir::Loc>, f: F<T>) -> F<T> {
        let Some(loc) = loc.filter(|_| self.copies) else { return f };
        let at = self.loc(loc);
        node(move |c, m, b| {
            let _at = dat_rt::copies::at(at);
            f(c, m, b)
        })
    }

    fn block(&self, b: &[Stmt]) -> Block {
        b.iter().map(|s| self.stmt(s)).collect()
    }

    fn stmt(&self, s: &Stmt) -> F<Flow> {
        match s {
            Stmt::Set(l, e, loc) => match e.appended_to(*l) {
                Some(parts) => self.at(Some(*loc), self.append(l.0 as usize, &parts)),
                None => self.store(l.0 as usize, e),
            },
            Stmt::Let(l, e) => self.store(l.0 as usize, e),
            Stmt::Store(place, e) => self.at(place.loc(), self.store_place(place, e)),
            Stmt::ForOf { var, iter, body } => self.for_of(*var, iter, body),
            Stmt::ForMut { var, list, body, loc } => self.at(Some(*loc), self.for_mut(*var, list, body)),
            Stmt::Expr(e) => {
                let e = self.effect(e);
                node(move |c, m, b| e(c, m, b).map(|()| Flow::Normal))
            }
            Stmt::If(cond, then, els) => {
                let cond = self.bool(cond);
                let then = self.block(then);
                if els.is_empty() {
                    return node(move |c, m, b| if cond(c, m, b)? { run(&then, c, m, b) } else { Ok(Flow::Normal) });
                }
                let els = self.block(els);
                node(move |c, m, b| if cond(c, m, b)? { run(&then, c, m, b) } else { run(&els, c, m, b) })
            }
            Stmt::Loop { cond, body, step } => {
                let cond = cond.as_ref().map(|e| self.bool(e));
                let body = self.block(body);
                let step = self.block(step);
                node(move |c, m, b| {
                    loop {
                        if let Some(cond) = &cond
                            && !cond(c, m, b)?
                        {
                            break;
                        }
                        match run(&body, c, m, b)? {
                            Flow::Normal | Flow::Continue => {}
                            Flow::Break => break,
                            Flow::Return => return Ok(Flow::Return),
                        }
                        run(&step, c, m, b)?;
                    }
                    Ok(Flow::Normal)
                })
            }
            Stmt::Break => node(|_, _, _| Ok(Flow::Break)),
            Stmt::Continue => node(|_, _, _| Ok(Flow::Continue)),
            Stmt::Return(None) => node(|_, m, _| {
                m.ret = Value::Void;
                Ok(Flow::Return)
            }),
            Stmt::Return(Some(e)) => {
                let e = self.value(e);
                node(move |c, m, b| {
                    m.ret = e(c, m, b)?;
                    Ok(Flow::Return)
                })
            }
            Stmt::Throw(e, loc) => {
                let e = self.error(e);
                let at = self.loc(*loc);
                node(move |c, m, b| {
                    let v = e(c, m, b)?;
                    Err(thrown(Box::new((*v).clone()), at))
                })
            }
            Stmt::Try { body, catches } => {
                let body = self.block(body);
                let catches: Box<[_]> = catches.iter().map(|c| (c.filter, c.bind.map(|l| l.0 as usize), self.block(&c.body))).collect();
                node(move |c, m, b| {
                    let err = match run(&body, c, m, b) {
                        Ok(flow) => return Ok(flow),
                        Err(err) => err,
                    };
                    for (filter, bind, body) in &catches {
                        let caught = match filter {
                            ir::CatchFilter::Is(t) => kind_of(*t) == err.kind,
                            ir::CatchFilter::All | ir::CatchFilter::AllAsError => true,
                        };
                        if !caught {
                            continue;
                        }
                        if let Some(slot) = bind {
                            let value = if *filter == ir::CatchFilter::AllAsError { (*err).into_error() } else { *err };
                            m.stack[b + slot] = Value::Error(Rc::new(value));
                        }
                        return run(body, c, m, b);
                    }
                    Err(err)
                })
            }
        }
    }

    fn store(&self, slot: usize, e: &ir::Expr) -> F<Flow> {
        let e = self.value(e);
        node(move |c, m, b| {
            m.stack[b + slot] = e(c, m, b)?;
            Ok(Flow::Normal)
        })
    }

    /// `s = s + ...` and `s += ...`, which append in place. Every part runs
    /// before the string changes, so a part that throws leaves it as it was;
    /// running them after reading `s` changes nothing, since no expression
    /// can assign a local.
    fn append(&self, slot: usize, parts: &[&ir::Expr]) -> F<Flow> {
        fn target(m: &mut Machine, i: usize) -> &mut Str {
            match &mut m.stack[i] {
                Value::Str(s) => s,
                _ => unreachable!("type checker guarantees a string"),
            }
        }
        if let [x] = parts {
            let x = self.str(x);
            return node(move |c, m, b| {
                let x = x(c, m, b)?;
                target(m, b + slot).push_str(x.as_str());
                Ok(Flow::Normal)
            });
        }
        let (pieces, cap) = self.pieces(parts);
        node(move |c, m, b| {
            let mut j = Join::with_capacity(cap);
            fill(&pieces, &mut j, c, m, b)?;
            target(m, b + slot).push_str(j.as_str());
            Ok(Flow::Normal)
        })
    }

    /// An expression of any type, as a `Value`.
    fn value(&self, e: &ir::Expr) -> F<Value> {
        match (&e.kind, e.ty) {
            (ExprKind::Local(l), _) => {
                let l = l.0 as usize;
                node(move |_, m, b| Ok(m.stack[b + l].clone()))
            }
            (ExprKind::Move(l), _) => {
                let l = l.0 as usize;
                node(move |_, m, b| Ok(std::mem::take(&mut m.stack[b + l])))
            }
            (ExprKind::Call { func, args, loc }, _) => self.call(*func, args, *loc, identity),
            (k, _) if is_general(k) => self.general(e),
            (ExprKind::Cond { cond, then, els }, Type::Of(_)) => self.cond(cond, self.value(then), self.value(els)),
            (ExprKind::Diverge(a), Type::Of(_)) => self.diverge(a),
            (ExprKind::Ref(_), _) => unreachable!("a `mut` argument is compiled by its call"),
            (_, Type::Of(_)) => unreachable!("not an expression of a compound type: {e:?}"),
            (_, Type::Int) => self.int_to(e, Value::Int),
            (_, Type::Float) => self.float_to(e, Value::Float),
            (_, Type::Bool) => self.bool_to(e, Value::Bool),
            (_, Type::Str) => {
                let s = self.str(e);
                node(move |c, m, b| s(c, m, b).map(Value::Str))
            }
            (_, Type::Error(_)) => {
                let x = self.error(e);
                node(move |c, m, b| x(c, m, b).map(Value::Error))
            }
            (_, Type::Void | Type::Never) => {
                let x = self.effect(e);
                node(move |c, m, b| x(c, m, b).map(|()| Value::Void))
            }
        }
    }

    /// An expression whose value, if it has one, is dropped.
    fn effect(&self, e: &ir::Expr) -> F<()> {
        let no_value = matches!(e.ty, Type::Void | Type::Never);
        match &e.kind {
            ExprKind::Call { func, args, loc } => self.call(*func, args, *loc, drop),
            ExprKind::Print(a) => {
                let a = self.str(a);
                node(move |c, m, b| {
                    dat_rt::print_str(a(c, m, b)?.as_str());
                    Ok(())
                })
            }
            ExprKind::Assert(a) => self.assert(a),
            ExprKind::Fail { message, loc } => {
                let message = self.str(message);
                let at = self.loc(*loc);
                node(move |c, m, b| {
                    let text = message(c, m, b)?;
                    Err(thrown(Box::new(DatError::new(ErrorKind::AssertionError, text.as_str())), at))
                })
            }
            ExprKind::Diverge(a) => self.diverge(a),
            k if is_general(k) => {
                let v = self.general(e);
                node(move |c, m, b| v(c, m, b).map(drop))
            }
            ExprKind::Cond { cond, then, els } if no_value => self.cond(cond, self.effect(then), self.effect(els)),
            _ if no_value => unreachable!("an expression with neither a value nor an effect: {e:?}"),
            _ => {
                let v = self.value(e);
                node(move |c, m, b| v(c, m, b).map(drop))
            }
        }
    }

    /// A call, whose result `out` converts.
    fn call<T: 'static>(&self, func: ir::FuncId, args: &[ir::Expr], loc: ir::Loc, out: impl Fn(Value) -> T + 'static) -> F<T> {
        if args.iter().any(|a| matches!(a.kind, ExprKind::Ref(_))) {
            return self.at(Some(loc), self.call_refs(func, args, loc, out));
        }
        let id = func.0 as usize;
        let given = args.len();
        let args: Box<[F<Value>]> = args.iter().map(|a| self.value(a)).collect();
        let at = self.loc(loc);
        node(move |c, m, b| {
            if dat_rt::stack_is_low() {
                // Thrown in the caller, at the call, as in compiled code.
                return Err(thrown(DatError::stack_overflow(), at));
            }
            let callee = m.stack.len();
            for a in &args {
                match a(c, m, b) {
                    Ok(v) => m.stack.push(v),
                    Err(err) => {
                        m.stack.truncate(callee);
                        return Err(err);
                    }
                }
            }
            let f = &c.funcs[id];
            let result = if given < f.params { f.run_defaulted(c, m, callee, given) } else { f.run(c, m, callee) };
            match result {
                Ok(v) => Ok(out(v)),
                Err(mut err) => {
                    // The error now leaves the caller's frame from this call.
                    if err.trace.pending.is_none() {
                        err.trace.pending = Some(at);
                    }
                    Err(err)
                }
            }
        })
    }

    fn cond<T: 'static>(&self, cond: &ir::Expr, then: F<T>, els: F<T>) -> F<T> {
        let cond = self.bool(cond);
        node(move |c, m, b| if cond(c, m, b)? { then(c, m, b) } else { els(c, m, b) })
    }

    /// An expression of type `never` where a value is expected.
    fn diverge<T: 'static>(&self, a: &ir::Expr) -> F<T> {
        let a = self.effect(a);
        node(move |c, m, b| {
            a(c, m, b)?;
            unreachable!("a never-typed expression finished")
        })
    }

    fn int(&self, e: &ir::Expr) -> F<i64> {
        self.int_to(e, identity)
    }

    /// An `int` expression, whose value `out` converts.
    fn int_to<T: 'static, O: Fn(i64) -> T + Copy + 'static>(&self, e: &ir::Expr, out: O) -> F<T> {
        match &e.kind {
            ExprKind::Int(n) => {
                let n = *n;
                node(move |_, _, _| Ok(out(n)))
            }
            ExprKind::Local(l) => {
                let l = l.0 as usize;
                node(move |_, m, b| Ok(out(m.stack[b + l].int())))
            }
            ExprKind::IntArith { op, lhs, rhs, loc } => {
                let at = self.loc(*loc);
                match op {
                    IntOp::Add if self.fault_plus_one => self.arith(add_plus_one, lhs, rhs, at, out),
                    IntOp::Add => self.arith(dat_rt::int::try_add, lhs, rhs, at, out),
                    IntOp::Sub => self.arith(dat_rt::int::try_sub, lhs, rhs, at, out),
                    IntOp::Mul => self.arith(dat_rt::int::try_mul, lhs, rhs, at, out),
                    IntOp::Rem => self.arith(dat_rt::int::try_rem, lhs, rhs, at, out),
                }
            }
            ExprKind::IntNeg { arg, loc } => {
                let arg = self.int(arg);
                let at = self.loc(*loc);
                node(move |c, m, b| match dat_rt::int::try_neg(arg(c, m, b)?) {
                    Ok(v) => Ok(out(v)),
                    Err(err) => Err(thrown(err, at)),
                })
            }
            ExprKind::Call { func, args, loc } => self.call(*func, args, *loc, move |v| out(v.int())),
            ExprKind::Cond { cond, then, els } => self.cond(cond, self.int_to(then, out), self.int_to(els, out)),
            ExprKind::Diverge(a) => self.diverge(a),
            ExprKind::Builtin { op, args, loc } => self.builtin_int(*op, args, self.loc(*loc), out),
            k if is_general(k) => {
                let v = self.general(e);
                node(move |c, m, b| Ok(out(v(c, m, b)?.int())))
            }
            _ => unreachable!("not an int expression: {e:?}"),
        }
    }

    /// Checked arithmetic with `op`, one of dat-rt's, by the shape of its operands.
    fn arith<T, O, A>(&self, op: A, lhs: &ir::Expr, rhs: &ir::Expr, at: Loc, out: O) -> F<T>
    where
        T: 'static,
        O: Fn(i64) -> T + Copy + 'static,
        A: Fn(i64, i64) -> R<i64> + Copy + 'static,
    {
        let done = move |r: R<i64>| match r {
            Ok(v) => Ok(out(v)),
            Err(err) => Err(thrown(err, at)),
        };
        match (&lhs.kind, &rhs.kind) {
            (ExprKind::Local(x), ExprKind::Int(k)) => {
                let (x, k) = (x.0 as usize, *k);
                node(move |_, m, b| done(op(m.stack[b + x].int(), k)))
            }
            (ExprKind::Local(x), ExprKind::Local(y)) => {
                let (x, y) = (x.0 as usize, y.0 as usize);
                node(move |_, m, b| done(op(m.stack[b + x].int(), m.stack[b + y].int())))
            }
            (_, ExprKind::Int(k)) => {
                let (lhs, k) = (self.int(lhs), *k);
                node(move |c, m, b| done(op(lhs(c, m, b)?, k)))
            }
            (ExprKind::Int(k), _) => {
                let (k, rhs) = (*k, self.int(rhs));
                node(move |c, m, b| done(op(k, rhs(c, m, b)?)))
            }
            _ => {
                let (lhs, rhs) = (self.int(lhs), self.int(rhs));
                node(move |c, m, b| {
                    let x = lhs(c, m, b)?;
                    done(op(x, rhs(c, m, b)?))
                })
            }
        }
    }

    fn float(&self, e: &ir::Expr) -> F<f64> {
        self.float_to(e, identity)
    }

    /// A `float` expression, whose value `out` converts.
    fn float_to<T: 'static, O: Fn(f64) -> T + Copy + 'static>(&self, e: &ir::Expr, out: O) -> F<T> {
        match &e.kind {
            ExprKind::Float(x) => {
                let x = *x;
                node(move |_, _, _| Ok(out(x)))
            }
            ExprKind::Local(l) => {
                let l = l.0 as usize;
                node(move |_, m, b| Ok(out(m.stack[b + l].float())))
            }
            ExprKind::FloatArith { op, lhs, rhs } => match op {
                FloatOp::Add => self.float_arith(|x, y| x + y, lhs, rhs, out),
                FloatOp::Sub => self.float_arith(|x, y| x - y, lhs, rhs, out),
                FloatOp::Mul => self.float_arith(|x, y| x * y, lhs, rhs, out),
                FloatOp::Div => self.float_arith(|x, y| x / y, lhs, rhs, out),
                FloatOp::Rem => self.float_arith(|x, y| x % y, lhs, rhs, out),
            },
            ExprKind::FloatNeg(a) => {
                let a = self.float(a);
                node(move |c, m, b| Ok(out(-a(c, m, b)?)))
            }
            ExprKind::Call { func, args, loc } => self.call(*func, args, *loc, move |v| out(v.float())),
            ExprKind::Cond { cond, then, els } => self.cond(cond, self.float_to(then, out), self.float_to(els, out)),
            ExprKind::Diverge(a) => self.diverge(a),
            ExprKind::Builtin { op, args, loc } => self.builtin_float(*op, args, self.loc(*loc), out),
            k if is_general(k) => {
                let v = self.general(e);
                node(move |c, m, b| Ok(out(v(c, m, b)?.float())))
            }
            _ => unreachable!("not a float expression: {e:?}"),
        }
    }

    /// Float arithmetic with `op`, by the shape of its operands.
    fn float_arith<T, O, A>(&self, op: A, lhs: &ir::Expr, rhs: &ir::Expr, out: O) -> F<T>
    where
        T: 'static,
        O: Fn(f64) -> T + Copy + 'static,
        A: Fn(f64, f64) -> f64 + Copy + 'static,
    {
        match (&lhs.kind, &rhs.kind) {
            (ExprKind::Local(x), ExprKind::Float(k)) => {
                let (x, k) = (x.0 as usize, *k);
                node(move |_, m, b| Ok(out(op(m.stack[b + x].float(), k))))
            }
            (ExprKind::Local(x), ExprKind::Local(y)) => {
                let (x, y) = (x.0 as usize, y.0 as usize);
                node(move |_, m, b| Ok(out(op(m.stack[b + x].float(), m.stack[b + y].float()))))
            }
            (_, ExprKind::Float(k)) => {
                let (lhs, k) = (self.float(lhs), *k);
                node(move |c, m, b| Ok(out(op(lhs(c, m, b)?, k))))
            }
            (ExprKind::Float(k), _) => {
                let (k, rhs) = (*k, self.float(rhs));
                node(move |c, m, b| Ok(out(op(k, rhs(c, m, b)?))))
            }
            _ => {
                let (lhs, rhs) = (self.float(lhs), self.float(rhs));
                node(move |c, m, b| {
                    let x = lhs(c, m, b)?;
                    Ok(out(op(x, rhs(c, m, b)?)))
                })
            }
        }
    }

    fn bool(&self, e: &ir::Expr) -> F<bool> {
        self.bool_to(e, identity)
    }

    /// A `bool` expression, whose value `out` converts.
    fn bool_to<T: 'static, O: Fn(bool) -> T + Copy + 'static>(&self, e: &ir::Expr, out: O) -> F<T> {
        match &e.kind {
            ExprKind::Bool(v) => {
                let v = *v;
                node(move |_, _, _| Ok(out(v)))
            }
            ExprKind::Local(l) => {
                let l = l.0 as usize;
                node(move |_, m, b| Ok(out(m.stack[b + l].bool())))
            }
            ExprKind::Compare { op, lhs, rhs } => match (lhs.ty, rhs.ty) {
                (Type::Int, Type::Float) => {
                    let (op, lhs, rhs) = (rt_cmp(*op), self.int(lhs), self.float(rhs));
                    node(move |c, m, b| {
                        let x = lhs(c, m, b)?;
                        Ok(out(num::holds(op, num::cmp_int_float(x, rhs(c, m, b)?))))
                    })
                }
                (Type::Float, Type::Int) => {
                    let (op, lhs, rhs) = (rt_cmp(*op), self.float(lhs), self.int(rhs));
                    node(move |c, m, b| {
                        let x = lhs(c, m, b)?;
                        Ok(out(num::holds(op, num::cmp_float_int(x, rhs(c, m, b)?))))
                    })
                }
                (Type::Float, _) => match op {
                    CmpOp::Lt => self.compare_floats(|x, y| x < y, lhs, rhs, out),
                    CmpOp::Le => self.compare_floats(|x, y| x <= y, lhs, rhs, out),
                    CmpOp::Gt => self.compare_floats(|x, y| x > y, lhs, rhs, out),
                    CmpOp::Ge => self.compare_floats(|x, y| x >= y, lhs, rhs, out),
                    CmpOp::Eq => self.compare_floats(|x, y| x == y, lhs, rhs, out),
                    CmpOp::Ne => self.compare_floats(|x, y| x != y, lhs, rhs, out),
                },
                (Type::Int, _) => match op {
                    CmpOp::Lt => self.compare_ints(|x, y| x < y, lhs, rhs, out),
                    CmpOp::Le => self.compare_ints(|x, y| x <= y, lhs, rhs, out),
                    CmpOp::Gt => self.compare_ints(|x, y| x > y, lhs, rhs, out),
                    CmpOp::Ge => self.compare_ints(|x, y| x >= y, lhs, rhs, out),
                    CmpOp::Eq => self.compare_ints(|x, y| x == y, lhs, rhs, out),
                    CmpOp::Ne => self.compare_ints(|x, y| x != y, lhs, rhs, out),
                },
                (Type::Bool, _) => {
                    let (op, lhs, rhs) = (*op, self.bool(lhs), self.bool(rhs));
                    node(move |c, m, b| {
                        let x = lhs(c, m, b)?;
                        Ok(out(holds(op, x.cmp(&rhs(c, m, b)?))))
                    })
                }
                (Type::Of(_), _) => {
                    let (eq, lhs, rhs) = (*op == CmpOp::Eq, self.value(lhs), self.value(rhs));
                    node(move |c, m, b| {
                        let x = lhs(c, m, b)?;
                        Ok(out(dat_rt::Equal::equal(&x, &rhs(c, m, b)?) == eq))
                    })
                }
                (Type::Str, _) => {
                    let (op, lhs, rhs) = (*op, self.str(lhs), self.str(rhs));
                    node(move |c, m, b| {
                        let x = lhs(c, m, b)?;
                        Ok(out(holds(op, x.as_str().cmp(rhs(c, m, b)?.as_str()))))
                    })
                }
                _ => unreachable!("type checker guarantees comparable operands"),
            },
            ExprKind::Not(a) => {
                let a = self.bool(a);
                node(move |c, m, b| Ok(out(!a(c, m, b)?)))
            }
            ExprKind::And(x, y) => {
                let (x, y) = (self.bool(x), self.bool(y));
                node(move |c, m, b| Ok(out(x(c, m, b)? && y(c, m, b)?)))
            }
            ExprKind::Or(x, y) => {
                let (x, y) = (self.bool(x), self.bool(y));
                node(move |c, m, b| Ok(out(x(c, m, b)? || y(c, m, b)?)))
            }
            ExprKind::Call { func, args, loc } => self.call(*func, args, *loc, move |v| out(v.bool())),
            ExprKind::Cond { cond, then, els } => self.cond(cond, self.bool_to(then, out), self.bool_to(els, out)),
            ExprKind::Diverge(a) => self.diverge(a),
            ExprKind::Builtin { op, args, .. } => self.builtin_bool(*op, args, out),
            k if is_general(k) => {
                let v = self.general(e);
                node(move |c, m, b| Ok(out(v(c, m, b)?.bool())))
            }
            _ => unreachable!("not a bool expression: {e:?}"),
        }
    }

    /// A `float` comparison with `p`, by the shape of its operands.
    fn compare_floats<T, O, P>(&self, p: P, lhs: &ir::Expr, rhs: &ir::Expr, out: O) -> F<T>
    where
        T: 'static,
        O: Fn(bool) -> T + Copy + 'static,
        P: Fn(f64, f64) -> bool + Copy + 'static,
    {
        match (&lhs.kind, &rhs.kind) {
            (ExprKind::Local(x), ExprKind::Float(k)) => {
                let (x, k) = (x.0 as usize, *k);
                node(move |_, m, b| Ok(out(p(m.stack[b + x].float(), k))))
            }
            (ExprKind::Local(x), ExprKind::Local(y)) => {
                let (x, y) = (x.0 as usize, y.0 as usize);
                node(move |_, m, b| Ok(out(p(m.stack[b + x].float(), m.stack[b + y].float()))))
            }
            _ => {
                let (lhs, rhs) = (self.float(lhs), self.float(rhs));
                node(move |c, m, b| {
                    let x = lhs(c, m, b)?;
                    Ok(out(p(x, rhs(c, m, b)?)))
                })
            }
        }
    }

    /// The builtins that give an `int`.
    fn builtin_int<T: 'static, O: Fn(i64) -> T + Copy + 'static>(&self, op: Builtin, args: &[ir::Expr], at: Loc, out: O) -> F<T> {
        let i = |k: usize| self.int(&args[k]);
        let f = |k: usize| self.float(&args[k]);
        let s = |k: usize| self.str(&args[k]);
        match op {
            Builtin::IntAbs => op1(i(0), num::try_int_abs, at, out),
            Builtin::IntMin => op2(i(0), i(1), |a, b| Ok(num::int_min(a, b)), at, out),
            Builtin::IntMax => op2(i(0), i(1), |a, b| Ok(num::int_max(a, b)), at, out),
            Builtin::IntPow => op2(i(0), i(1), num::try_int_pow, at, out),
            Builtin::FloatToInt => op1(f(0), num::try_float_to_int, at, out),
            Builtin::FloatFloor => op1(f(0), num::try_float_floor, at, out),
            Builtin::FloatCeil => op1(f(0), num::try_float_ceil, at, out),
            Builtin::FloatRound => op1(f(0), num::try_float_round, at, out),
            Builtin::DivToInt => op2(i(0), i(1), num::try_div_to_int, at, out),
            Builtin::DivFloor => op2(i(0), i(1), num::try_div_floor, at, out),
            Builtin::DivCeil => op2(i(0), i(1), num::try_div_ceil, at, out),
            Builtin::DivRound => op2(i(0), i(1), num::try_div_round, at, out),
            Builtin::StrLength => op1(s(0), |a: Str| Ok(text::length(a.as_str())), at, out),
            Builtin::StrIndexOf => op2(s(0), s(1), |a: Str, b: Str| Ok(text::index_of(a.as_str(), b.as_str())), at, out),
            Builtin::StrLastIndexOf => op2(s(0), s(1), |a: Str, b: Str| Ok(text::last_index_of(a.as_str(), b.as_str())), at, out),
            _ => unreachable!("not an int builtin: {op:?}"),
        }
    }

    /// The builtins that give a `float`.
    fn builtin_float<T: 'static, O: Fn(f64) -> T + Copy + 'static>(&self, op: Builtin, args: &[ir::Expr], at: Loc, out: O) -> F<T> {
        let i = |k: usize| self.int(&args[k]);
        let f = |k: usize| self.float(&args[k]);
        match op {
            Builtin::IntSqrt => op1(i(0), |a| Ok(num::int_sqrt(a)), at, out),
            Builtin::IntToFloat => op1(i(0), |a| Ok(num::int_to_float(a)), at, out),
            Builtin::FloatAbs => op1(f(0), |a| Ok(num::float_abs(a)), at, out),
            Builtin::FloatSqrt => op1(f(0), |a| Ok(num::float_sqrt(a)), at, out),
            Builtin::FloatPow => op2(f(0), f(1), |a, b| Ok(num::float_pow(a, b)), at, out),
            Builtin::FloatMin => op2(f(0), f(1), |a, b| Ok(num::float_min(a, b)), at, out),
            Builtin::FloatMax => op2(f(0), f(1), |a, b| Ok(num::float_max(a, b)), at, out),
            _ => unreachable!("not a float builtin: {op:?}"),
        }
    }

    /// The builtins that give a `bool`. None of them throws.
    fn builtin_bool<T: 'static, O: Fn(bool) -> T + Copy + 'static>(&self, op: Builtin, args: &[ir::Expr], out: O) -> F<T> {
        let s = |k: usize| self.str(&args[k]);
        let at = Loc { file: self.file, func: self.func, line: 0, col: 0 };
        let two = |p: fn(&str, &str) -> bool| op2(s(0), s(1), move |a: Str, b: Str| Ok(p(a.as_str(), b.as_str())), at, out);
        match op {
            Builtin::FloatIsNaN => op1(self.float(&args[0]), |a| Ok(num::float_is_nan(a)), at, out),
            Builtin::StrIsEmpty => op1(s(0), |a: Str| Ok(text::is_empty(a.as_str())), at, out),
            Builtin::StrIncludes => two(text::includes),
            Builtin::StrStartsWith => two(text::starts_with),
            Builtin::StrEndsWith => two(text::ends_with),
            Builtin::StrIn => two(text::is_in),
            _ => unreachable!("not a bool builtin: {op:?}"),
        }
    }

    /// The builtins that give a `string`.
    fn builtin_str(&self, op: Builtin, args: &[ir::Expr], at: Loc) -> F<Str> {
        let i = |k: usize| self.int(&args[k]);
        let s = |k: usize| self.str(&args[k]);
        let id = identity;
        let one = |p: fn(&str) -> Str| op1(s(0), move |a: Str| Ok(p(a.as_str())), at, id);
        match op {
            Builtin::FloatToFixed => op2(self.float(&args[0]), i(1), num::try_float_to_fixed, at, id),
            Builtin::StrSlice => op3(s(0), i(1), i(2), |a: Str, x, y| text::try_slice(a.as_str(), x, y), at),
            Builtin::StrTrim => one(text::trim),
            Builtin::StrTrimStart => one(text::trim_start),
            Builtin::StrTrimEnd => one(text::trim_end),
            Builtin::StrToUpperCase => one(text::to_upper_case),
            Builtin::StrToLowerCase => one(text::to_lower_case),
            Builtin::StrReplace => {
                op3(s(0), s(1), s(2), |a: Str, x: Str, y: Str| Ok(text::replace(a.as_str(), x.as_str(), y.as_str())), at)
            }
            Builtin::StrReplaceAll => {
                op3(s(0), s(1), s(2), |a: Str, x: Str, y: Str| Ok(text::replace_all(a.as_str(), x.as_str(), y.as_str())), at)
            }
            Builtin::StrRepeat => op2(s(0), i(1), |a: Str, n| text::try_repeat(a.as_str(), n), at, id),
            Builtin::StrPadStart => op3(s(0), i(1), s(2), |a: Str, n, f: Str| text::try_pad_start(a.as_str(), n, f.as_str()), at),
            Builtin::StrPadEnd => op3(s(0), i(1), s(2), |a: Str, n, f: Str| text::try_pad_end(a.as_str(), n, f.as_str()), at),
            Builtin::StrAt => op2(s(0), i(1), |a: Str, n| text::try_at(a.as_str(), n), at, id),
            _ => unreachable!("not a string builtin: {op:?}"),
        }
    }

    /// An `int` comparison with `p`, by the shape of its operands.
    fn compare_ints<T, O, P>(&self, p: P, lhs: &ir::Expr, rhs: &ir::Expr, out: O) -> F<T>
    where
        T: 'static,
        O: Fn(bool) -> T + Copy + 'static,
        P: Fn(i64, i64) -> bool + Copy + 'static,
    {
        match (&lhs.kind, &rhs.kind) {
            (ExprKind::Local(x), ExprKind::Int(k)) => {
                let (x, k) = (x.0 as usize, *k);
                node(move |_, m, b| Ok(out(p(m.stack[b + x].int(), k))))
            }
            (ExprKind::Local(x), ExprKind::Local(y)) => {
                let (x, y) = (x.0 as usize, y.0 as usize);
                node(move |_, m, b| Ok(out(p(m.stack[b + x].int(), m.stack[b + y].int()))))
            }
            (_, ExprKind::Int(k)) => {
                let (lhs, k) = (self.int(lhs), *k);
                node(move |c, m, b| Ok(out(p(lhs(c, m, b)?, k))))
            }
            _ => {
                let (lhs, rhs) = (self.int(lhs), self.int(rhs));
                node(move |c, m, b| {
                    let x = lhs(c, m, b)?;
                    Ok(out(p(x, rhs(c, m, b)?)))
                })
            }
        }
    }

    fn str(&self, e: &ir::Expr) -> F<Str> {
        match &e.kind {
            // Under `--report-copies`, a literal is made each time, as in
            // generated code; otherwise appending to it would copy it.
            ExprKind::Str(s) if self.copies => {
                let s: Box<str> = s.as_str().into();
                node(move |_, _, _| Ok(Str::new(&s)))
            }
            ExprKind::Str(s) => {
                let s = Str::new(s);
                node(move |_, _, _| Ok(s.clone()))
            }
            ExprKind::Local(l) => {
                let l = l.0 as usize;
                node(move |_, m, b| Ok(m.stack[b + l].str().clone()))
            }
            ExprKind::Move(l) => {
                let l = l.0 as usize;
                node(move |_, m, b| Ok(std::mem::take(&mut m.stack[b + l]).into_str()))
            }
            ExprKind::Call { func, args, loc } => self.call(*func, args, *loc, Value::into_str),
            ExprKind::Cond { cond, then, els } => self.cond(cond, self.str(then), self.str(els)),
            ExprKind::Concat(_) => self.join(e),
            ExprKind::Builtin { op, args, loc } => self.builtin_str(*op, args, self.loc(*loc)),
            ExprKind::ToStr(a) => match a.ty {
                Type::Int => self.int_to(a, dat_rt::int_to_str),
                Type::Float => self.float_to(a, dat_rt::float_to_str),
                Type::Bool => self.bool_to(a, dat_rt::bool_to_str),
                Type::Str => self.str(a),
                Type::Error(_) => {
                    let a = self.error(a);
                    node(move |c, m, b| Ok(a(c, m, b)?.to_str()))
                }
                Type::Of(_) => {
                    let a = self.value(a);
                    node(move |c, m, b| Ok(dat_rt::text(&a(c, m, b)?)))
                }
                _ => unreachable!("void has no string form"),
            },
            ExprKind::ErrorMessage(a) => {
                let a = self.error(a);
                node(move |c, m, b| Ok(a(c, m, b)?.message_str()))
            }
            ExprKind::Diverge(a) => self.diverge(a),
            k if is_general(k) => {
                let v = self.general(e);
                node(move |c, m, b| Ok(v(c, m, b)?.into_str()))
            }
            _ => unreachable!("not a string expression: {e:?}"),
        }
    }

    fn join(&self, e: &ir::Expr) -> F<Str> {
        let (pieces, cap) = self.pieces(&e.joined_parts());
        node(move |c, m, b| {
            let mut j = Join::with_capacity(cap);
            fill(&pieces, &mut j, c, m, b)?;
            Ok(j.finish())
        })
    }

    /// The parts of a join, and a first guess at its length.
    fn pieces(&self, parts: &[&ir::Expr]) -> (Box<[Piece]>, usize) {
        let pieces: Box<[Piece]> = parts
            .iter()
            .map(|p| match &p.kind {
                ExprKind::Str(text) => Piece::Text(text.as_str().into()),
                ExprKind::Local(l) => Piece::Local(l.0 as usize),
                ExprKind::ToStr(a) if a.ty == Type::Int => Piece::Int(self.int(a)),
                ExprKind::ToStr(a) if a.ty == Type::Float => Piece::Float(self.float(a)),
                ExprKind::ToStr(a) if a.ty == Type::Bool => Piece::Bool(self.bool(a)),
                _ => Piece::Str(self.str(p)),
            })
            .collect();
        let cap = pieces.iter().map(|p| if let Piece::Text(t) = p { t.len() } else { 16 }).sum();
        (pieces, cap)
    }

    fn error(&self, e: &ir::Expr) -> F<Rc<DatError>> {
        match &e.kind {
            ExprKind::Local(l) | ExprKind::Move(l) => {
                let l = l.0 as usize;
                node(move |_, m, b| Ok(m.stack[b + l].error().clone()))
            }
            ExprKind::Call { func, args, loc } => self.call(*func, args, *loc, Value::into_error),
            ExprKind::Cond { cond, then, els } => self.cond(cond, self.error(then), self.error(els)),
            ExprKind::NewError { kind, message } => {
                let (kind, message) = (kind_of(*kind), self.str(message));
                node(move |c, m, b| Ok(Rc::new(DatError::new(kind, message(c, m, b)?.as_str()))))
            }
            ExprKind::Diverge(a) => self.diverge(a),
            k if is_general(k) => {
                let v = self.general(e);
                node(move |c, m, b| Ok(v(c, m, b)?.into_error()))
            }
            _ => unreachable!("not an error expression: {e:?}"),
        }
    }

    fn assert(&self, a: &ir::Assert) -> F<()> {
        enum Cond {
            Plain(F<bool>),
            Compare(CmpOp, F<Value>, F<Value>),
        }
        let cond = match &a.cond {
            ir::AssertCond::Plain(c) => Cond::Plain(self.bool(c)),
            ir::AssertCond::Compare { op, lhs, rhs } => Cond::Compare(*op, self.value(lhs), self.value(rhs)),
        };
        let message = a.message.as_ref().map(|m| self.str(m));
        let (src, cond_src, at) = (a.src.clone(), a.cond_src.clone(), self.loc(a.loc));
        node(move |c, m, b| {
            let (ok, sides) = match &cond {
                Cond::Plain(f) => (f(c, m, b)?, None),
                Cond::Compare(op, lhs, rhs) => {
                    let l = lhs(c, m, b)?;
                    let r = rhs(c, m, b)?;
                    (compare(*op, &l, &r), Some((l, r)))
                }
            };
            let message = match &message {
                Some(f) => Some(f(c, m, b)?),
                None => None,
            };
            if ok {
                return Ok(());
            }
            let sides = sides.map(|(l, r)| (debug(&l), debug(&r)));
            Err(thrown(DatError::assertion(&src, &cond_src, sides, message.as_ref().map(Str::as_str)), at))
        })
    }
}

/// One part of a string join.
enum Piece {
    Text(Box<str>),
    Local(usize),
    Int(F<i64>),
    Float(F<f64>),
    Bool(F<bool>),
    Str(F<Str>),
}

/// Runs the parts of a join in order, writing each into `j`.
fn fill(pieces: &[Piece], j: &mut Join, c: &Code, m: &mut Machine, b: usize) -> R<()> {
    for p in pieces {
        match p {
            Piece::Text(t) => j.push_str(t),
            Piece::Local(l) => j.push_str(m.stack[b + l].str().as_str()),
            Piece::Int(f) => j.push_int(f(c, m, b)?),
            Piece::Float(f) => j.push_float(f(c, m, b)?),
            Piece::Bool(f) => j.push_bool(f(c, m, b)?),
            Piece::Str(f) => j.push_str(f(c, m, b)?.as_str()),
        }
    }
    Ok(())
}

fn kind_of(t: ir::ErrorType) -> ErrorKind {
    match t {
        ir::ErrorType::Error | ir::ErrorType::Any => ErrorKind::Error,
        ir::ErrorType::ValueError => ErrorKind::ValueError,
        ir::ErrorType::RangeError => ErrorKind::RangeError,
        ir::ErrorType::KeyError => ErrorKind::KeyError,
        ir::ErrorType::TypeError => ErrorKind::TypeError,
        ir::ErrorType::AssertionError => ErrorKind::AssertionError,
        ir::ErrorType::StackOverflowError => ErrorKind::StackOverflowError,
    }
}

fn holds(op: CmpOp, ord: Ordering) -> bool {
    match op {
        CmpOp::Lt => ord == Ordering::Less,
        CmpOp::Le => ord != Ordering::Greater,
        CmpOp::Gt => ord == Ordering::Greater,
        CmpOp::Ge => ord != Ordering::Less,
        CmpOp::Eq => ord == Ordering::Equal,
        CmpOp::Ne => ord != Ordering::Equal,
    }
}

fn compare(op: CmpOp, a: &Value, b: &Value) -> bool {
    let ord = match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        (Value::Int(x), Value::Float(y)) => num::cmp_int_float(*x, *y),
        (Value::Float(x), Value::Int(y)) => num::cmp_float_int(*x, *y),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        (Value::Str(x), Value::Str(y)) => Some(x.as_str().cmp(y.as_str())),
        (x, y) => Some(if dat_rt::Equal::equal(x, y) { Ordering::Equal } else { Ordering::Less }),
    };
    num::holds(rt_cmp(op), ord)
}

fn rt_cmp(op: CmpOp) -> num::Cmp {
    match op {
        CmpOp::Lt => num::Cmp::Lt,
        CmpOp::Le => num::Cmp::Le,
        CmpOp::Gt => num::Cmp::Gt,
        CmpOp::Ge => num::Cmp::Ge,
        CmpOp::Eq => num::Cmp::Eq,
        CmpOp::Ne => num::Cmp::Ne,
    }
}

/// A builtin of one argument; `f` gives its result or its error, and `out`
/// converts the result.
fn op1<A: 'static, V, T: 'static>(a: F<A>, f: impl Fn(A) -> R<V> + 'static, at: Loc, out: impl Fn(V) -> T + 'static) -> F<T> {
    node(move |c, m, b| match f(a(c, m, b)?) {
        Ok(v) => Ok(out(v)),
        Err(err) => Err(thrown(err, at)),
    })
}

fn op2<A: 'static, B: 'static, V, T: 'static>(
    a: F<A>,
    b2: F<B>,
    f: impl Fn(A, B) -> R<V> + 'static,
    at: Loc,
    out: impl Fn(V) -> T + 'static,
) -> F<T> {
    node(move |c, m, b| {
        let x = a(c, m, b)?;
        match f(x, b2(c, m, b)?) {
            Ok(v) => Ok(out(v)),
            Err(err) => Err(thrown(err, at)),
        }
    })
}

fn op3<A: 'static, B: 'static, C: 'static>(a: F<A>, b2: F<B>, c3: F<C>, f: impl Fn(A, B, C) -> R<Str> + 'static, at: Loc) -> F<Str> {
    node(move |c, m, b| {
        let x = a(c, m, b)?;
        let y = b2(c, m, b)?;
        f(x, y, c3(c, m, b)?).map_err(|err| thrown(err, at))
    })
}

fn debug(v: &Value) -> String {
    match v {
        Value::Int(n) => dat_rt::debug_int(*n),
        Value::Float(x) => dat_rt::debug_float(*x),
        Value::Bool(b) => dat_rt::debug_bool(*b),
        Value::Str(s) => dat_rt::debug_str(s.as_str()),
        Value::Error(e) => e.to_display(),
        Value::Void => "void".into(),
        other => dat_rt::debug(other),
    }
}

/// Runs `main` and returns the exit status: 0, or 1 if an error escaped.
pub fn run_main(program: &ir::Program) -> i32 {
    let main = program.main.expect("the checker requires main for run");
    dat_rt::run_on_dat_thread(STACK_SIZE, || {
        // Compiling leaks the names traces use, so it comes before the
        // debug checks start counting.
        let code = Code::compile(program);
        let window = dat_rt::checks::begin();
        let status = {
            let mut m = Machine::new();
            match code.funcs[main.0 as usize].run(&code, &mut m, 0) {
                Ok(_) => 0,
                Err(err) => {
                    dat_rt::report_uncaught(&err);
                    1
                }
            }
        };
        dat_rt::flush_stdout();
        let status = dat_rt::checks::end(window, status);
        dat_rt::copies::finish();
        status
    })
}

/// Runs the tests whose names contain `filter`, from every program, and prints
/// one report. Returns the exit status: 0, 1 if a test failed, 2 if no test matched.
pub fn run_tests(programs: &[&ir::Program], filter: Option<&str>, verbose: bool) -> i32 {
    let selected = |p: &'_ ir::Program| -> Vec<usize> {
        p.tests.iter().enumerate().filter(|(_, t)| filter.is_none_or(|f| t.name.contains(f))).map(|(i, _)| i).collect()
    };
    if programs.iter().all(|p| selected(p).is_empty()) {
        return 2;
    }
    dat_rt::run_on_dat_thread(STACK_SIZE, || {
        let codes: Vec<Code> = programs.iter().map(|p| Code::compile(p)).collect();
        let window = dat_rt::checks::begin();
        let status = {
            let mut infos = Vec::new();
            let mut outcomes = Vec::new();
            let start = Instant::now();
            for (p, code) in programs.iter().zip(&codes) {
                let mut m = Machine::new();
                for i in selected(p) {
                    let t = &p.tests[i];
                    let f = &code.funcs[t.func.0 as usize];
                    let loc = Loc { file: code.file, func: f.name, line: t.loc.line, col: t.loc.col };
                    infos.push(TestInfo { name: t.name.clone(), frame: f.name, loc });
                    m.stack.clear();
                    outcomes.push(dat_rt::testing::run_one(|| f.run(code, &mut m, 0).map(drop)));
                }
            }
            dat_rt::testing::report(&infos, &outcomes, start.elapsed(), verbose)
        };
        let status = dat_rt::checks::end(window, status);
        dat_rt::copies::finish();
        status
    })
}

/// The expressions only [`Compiler::general`] compiles.
fn is_general(k: &ExprKind) -> bool {
    matches!(
        k,
        ExprKind::ListLit(_)
            | ExprKind::SetLit(_)
            | ExprKind::MapLit(_)
            | ExprKind::TupleLit(_)
            | ExprKind::Index { .. }
            | ExprKind::TupleGet(..)
            | ExprKind::FuncRef(_)
            | ExprKind::Closure { .. }
            | ExprKind::CallValue { .. }
            | ExprKind::Method { .. }
            | ExprKind::Mutate { .. }
    )
}
