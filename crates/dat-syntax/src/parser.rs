//! A hand-written recursive-descent parser with Pratt expressions.
//!
//! Syntax errors are reported as `E0001`. After one, the parser resumes at
//! the next statement, or at the next top-level declaration when the error
//! is outside a function body, so one run finds every error. A statement
//! that fails becomes `StmtKind::Error`, and a `let` keeps its name, so the
//! checker reports nothing that follows from the syntax error.

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
        ast: Ast { file, items: Vec::new(), exprs: Vec::new(), stmts: Vec::new(), broken: Vec::new() },
        diags: Vec::new(),
        depth: 0,
        fatal: false,
        speculating: false,
        seen_declaration: false,
        split: false,
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
    /// Nesting went too deep: give up on the declaration instead of resuming.
    fatal: bool,
    /// Trying out a parse that will be undone.
    speculating: bool,
    /// A declaration other than an import has been read.
    seen_declaration: bool,
    /// The token at `pos` is a `>=` whose `>` closed a type, as in
    /// `list<int>= []`: what's left of it reads as `=`.
    split: bool,
}

/// Deeper nesting is an error, so no later stage can overflow its stack.
const MAX_NESTING: u32 = 8000;

impl<'a> Parser<'a> {
    // ---- token access ----

    fn tok(&self, k: usize) -> Token {
        let i = (self.pos + k).min(self.sig.len() - 1);
        let t = self.tokens[self.sig[i] as usize];
        if k == 0 && self.split { Token { kind: T::Eq, span: Span { start: t.span.start + 1, end: t.span.end } } } else { t }
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
        self.split = false;
        if self.pos < self.sig.len() - 1 {
            self.pos += 1;
        }
        t
    }

    /// The `>` that closes type arguments. A `>=` right after them, as in
    /// `list<int>= []`, is that `>` and an `=`.
    fn close_angle(&mut self) -> PResult<Token> {
        if self.at(T::Ge) && !self.split {
            let t = self.tok(0);
            self.split = true;
            return Ok(Token { kind: T::Gt, span: Span { start: t.span.start, end: t.span.start + 1 } });
        }
        self.expect(T::Gt, "`>` or `,` in the type")
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

    /// Words such as `type`, `match` and `of` aren't reserved: they're
    /// identifiers that mean something only where they appear.
    fn at_word(&self, word: &str) -> bool {
        self.word_at(0, word)
    }

    fn word_at(&self, k: usize, word: &str) -> bool {
        let t = self.tok(k);
        t.kind == T::Ident && self.text_of(t.span) == word
    }

    fn eat_word(&mut self, word: &str) -> bool {
        if self.at_word(word) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// `mut` before a name marks a `mut` parameter or argument. Anywhere else,
    /// as in `mut: int` or `f(mut)`, it's a name.
    fn eat_mut(&mut self) -> bool {
        if self.at_word("mut") && matches!(self.peek_at(1), T::Ident | T::Keyword(Kw::This)) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// The offset of the bracket that closes the one `k` tokens ahead.
    fn closing(&self, mut k: usize) -> Option<usize> {
        let mut depth = 0;
        loop {
            match self.peek_at(k) {
                T::LParen | T::LBracket | T::LBrace => depth += 1,
                T::RParen | T::RBracket | T::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(k);
                    }
                }
                T::Eof => return None,
                _ => {}
            }
            k += 1;
        }
    }

    /// At `match (...) {`, a `match` expression rather than a call to a
    /// function named `match`.
    fn at_match(&self) -> bool {
        self.at_word("match") && self.peek_at(1) == T::LParen && self.closing(1).is_some_and(|close| self.peek_at(close + 1) == T::LBrace)
    }

    /// The start of a top-level declaration, `k` tokens ahead: `function`, or
    /// a declaration word followed by what that declaration needs next.
    fn item_start_at(&self, k: usize) -> bool {
        let t = self.tok(k);
        let next = self.peek_at(k + 1);
        match t.kind {
            T::Keyword(Kw::Function) => true,
            T::Ident => match self.text_of(t.span) {
                "test" => next == T::Str,
                "struct" | "enum" | "interface" | "type" => next == T::Ident,
                "import" => matches!(next, T::LBrace | T::Star),
                "export" => matches!(next, T::LBrace | T::Star | T::Keyword(Kw::Const)) || self.item_start_at(k + 1),
                _ => false,
            },
            _ => false,
        }
    }

    /// End of the previous significant token.
    fn prev_end(&self) -> u32 {
        if self.split {
            self.tokens[self.sig[self.pos] as usize].span.start + 1
        } else if self.pos == 0 {
            0
        } else {
            self.tokens[self.sig[self.pos - 1] as usize].span.end
        }
    }

    fn text_of(&self, span: Span) -> &'a str {
        &self.text[span.start as usize..span.end as usize]
    }

