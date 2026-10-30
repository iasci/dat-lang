//! The lexer. It keeps every byte: whitespace and comments become trivia
//! tokens, so the token vector is lossless and fixes can be computed as plain
//! byte edits.

use dat_diag::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Whitespace,
    LineComment,
    BlockComment,

    Ident,
    Int,
    Float,
    /// A double-quoted string literal, escapes not yet decoded.
    Str,
    /// `` `text` `` with no substitutions.
    TemplateFull,
    /// `` `text${ ``
    TemplateHead,
    /// `}text${`
    TemplateMiddle,
    /// `` }text` ``
    TemplateTail,
    Keyword(Kw),

    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    Dot,
    Ellipsis,
    Question,
    QuestionDot,
    QuestionQuestion,
    Bang,
    BangEq,
    BangEqEq,
    Eq,
    EqEq,
    EqEqEq,
    Arrow,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    PlusPlus,
    PlusEq,
    Minus,
    MinusMinus,
    MinusEq,
    Star,
    StarEq,
    Slash,
    SlashEq,
    Percent,
    PercentEq,
    AndAnd,
    OrOr,
    Pipe,
    Amp,

    /// A byte the language doesn't use, or an unterminated literal.
    Unknown,
    Eof,
}

impl TokenKind {
    pub fn is_trivia(self) -> bool {
        matches!(self, TokenKind::Whitespace | TokenKind::LineComment | TokenKind::BlockComment)
    }
}

macro_rules! keywords {
    ($($variant:ident = $text:literal),* $(,)?) => {
        /// The reserved keywords of `reference/13-reserved-keywords.md`. Every other
        /// word, such as `type` or `match`, is an identifier, and the parser
        /// gives it its meaning where it appears.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Kw { $($variant),* }

        impl Kw {
            pub fn from_word(s: &str) -> Option<Kw> {
                match s { $($text => Some(Kw::$variant),)* _ => None }
            }

            pub fn as_str(self) -> &'static str {
                match self { $(Kw::$variant => $text),* }
            }
        }
    };
}

keywords! {
    Let = "let", Const = "const", Function = "function", Return = "return",
    If = "if", Else = "else", For = "for", While = "while", Break = "break", Continue = "continue",
    Try = "try", Throw = "throw", Catch = "catch", Finally = "finally",
    This = "this", True = "true", False = "false", Null = "null",
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// Splits `text` into tokens, trivia included. The last token is `Eof`.
pub fn lex(text: &str) -> Vec<Token> {
    let mut lx = Lexer { s: text.as_bytes(), i: 0, out: Vec::new(), braces: Vec::new() };
    lx.run();
    lx.out
}

struct Lexer<'a> {
    s: &'a [u8],
    i: usize,
    out: Vec<Token>,
    /// One entry per open `${`: how many `{` are open inside it.
    braces: Vec<u32>,
}

