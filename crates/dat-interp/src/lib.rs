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

use dat_ir::{self as ir, CmpOp, ExprKind, IntOp, Stmt, Type};
use dat_rt::testing::TestInfo;
use dat_rt::{DatError, ErrorKind, Join, Loc, Str, Trace};
use std::cmp::Ordering;
use std::convert::identity;
use std::rc::Rc;
use std::time::Instant;

/// The interpreter's stack. Its frames are larger than compiled code's, so
/// it gets more room; both modes guarantee at least 10,000 nested calls.
pub const STACK_SIZE: usize = 512 << 20;

#[derive(Clone, Debug, Default)]
enum Value {
    #[default]
    Void,
    Int(i64),
    Bool(bool),
    Str(Str),
    Error(Rc<DatError>),
}

impl Value {
    #[inline]
    fn int(&self) -> i64 {
        match self {
            Value::Int(n) => *n,
            _ => unreachable!("type checker guarantees an int"),
        }
    }

    #[inline]
    fn bool(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            _ => unreachable!("type checker guarantees a bool"),
        }
    }

    fn str(&self) -> &Str {
        match self {
            Value::Str(s) => s,
            _ => unreachable!("type checker guarantees a string"),
        }
    }

    fn error(&self) -> &Rc<DatError> {
        match self {
            Value::Error(e) => e,
            _ => unreachable!("type checker guarantees an error"),
        }
    }

    fn into_str(self) -> Str {
        match self {
            Value::Str(s) => s,
            _ => unreachable!("type checker guarantees a string"),
        }
    }

    fn into_error(self) -> Rc<DatError> {
        match self {
            Value::Error(e) => e,
            _ => unreachable!("type checker guarantees an error"),
        }
    }
}

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
}

impl Machine {
    fn new() -> Machine {
        Machine { stack: Vec::with_capacity(1 << 16), ret: Value::Void }
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
}

impl Func {
    /// Runs a call whose arguments are already on the stack, from `base`.
    #[inline]
    fn run(&self, c: &Code, m: &mut Machine, base: usize) -> R<Value> {
        m.stack.resize(base + self.locals, Value::Void);
        let result = run(&self.body, c, m, base);
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
                let cx = Compiler { file, func: leak(&f.name), fault_plus_one };
                Func { name: cx.func, at: cx.loc(f.loc), locals: f.locals.len(), body: cx.block(&f.body) }
            })
            .collect();
        Code { file, funcs }
    }
}

/// Compiles one function.
struct Compiler {
    file: &'static str,
    func: &'static str,
    fault_plus_one: bool,
}

impl Compiler {
    fn loc(&self, l: ir::Loc) -> Loc {
        Loc { file: self.file, func: self.func, line: l.line, col: l.col }
    }

    fn block(&self, b: &[Stmt]) -> Block {
        b.iter().map(|s| self.stmt(s)).collect()
    }

