//! The typed IR: the only thing the interpreter and the Rust code generator read.
//!
//! Locals are resolved to slots, every operation that can fail carries its
//! source location, and evaluation order is the tree order: children are
//! evaluated left to right, as `reference/03-operators.md` requires. Neither
//! backend may depend on the parser or the checker.

#![forbid(unsafe_code)]

mod dump;
mod types;
mod visit;

pub use dump::dump;
pub use types::{Kind, TypeId, Types};
pub use visit::{Part, each_child, each_part};

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
    /// The compound types the program uses.
    pub types: Types,
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
    /// An arrow function's copies of the variables it uses, in the order a
    /// closure value stores them. A call puts them in these locals.
    pub captures: Vec<LocalId>,
    /// The values of the last `defaults.len()` parameters when a call leaves
    /// them out, in order. Each may read the parameters before it.
    pub defaults: Vec<Expr>,
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
    /// A `mut` parameter or a `mut` loop variable: it stands for the
    /// caller's value or the list's element, which its changes change.
    pub by_ref: bool,
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
    Float,
    Bool,
    Str,
    Void,
    Never,
    Error(ErrorType),
    /// A list, set, map, tuple or function type, from [`Program::types`].
    Of(TypeId),
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
    /// `x = value`. The location is where `s += t` appends in place, for
    /// `--report-copies`.
    Set(LocalId, Expr, Loc),
    /// `xs[i] = v` and `m[k] = v`: the place's index and key operands are
    /// evaluated first, then the value. A last `Key` step inserts.
    Store(Place, Expr),
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
    /// `for (const x of xs)`: goes through the collection as it was when
    /// the loop started. `continue` goes to the next element.
    ForOf {
        var: LocalId,
        iter: Iter,
        body: Block,
    },
    /// `for (mut x of xs)`: `var` stands for each element of the list at
    /// `list` in turn, which nothing else can reach during the loop.
    ForMut {
        var: LocalId,
        list: Place,
        body: Block,
        /// The list, for `--report-copies`: the loop changes it in place.
        loc: Loc,
    },
}

/// What a `for...of` loop goes through.
#[derive(Clone, Debug)]
pub enum Iter {
    /// `range(start, end, step)`, without building a list.
    Range { start: Expr, end: Expr, step: Expr, loc: Loc },
    /// A list's elements, a set's, a map's `[key, value]` entries as
    /// tuples, or a string's characters, by the expression's type.
    Each(Expr),
}

/// A variable, or a part of one reached through indexes, keys and tuple
/// positions: what can be assigned, changed by a method, or passed `mut`.
#[derive(Clone, Debug)]
pub struct Place {
    pub root: LocalId,
    pub steps: Vec<Step>,
    /// The type of the value at the place.
    pub ty: Type,
}

impl Place {
    /// Where its first index or key is: where a change through the place
    /// happens, for `--report-copies`. A place of tuple positions only
    /// changes nothing that's shared.
    pub fn loc(&self) -> Option<Loc> {
        self.steps.iter().find_map(|s| match s {
            Step::Index(_, l) | Step::Key(_, l) => Some(*l),
            Step::Pos(_) => None,
        })
    }
}

#[derive(Clone, Debug)]
pub enum Step {
    /// A list element; out of range throws a `RangeError` at `loc`.
    Index(Expr, Loc),
    /// A map value; a missing key throws a `KeyError` at `loc`.
    Key(Expr, Loc),
    /// A tuple position.
    Pos(u32),
}

impl Step {
    /// Whether two steps, taken from the same value, reach the same part
    /// of it: `Some(true)` for one local, literal or position, `Some(false)`
    /// for different literals or positions, `None` when only the values
    /// at run time can tell. A literal index below zero throws, so it
    /// reaches no part at all.
    pub fn same_part(&self, other: &Step) -> Option<bool> {
        match (self, other) {
            (Step::Pos(a), Step::Pos(b)) => Some(a == b),
            (Step::Index(a, _), Step::Index(b, _)) | (Step::Key(a, _), Step::Key(b, _)) => match (&a.kind, &b.kind) {
                (ExprKind::Int(x), ExprKind::Int(y)) => Some(x == y),
                (ExprKind::Str(x), ExprKind::Str(y)) => Some(x == y),
                (ExprKind::Bool(x), ExprKind::Bool(y)) => Some(x == y),
                (ExprKind::Local(x) | ExprKind::Move(x), ExprKind::Local(y) | ExprKind::Move(y)) if x == y => Some(true),
                _ => None,
            },
            _ => None,
        }
    }
}