impl Lexer<'_> {
    fn peek(&self, k: usize) -> u8 {
        *self.s.get(self.i + k).unwrap_or(&0)
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        self.out.push(Token { kind, span: Span::new(start as u32, self.i as u32) });
    }

    fn run(&mut self) {
        // A UTF-8 byte order mark, which some editors write.
        if self.s.starts_with(b"\xEF\xBB\xBF") {
            self.i = 3;
            self.push(TokenKind::Whitespace, 0);
        }
        while self.i < self.s.len() {
            let start = self.i;
            let c = self.s[self.i];
            let kind = match c {
                b' ' | b'\t' | b'\n' | b'\r' => {
                    while matches!(self.peek(0), b' ' | b'\t' | b'\n' | b'\r') {
                        self.i += 1;
                    }
                    TokenKind::Whitespace
                }
                b'/' if self.peek(1) == b'/' => {
                    while self.i < self.s.len() && self.s[self.i] != b'\n' {
                        self.i += 1;
                    }
                    TokenKind::LineComment
                }
                b'/' if self.peek(1) == b'*' => {
                    self.i += 2;
                    while self.i < self.s.len() && !(self.peek(0) == b'*' && self.peek(1) == b'/') {
                        self.i += 1;
                    }
                    if self.i < self.s.len() {
                        self.i += 2;
                        TokenKind::BlockComment
                    } else {
                        TokenKind::Unknown
                    }
                }
                b'0'..=b'9' => self.number(),
                b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                    while matches!(self.peek(0), b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_') {
                        self.i += 1;
                    }
                    let word = std::str::from_utf8(&self.s[start..self.i]).unwrap();
                    match Kw::from_word(word) {
                        Some(kw) => TokenKind::Keyword(kw),
                        None => TokenKind::Ident,
                    }
                }
                b'"' => self.string(),
                b'`' => {
                    self.i += 1;
                    self.template(TokenKind::TemplateFull, TokenKind::TemplateHead)
                }
                b'{' => {
                    self.i += 1;
                    if let Some(top) = self.braces.last_mut() {
                        *top += 1;
                    }
                    TokenKind::LBrace
                }
                b'}' => {
                    self.i += 1;
                    match self.braces.last_mut() {
                        Some(0) => {
                            self.braces.pop();
                            self.template(TokenKind::TemplateTail, TokenKind::TemplateMiddle)
                        }
                        Some(top) => {
                            *top -= 1;
                            TokenKind::RBrace
                        }
                        None => TokenKind::RBrace,
                    }
                }
                _ => self.punct(),
            };
            // An unclosed comment or template string: its opening is the
            // error, and the rest of the file is skipped as trivia.
            if kind == TokenKind::Unknown && matches!(c, b'/' | b'`' | b'}') {
                let open = start + if c == b'/' { 2 } else { 1 };
                self.out.push(Token { kind, span: Span::new(start as u32, open as u32) });
                self.push(TokenKind::BlockComment, open);
                continue;
            }
            self.push(kind, start);
        }
        let end = self.s.len();
        self.out.push(Token { kind: TokenKind::Eof, span: Span::new(end as u32, end as u32) });
    }

    fn number(&mut self) -> TokenKind {
        if self.peek(0) == b'0' && matches!(self.peek(1), b'x' | b'X' | b'b' | b'B') {
            self.i += 2;
            while self.peek(0).is_ascii_alphanumeric() || self.peek(0) == b'_' {
                self.i += 1;
            }
            return TokenKind::Int;
        }
        let digits = |lx: &mut Self| {
            while lx.peek(0).is_ascii_digit() || lx.peek(0) == b'_' {
                lx.i += 1;
            }
        };
        digits(self);
        let mut kind = TokenKind::Int;
        if self.peek(0) == b'.' && self.peek(1).is_ascii_digit() {
            self.i += 1;
            digits(self);
            kind = TokenKind::Float;
        }
        if matches!(self.peek(0), b'e' | b'E')
            && (self.peek(1).is_ascii_digit() || (matches!(self.peek(1), b'+' | b'-') && self.peek(2).is_ascii_digit()))
        {
            self.i += 2;
            digits(self);
            kind = TokenKind::Float;
        }
        kind
    }

    fn string(&mut self) -> TokenKind {
        self.i += 1;
        while self.i < self.s.len() {
            match self.s[self.i] {
                b'"' => {
                    self.i += 1;
                    return TokenKind::Str;
                }
                b'\\' => self.i += 2,
                b'\n' => return TokenKind::Unknown,
                _ => self.i += 1,
            }
        }
        self.i = self.i.min(self.s.len());
        TokenKind::Unknown
    }

    /// Scans template text after a `` ` `` or a closing `}`, up to the closing
    /// backtick (`end`) or the next `${` (`open`).
    fn template(&mut self, end: TokenKind, open: TokenKind) -> TokenKind {
        while self.i < self.s.len() {
            match self.s[self.i] {
                b'`' => {
                    self.i += 1;
                    return end;
                }
                b'\\' => self.i += 2,
                b'$' if self.peek(1) == b'{' => {
                    self.i += 2;
                    self.braces.push(0);
                    return open;
                }
                _ => self.i += 1,
            }
        }
        self.i = self.i.min(self.s.len());
        TokenKind::Unknown
    }

    fn punct(&mut self) -> TokenKind {
        use TokenKind as T;
        let three = [self.peek(0), self.peek(1), self.peek(2)];
        let (kind, len) = match three {
            [b'=', b'=', b'='] => (T::EqEqEq, 3),
            [b'!', b'=', b'='] => (T::BangEqEq, 3),
            [b'.', b'.', b'.'] => (T::Ellipsis, 3),
            [b'=', b'=', _] => (T::EqEq, 2),
            [b'!', b'=', _] => (T::BangEq, 2),
            [b'=', b'>', _] => (T::Arrow, 2),
            [b'<', b'=', _] => (T::Le, 2),
            [b'>', b'=', _] => (T::Ge, 2),
            [b'+', b'+', _] => (T::PlusPlus, 2),
            [b'+', b'=', _] => (T::PlusEq, 2),
            [b'-', b'-', _] => (T::MinusMinus, 2),
            [b'-', b'=', _] => (T::MinusEq, 2),
            [b'*', b'=', _] => (T::StarEq, 2),
            [b'/', b'=', _] => (T::SlashEq, 2),
            [b'%', b'=', _] => (T::PercentEq, 2),
            [b'&', b'&', _] => (T::AndAnd, 2),
            [b'|', b'|', _] => (T::OrOr, 2),
            [b'?', b'?', _] => (T::QuestionQuestion, 2),
            [b'?', b'.', c] if !c.is_ascii_digit() => (T::QuestionDot, 2),
            [b'(', ..] => (T::LParen, 1),
            [b')', ..] => (T::RParen, 1),
            [b'[', ..] => (T::LBracket, 1),
            [b']', ..] => (T::RBracket, 1),
            [b',', ..] => (T::Comma, 1),
            [b';', ..] => (T::Semi, 1),
            [b':', ..] => (T::Colon, 1),
            [b'.', ..] => (T::Dot, 1),
            [b'?', ..] => (T::Question, 1),
            [b'!', ..] => (T::Bang, 1),
            [b'=', ..] => (T::Eq, 1),
            [b'<', ..] => (T::Lt, 1),
            [b'>', ..] => (T::Gt, 1),
            [b'+', ..] => (T::Plus, 1),
            [b'-', ..] => (T::Minus, 1),
            [b'*', ..] => (T::Star, 1),
            [b'/', ..] => (T::Slash, 1),
            [b'%', ..] => (T::Percent, 1),
            [b'|', ..] => (T::Pipe, 1),
            [b'&', ..] => (T::Amp, 1),
            _ => {
                // Skip one whole UTF-8 character.
                let c = std::str::from_utf8(&self.s[self.i..]).ok().and_then(|s| s.chars().next());
                (T::Unknown, c.map_or(1, char::len_utf8))
            }
        };
        self.i += len;
        kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(s: &str) -> Vec<TokenKind> {
        lex(s).into_iter().map(|t| t.kind).filter(|k| !k.is_trivia()).collect()
    }

    #[test]
    fn template_with_nested_braces() {
        use TokenKind as T;
        assert_eq!(
            kinds("`a${ {x} }b${c}d`"),
            vec![T::TemplateHead, T::LBrace, T::Ident, T::RBrace, T::TemplateMiddle, T::Ident, T::TemplateTail, T::Eof]
        );
    }

    #[test]
    fn numbers_and_members() {
        use TokenKind as T;
        assert_eq!(
            kinds("42.toString() 1_000 2.5 1e9 0xff"),
            vec![T::Int, T::Dot, T::Ident, T::LParen, T::RParen, T::Int, T::Float, T::Float, T::Int, T::Eof]
        );
    }

    #[test]
    fn lossless() {
        let src = "let x = 1; // hi\n/* c */ `t${x}` \"s\\\"\"";
        let toks = lex(src);
        let joined: String = toks.iter().map(|t| &src[t.span.start as usize..t.span.end as usize]).collect();
        assert_eq!(joined, src);
    }
}
