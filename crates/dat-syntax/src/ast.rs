//! The arena AST. Nodes live in flat vectors and refer to each other by `u32`
//! ids; every node has a byte span.
//!
//! The parser accepts much more than the current milestone implements, so the
//! checker can report "not supported yet" with a precise span instead of a
//! syntax error. Constructs the parser only skips over become `Unsupported`.

use dat_diag::{FileId, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExprId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StmtId(pub u32);

#[derive(Clone, Debug)]
pub struct Ast {
    pub file: FileId,
    pub items: Vec<Item>,
    pub exprs: Vec<Expr>,
    pub stmts: Vec<Stmt>,
}

impl Ast {
    pub fn expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id.0 as usize]
    }

    pub fn stmt(&self, id: StmtId) -> &Stmt {
        &self.stmts[id.0 as usize]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub kind: ItemKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ItemKind {
    Function(Function),
    Test(Test),
    /// A declaration the parser recognised but skipped, such as `struct`.
    Unsupported {
        what: &'static str,
    },
}

#[derive(Clone, Debug)]
pub struct Function {
    pub name: Ident,
    pub exported: bool,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub throws: Option<TypeExpr>,
    pub body: Block,
    /// Span of `<...>` when the function declares type parameters.
    pub generics: Option<Span>,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: Ident,
    pub is_mut: bool,
    pub ty: Option<TypeExpr>,
    pub default: Option<ExprId>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Test {
    pub name: String,
    pub name_span: Span,
    pub body: Block,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<StmtId>,
    /// From `{` to `}` inclusive.
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct TypeExpr {
    pub kind: TypeKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum TypeKind {
    /// `int`, `string`, `RangeError`, ...
    Named(String),
    /// Generic, nullable, union, tuple and function types.
    Unsupported(&'static str),
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    /// Includes the closing `;` when there is one.
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl AssignOp {
    pub fn as_str(self) -> &'static str {
        match self {
            AssignOp::Set => "=",
            AssignOp::Add => "+=",
            AssignOp::Sub => "-=",
            AssignOp::Mul => "*=",
            AssignOp::Div => "/=",
            AssignOp::Rem => "%=",
        }
    }
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Let {
        is_const: bool,
        name: Ident,
        ty: Option<TypeExpr>,
        init: Option<ExprId>,
    },
    Assign {
        target: ExprId,
        op: AssignOp,
        value: ExprId,
    },
    /// `i++;` and `i--;`. `++i;` parses too, with `prefix` set, so the checker can suggest `i++;`.
    IncDec {
        target: ExprId,
        inc: bool,
        prefix: bool,
    },
    Expr(ExprId),
    If {
        cond: ExprId,
        then: Block,
        els: Option<Else>,
    },
    For {
        init: Option<StmtId>,
        cond: Option<ExprId>,
        update: Option<StmtId>,
        body: Block,
    },
    While {
        cond: ExprId,
        body: Block,
    },
    Break,
    Continue,
    Return(Option<ExprId>),
    Throw(ExprId),
    Try {
        body: Block,
        catches: Vec<Catch>,
        finally: Option<Block>,
    },
    /// A statement the parser recognised but skipped, such as `for...of`.
    Unsupported {
        what: &'static str,
    },
}

#[derive(Clone, Debug)]
pub enum Else {
    Block(Block),
    If(StmtId),
}

#[derive(Clone, Debug)]
pub struct Catch {
    pub name: Option<Ident>,
    pub ty: Option<TypeExpr>,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    Nullish,
    In,
}

impl BinOp {
    pub fn as_str(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::Nullish => "??",
            BinOp::In => "in",
        }
    }

    pub fn is_comparison(self) -> bool {
        matches!(self, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Debug)]
pub enum TemplatePart {
    Text(String),
    Expr(ExprId),
}

#[derive(Clone, Debug)]
pub struct Arg {
    pub is_mut: bool,
    pub expr: ExprId,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    /// An integer literal. `None` when it doesn't fit in 64 bits.
    Int(Option<u64>),
    Float(f64),
    Str(String),
    Template(Vec<TemplatePart>),
    Bool(bool),
    Null,
    Name(String),
    Unary {
        op: UnOp,
        expr: ExprId,
    },
    Binary {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    Call {
        callee: ExprId,
        args: Vec<Arg>,
    },
    Field {
        base: ExprId,
        name: Ident,
        optional: bool,
    },
    Index {
        base: ExprId,
        index: ExprId,
    },
    StructLit {
        name: Ident,
        fields: Vec<(Ident, ExprId)>,
    },
    List(Vec<ExprId>),
    Paren(ExprId),
    /// `cond ? then : els`
    Cond {
        cond: ExprId,
        then: ExprId,
        els: ExprId,
    },
    /// `i++` or `++i` used as an expression; always an error.
    IncDec {
        target: ExprId,
        inc: bool,
        prefix: bool,
    },
    /// An expression the parser recognised but skipped, such as an arrow function.
    Unsupported {
        what: &'static str,
    },
}
