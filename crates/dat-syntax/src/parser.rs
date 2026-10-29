//! A hand-written recursive-descent parser with Pratt expressions.
//!
//! Syntax errors are reported as `E0001` and the parser resumes at the next
//! top-level declaration, so one run finds an error per declaration. Inside a
//! declaration it stops at the first error.

use crate::ast::*;
use crate::lexer::{Kw, Token, TokenKind as T, lex};
use dat_diag::{Applicability, Diagnostic, Edit, FileId, Span};

/// A syntax error.
pub const E_SYNTAX: &str = "E0001";

pub struct Parsed {
    pub ast: Ast,
    /// Every token of the file, trivia included.
    pub tokens: Vec<Token>,
    pub diags: Vec<Diagnostic>,
}

pub fn parse(file: FileId, text: &str) -> Parsed {
    let tokens = lex(text);
    let sig: Vec<u32> = tokens.iter().enumerate().filter(|(_, t)| !t.kind.is_trivia()).map(|(i, _)| i as u32).collect();
    let mut p = Parser {
        text,
        file,
        tokens: &tokens,
        sig,
        pos: 0,
        ast: Ast { file, items: Vec::new(), exprs: Vec::new(), stmts: Vec::new() },
        diags: Vec::new(),
        depth: 0,
    };
    p.items();
    let Parser { ast, diags, .. } = p;
    Parsed { ast, tokens, diags }
}

struct Fail;
type PResult<T> = Result<T, Fail>;

struct Parser<'a> {
    text: &'a str,
    file: FileId,
    tokens: &'a [Token],
    /// Indices of the non-trivia tokens.
    sig: Vec<u32>,
    pos: usize,
    ast: Ast,
    diags: Vec<Diagnostic>,
    /// How deeply expressions and statements nest right now.
    depth: u32,
}

/// Deeper nesting is an error, so no later stage can overflow its stack.
const MAX_NESTING: u32 = 8000;

const ITEM_KEYWORDS: [Kw; 8] = [Kw::Function, Kw::Test, Kw::Struct, Kw::Enum, Kw::Interface, Kw::Type, Kw::Import, Kw::Export];

impl<'a> Parser<'a> {
    // ---- token access ----

    fn tok(&self, k: usize) -> Token {
        let i = (self.pos + k).min(self.sig.len() - 1);
        self.tokens[self.sig[i] as usize]
    }

    fn peek(&self) -> T {
        self.tok(0).kind
    }

    fn peek_at(&self, k: usize) -> T {
        self.tok(k).kind
    }

    fn at(&self, kind: T) -> bool {
        self.peek() == kind
    }

    fn at_kw(&self, kw: Kw) -> bool {
        self.peek() == T::Keyword(kw)
    }