/// An element of a list or set literal.
#[derive(Clone, Debug)]
pub enum Elem {
    One(Expr),
    /// `...xs`: the elements of a list or a set, or a string's characters.
    Spread(Expr),
}

/// An entry of a map literal.
#[derive(Clone, Debug)]
pub enum Entry {
    Pair(Expr, Expr),
    /// `...m`: later entries win.
    Spread(Expr),
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
            ExprKind::Local(first) | ExprKind::Move(first) if first == s && all.len() > 1 => Some(all.split_off(1)),
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

/// Float arithmetic follows IEEE 754 and never throws: dividing by zero
/// gives `Infinity`, `-Infinity` or `NaN`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl FloatOp {
    pub fn symbol(self) -> &'static str {
        match self {
            FloatOp::Add => "+",
            FloatOp::Sub => "-",
            FloatOp::Mul => "*",
            FloatOp::Div => "/",
            FloatOp::Rem => "%",
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
    Float(f64),
    Bool(bool),
    Str(String),
    Local(LocalId),
    /// The last use of a local, found by the liveness pass (dat-own): its
    /// value is handed over, not copied, and the local isn't read again
    /// before it's assigned. Only locals of types that share their memory
    /// (strings, errors, collections and functions) get it.
    Move(LocalId),
    /// A call. An argument for a `mut` parameter is a [`ExprKind::Ref`].
    Call {
        func: FuncId,
        args: Vec<Expr>,
        loc: Loc,
    },
    /// `mut place` as an argument: the callee changes the caller's value.
    Ref(Place),
    /// A named function used as a value.
    FuncRef(FuncId),
    /// An arrow function: `func`, with copies of `captures` taken now.
    Closure {
        func: FuncId,
        captures: Vec<LocalId>,
    },
    /// A call of a function value.
    CallValue {
        callee: Box<Expr>,
        args: Vec<Expr>,
        loc: Loc,
    },
    ListLit(Vec<Elem>),
    SetLit(Vec<Elem>),
    MapLit(Vec<Entry>),
    TupleLit(Vec<Expr>),
    /// `xs[i]` on a list (a `RangeError` when out of range) or `m[k]` on a
    /// map (a `KeyError` when missing), thrown at `loc`.
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
        loc: Loc,
    },
    /// A tuple position, `entry[1]`.
    TupleGet(Box<Expr>, u32),
    /// A method of a collection, or a function on collections, by its one
    /// implementation in dat-rt. `args[0]` is the receiver. `loc` is where
    /// it throws, or where it calls a callback that throws.
    Method {
        op: Method,
        args: Vec<Expr>,
        loc: Loc,
    },
    /// A method that changes its receiver, at `place`.
    Mutate {
        op: Method,
        place: Place,
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
    FloatArith {
        op: FloatOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    FloatNeg(Box<Expr>),
    /// A method of a number or a string, or a standard library function, by
    /// its one implementation in dat-rt. `loc` is where it throws, if it can.
    Builtin {
        op: Builtin,
        args: Vec<Expr>,
        loc: Loc,
    },
    /// Compares two operands of the same type, except that an `int` and a
    /// `float` compare by their exact values. `<` and the others take
    /// `int`, `float` and `string`; `==` and `!=` any equatable type.
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
    /// `toString()` of a value of any type but `void` and `never`.
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

/// The operations on numbers and strings that dat-rt implements, each once.
/// The checker lowers methods and standard library functions to them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    IntAbs,
    IntMin,
    IntMax,
    IntSqrt,
    IntPow,
    IntToFloat,
    FloatToInt,
    FloatFloor,
    FloatCeil,
    FloatRound,
    FloatAbs,
    FloatSqrt,
    FloatPow,
    FloatMin,
    FloatMax,
    FloatToFixed,
    FloatIsNaN,
    /// `(a / b).toInt()` with two `int`s, computed exactly.
    DivToInt,
    DivFloor,
    DivCeil,
    DivRound,
    StrLength,
    StrIsEmpty,
    StrIncludes,
    StrStartsWith,
    StrEndsWith,
    StrIndexOf,
    StrLastIndexOf,
    StrSlice,
    StrTrim,
    StrTrimStart,
    StrTrimEnd,
    StrToUpperCase,
    StrToLowerCase,
    StrReplace,
    StrReplaceAll,
    StrRepeat,
    StrPadStart,
    StrPadEnd,
    /// `s[i]`
    StrAt,
    /// `item in s`: the item first, then the string.
    StrIn,
}

impl Builtin {
    /// The argument types, the receiver first, and the result type.
    pub fn sig(self) -> (&'static [Type], Type) {
        use Builtin::*;
        use Type::{Bool as B, Float as F, Int as I, Str as S};
        match self {
            IntAbs => (&[I], I),
            IntMin | IntMax | IntPow => (&[I, I], I),
            IntSqrt | IntToFloat => (&[I], F),
            FloatToInt | FloatFloor | FloatCeil | FloatRound => (&[F], I),
            FloatAbs | FloatSqrt => (&[F], F),
            FloatPow | FloatMin | FloatMax => (&[F, F], F),
            FloatToFixed => (&[F, I], S),
            FloatIsNaN => (&[F], B),
            DivToInt | DivFloor | DivCeil | DivRound => (&[I, I], I),
            StrLength => (&[S], I),
            StrIsEmpty => (&[S], B),
            StrIncludes | StrStartsWith | StrEndsWith => (&[S, S], B),
            StrIndexOf | StrLastIndexOf => (&[S, S], I),
            StrSlice => (&[S, I, I], S),
            StrTrim | StrTrimStart | StrTrimEnd | StrToUpperCase | StrToLowerCase => (&[S], S),
            StrReplace | StrReplaceAll => (&[S, S, S], S),
            StrRepeat | StrAt => (&[S, I], S),
            StrIn => (&[S, S], B),
            StrPadStart | StrPadEnd => (&[S, I, S], S),
        }
    }

    /// Whether it can throw a `RangeError`.
    pub fn throws(self) -> bool {
        use Builtin::*;
        matches!(
            self,
            IntAbs
                | IntPow
                | FloatToInt
                | FloatFloor
                | FloatCeil
                | FloatRound
                | FloatToFixed
                | DivToInt
                | DivFloor
                | DivCeil
                | DivRound
                | StrSlice
                | StrRepeat
                | StrPadStart
                | StrPadEnd
                | StrAt
        )
    }

    /// The name of its dat-rt function, in `dat_rt::num` or `dat_rt::text`.
    pub fn rt_name(self) -> &'static str {
        use Builtin::*;
        match self {
            IntAbs => "int_abs",
            IntMin => "int_min",
            IntMax => "int_max",
            IntSqrt => "int_sqrt",
            IntPow => "int_pow",
            IntToFloat => "int_to_float",
            FloatToInt => "float_to_int",
            FloatFloor => "float_floor",
            FloatCeil => "float_ceil",
            FloatRound => "float_round",
            FloatAbs => "float_abs",
            FloatSqrt => "float_sqrt",
            FloatPow => "float_pow",
            FloatMin => "float_min",
            FloatMax => "float_max",
            FloatToFixed => "float_to_fixed",
            FloatIsNaN => "float_is_nan",
            DivToInt => "div_to_int",
            DivFloor => "div_floor",
            DivCeil => "div_ceil",
            DivRound => "div_round",
            StrLength => "length",
            StrIsEmpty => "is_empty",
            StrIncludes => "includes",
            StrStartsWith => "starts_with",
            StrEndsWith => "ends_with",
            StrIndexOf => "index_of",
            StrLastIndexOf => "last_index_of",
            StrSlice => "slice",
            StrTrim => "trim",
            StrTrimStart => "trim_start",
            StrTrimEnd => "trim_end",
            StrToUpperCase => "to_upper_case",
            StrToLowerCase => "to_lower_case",
            StrReplace => "replace",
            StrReplaceAll => "replace_all",
            StrRepeat => "repeat",
            StrPadStart => "pad_start",
            StrPadEnd => "pad_end",
            StrAt => "at",
            StrIn => "is_in",
        }
    }

    /// Its dat-rt module: `num` for numbers, `text` for strings.
    /// The operation that gives this rounding's result exactly when its
    /// operand is `a / b` with two `int`s (§03).
    pub fn on_int_division(self) -> Option<Builtin> {
        use Builtin::*;
        Some(match self {
            FloatToInt => DivToInt,
            FloatFloor => DivFloor,
            FloatCeil => DivCeil,
            FloatRound => DivRound,
            _ => return None,
        })
    }

    pub fn rt_module(self) -> &'static str {
        if self.sig().0[0] == Type::Str { "text" } else { "num" }
    }
}