    fn describe(&self, t: Token) -> String {
        let text = self.text_of(self.head(t));
        match t.kind {
            T::Eof => "the end of the file".to_string(),
            T::Str | T::TemplateFull | T::TemplateHead => "a string".to_string(),
            T::Unknown if text == "/*" => "a comment that's never closed".to_string(),
            T::Unknown if text.starts_with('"') => "a string that's never closed".to_string(),
            T::Unknown if text == "`" || text == "}" => "a template string that's never closed".to_string(),
            _ => format!("`{text}`"),
        }
    }

    /// The part of `t` an error points at: the `}` that continues a
    /// template string, not the text after it.
    fn head(&self, t: Token) -> Span {
        match t.kind {
            T::TemplateMiddle | T::TemplateTail => Span::new(t.span.start, t.span.start + 1),
            _ => t.span,
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
            let d = Diagnostic::error(E_SYNTAX, self.file, self.head(t), format!("expected `;`, found {found}"))
                .with_label("the statement before this needs a `;`");
            if !self.semicolon_fits() {
                self.diags.push(d);
                return Err(Fail);
            }
            let at = self.prev_end();
            if self.speculating {
                return Ok(Token { kind: T::Semi, span: Span::new(at, at) });
            }
            // The fix is exact: insert it after the previous token. Parsing
            // goes on as if it were there.
            let d = d.with_fix(
                "add `;`",
                Applicability::Safe,
                vec![Edit { file: self.file, span: Span::new(at, at), before: String::new(), after: ";".into() }],
            );
            self.diags.push(d);
            return Ok(Token { kind: T::Semi, span: Span::new(at, at) });
        }
        self.error(self.head(t), format!("expected {what}, found {found}"))
    }

    /// Whether a `;` inserted before the current token ends the statement
    /// the way the code means: the token is on a new line or closes the
    /// block, and what follows then parses. So `return 1 }` gets the fix,
    /// and `var x = 1;`, where a `;` would make `var;`, doesn't.
    fn semicolon_fits(&mut self) -> bool {
        let t = self.tok(0);
        if matches!(t.kind, T::RBrace | T::Eof) {
            return true;
        }
        if !self.on_new_line(t) {
            return false;
        }
        if self.speculating {
            return true;
        }
        if self.item_start_at(0) {
            return self.speculate(|p| p.item().is_ok());
        }
        self.speculate(|p| p.stmt().is_ok())
    }

    /// Whether a line break separates `t` from the token before it.
    fn on_new_line(&self, t: Token) -> bool {
        self.text[self.prev_end() as usize..t.span.start as usize].contains('\n')
    }