    fn bump(&mut self) -> Token {
        let t = self.tok(0);
        if self.pos < self.sig.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, kind: T) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, kw: Kw) -> bool {
        self.eat(T::Keyword(kw))
    }

    /// End of the previous significant token.
    fn prev_end(&self) -> u32 {
        if self.pos == 0 { 0 } else { self.tokens[self.sig[self.pos - 1] as usize].span.end }
    }

    fn text_of(&self, span: Span) -> &'a str {
        &self.text[span.start as usize..span.end as usize]
    }

    fn describe(&self, t: Token) -> String {
        match t.kind {
            T::Eof => "the end of the file".to_string(),
            T::Str | T::TemplateFull | T::TemplateHead => "a string".to_string(),
            _ => format!("`{}`", self.text_of(t.span)),
        }
    }

    fn error<X>(&mut self, span: Span, message: impl Into<String>) -> PResult<X> {
        self.diags.push(Diagnostic::error(E_SYNTAX, self.file, span, message));
        Err(Fail)
    }

    fn expect(&mut self, kind: T, what: &str) -> PResult<Token> {
        if self.at(kind) {
            return Ok(self.bump());
        }
        let t = self.tok(0);
        let found = self.describe(t);
        if kind == T::Semi {
            // A missing `;` has an exact fix: insert it after the previous token.
            let at = self.prev_end();
            let d = Diagnostic::error(E_SYNTAX, self.file, t.span, format!("expected `;`, found {found}"))
                .with_label("the statement before this needs a `;`")
                .with_fix(
                    "add `;`",
                    Applicability::Safe,
                    vec![Edit { file: self.file, span: Span::new(at, at), before: String::new(), after: ";".into() }],
                );
            self.diags.push(d);
            return Err(Fail);
        }
        self.error(t.span, format!("expected {what}, found {found}"))
    }

    fn ident(&mut self, what: &str) -> PResult<Ident> {
        let t = self.expect(T::Ident, what)?;
        Ok(Ident { name: self.text_of(t.span).to_string(), span: t.span })
    }

    /// A member name after `.`: any identifier or keyword.
    fn member_name(&mut self) -> PResult<Ident> {
        let t = self.tok(0);
        match t.kind {
            T::Ident | T::Keyword(_) => {
                self.bump();
                Ok(Ident { name: self.text_of(t.span).to_string(), span: t.span })
            }
            _ => {
                let found = self.describe(t);
                self.error(t.span, format!("expected a member name, found {found}"))
            }
        }
    }

    /// Skips a bracketed group starting at the current `(`, `[`, `{` or `<`,
    /// and returns its span.
    fn skip_group(&mut self) -> PResult<Span> {
        let open = self.bump();
        let close = match open.kind {
            T::LParen => T::RParen,
            T::LBracket => T::RBracket,
            T::LBrace => T::RBrace,
            T::Lt => T::Gt,
            _ => return Ok(open.span),
        };
        let mut depth = 0i32;
        loop {
            let t = self.tok(0);
            match t.kind {
                T::Eof => return self.error(open.span, "this bracket is never closed"),
                k if k == open.kind => depth += 1,
                k if k == close && depth == 0 => {
                    self.bump();
                    return Ok(open.span.to(t.span));
                }
                k if k == close => depth -= 1,
                _ => {}
            }
            self.bump();
        }
    }

    /// Skips tokens up to and including the next `;` outside brackets, or up
    /// to (not including) a `}` that closes the enclosing block.
    fn skip_statement(&mut self) -> PResult<Span> {
        let start = self.tok(0).span;
        loop {
            match self.peek() {
                T::Semi => {
                    let t = self.bump();
                    return Ok(start.to(t.span));
                }
                T::RBrace | T::Eof => return Ok(start.to(Span::new(self.prev_end(), self.prev_end()))),
                T::LParen | T::LBracket | T::LBrace => {
                    self.skip_group()?;
                }
                _ => {
                    self.bump();
                }
            }
        }
    }

    fn alloc_expr(&mut self, kind: ExprKind, span: Span) -> ExprId {
        self.ast.exprs.push(Expr { kind, span });
        ExprId(self.ast.exprs.len() as u32 - 1)
    }

    fn alloc_stmt(&mut self, kind: StmtKind, span: Span) -> StmtId {
        self.ast.stmts.push(Stmt { kind, span });
        StmtId(self.ast.stmts.len() as u32 - 1)
    }

    fn espan(&self, e: ExprId) -> Span {
        self.ast.expr(e).span
    }

    // ---- items ----

    fn items(&mut self) {
        while !self.at(T::Eof) {
            let start = self.pos;
            if self.item().is_err() {
                // Resume at the next declaration keyword.
                if self.pos == start {
                    self.bump();
                }
                while !self.at(T::Eof) && !ITEM_KEYWORDS.iter().any(|&k| self.at_kw(k)) {
                    self.bump();
                }
            }
        }
    }

    fn item(&mut self) -> PResult<()> {
        let start = self.tok(0).span;
        let exported = self.eat_kw(Kw::Export);
        let kind = match self.peek() {
            T::Keyword(Kw::Function) => ItemKind::Function(self.function(exported)?),
            T::Keyword(Kw::Test) => {
                self.bump();
                let name_tok = self.expect(T::Str, "the test name, a string")?;
                let name = self.decode_string(name_tok)?;
                let body = self.block()?;
                ItemKind::Test(Test { name, name_span: name_tok.span, body })
            }
            T::Keyword(kw @ (Kw::Struct | Kw::Enum | Kw::Interface)) => {
                while !self.at(T::LBrace) && !self.at(T::Eof) {
                    self.bump();
                }
                self.skip_group()?;
                ItemKind::Unsupported {
                    what: match kw {
                        Kw::Struct => "struct declarations",
                        Kw::Enum => "enum declarations",
                        _ => "interface declarations",
                    },
                }
            }
            T::Keyword(Kw::Type) => {
                self.skip_statement()?;
                ItemKind::Unsupported { what: "type aliases" }
            }
            T::Keyword(Kw::Import) => {
                self.skip_statement()?;
                ItemKind::Unsupported { what: "imports" }
            }
            T::Keyword(Kw::Const) => {
                self.skip_statement()?;
                ItemKind::Unsupported { what: "module constants" }
            }
            T::LBrace if exported => {
                self.skip_statement()?;
                ItemKind::Unsupported { what: "re-exports" }
            }
            T::Keyword(Kw::Let) => {
                let t = self.tok(0);
                return self.error(t.span, "a module's top level can't declare `let` variables; use `const`, or move it into a function");
            }
            _ => {
                let t = self.tok(0);
                let found = self.describe(t);
                return self.error(
                    t.span,
                    format!(
                        "expected a declaration such as `function` or `test`, found {found}; statements aren't allowed outside functions"
                    ),
                );
            }
        };
        let span = start.to(Span::new(self.prev_end(), self.prev_end()));
        self.ast.items.push(Item { kind, span });
        Ok(())
    }

    fn function(&mut self, exported: bool) -> PResult<Function> {
        self.bump(); // function
        let name = self.ident("a function name")?;
        let generics = if self.at(T::Lt) { Some(self.skip_group()?) } else { None };
        self.expect(T::LParen, "`(`")?;
        let mut params = Vec::new();
        while !self.at(T::RParen) {
            let start = self.tok(0).span;
            let is_mut = self.eat_kw(Kw::Mut);
            let name = if self.at_kw(Kw::This) {
                let t = self.bump();
                Ident { name: "this".into(), span: t.span }
            } else {
                self.ident("a parameter name")?
            };
            let ty = if self.eat(T::Colon) { Some(self.type_expr()?) } else { None };
            let default = if self.eat(T::Eq) { Some(self.expr()?) } else { None };
            let span = start.to(Span::new(self.prev_end(), self.prev_end()));
            params.push(Param { name, is_mut, ty, default, span });
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect(T::RParen, "`)` or `,`")?;
        let ret = if self.eat(T::Colon) { Some(self.type_expr()?) } else { None };
        let throws = if self.eat_kw(Kw::Throws) { Some(self.type_expr()?) } else { None };
        let body = self.block()?;
        Ok(Function { name, exported, params, ret, throws, body, generics })
    }

    // ---- types ----

    fn type_expr(&mut self) -> PResult<TypeExpr> {
        let first = self.type_postfix()?;
        if self.at(T::Pipe) {
            let mut span = first.span;
            while self.eat(T::Pipe) {
                span = span.to(self.type_postfix()?.span);
            }
            return Ok(TypeExpr { kind: TypeKind::Unsupported("union types"), span });
        }
        Ok(first)
    }

    fn type_postfix(&mut self) -> PResult<TypeExpr> {
        let mut ty = self.type_primary()?;
        while self.at(T::Question) {
            let q = self.bump();
            ty = TypeExpr { kind: TypeKind::Unsupported("nullable types"), span: ty.span.to(q.span) };
        }
        Ok(ty)
    }

    fn type_primary(&mut self) -> PResult<TypeExpr> {
        let t = self.tok(0);
        match t.kind {
            T::Keyword(Kw::List | Kw::Set | Kw::Map) => {
                self.bump();
                let span = if self.at(T::Lt) { t.span.to(self.skip_group()?) } else { t.span };
                Ok(TypeExpr { kind: TypeKind::Unsupported("collection types"), span })
            }
            T::Ident | T::Keyword(Kw::Int | Kw::Float | Kw::String | Kw::Bool | Kw::Void | Kw::Never | Kw::Null) => {
                self.bump();
                let mut span = t.span;
                let mut name = self.text_of(t.span).to_string();
                let mut qualified = false;
                while self.at(T::Dot) {
                    self.bump();
                    span = span.to(self.member_name()?.span);
                    qualified = true;
                }
                if self.at(T::Lt) {
                    span = span.to(self.skip_group()?);
                    return Ok(TypeExpr { kind: TypeKind::Unsupported("generic types"), span });
                }
                if qualified {
                    return Ok(TypeExpr { kind: TypeKind::Unsupported("imported types"), span });
                }
                if name == "null" {
                    name = "null".into();
                }
                Ok(TypeExpr { kind: TypeKind::Named(name), span })
            }
            T::LBracket => {
                let span = self.skip_group()?;
                Ok(TypeExpr { kind: TypeKind::Unsupported("tuple types"), span })
            }
            T::LParen => {
                let mut span = self.skip_group()?;
                self.expect(T::Arrow, "`=>` in a function type")?;
                span = span.to(self.type_postfix()?.span);
                if self.eat_kw(Kw::Throws) {
                    span = span.to(self.type_postfix()?.span);
                }
                Ok(TypeExpr { kind: TypeKind::Unsupported("function types"), span })
            }
            _ => {
                let found = self.describe(t);
                self.error(t.span, format!("expected a type, found {found}"))
            }
        }
    }

    // ---- statements ----

    fn block(&mut self) -> PResult<Block> {
        let open = self.expect(T::LBrace, "`{`")?;
        let mut stmts = Vec::new();
        while !self.at(T::RBrace) {
            if self.at(T::Eof) {
                return self.error(open.span, "this `{` is never closed");
            }
            stmts.push(self.stmt()?);
        }
        let close = self.bump();
        Ok(Block { stmts, span: open.span.to(close.span) })
    }

    fn stmt(&mut self) -> PResult<StmtId> {
        self.nested(Self::stmt_inner)
    }

    fn stmt_inner(&mut self) -> PResult<StmtId> {
        let start = self.tok(0).span;
        let kind = match self.peek() {
            T::Keyword(Kw::Let | Kw::Const) => {
                let kind = self.let_decl()?;
                if !matches!(kind, StmtKind::Unsupported { .. }) {
                    self.expect(T::Semi, "`;`")?;
                }
                kind
            }
            T::Keyword(Kw::If) => self.if_stmt()?,
            T::Keyword(Kw::For) => self.for_stmt()?,
            T::Keyword(Kw::While) => {
                self.bump();
                self.expect(T::LParen, "`(` after `while`")?;
                let cond = self.expr()?;
                self.expect(T::RParen, "`)`")?;
                let body = self.block()?;
                StmtKind::While { cond, body }
            }
            T::Keyword(Kw::Break) => {
                self.bump();
                self.expect(T::Semi, "`;`")?;
                StmtKind::Break
            }
            T::Keyword(Kw::Continue) => {
                self.bump();
                self.expect(T::Semi, "`;`")?;
                StmtKind::Continue
            }
            T::Keyword(Kw::Return) => {
                self.bump();
                let value = if self.at(T::Semi) || self.at(T::RBrace) { None } else { Some(self.expr()?) };
                self.expect(T::Semi, "`;`")?;
                StmtKind::Return(value)
            }
            T::Keyword(Kw::Throw) => {
                self.bump();
                let value = self.expr()?;
                self.expect(T::Semi, "`;`")?;
                StmtKind::Throw(value)
            }
            T::Keyword(Kw::Try) if self.peek_at(1) == T::LBrace => self.try_stmt()?,
            T::Keyword(Kw::Match) => {
                // A `match` used as a statement ends with its `}`.
                let e = self.expr()?;
                self.eat(T::Semi);
                StmtKind::Expr(e)
            }
            T::Keyword(Kw::Function) => {
                let t = self.tok(0);
                return self
                    .error(t.span, "named functions can't be declared inside other functions; assign an arrow function to a `const`");
            }
            T::LBrace => {
                self.skip_group()?;
                StmtKind::Unsupported { what: "bare blocks" }
            }
            _ => {
                let kind = self.simple_stmt()?;
                self.expect(T::Semi, "`;`")?;
                kind
            }
        };
        let span = start.to(Span::new(self.prev_end(), self.prev_end()));
        Ok(self.alloc_stmt(kind, span))
    }

    fn let_decl(&mut self) -> PResult<StmtKind> {
        let is_const = self.bump().kind == T::Keyword(Kw::Const);
        if self.at(T::LBracket) || self.at(T::LBrace) {
            self.skip_statement()?;
            return Ok(StmtKind::Unsupported { what: "destructuring" });
        }
        let name = self.ident("a variable name")?;
        let ty = if self.eat(T::Colon) { Some(self.type_expr()?) } else { None };
        let init = if self.eat(T::Eq) { Some(self.expr()?) } else { None };
        Ok(StmtKind::Let { is_const, name, ty, init })
    }

    /// An assignment, increment or expression, without the `;`.
    fn simple_stmt(&mut self) -> PResult<StmtKind> {
        if self.at_kw(Kw::Let) || self.at_kw(Kw::Const) {
            return self.let_decl();
        }
        let target = self.expr()?;
        let op = match self.peek() {
            T::Eq => AssignOp::Set,
            T::PlusEq => AssignOp::Add,
            T::MinusEq => AssignOp::Sub,
            T::StarEq => AssignOp::Mul,
            T::SlashEq => AssignOp::Div,
            T::PercentEq => AssignOp::Rem,
            _ => {
                if let ExprKind::IncDec { target, inc, prefix } = self.ast.expr(target).kind {
                    return Ok(StmtKind::IncDec { target, inc, prefix });
                }
                return Ok(StmtKind::Expr(target));
            }
        };
        self.bump();
        let value = self.expr()?;
        if matches!(self.peek(), T::Eq | T::PlusEq | T::MinusEq | T::StarEq | T::SlashEq | T::PercentEq) {
            let t = self.tok(0);
            return self.error(t.span, "assignments are statements, not expressions, so they can't be chained");
        }
        Ok(StmtKind::Assign { target, op, value })
    }

    fn if_stmt(&mut self) -> PResult<StmtKind> {
        self.bump();
        self.expect(T::LParen, "`(` after `if`")?;
        let cond = self.expr()?;
        if self.at(T::Eq) {
            let t = self.tok(0);
            return self.error(t.span, "assignments are statements, so `=` can't appear in a condition; compare with `==`");
        }
        self.expect(T::RParen, "`)`")?;
        let then = self.block()?;
        let els = if self.eat_kw(Kw::Else) {
            if self.at_kw(Kw::If) {
                let start = self.tok(0).span;
                let kind = self.nested(Self::if_stmt)?;
                let span = start.to(Span::new(self.prev_end(), self.prev_end()));
                Some(Else::If(self.alloc_stmt(kind, span)))
            } else {
                Some(Else::Block(self.block()?))
            }
        } else {
            None
        };
        Ok(StmtKind::If { cond, then, els })
    }

    fn for_stmt(&mut self) -> PResult<StmtKind> {
        self.bump();
        if !self.at(T::LParen) {
            let t = self.tok(0);
            let found = self.describe(t);
            return self.error(t.span, format!("expected `(` after `for`, found {found}"));
        }
        // `for (const x of xs)`: look for `of` before the first `;` inside the parentheses.
        let mut k = 1;
        let mut depth = 0;
        let mut is_of = false;
        loop {
            match self.peek_at(k) {
                T::LParen | T::LBracket | T::LBrace => depth += 1,
                T::RParen | T::RBracket | T::RBrace if depth == 0 => break,
                T::RParen | T::RBracket | T::RBrace => depth -= 1,
                T::Semi | T::Eof => break,
                T::Keyword(Kw::Of) if depth == 0 => {
                    is_of = true;
                    break;
                }
                T::Keyword(Kw::In) if depth == 0 && k == 3 => {
                    let t = self.tok(k);
                    return self.error(t.span, "`for...in` isn't valid in dat; use `for...of`");
                }
                _ => {}
            }
            k += 1;
        }
        if is_of {
            self.skip_group()?;
            self.skip_group()?;
            return Ok(StmtKind::Unsupported { what: "for...of loops" });
        }
        self.bump(); // (
        let init = if self.at(T::Semi) { None } else { Some(self.simple_stmt_node()?) };
        self.expect(T::Semi, "`;`")?;
        let cond = if self.at(T::Semi) { None } else { Some(self.expr()?) };
        self.expect(T::Semi, "`;`")?;
        let update = if self.at(T::RParen) { None } else { Some(self.simple_stmt_node()?) };
        self.expect(T::RParen, "`)`")?;
        let body = self.block()?;
        Ok(StmtKind::For { init, cond, update, body })
    }

    fn simple_stmt_node(&mut self) -> PResult<StmtId> {
        let start = self.tok(0).span;
        let kind = self.simple_stmt()?;
        let span = start.to(Span::new(self.prev_end(), self.prev_end()));
        Ok(self.alloc_stmt(kind, span))
    }

    fn try_stmt(&mut self) -> PResult<StmtKind> {
        self.bump();
        let body = self.block()?;
        let mut catches = Vec::new();
        while self.at_kw(Kw::Catch) {
            let start = self.bump().span;
            let (mut name, mut ty) = (None, None);
            if self.eat(T::LParen) {
                name = Some(self.ident("the name of the caught error")?);
                if self.eat(T::Colon) {
                    ty = Some(self.type_expr()?);
                }
                self.expect(T::RParen, "`)`")?;
            }
            let body = self.block()?;
            let span = start.to(body.span);
            catches.push(Catch { name, ty, body, span });
        }
        let finally = if self.eat_kw(Kw::Finally) { Some(self.block()?) } else { None };
        if catches.is_empty() && finally.is_none() {
            let t = self.tok(0);
            let found = self.describe(t);
            return self.error(t.span, format!("expected `catch` or `finally` after the `try` block, found {found}"));
        }
        Ok(StmtKind::Try { body, catches, finally })
    }

    // ---- expressions ----

    /// Runs `f` one nesting level deeper.
    fn nested<X>(&mut self, f: impl FnOnce(&mut Self) -> PResult<X>) -> PResult<X> {
        if self.depth >= MAX_NESTING {
            let t = self.tok(0);
            return self.error(t.span, format!("this nests more than {MAX_NESTING} levels deep; split it into functions or variables"));
        }
        self.depth += 1;
        let r = f(self);
        self.depth -= 1;
        r
    }

    pub(crate) fn expr(&mut self) -> PResult<ExprId> {
        self.nested(Self::expr_inner)
    }

    fn expr_inner(&mut self) -> PResult<ExprId> {
        let cond = self.binary(0)?;
        if self.at(T::Question) {
            self.bump();
            let then = self.expr()?;
            self.expect(T::Colon, "`:` in a conditional expression")?;
            let els = self.expr()?;
            let span = self.espan(cond).to(self.espan(els));
            return Ok(self.alloc_expr(ExprKind::Cond { cond, then, els }, span));
        }
        Ok(cond)
    }

    fn binop(&self) -> Option<(BinOp, u8)> {
        Some(match self.peek() {
            T::QuestionQuestion => (BinOp::Nullish, 1),
            T::OrOr => (BinOp::Or, 2),
            T::AndAnd => (BinOp::And, 3),
            T::EqEq | T::EqEqEq => (BinOp::Eq, 4),
            T::BangEq | T::BangEqEq => (BinOp::Ne, 4),
            T::Lt => (BinOp::Lt, 5),
            T::Le => (BinOp::Le, 5),
            T::Gt => (BinOp::Gt, 5),
            T::Ge => (BinOp::Ge, 5),
            T::Keyword(Kw::In) => (BinOp::In, 5),
            T::Plus => (BinOp::Add, 6),
            T::Minus => (BinOp::Sub, 6),
            T::Star => (BinOp::Mul, 7),
            T::Slash => (BinOp::Div, 7),
            T::Percent => (BinOp::Rem, 7),
            _ => return None,
        })
    }

    fn binary(&mut self, min_bp: u8) -> PResult<ExprId> {
        let mut lhs = self.unary()?;
        loop {
            if (self.at_kw(Kw::Is) || self.at_kw(Kw::As)) && min_bp < 5 {
                self.bump();
                self.eat(T::Question);
                let ty = self.type_postfix()?;
                let span = self.espan(lhs).to(ty.span);
                lhs = self.alloc_expr(ExprKind::Unsupported { what: "type tests and casts" }, span);
                continue;
            }
            let Some((op, bp)) = self.binop() else { break };
            if bp <= min_bp {
                break;
            }
            let op_tok = self.bump();
            if matches!(op_tok.kind, T::EqEqEq | T::BangEqEq) {
                let (bad, good) = if op_tok.kind == T::EqEqEq { ("===", "==") } else { ("!==", "!=") };
                let d = Diagnostic::error(E_SYNTAX, self.file, op_tok.span, format!("dat has no `{bad}`; `{good}` compares contents"))
                    .with_fix(
                        format!("use `{good}`"),
                        Applicability::Safe,
                        vec![Edit { file: self.file, span: op_tok.span, before: bad.into(), after: good.into() }],
                    );
                self.diags.push(d);
                return Err(Fail);
            }
            let rhs = self.binary(bp)?;
            let span = self.espan(lhs).to(self.espan(rhs));
            lhs = self.alloc_expr(ExprKind::Binary { op, lhs, rhs }, span);
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> PResult<ExprId> {
        let t = self.tok(0);
        match t.kind {
            T::Minus | T::Bang => {
                self.bump();
                let expr = self.nested(Self::unary)?;
                let op = if t.kind == T::Minus { UnOp::Neg } else { UnOp::Not };
                let span = t.span.to(self.espan(expr));
                Ok(self.alloc_expr(ExprKind::Unary { op, expr }, span))
            }
            T::PlusPlus | T::MinusMinus => {
                self.bump();
                let target = self.unary()?;
                let span = t.span.to(self.espan(target));
                Ok(self.alloc_expr(ExprKind::IncDec { target, inc: t.kind == T::PlusPlus, prefix: true }, span))
            }
            T::Keyword(Kw::Try) => {
                self.bump();
                if !self.eat(T::Question) {
                    self.eat(T::Bang);
                }
                let expr = self.unary()?;
                let span = t.span.to(self.espan(expr));
                Ok(self.alloc_expr(ExprKind::Unsupported { what: "`try` expressions" }, span))
            }
            _ => {
                let e = self.primary()?;
                self.postfix(e)
            }
        }
    }

    fn postfix(&mut self, mut e: ExprId) -> PResult<ExprId> {
        loop {
            let t = self.tok(0);
            match t.kind {
                T::Dot | T::QuestionDot => {
                    self.bump();
                    let name = self.member_name()?;
                    let span = self.espan(e).to(name.span);
                    e = self.alloc_expr(ExprKind::Field { base: e, name, optional: t.kind == T::QuestionDot }, span);
                }
                T::LParen => {
                    self.bump();
                    let mut args = Vec::new();
                    while !self.at(T::RParen) {
                        let is_mut = self.eat_kw(Kw::Mut);
                        let expr = if self.at(T::Ellipsis) {
                            let s = self.bump().span;
                            let inner = self.expr()?;
                            let span = s.to(self.espan(inner));
                            self.alloc_expr(ExprKind::Unsupported { what: "spread arguments" }, span)
                        } else {
                            self.expr()?
                        };
                        args.push(Arg { is_mut, expr });
                        if !self.eat(T::Comma) {
                            break;
                        }
                    }
                    let close = self.expect(T::RParen, "`)` or `,` in the argument list")?;
                    let span = self.espan(e).to(close.span);
                    e = self.alloc_expr(ExprKind::Call { callee: e, args }, span);
                }
                T::LBracket => {
                    self.bump();
                    let index = self.expr()?;
                    let close = self.expect(T::RBracket, "`]`")?;
                    let span = self.espan(e).to(close.span);
                    e = self.alloc_expr(ExprKind::Index { base: e, index }, span);
                }
                T::Bang => {
                    self.bump();
                    let span = self.espan(e).to(t.span);
                    e = self.alloc_expr(ExprKind::Unsupported { what: "non-null assertions" }, span);
                }
                T::PlusPlus | T::MinusMinus => {
                    self.bump();
                    let span = self.espan(e).to(t.span);
                    e = self.alloc_expr(ExprKind::IncDec { target: e, inc: t.kind == T::PlusPlus, prefix: false }, span);
                }
                _ => return Ok(e),
            }
        }
    }

    fn primary(&mut self) -> PResult<ExprId> {
        let t = self.tok(0);
        let kind = match t.kind {
            T::Int => {
                self.bump();
                ExprKind::Int(parse_int(self.text_of(t.span)))
            }
            T::Float => {
                self.bump();
                let text: String = self.text_of(t.span).chars().filter(|&c| c != '_').collect();
                ExprKind::Float(text.parse().unwrap_or(f64::NAN))
            }
            T::Str => {
                self.bump();
                ExprKind::Str(self.decode_string(t)?)
            }
            T::TemplateFull | T::TemplateHead => return self.template(),
            T::Keyword(Kw::True) => {
                self.bump();
                ExprKind::Bool(true)
            }
            T::Keyword(Kw::False) => {
                self.bump();
                ExprKind::Bool(false)
            }
            T::Keyword(Kw::Null) => {
                self.bump();
                ExprKind::Null
            }
            T::Keyword(Kw::This) => {
                self.bump();
                ExprKind::Unsupported { what: "`this`" }
            }
            T::Ident => {
                self.bump();
                let name = self.text_of(t.span).to_string();
                if self.at(T::LBrace) {
                    return self.struct_lit(Ident { name, span: t.span });
                }
                ExprKind::Name(name)
            }
            T::LParen => {
                if self.is_arrow_function() {
                    return self.skip_arrow_function();
                }
                self.bump();
                let inner = self.expr()?;
                let close = self.expect(T::RParen, "`)`")?;
                let span = t.span.to(close.span);
                return Ok(self.alloc_expr(ExprKind::Paren(inner), span));
            }
            T::LBracket => {
                self.bump();
                let mut items = Vec::new();
                while !self.at(T::RBracket) {
                    if self.at(T::Ellipsis) {
                        let s = self.bump().span;
                        let inner = self.expr()?;
                        let span = s.to(self.espan(inner));
                        items.push(self.alloc_expr(ExprKind::Unsupported { what: "spread" }, span));
                    } else {
                        items.push(self.expr()?);
                    }
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                let close = self.expect(T::RBracket, "`]` or `,` in the list")?;
                let span = t.span.to(close.span);
                return Ok(self.alloc_expr(ExprKind::List(items), span));
            }
            T::LBrace => {
                let span = self.skip_group()?;
                return Ok(self.alloc_expr(ExprKind::Unsupported { what: "map literals" }, span));
            }
            T::Keyword(Kw::Match) => {
                self.bump();
                let mut span = t.span;
                if self.at(T::LParen) {
                    span = span.to(self.skip_group()?);
                }
                if self.at(T::LBrace) {
                    span = span.to(self.skip_group()?);
                }
                return Ok(self.alloc_expr(ExprKind::Unsupported { what: "`match` expressions" }, span));
            }
            T::Keyword(Kw::Int | Kw::Float | Kw::String | Kw::Bool) => {
                // `int.MAX` and similar.
                self.bump();
                ExprKind::Unsupported { what: "type members" }
            }
            T::Unknown => {
                let text = self.text_of(t.span);
                let msg = if text.starts_with('"') || text.starts_with('`') {
                    "this string is never closed".to_string()
                } else {
                    format!("unexpected character `{text}`")
                };
                return self.error(t.span, msg);
            }
            _ => {
                let found = self.describe(t);
                return self.error(t.span, format!("expected an expression, found {found}"));
            }
        };
        Ok(self.alloc_expr(kind, t.span))
    }

    fn struct_lit(&mut self, name: Ident) -> PResult<ExprId> {
        self.bump(); // {
        let mut fields = Vec::new();
        while !self.at(T::RBrace) {
            if self.at(T::Ellipsis) {
                let open_span = name.span;
                while !self.at(T::RBrace) && !self.at(T::Eof) {
                    if matches!(self.peek(), T::LParen | T::LBracket | T::LBrace) {
                        self.skip_group()?;
                    } else {
                        self.bump();
                    }
                }
                let close = self.expect(T::RBrace, "`}`")?;
                return Ok(self.alloc_expr(ExprKind::Unsupported { what: "struct spread" }, open_span.to(close.span)));
            }
            let field = self.member_name()?;
            let value = if self.eat(T::Colon) { self.expr()? } else { self.alloc_expr(ExprKind::Name(field.name.clone()), field.span) };
            fields.push((field, value));
            if !self.eat(T::Comma) {
                break;
            }
        }
        let close = self.expect(T::RBrace, "`}` or `,` in the struct literal")?;
        let span = name.span.to(close.span);
        Ok(self.alloc_expr(ExprKind::StructLit { name, fields }, span))
    }

    /// At `(`: is this the parameter list of an arrow function?
    fn is_arrow_function(&self) -> bool {
        let mut k = 0;
        let mut depth = 0;
        loop {
            match self.peek_at(k) {
                T::LParen | T::LBracket | T::LBrace => depth += 1,
                T::RParen | T::RBracket | T::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                T::Eof => return false,
                _ => {}
            }
            k += 1;
        }
        match self.peek_at(k + 1) {
            T::Arrow => true,
            // `(x: int): int => ...`
            T::Colon => {
                let mut j = k + 2;
                loop {
                    match self.peek_at(j) {
                        T::Arrow => return true,
                        T::Semi | T::Eof | T::LBrace | T::RBrace | T::RParen | T::Comma => return false,
                        _ => j += 1,
                    }
                }
            }
            _ => false,
        }
    }

    fn skip_arrow_function(&mut self) -> PResult<ExprId> {
        let start = self.skip_group()?;
        while !self.at(T::Arrow) && !self.at(T::Eof) {
            self.bump();
        }
        self.expect(T::Arrow, "`=>`")?;
        let end = if self.at(T::LBrace) {
            self.skip_group()?
        } else {
            let body = self.expr()?;
            self.espan(body)
        };
        Ok(self.alloc_expr(ExprKind::Unsupported { what: "arrow functions" }, start.to(end)))
    }

    fn template(&mut self) -> PResult<ExprId> {
        let first = self.bump();
        let mut parts = Vec::new();
        let decode = |p: &mut Self, tok: Token, skip_start: usize, skip_end: usize| -> PResult<String> {
            let raw = p.text_of(tok.span);
            let inner = &raw[skip_start..raw.len() - skip_end];
            p.decode_escapes(inner, tok.span.start + skip_start as u32, true)
        };
        if first.kind == T::TemplateFull {
            let text = decode(self, first, 1, 1)?;
            parts.push(TemplatePart::Text(text));
            return Ok(self.alloc_expr(ExprKind::Template(parts), first.span));
        }
        parts.push(TemplatePart::Text(decode(self, first, 1, 2)?));
        loop {
            let e = self.expr()?;
            parts.push(TemplatePart::Expr(e));
            let t = self.tok(0);
            match t.kind {
                T::TemplateMiddle => {
                    self.bump();
                    parts.push(TemplatePart::Text(decode(self, t, 1, 2)?));
                }
                T::TemplateTail => {
                    self.bump();
                    parts.push(TemplatePart::Text(decode(self, t, 1, 1)?));
                    return Ok(self.alloc_expr(ExprKind::Template(parts), first.span.to(t.span)));
                }
                _ => {
                    let found = self.describe(t);
                    return self.error(t.span, format!("expected `}}` to end the template substitution, found {found}"));
                }
            }
        }
    }

    fn decode_string(&mut self, t: Token) -> PResult<String> {
        let raw = self.text_of(t.span);
        self.decode_escapes(&raw[1..raw.len() - 1], t.span.start + 1, false)
    }

    fn decode_escapes(&mut self, s: &str, base: u32, template: bool) -> PResult<String> {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.char_indices();
        while let Some((i, c)) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            let Some((_, e)) = chars.next() else { break };
            match e {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '0' => out.push('\0'),
                '`' if template => out.push('`'),
                '$' if template => out.push('$'),
                'u' => {
                    let rest = &s[i + 2..];
                    let code = rest.strip_prefix('{').and_then(|r| r.split_once('}')).map(|(hex, _)| hex);
                    match code.and_then(|h| u32::from_str_radix(h, 16).ok()).and_then(char::from_u32) {
                        Some(ch) => {
                            out.push(ch);
                            for _ in 0..code.unwrap().len() + 2 {
                                chars.next();
                            }
                        }
                        None => {
                            let span = Span::new(base + i as u32, base + i as u32 + 2);
                            return self.error(span, "`\\u` must be followed by a code point in braces, such as `\\u{1F600}`");
                        }
                    }
                }
                other => {
                    let span = Span::new(base + i as u32, base + (i + 1 + other.len_utf8()) as u32);
                    let d = Diagnostic::error(E_SYNTAX, self.file, span, format!("unknown escape `\\{other}`"))
                        .with_label("supported escapes are \\n \\t \\r \\\" \\\\ \\0 and \\u{...}")
                        .with_fix(
                            "escape the backslash",
                            Applicability::Review,
                            vec![Edit { file: self.file, span, before: format!("\\{other}"), after: format!("\\\\{other}") }],
                        );
                    self.diags.push(d);
                    return Err(Fail);
                }
            }
        }
        Ok(out)
    }
}

/// Parses an integer literal: decimal with `_` separators, `0x` or `0b`.
/// `None` when it doesn't fit in a `u64`.
fn parse_int(text: &str) -> Option<u64> {
    let clean: String = text.chars().filter(|&c| c != '_').collect();
    if let Some(hex) = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else if let Some(bin) = clean.strip_prefix("0b").or_else(|| clean.strip_prefix("0B")) {
        u64::from_str_radix(bin, 2).ok()
    } else {
        clean.parse().ok()
    }
}