/// The methods of lists, sets and maps, and the functions on collections,
/// each implemented once in dat-rt. The checker's method table gives each
/// its signature.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    ListLength,
    ListIsEmpty,
    ListIncludes,
    ListIndexOf,
    ListLastIndexOf,
    ListAt,
    ListSlice,
    ListConcat,
    ListJoin,
    ListMap,
    ListFilter,
    ListFlatMap,
    ListFlat,
    ListReduce,
    ListForEach,
    ListFind,
    ListFindIndex,
    ListSome,
    ListEvery,
    ListCount,
    ListEntries,
    ListToSorted,
    ListToSortedWith,
    ListToSortedBy,
    ListToReversed,
    ListUnique,
    ListZip,
    ListGroupBy,
    /// `push(a, b)`: each argument, in order.
    ListPush,
    /// `push(...xs)`, or a mix: the argument is a list of all of them.
    ListPushAll,
    ListPop,
    /// `unshift(...)`: the argument is a list of everything to add.
    ListUnshift,
    ListShift,
    ListInsert,
    ListRemoveAt,
    ListRemove,
    ListSort,
    ListSortWith,
    ListSortBy,
    ListReverse,
    ListClear,
    SetSize,
    SetIsEmpty,
    SetHas,
    SetUnion,
    SetIntersection,
    SetDifference,
    SetSymmetricDifference,
    SetIsSubsetOf,
    SetIsSupersetOf,
    SetIsDisjointFrom,
    SetAdd,
    SetDelete,
    SetClear,
    MapSize,
    MapIsEmpty,
    MapGet,
    MapHas,
    MapKeys,
    MapValues,
    MapEntries,
    MapSet,
    MapDelete,
    MapClear,
    StrSplit,
    /// `[...s]`: a string's characters.
    StrChars,
    /// `range(start, end, step)`.
    Range,
    /// `max(...xs)` and the others from `@dat/math`, on a spread list.
    MaxInts,
    MinInts,
    MaxFloats,
    MinFloats,
    /// Destructuring: the list has at least `args[1]` elements, or a `RangeError`.
    ListCheckLen,
    /// `item in xs`, `item in s` and `key in m`: unlike the other methods,
    /// `args[0]` is the item, which is evaluated first, and `args[1]` the
    /// collection.
    InList,
    InSet,
    InMap,
    /// Take-and-restore (dat-own): `m.get(k, fallback)` whose value goes
    /// back under `k` before anything else reads the map. The value moves
    /// out, leaving a placeholder that keeps the entry's place.
    MapTake,
    /// Take-and-restore of `m[k]` or `xs[i]`: the value at the place moves
    /// out, leaving a placeholder.
    Take,
}