    /// Runs `f`, then undoes everything it parsed and reported. True when
    /// `f` succeeds without reporting anything.
    fn speculate(&mut self, f: impl FnOnce(&mut Self) -> bool) -> bool {
        let (pos, split, items, exprs, stmts, broken) =
            (self.pos, self.split, self.ast.items.len(), self.ast.exprs.len(), self.ast.stmts.len(), self.ast.broken.len());
        let (diags, depth, fatal, seen_declaration) = (self.diags.len(), self.depth, self.fatal, self.seen_declaration);
        let was = std::mem::replace(&mut self.speculating, true);
        let ok = f(self) && self.diags.len() == diags;
        self.speculating = was;
        self.pos = pos;
        self.split = split;
        self.ast.items.truncate(items);
        self.ast.exprs.truncate(exprs);
        self.ast.stmts.truncate(stmts);
        self.ast.broken.truncate(broken);
        self.diags.truncate(diags);
        self.depth = depth;
        self.fatal = fatal;
        self.seen_declaration = seen_declaration;
        ok
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
                self.error(self.head(t), format!("expected a member name, found {found}"))
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
                while !self.at(T::Eof) && !self.item_start_at(0) {
                    self.bump();
                }
            }
        }
    }

    fn item(&mut self) -> PResult<()> {
        let start = self.tok(0).span;
        let exported = self.eat_word("export");
        let t = self.tok(0);
        let word = if t.kind == T::Ident { self.text_of(t.span) } else { "" };
        let kind = match (t.kind, word) {
            (T::Keyword(Kw::Function), _) => ItemKind::Function(self.function(exported)?),
            (_, "test") => {
                self.bump();
                let name_tok = self.expect(T::Str, "the test name, a string")?;
                let name = self.decode_string(name_tok)?;
                let body = self.block()?;
                ItemKind::Test(Test { name, name_span: name_tok.span, body })
            }
            (_, w @ ("struct" | "enum" | "interface")) => {
                while !self.at(T::LBrace) && !self.at(T::Eof) {
                    self.bump();
                }
                self.skip_group()?;
                ItemKind::Unsupported {
                    what: match w {
                        "struct" => "struct declarations",
                        "enum" => "enum declarations",
                        _ => "interface declarations",
                    },
                }
            }
            (_, "type") => {
                self.skip_statement()?;
                ItemKind::Unsupported { what: "type aliases" }
            }
            (_, "import") if !exported => {
                if self.seen_declaration {
                    let d = Diagnostic::error(E_SYNTAX, self.file, t.span, "imports go at the top of the file, before any declarations");
                    self.diags.push(d);
                }
                ItemKind::Import(self.import()?)
            }
            (T::Keyword(Kw::Const), _) => {
                self.skip_statement()?;
                ItemKind::Unsupported { what: "module constants" }
            }
            (T::LBrace, _) if exported => {
                self.skip_statement()?;
                ItemKind::Unsupported { what: "re-exports" }
            }
            (T::Keyword(Kw::Let), _) => {
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
        if !matches!(kind, ItemKind::Import(_)) {
            self.seen_declaration = true;
        }
        let span = start.to(Span::new(self.prev_end(), self.prev_end()));
        self.ast.items.push(Item { kind, span });
        Ok(())
    }

    /// After `import`: `{ a, b as c } from "path";` or `* as ns from "path";`.
    fn import(&mut self) -> PResult<Import> {
        self.bump(); // import
        let kind = if self.eat(T::Star) {
            if !self.eat_word("as") {
                let t = self.tok(0);
                let found = self.describe(t);
                return self.error(self.head(t), format!("expected `as` and a name for the module, found {found}"));
            }
            ImportKind::Namespace(self.ident("a name for the module")?)
        } else {
            self.expect(T::LBrace, "`{` or `*`")?;
            let mut names = Vec::new();
            while !self.at(T::RBrace) {
                let name = self.ident("a name to import")?;
                let alias = if self.eat_word("as") { Some(self.ident("a name after `as`")?) } else { None };
                names.push(ImportName { name, alias });
                if !self.eat(T::Comma) {
                    break;
                }
            }
            let close = self.expect(T::RBrace, "`}` or `,` in the import")?.span;
            ImportKind::Named { names, close }
        };
        if !self.eat_word("from") {
            let t = self.tok(0);
            let found = self.describe(t);
            return self.error(self.head(t), format!("expected `from` and the module's path, found {found}"));
        }
        let path_tok = self.expect(T::Str, "the module's path, a string")?;
        let path = self.decode_string(path_tok)?;
        self.expect(T::Semi, "`;`")?;
        Ok(Import { kind, path, path_span: path_tok.span })
    }

    fn function(&mut self, exported: bool) -> PResult<Function> {
        self.bump(); // function
        let name = self.ident("a function name")?;
        let header = self.function_header();
        let body = match header {
            Ok(_) => self.block(),
            Err(Fail) => {
                // Read the body anyway, for the syntax errors in it.
                if self.skip_to_body() {
                    let _ = self.block();
                }
                Err(Fail)
            }
        };
        match (header, body) {
            (Ok((generics, params, ret, throws)), Ok(body)) => Ok(Function { name, exported, params, ret, throws, body, generics }),
            _ => {
                self.ast.broken.push(name);
                Err(Fail)
            }
        }
    }

    /// After a syntax error in a function's header: moves to the `{` that
    /// starts its body. False if there's none before the next declaration.
    fn skip_to_body(&mut self) -> bool {
        while !self.fatal && !self.at(T::Eof) && !self.item_start_at(0) {
            if self.at(T::LBrace) {
                return true;
            }
            self.bump();
        }
        false
    }

    /// A function's header after its name, up to the `{` of its body.
    #[allow(clippy::type_complexity)]
    fn function_header(&mut self) -> PResult<(Option<Span>, Vec<Param>, Option<TypeExpr>, Option<TypeExpr>)> {
        let generics = if self.at(T::Lt) { Some(self.skip_group()?) } else { None };
        self.expect(T::LParen, "`(`")?;
        let mut params = Vec::new();
        while !self.at(T::RParen) {
            let start = self.tok(0).span;
            let is_mut = self.eat_mut();
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
        let throws = if self.eat_word("throws") { Some(self.type_expr()?) } else { None };
        if !self.at(T::LBrace) {
            self.expect(T::LBrace, "`{`")?;
        }
        Ok((generics, params, ret, throws))
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
            T::Ident if matches!(self.text_of(t.span), "list" | "set" | "map") => {
                self.bump();
                let name = self.text_of(t.span).to_string();
                let mut args = Vec::new();
                let mut span = t.span;
                if self.eat(T::Lt) {
                    loop {
                        args.push(self.type_expr()?);
                        if !self.eat(T::Comma) {
                            break;
                        }
                    }
                    span = span.to(self.close_angle()?.span);
                }
                Ok(TypeExpr { kind: TypeKind::Collection { name, args }, span })
            }
            T::Ident | T::Keyword(Kw::Null) => {
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
                self.bump();
                let mut items = Vec::new();
                while !self.at(T::RBracket) {
                    items.push(self.type_expr()?);
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                let close = self.expect(T::RBracket, "`]` or `,` in the tuple type")?;
                Ok(TypeExpr { kind: TypeKind::Tuple(items), span: t.span.to(close.span) })
            }
            T::LParen => {
                self.bump();
                let mut params = Vec::new();
                while !self.at(T::RParen) {
                    // `(value: int) => ...`: the name is documentation only.
                    if matches!(self.peek(), T::Ident | T::Keyword(_)) && self.peek_at(1) == T::Colon {
                        self.bump();
                        self.bump();
                    }
                    params.push(self.type_expr()?);
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                self.expect(T::RParen, "`)` or `,` in the function type")?;
                self.expect(T::Arrow, "`=>` in a function type")?;
                let ret = self.type_postfix()?;
                let mut span = t.span.to(ret.span);
                if self.eat_word("throws") {
                    span = span.to(self.type_postfix()?.span);
                    return Ok(TypeExpr { kind: TypeKind::Unsupported("declared errors (`throws`) in function types"), span });
                }
                Ok(TypeExpr { kind: TypeKind::Func { params, ret: Box::new(ret) }, span })
            }
            _ => {
                let found = self.describe(t);
                self.error(self.head(t), format!("expected a type, found {found}"))
            }
        }
    }

    // ---- statements ----

    fn block(&mut self) -> PResult<Block> {
        let open = self.expect(T::LBrace, "`{`")?;
        let errors = self.diags.len();
        let mut stmts = Vec::new();
        while !self.at(T::RBrace) {
            if self.at(T::Eof) {
                // An error inside already explains it, such as a missing `)`.
                if self.diags.len() > errors {
                    return Err(Fail);
                }
                return self.error(open.span, "this `{` is never closed");
            }
            let start = self.pos;
            match self.stmt() {
                Ok(s) => stmts.push(s),
                Err(Fail) if self.fatal || self.speculating => return Err(Fail),
                Err(Fail) => stmts.push(self.recover_statement(start)),
            }
        }
        let close = self.bump();
        Ok(Block { stmts, span: open.span.to(close.span) })
    }

    /// After a syntax error in the statement that started at token `start`:
    /// skips to the start of the next statement and returns an `Error`
    /// statement for the one that failed. A `let` keeps its name, with an
    /// unknown value, so its later uses aren't reported as undefined.
    fn recover_statement(&mut self, start: usize) -> StmtId {
        let first = self.pos;
        let mut depth = 0i32;
        loop {
            let t = self.tok(0);
            match t.kind {
                T::Eof => break,
                T::Semi | T::RBrace if depth == 0 => {
                    if t.kind == T::Semi {
                        self.bump();
                    }
                    break;
                }
                // A statement keyword that starts a line starts the next statement.
                T::Keyword(
                    Kw::Let | Kw::Const | Kw::If | Kw::For | Kw::While | Kw::Return | Kw::Throw | Kw::Break | Kw::Continue | Kw::Try,
                ) if self.pos > first && self.on_new_line(t) => break,
                T::LParen | T::LBracket | T::LBrace => depth += 1,
                T::RParen | T::RBracket => depth = (depth - 1).max(0),
                T::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        // A statement that ends with a block, unless more follows it.
                        self.bump();
                        if matches!(self.peek(), T::Keyword(Kw::Else | Kw::Catch | Kw::Finally)) {
                            continue;
                        }
                        self.eat(T::Semi);
                        break;
                    }
                }
                _ => {}
            }
            self.bump();
        }
        if self.pos == start {
            self.bump();
        }
        let span = self.tokens[self.sig[start] as usize].span.to(Span::new(self.prev_end(), self.prev_end()));
        let at = |k: usize| self.tokens[self.sig[(start + k).min(self.sig.len() - 1)] as usize];
        let (t0, t1, t2) = (at(0), at(1), at(2));
        // A word, a name and `=` or `:` look like a declaration of the name.
        let declares = match (t0.kind, t1.kind, t2.kind) {
            (T::Keyword(kw @ (Kw::Let | Kw::Const)), T::Ident, _) => Some(kw == Kw::Const),
            (T::Ident, T::Ident, T::Eq | T::Colon) => Some(false),
            _ => None,
        };
        let kind = match declares {
            Some(is_const) => {
                let name = Ident { name: self.text_of(t1.span).to_string(), span: t1.span };
                let init = Some(self.alloc_expr(ExprKind::Error, span));
                StmtKind::Let { is_const, name, ty: None, init }
            }
            None => StmtKind::Error,
        };
        self.alloc_stmt(kind, span)
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
            T::Ident if self.at_match() => {
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
            let pattern = self.pattern()?;
            let ty = if self.eat(T::Colon) { Some(self.type_expr()?) } else { None };
            self.expect(T::Eq, "`=` and a value to destructure")?;
            let init = self.expr()?;
            return Ok(StmtKind::Destructure { is_const, pattern, ty, init });
        }
        let name = self.ident("a variable name")?;
        let ty = if self.eat(T::Colon) { Some(self.type_expr()?) } else { None };
        let init = if self.eat(T::Eq) { Some(self.expr()?) } else { None };
        Ok(StmtKind::Let { is_const, name, ty, init })
    }

    /// A name, `_`, or `[a, b = 0, ...rest]`, as a declaration or a loop
    /// header binds it.
    fn pattern(&mut self) -> PResult<Pattern> {
        let t = self.tok(0);
        match t.kind {
            T::LBracket => {
                self.bump();
                let mut items = Vec::new();
                let mut rest = None;
                while !self.at(T::RBracket) {
                    if self.eat(T::Ellipsis) {
                        rest = Some(self.ident("a name for the rest")?);
                        if self.at(T::Comma) {
                            let c = self.tok(0);
                            return self.error(c.span, "`...rest` must be the last element of the pattern");
                        }
                        break;
                    }
                    let pattern = self.pattern()?;
                    let default = if self.eat(T::Eq) { Some(self.expr()?) } else { None };
                    items.push(PatItem { pattern, default });
                    if !self.eat(T::Comma) {
                        break;
                    }
                }
                let close = self.expect(T::RBracket, "`]` or `,` in the pattern")?;
                Ok(Pattern::List { items, rest, span: t.span.to(close.span) })
            }
            T::LBrace => {
                let span = self.skip_group()?;
                Ok(Pattern::Unsupported { what: "destructuring structs", span })
            }
            T::Ident if self.text_of(t.span) == "_" => {
                self.bump();
                Ok(Pattern::Ignore(t.span))
            }
            _ => Ok(Pattern::Name(self.ident("a variable name")?)),
        }
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
            return self.error(self.head(t), format!("expected `(` after `for`, found {found}"));
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
                // The `of` after the loop variable: in `for (const of of ofs)`, the first `of` is the variable.
                T::Ident if depth == 0 && self.word_at(k, "of") && matches!(self.peek_at(k - 1), T::Ident | T::RBracket | T::RBrace) => {
                    is_of = true;
                    break;
                }
                T::Ident if depth == 0 && k == 3 && self.word_at(k, "in") => {
                    let t = self.tok(k);
                    return self.error(t.span, "`for...in` isn't valid in dat; use `for...of`");
                }
                _ => {}
            }
            k += 1;
        }
        if is_of {
            self.bump(); // (
            let binding = if self.eat_kw(Kw::Const) {
                Binding::Const
            } else if self.eat_kw(Kw::Let) {
                Binding::Let
            } else if self.at_word("mut") && !self.word_at(1, "of") {
                self.bump();
                Binding::Mut
            } else {
                let t = self.tok(0);
                return self.error(self.head(t), "a loop variable is declared with `const`, `let` or `mut`, as in `for (const x of xs)`");
            };
            let pattern = self.pattern()?;
            if !self.eat_word("of") {
                let t = self.tok(0);
                let found = self.describe(t);
                return self.error(self.head(t), format!("expected `of`, found {found}"));
            }
            let iter = self.expr()?;
            self.expect(T::RParen, "`)`")?;
            let body = self.block()?;
            return Ok(StmtKind::ForOf { binding, pattern, iter, body });
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
            return self.error(self.head(t), format!("expected `catch` or `finally` after the `try` block, found {found}"));
        }
        Ok(StmtKind::Try { body, catches, finally })
    }

    // ---- expressions ----

    /// Runs `f` one nesting level deeper.
    fn nested<X>(&mut self, f: impl FnOnce(&mut Self) -> PResult<X>) -> PResult<X> {
        if self.depth >= MAX_NESTING {
            let t = self.tok(0);
            self.fatal = true;
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
            T::Ident if self.at_word("in") => (BinOp::In, 5),
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
            if (self.at_word("is") || self.at_word("as")) && min_bp < 5 {
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
                // The fix is exact, so parsing goes on as if it were applied.
                let (bad, good) = if op_tok.kind == T::EqEqEq { ("===", "==") } else { ("!==", "!=") };
                let d = Diagnostic::error(E_SYNTAX, self.file, op_tok.span, format!("dat has no `{bad}`; `{good}` compares contents"))
                    .with_fix(
                        format!("use `{good}`"),
                        Applicability::Safe,
                        vec![Edit { file: self.file, span: op_tok.span, before: bad.into(), after: good.into() }],
                    );
                self.diags.push(d);
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
                        let is_mut = self.eat_mut();
                        let expr = if self.at(T::Ellipsis) {
                            let s = self.bump().span;
                            let inner = self.expr()?;
                            let span = s.to(self.espan(inner));
                            self.alloc_expr(ExprKind::Spread(inner), span)
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
                let text = self.text_of(t.span);
                match parse_int(text) {
                    Ok(v) => ExprKind::Int(v),
                    Err(problem) => {
                        let d =
                            Diagnostic::error(E_SYNTAX, self.file, t.span, format!("`{text}` isn't a valid number")).with_label(problem);
                        self.diags.push(d);
                        ExprKind::Error
                    }
                }
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
            // `.Active`, an enum member whose enum comes from the expected type.
            T::Dot if self.tok(1).kind == T::Ident => {
                self.bump();
                self.bump();
                ExprKind::Unsupported { what: "enum members without the enum's name" }
            }
            T::Ident if self.at_match() => {
                self.bump();
                let span = t.span.to(self.skip_group()?).to(self.skip_group()?);
                return Ok(self.alloc_expr(ExprKind::Unsupported { what: "`match` expressions" }, span));
            }
            // `x => x * 2`: the parameter list always has parentheses. The
            // fix is exact, so parsing goes on as if it were applied.
            T::Ident if self.peek_at(1) == T::Arrow => {
                let name = self.text_of(t.span).to_string();
                let d = Diagnostic::error(E_SYNTAX, self.file, t.span, "an arrow function's parameters go in parentheses").with_fix(
                    format!("write `({name})`"),
                    Applicability::Safe,
                    vec![Edit { file: self.file, span: t.span, before: name.clone(), after: format!("({name})") }],
                );
                self.diags.push(d);
                return self.arrow_function();
            }
            T::Ident => {
                self.bump();
                let name = self.text_of(t.span).to_string();
                if self.at(T::LBrace) && self.struct_lit_ahead() {
                    return self.struct_lit(Ident { name, span: t.span });
                }
                ExprKind::Name(name)
            }
            T::LParen => {
                if self.is_arrow_function() {
                    return self.arrow_function();
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
                        items.push(self.alloc_expr(ExprKind::Spread(inner), span));
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
            T::LBrace => return self.map_literal(),
            T::Unknown => {
                let text = self.text_of(t.span);
                let msg = if text.starts_with('"') || text.starts_with('`') {
                    "this string is never closed".to_string()
                } else if text == "/*" {
                    "this comment is never closed; end it with `*/`".to_string()
                } else {
                    format!("unexpected character `{text}`")
                };
                return self.error(t.span, msg);
            }
            _ => {
                let found = self.describe(t);
                return self.error(self.head(t), format!("expected an expression, found {found}"));
            }
        };
        Ok(self.alloc_expr(kind, t.span))
    }

    /// At `{` after a name: does a struct literal's body follow? It's `{}`,
    /// a spread, or a field followed by `:`, `,` or `}`. Otherwise the `{`
    /// starts a block, as in `if (ready {` with its `)` missing.
    fn struct_lit_ahead(&self) -> bool {
        match self.peek_at(1) {
            T::RBrace | T::Ellipsis => true,
            T::Ident | T::Keyword(_) => matches!(self.peek_at(2), T::Colon | T::Comma | T::RBrace),
            _ => false,
        }
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
        let Some(k) = self.closing(0) else { return false };
        match self.peek_at(k + 1) {
            T::Arrow => true,
            // `(x: int): int => ...`, and a return type with brackets of its
            // own, as `: [int, string]` or `: map<string, int>`.
            T::Colon => {
                let mut j = k + 2;
                let mut depth = 0i32;
                loop {
                    match self.peek_at(j) {
                        T::Arrow if depth == 0 => return true,
                        T::LBracket | T::LParen | T::Lt => depth += 1,
                        T::RBracket | T::Gt if depth > 0 => depth -= 1,
                        T::RParen if depth > 0 => depth -= 1,
                        T::Semi | T::Eof | T::LBrace | T::RBrace | T::RParen | T::RBracket | T::Comma if depth == 0 => return false,
                        T::Semi | T::Eof | T::LBrace | T::RBrace => return false,
                        _ => {}
                    }
                    j += 1;
                }
            }
            _ => false,
        }
    }

    /// `(x: int): int => x * 2`, or with a block body.
    fn arrow_function(&mut self) -> PResult<ExprId> {
        if self.at(T::Ident) {
            // `x => ...`, already reported: one untyped parameter.
            let t = self.bump();
            let name = Ident { name: self.text_of(t.span).to_string(), span: t.span };
            let params = vec![Param { name, is_mut: false, ty: None, default: None, span: t.span }];
            let arrow = self.expect(T::Arrow, "`=>`")?;
            let head = t.span.to(arrow.span);
            let (body, end) = if self.at(T::LBrace) {
                let b = self.block()?;
                let span = b.span;
                (ArrowBody::Block(b), span)
            } else {
                let e = self.expr()?;
                (ArrowBody::Expr(e), self.espan(e))
            };
            return Ok(self.alloc_expr(ExprKind::Arrow(Box::new(Arrow { params, ret: None, body, head })), t.span.to(end)));
        }
        let open = self.bump();
        let mut params = Vec::new();
        while !self.at(T::RParen) {
            let start = self.tok(0).span;
            let is_mut = self.eat_mut();
            let name = self.ident("a parameter name")?;
            let ty = if self.eat(T::Colon) { Some(self.type_expr()?) } else { None };
            let default = if self.eat(T::Eq) { Some(self.expr()?) } else { None };
            let span = start.to(Span::new(self.prev_end(), self.prev_end()));
            params.push(Param { name, is_mut, ty, default, span });
            if !self.eat(T::Comma) {
                break;
            }
        }
        self.expect(T::RParen, "`)` or `,`")?;
        let ret = if self.eat(T::Colon) { Some(self.type_postfix()?) } else { None };
        let arrow = self.expect(T::Arrow, "`=>`")?;
        let head = open.span.to(arrow.span);
        // `=> { a: 1 }` is a map: a block can't start with `name:`, `"key":`,
        // `[key]:` or `...`, since dat has no labels.
        let map_body = self.at(T::LBrace)
            && match self.peek_at(1) {
                T::Ellipsis => true,
                T::Ident | T::Str | T::Int | T::Keyword(_) => self.peek_at(2) == T::Colon,
                T::LBracket => self.closing(1).is_some_and(|k| self.peek_at(k + 1) == T::Colon),
                _ => false,
            };
        let (body, end) = if self.at(T::LBrace) && !map_body {
            let b = self.block()?;
            let span = b.span;
            (ArrowBody::Block(b), span)
        } else {
            let e = self.expr()?;
            (ArrowBody::Expr(e), self.espan(e))
        };
        Ok(self.alloc_expr(ExprKind::Arrow(Box::new(Arrow { params, ret, body, head })), open.span.to(end)))
    }

    /// `{ alice: 30, "b": 1, [key]: value, ...other }`. A name before `:`
    /// is a string key; any other key is an expression.
    fn map_literal(&mut self) -> PResult<ExprId> {
        let open = self.bump();
        let mut entries = Vec::new();
        while !self.at(T::RBrace) {
            if self.at(T::Ellipsis) {
                self.bump();
                entries.push(MapEntry::Spread(self.expr()?));
            } else if self.at(T::LBracket) {
                self.bump();
                let key = self.expr()?;
                self.expect(T::RBracket, "`]`")?;
                self.expect(T::Colon, "`:` after the key")?;
                entries.push(MapEntry::Keyed(key, self.expr()?));
            } else if matches!(self.peek(), T::Ident | T::Keyword(_))
                && !matches!(self.peek(), T::Keyword(Kw::True | Kw::False | Kw::Null))
                && self.peek_at(1) == T::Colon
            {
                let name = self.member_name()?;
                self.bump();
                entries.push(MapEntry::Named(name, self.expr()?));
            } else {
                let key = self.expr()?;
                self.expect(T::Colon, "`:` after the key")?;
                entries.push(MapEntry::Keyed(key, self.expr()?));
            }
            if !self.eat(T::Comma) {
                break;
            }
        }
        let close = self.expect(T::RBrace, "`}` or `,` in the map")?;
        Ok(self.alloc_expr(ExprKind::Map(entries), open.span.to(close.span)))
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
                T::Unknown if self.text_of(t.span) == "}" => {
                    let open = Span::new(first.span.start, first.span.start + 1);
                    return self.error(open, "this template string is never closed");
                }
                _ => {
                    let found = self.describe(t);
                    return self.error(self.head(t), format!("expected `}}` to end the template substitution, found {found}"));
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
/// `Ok(None)` when it doesn't fit in a `u64`, and `Err` with the problem
/// when it isn't a number, as `0x` or `0b2`.
fn parse_int(text: &str) -> Result<Option<u64>, String> {
    let clean: String = text.chars().filter(|&c| c != '_').collect();
    let (digits, radix, kind) = match clean.get(..2) {
        Some("0x" | "0X") => (&clean[2..], 16, "hexadecimal"),
        Some("0b" | "0B") => (&clean[2..], 2, "binary"),
        _ => (&clean[..], 10, "decimal"),
    };
    if digits.is_empty() {
        return Err(format!("`{}` needs digits after it", &text[..2]));
    }
    if let Some(c) = digits.chars().find(|c| !c.is_digit(radix)) {
        return Err(format!("`{c}` isn't a {kind} digit"));
    }
    Ok(u64::from_str_radix(digits, radix).ok())
}
