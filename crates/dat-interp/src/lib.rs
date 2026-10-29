//! The interpreter: walks the typed IR with slot-resolved locals.
//!
//! Every observable operation goes through dat-rt, as generated code does.
//! Errors propagate as `Err(Box<DatError>)`; each call they leave records its
//! frame, by the same rule the generated code's frame guards follow.

#![forbid(unsafe_code)]

use dat_ir::{self as ir, ExprKind, Stmt};
use dat_rt::testing::TestInfo;
use dat_rt::{DatError, ErrorKind, Loc, Str};
use std::rc::Rc;
use std::time::Instant;

/// The interpreter's stack. Its frames are larger than compiled code's, so
/// it gets more room; both modes guarantee at least 10,000 nested calls.
pub const STACK_SIZE: usize = 512 << 20;

#[derive(Clone, Debug)]
enum Value {
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
}

/// How a statement finished.
enum Flow {
    Normal,
    Return(Value),
    Break,
    Continue,
}

type R<T> = Result<T, Box<DatError>>;

/// Names and locations with the `'static` lifetime traces need. Built once per run.
struct Names {
    file: &'static str,
    funcs: Vec<&'static str>,
}

struct Interp<'p> {
    program: &'p ir::Program,
    names: Names,
    /// The function running now, for the locations its errors carry.
    cur: usize,
    stack: Vec<Value>,
    /// `DAT_FAULT=interp-plus-one`: compute `+` wrong, to prove the differential harness notices.
    fault_plus_one: bool,
}