impl Method {
    /// Changes its receiver, so it needs a place ([`ExprKind::Mutate`]).
    pub fn mutates(self) -> bool {
        use Method::*;
        matches!(
            self,
            ListPush
                | ListPushAll
                | ListPop
                | ListUnshift
                | ListShift
                | ListInsert
                | ListRemoveAt
                | ListRemove
                | ListSort
                | ListSortWith
                | ListSortBy
                | ListReverse
                | ListClear
                | SetAdd
                | SetDelete
                | SetClear
                | MapSet
                | MapDelete
                | MapClear
                | MapTake
                | Take
        )
    }

    /// Throws a `RangeError` of its own.
    pub fn throws(self) -> bool {
        use Method::*;
        matches!(
            self,
            ListAt | ListPop | ListShift | ListInsert | ListRemoveAt | Range | MaxInts | MinInts | MaxFloats | MinFloats | ListCheckLen
        )
    }

    /// Its first argument after the receiver is a callback.
    pub fn takes_callback(self) -> bool {
        use Method::*;
        matches!(
            self,
            ListMap
                | ListFilter
                | ListFlatMap
                | ListReduce
                | ListForEach
                | ListFind
                | ListFindIndex
                | ListSome
                | ListEvery
                | ListCount
                | ListToSortedWith
                | ListToSortedBy
                | ListGroupBy
                | ListSortWith
                | ListSortBy
        )
    }

    /// The name the IR dump shows.
    pub fn name(self) -> String {
        format!("{self:?}")
    }
}