    fn stmt(&self, s: &Stmt) -> F<Flow> {
        match s {
            Stmt::Set(l, e) => match e.appended_to(*l) {
                Some(parts) => self.append(l.0 as usize, &parts),
                None => self.store(l.0 as usize, e),
            },
            Stmt::Let(l, e) => self.store(l.0 as usize, e),
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
            (ExprKind::Call { func, args, loc }, _) => self.call(*func, args, *loc, identity),
            (_, Type::Int) => self.int_to(e, Value::Int),
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
        let id = func.0 as usize;
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
            match c.funcs[id].run(c, m, callee) {
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
            ExprKind::Compare { op, lhs, rhs } => match lhs.ty {
                Type::Int => match op {
                    CmpOp::Lt => self.compare_ints(|x, y| x < y, lhs, rhs, out),
                    CmpOp::Le => self.compare_ints(|x, y| x <= y, lhs, rhs, out),
                    CmpOp::Gt => self.compare_ints(|x, y| x > y, lhs, rhs, out),
                    CmpOp::Ge => self.compare_ints(|x, y| x >= y, lhs, rhs, out),
                    CmpOp::Eq => self.compare_ints(|x, y| x == y, lhs, rhs, out),
                    CmpOp::Ne => self.compare_ints(|x, y| x != y, lhs, rhs, out),
                },
                Type::Bool => {
                    let (op, lhs, rhs) = (*op, self.bool(lhs), self.bool(rhs));
                    node(move |c, m, b| {
                        let x = lhs(c, m, b)?;
                        Ok(out(holds(op, x.cmp(&rhs(c, m, b)?))))
                    })
                }
                Type::Str => {
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
            _ => unreachable!("not a bool expression: {e:?}"),
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
            ExprKind::Str(s) => {
                let s = Str::new(s);
                node(move |_, _, _| Ok(s.clone()))
            }
            ExprKind::Local(l) => {
                let l = l.0 as usize;
                node(move |_, m, b| Ok(m.stack[b + l].str().clone()))
            }
            ExprKind::Call { func, args, loc } => self.call(*func, args, *loc, Value::into_str),
            ExprKind::Cond { cond, then, els } => self.cond(cond, self.str(then), self.str(els)),
            ExprKind::Concat(_) => self.join(e),
            ExprKind::ToStr(a) => match a.ty {
                Type::Int => self.int_to(a, dat_rt::int_to_str),
                Type::Bool => self.bool_to(a, dat_rt::bool_to_str),
                Type::Str => self.str(a),
                Type::Error(_) => {
                    let a = self.error(a);
                    node(move |c, m, b| Ok(a(c, m, b)?.to_str()))
                }
                _ => unreachable!("void has no string form"),
            },
            ExprKind::ErrorMessage(a) => {
                let a = self.error(a);
                node(move |c, m, b| Ok(a(c, m, b)?.message_str()))
            }
            ExprKind::Diverge(a) => self.diverge(a),
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
                ExprKind::ToStr(a) if a.ty == Type::Bool => Piece::Bool(self.bool(a)),
                _ => Piece::Str(self.str(p)),
            })
            .collect();
        let cap = pieces.iter().map(|p| if let Piece::Text(t) = p { t.len() } else { 16 }).sum();
        (pieces, cap)
    }

    fn error(&self, e: &ir::Expr) -> F<Rc<DatError>> {
        match &e.kind {
            ExprKind::Local(l) => {
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
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Str(x), Value::Str(y)) => x.as_str().cmp(y.as_str()),
        _ => unreachable!("type checker guarantees comparable operands"),
    };
    holds(op, ord)
}

fn debug(v: &Value) -> String {
    match v {
        Value::Int(n) => dat_rt::debug_int(*n),
        Value::Bool(b) => dat_rt::debug_bool(*b),
        Value::Str(s) => dat_rt::debug_str(s.as_str()),
        Value::Error(e) => e.to_display(),
        Value::Void => "void".into(),
    }
}

/// Runs `main` and returns the exit status: 0, or 1 if an error escaped.
pub fn run_main(program: &ir::Program) -> i32 {
    let main = program.main.expect("the checker requires main for run");
    dat_rt::run_on_dat_thread(STACK_SIZE, || {
        let code = Code::compile(program);
        let mut m = Machine::new();
        let status = match code.funcs[main.0 as usize].run(&code, &mut m, 0) {
            Ok(_) => 0,
            Err(err) => {
                dat_rt::report_uncaught(&err);
                1
            }
        };
        dat_rt::flush_stdout();
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
        let mut infos = Vec::new();
        let mut outcomes = Vec::new();
        let start = Instant::now();
        for p in programs {
            let code = Code::compile(p);
            let mut m = Machine::new();
            for i in selected(p) {
                let t = &p.tests[i];
                let f = &code.funcs[t.func.0 as usize];
                let loc = Loc { file: code.file, func: f.name, line: t.loc.line, col: t.loc.col };
                infos.push(TestInfo { name: t.name.clone(), frame: f.name, loc });
                m.stack.clear();
                outcomes.push(dat_rt::testing::run_one(|| f.run(&code, &mut m, 0).map(drop)));
            }
        }
        dat_rt::testing::report(&infos, &outcomes, start.elapsed(), verbose)
    })
}