impl<'p> Interp<'p> {
    fn new(program: &'p ir::Program) -> Interp<'p> {
        let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
        let names = Names { file: leak(&program.file), funcs: program.functions.iter().map(|f| leak(&f.name)).collect() };
        Interp {
            program,
            names,
            cur: 0,
            stack: Vec::with_capacity(1 << 16),
            fault_plus_one: std::env::var("DAT_FAULT").is_ok_and(|v| v == "interp-plus-one"),
        }
    }

    fn loc(&self, l: ir::Loc) -> Loc {
        Loc { file: self.names.file, func: self.names.funcs[self.cur], line: l.line, col: l.col }
    }

    fn throw<T>(&self, mut err: Box<DatError>, at: ir::Loc) -> R<T> {
        err.trace = dat_rt::Trace::at(self.loc(at));
        Err(err)
    }

    /// Calls a function whose arguments are already on the stack.
    fn call(&mut self, id: ir::FuncId, base: usize) -> R<Value> {
        let f = self.program.func(id);
        self.stack.resize(base + f.locals.len(), Value::Void);
        let caller = std::mem::replace(&mut self.cur, id.0 as usize);
        let result = self.block(&f.body, base);
        self.cur = caller;
        self.stack.truncate(base);
        match result {
            Ok(Flow::Return(v)) => Ok(v),
            Ok(_) => Ok(Value::Void),
            Err(mut e) => {
                let at = Loc { file: self.names.file, func: self.names.funcs[id.0 as usize], line: f.loc.line, col: f.loc.col };
                e.trace.leave(self.names.funcs[id.0 as usize], at);
                Err(e)
            }
        }
    }

    fn block(&mut self, b: &'p [Stmt], base: usize) -> R<Flow> {
        for s in b {
            match self.stmt(s, base)? {
                Flow::Normal => {}
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal)
    }

    fn stmt(&mut self, s: &'p Stmt, base: usize) -> R<Flow> {
        match s {
            Stmt::Let(l, e) | Stmt::Set(l, e) => {
                let v = self.expr(e, base)?;
                self.stack[base + l.0 as usize] = v;
            }
            Stmt::Expr(e) => {
                self.expr(e, base)?;
            }
            Stmt::If(c, t, e) => {
                let branch = if self.cond(c, base)? { t } else { e };
                return self.block(branch, base);
            }
            Stmt::Loop { cond, body, step } => loop {
                if let Some(c) = cond
                    && !self.cond(c, base)?
                {
                    break;
                }
                match self.block(body, base)? {
                    Flow::Normal | Flow::Continue => {}
                    Flow::Break => break,
                    r @ Flow::Return(_) => return Ok(r),
                }
                self.block(step, base)?;
            },
            Stmt::Break => return Ok(Flow::Break),
            Stmt::Continue => return Ok(Flow::Continue),
            Stmt::Return(None) => return Ok(Flow::Return(Value::Void)),
            Stmt::Return(Some(e)) => return Ok(Flow::Return(self.expr(e, base)?)),
            Stmt::Throw(e, loc) => {
                let v = self.expr(e, base)?;
                let err = Box::new((**v.error()).clone());
                return self.throw(err, *loc);
            }
            Stmt::Try { body, catches } => {
                let err = match self.block(body, base) {
                    Ok(flow) => return Ok(flow),
                    Err(err) => err,
                };
                // The try block's frame is still running: record where it was.
                for c in catches {
                    let caught = match c.filter {
                        ir::CatchFilter::Is(t) => kind_of(t) == err.kind,
                        ir::CatchFilter::All | ir::CatchFilter::AllAsError => true,
                    };
                    if !caught {
                        continue;
                    }
                    if let Some(bind) = c.bind {
                        let value = if c.filter == ir::CatchFilter::AllAsError { (*err).into_error() } else { *err };
                        self.stack[base + bind.0 as usize] = Value::Error(Rc::new(value));
                    }
                    return self.block(&c.body, base);
                }
                return Err(err);
            }
        }
        Ok(Flow::Normal)
    }

    /// An `int`-typed expression, without going through `Value`.
    #[inline]
    fn int(&mut self, e: &'p ir::Expr, base: usize) -> R<i64> {
        match &e.kind {
            ExprKind::Int(n) => Ok(*n),
            ExprKind::Local(l) => Ok(self.stack[base + l.0 as usize].int()),
            ExprKind::IntArith { op, lhs, rhs, loc } => {
                let a = self.int(lhs, base)?;
                let b = self.int(rhs, base)?;
                let r = match op {
                    ir::IntOp::Add => dat_rt::int::try_add(a, b).map(|v| if self.fault_plus_one { v.wrapping_add(1) } else { v }),
                    ir::IntOp::Sub => dat_rt::int::try_sub(a, b),
                    ir::IntOp::Mul => dat_rt::int::try_mul(a, b),
                    ir::IntOp::Rem => dat_rt::int::try_rem(a, b),
                };
                match r {
                    Ok(v) => Ok(v),
                    Err(err) => self.throw(err, *loc),
                }
            }
            _ => Ok(self.expr(e, base)?.int()),
        }
    }

    /// A `bool`-typed expression, without going through `Value`.
    #[inline]
    fn cond(&mut self, e: &'p ir::Expr, base: usize) -> R<bool> {
        match &e.kind {
            ExprKind::Compare { op, lhs, rhs } if lhs.ty == ir::Type::Int => {
                let a = self.int(lhs, base)?;
                let b = self.int(rhs, base)?;
                Ok(match op {
                    ir::CmpOp::Lt => a < b,
                    ir::CmpOp::Le => a <= b,
                    ir::CmpOp::Gt => a > b,
                    ir::CmpOp::Ge => a >= b,
                    ir::CmpOp::Eq => a == b,
                    ir::CmpOp::Ne => a != b,
                })
            }
            ExprKind::Bool(b) => Ok(*b),
            ExprKind::Not(a) => Ok(!self.cond(a, base)?),
            ExprKind::And(a, b) => Ok(self.cond(a, base)? && self.cond(b, base)?),
            ExprKind::Or(a, b) => Ok(self.cond(a, base)? || self.cond(b, base)?),
            _ => Ok(self.expr(e, base)?.bool()),
        }
    }

    fn expr(&mut self, e: &'p ir::Expr, base: usize) -> R<Value> {
        Ok(match &e.kind {
            ExprKind::Int(n) => Value::Int(*n),
            ExprKind::Bool(b) => Value::Bool(*b),
            ExprKind::Str(s) => Value::Str(Str::new(s)),
            ExprKind::Local(l) => self.stack[base + l.0 as usize].clone(),
            ExprKind::Call { func, args, loc } => {
                if dat_rt::stack_is_low() {
                    // Thrown in the caller, at the call, as in compiled code.
                    return self.throw(DatError::stack_overflow(), *loc);
                }
                let callee_base = self.stack.len();
                for a in args {
                    let v = match self.expr(a, base) {
                        Ok(v) => v,
                        Err(err) => {
                            self.stack.truncate(callee_base);
                            return Err(err);
                        }
                    };
                    self.stack.push(v);
                }
                match self.call(*func, callee_base) {
                    Ok(v) => v,
                    Err(mut err) => {
                        // The error now leaves the caller's frame from this call.
                        if err.trace.pending.is_none() {
                            err.trace.pending = Some(self.loc(*loc));
                        }
                        return Err(err);
                    }
                }
            }
            ExprKind::IntArith { .. } => Value::Int(self.int(e, base)?),
            ExprKind::IntNeg { arg, loc } => match dat_rt::int::try_neg(self.expr(arg, base)?.int()) {
                Ok(v) => Value::Int(v),
                Err(err) => return self.throw(err, *loc),
            },
            ExprKind::Compare { op, lhs, rhs } if lhs.ty != ir::Type::Int => {
                let a = self.expr(lhs, base)?;
                let b = self.expr(rhs, base)?;
                Value::Bool(compare(*op, &a, &b))
            }
            ExprKind::Compare { .. } | ExprKind::Not(_) | ExprKind::And(..) | ExprKind::Or(..) => Value::Bool(self.cond(e, base)?),
            ExprKind::Cond { cond, then, els } => {
                if self.cond(cond, base)? {
                    self.expr(then, base)?
                } else {
                    self.expr(els, base)?
                }
            }
            ExprKind::Concat(parts) => {
                let mut values = Vec::with_capacity(parts.len());
                for p in parts {
                    values.push(self.expr(p, base)?);
                }
                let strs: Vec<&str> = values.iter().map(|v| v.str().as_str()).collect();
                Value::Str(Str::concat(&strs))
            }
            ExprKind::ToStr(a) => Value::Str(to_str(&self.expr(a, base)?)),
            ExprKind::NewError { kind, message } => {
                let m = self.expr(message, base)?;
                Value::Error(Rc::new(DatError::new(kind_of(*kind), m.str().as_str())))
            }
            ExprKind::ErrorMessage(a) => Value::Str(self.expr(a, base)?.error().message_str()),
            ExprKind::Print(a) => {
                let s = self.expr(a, base)?;
                dat_rt::print_str(s.str().as_str());
                Value::Void
            }
            ExprKind::Assert(a) => {
                self.assert(a, base)?;
                Value::Void
            }
            ExprKind::Diverge(a) => {
                self.expr(a, base)?;
                unreachable!("a never-typed expression finished")
            }
            ExprKind::Fail { message, loc } => {
                let m = self.expr(message, base)?;
                return self.throw(Box::new(DatError::new(ErrorKind::AssertionError, m.str().as_str())), *loc);
            }
        })
    }

    fn assert(&mut self, a: &'p ir::Assert, base: usize) -> R<()> {
        let (ok, sides) = match &a.cond {
            ir::AssertCond::Plain(c) => (self.cond(c, base)?, None),
            ir::AssertCond::Compare { op, lhs, rhs } => {
                let l = self.expr(lhs, base)?;
                let r = self.expr(rhs, base)?;
                (compare(*op, &l, &r), Some((l, r)))
            }
        };
        let message = match &a.message {
            Some(m) => Some(self.expr(m, base)?),
            None => None,
        };
        if ok {
            return Ok(());
        }
        let sides = sides.map(|(l, r)| (debug(&l), debug(&r)));
        let err = DatError::assertion(&a.src, &a.cond_src, sides, message.as_ref().map(|m| m.str().as_str()));
        self.throw(err, a.loc)
    }
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

fn compare(op: ir::CmpOp, a: &Value, b: &Value) -> bool {
    use std::cmp::Ordering;
    let ord = match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.cmp(y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Str(x), Value::Str(y)) => x.as_str().cmp(y.as_str()),
        _ => unreachable!("type checker guarantees comparable operands"),
    };
    match op {
        ir::CmpOp::Lt => ord == Ordering::Less,
        ir::CmpOp::Le => ord != Ordering::Greater,
        ir::CmpOp::Gt => ord == Ordering::Greater,
        ir::CmpOp::Ge => ord != Ordering::Less,
        ir::CmpOp::Eq => ord == Ordering::Equal,
        ir::CmpOp::Ne => ord != Ordering::Equal,
    }
}

fn to_str(v: &Value) -> Str {
    match v {
        Value::Int(n) => dat_rt::int_to_str(*n),
        Value::Bool(b) => dat_rt::bool_to_str(*b),
        Value::Str(s) => s.clone(),
        Value::Error(e) => e.to_str(),
        Value::Void => unreachable!("void has no string form"),
    }
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
        let mut interp = Interp::new(program);
        let code = match interp.call(main, 0) {
            Ok(_) => 0,
            Err(err) => {
                dat_rt::report_uncaught(&err);
                1
            }
        };
        dat_rt::flush_stdout();
        code
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
            let mut interp = Interp::new(p);
            for i in selected(p) {
                let t = &p.tests[i];
                interp.cur = t.func.0 as usize;
                infos.push(TestInfo { name: t.name.clone(), frame: interp.names.funcs[t.func.0 as usize], loc: interp.loc(t.loc) });
                interp.stack.clear();
                outcomes.push(dat_rt::testing::run_one(|| interp.call(t.func, 0).map(|_| ())));
            }
        }
        dat_rt::testing::report(&infos, &outcomes, start.elapsed(), verbose)
    })
}
