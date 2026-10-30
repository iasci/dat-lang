//! The typed IR: the only thing the interpreter and the Rust code generator read.
//!
//! Locals are resolved to slots, every operation that can fail carries its
//! source location, and evaluation order is the tree order: children are
//! evaluated left to right, as `reference/03-operators.md` requires. Neither
//! backend may depend on the parser or the checker.

#![forbid(unsafe_code)]

mod dump;

pub use dump::dump;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FuncId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalId(pub u32);

/// A 1-based source position in the program's file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Loc {
    pub line: u32,
    pub col: u32,
}

#[derive(Clone, Debug)]
pub struct Program {
    /// The path as the user gave it. Traces and test reports print it.
    pub file: String,
    pub functions: Vec<Function>,
    pub tests: Vec<Test>,
    pub main: Option<FuncId>,
}

impl Program {
    pub fn func(&self, id: FuncId) -> &Function {
        &self.functions[id.0 as usize]
    }
}

#[derive(Clone, Debug)]
pub struct Function {
    /// The name traces print: `fib`, or `test "name"` for a test body.
    pub name: String,
    /// The first `params` locals are the parameters, in order.
    pub params: u32,
    pub ret: Type,
    pub locals: Vec<Local>,
    pub body: Block,
    /// Part of a call-graph cycle, so it checks the stack on entry.
    pub recursive: bool,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub struct Local {
    pub name: String,
    pub ty: Type,
    /// Assigned after its declaration.
    pub reassigned: bool,
}

#[derive(Clone, Debug)]
pub struct Test {
    pub name: String,
    pub func: FuncId,
    pub loc: Loc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Int,
    Bool,
    Str,
    Void,
    Never,
    Error(ErrorType),
}

impl Type {
    pub fn name(self) -> &'static str {
        match self {
            Type::Int => "int",
            Type::Bool => "bool",
            Type::Str => "string",
            Type::Void => "void",
            Type::Never => "never",
            Type::Error(e) => e.name(),
        }
    }
}

/// The built-in error structs of `reference/12-errors.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorType {
    Error,
    ValueError,
    RangeError,
    KeyError,
    TypeError,
    AssertionError,
    StackOverflowError,
    /// The type of `e` in an untyped `catch (e)`: any runtime error.
    Any,
}

impl ErrorType {
    pub const ALL: [ErrorType; 7] = [
        ErrorType::Error,
        ErrorType::ValueError,
        ErrorType::RangeError,
        ErrorType::KeyError,
        ErrorType::TypeError,
        ErrorType::AssertionError,
        ErrorType::StackOverflowError,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ErrorType::Error => "Error",
            ErrorType::ValueError => "ValueError",
            ErrorType::RangeError => "RangeError",
            ErrorType::KeyError => "KeyError",
            ErrorType::TypeError => "TypeError",
            ErrorType::AssertionError => "AssertionError",
            ErrorType::StackOverflowError => "StackOverflowError",
            ErrorType::Any => "Error",
        }
    }

    pub fn from_name(name: &str) -> Option<ErrorType> {
        ErrorType::ALL.into_iter().find(|e| e.name() == name)
    }

    /// Runtime errors need no `throws` and no `try`.
    pub fn is_runtime(self) -> bool {
        !matches!(self, ErrorType::Error | ErrorType::ValueError)
    }
}

pub type Block = Vec<Stmt>;

#[derive(Clone, Debug)]
pub enum Stmt {
    /// Declares a local and gives it its first value.
    Let(LocalId, Expr),
    Set(LocalId, Expr),
    Expr(Expr),
    If(Expr, Block, Block),
    /// `while (cond) body` and `for (; cond; step) body`. `continue` runs `step`.
    Loop {
        cond: Option<Expr>,
        body: Block,
        step: Block,
    },
    /// Leaves the innermost loop.
    Break,
    /// Goes to the next iteration of the innermost loop.
    Continue,
    Return(Option<Expr>),
    Throw(Expr, Loc),
    Try {
        body: Block,
        catches: Vec<Catch>,
    },
}

#[derive(Clone, Debug)]
pub struct Catch {
    pub filter: CatchFilter,
    pub bind: Option<LocalId>,
    pub body: Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatchFilter {
    /// `catch (e: RangeError)`
    Is(ErrorType),
    /// `catch (e)`: every error, keeping its type.
    All,
    /// `catch (e: Error)`: every error, converted to `Error`.
    AllAsError,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Type,
}

impl Expr {
    /// The parts of a string join, with joins inside it opened up, in
    /// order: `a + (b + c)` gives `a`, `b` and `c`. Anything else is one part.
    pub fn joined_parts(&self) -> Vec<&Expr> {
        fn parts<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
            match &e.kind {
                ExprKind::Concat(ps) => ps.iter().for_each(|p| parts(p, out)),
                _ => out.push(e),
            }
        }
        let mut all = Vec::new();
        parts(self, &mut all);
        all
    }

    /// For `s = s + a + b` and `s += a + b`: the parts joined after `s`,
    /// here `a` and `b`, if this expression is `s` joined with more parts.
    /// Both backends append them in place, so building a string in a loop
    /// stays linear.
    pub fn appended_to(&self, s: LocalId) -> Option<Vec<&Expr>> {
        if !matches!(self.kind, ExprKind::Concat(_)) {
            return None;
        }
        let mut all = self.joined_parts();
        match all[0].kind {
            ExprKind::Local(first) if first == s && all.len() > 1 => Some(all.split_off(1)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntOp {
    Add,
    Sub,
    Mul,
    Rem,
}

impl IntOp {
    pub fn symbol(self) -> &'static str {
        match self {
            IntOp::Add => "+",
            IntOp::Sub => "-",
            IntOp::Mul => "*",
            IntOp::Rem => "%",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl CmpOp {
    pub fn symbol(self) -> &'static str {
        match self {
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
        }
    }
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(i64),
    Bool(bool),
    Str(String),
    Local(LocalId),
    Call {
        func: FuncId,
        args: Vec<Expr>,
        loc: Loc,
    },
    /// Checked: overflow and `% 0` throw a `RangeError` at `loc`.
    IntArith {
        op: IntOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        loc: Loc,
    },
    IntNeg {
        arg: Box<Expr>,
        loc: Loc,
    },
    /// Compares two operands of type `int`, `bool` or `string`, given by `lhs.ty`.
    Compare {
        op: CmpOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Not(Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cond {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    /// Joins string-typed parts.
    Concat(Vec<Expr>),
    /// `toString()` of an `int`, `bool`, `string` or error value.
    ToStr(Box<Expr>),
    NewError {
        kind: ErrorType,
        message: Box<Expr>,
    },
    /// `e.message`
    ErrorMessage(Box<Expr>),
    /// `print`, already joined into one string.
    Print(Box<Expr>),
    Assert(Box<Assert>),
    /// An expression of type `never` used where a value of `ty` is expected,
    /// such as `fail(...)` as an argument. It never finishes, so it has any type.
    Diverge(Box<Expr>),
    /// `fail(message)`: throws an `AssertionError`.
    Fail {
        message: Box<Expr>,
        loc: Loc,
    },
}

#[derive(Clone, Debug)]
pub struct Assert {
    pub cond: AssertCond,
    pub message: Option<Expr>,
    /// The whole call as written, such as `assert(fib(10) == 56)`.
    pub src: String,
    /// The condition as written, such as `fib(10) == 56`.
    pub cond_src: String,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub enum AssertCond {
    Plain(Expr),
    /// A comparison: both sides are evaluated once and shown when it fails.
    Compare {
        op: CmpOp,
        lhs: Expr,
        rhs: Expr,
    },
}
