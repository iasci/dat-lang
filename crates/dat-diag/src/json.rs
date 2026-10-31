//! A small JSON value with a deterministic pretty printer and a parser, so the
//! toolchain has no dependencies and its JSON output is byte-stable.

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<Json>),
    /// Keys keep the order they were written in.
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn str(s: &str) -> Json {
        Json::Str(s.to_string())
    }

    pub fn obj<const N: usize>(fields: [(&str, Json); N]) -> Json {
        Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> &[Json] {
        match self {
            Json::Arr(items) => items,
            _ => &[],
        }
    }

    /// Two-space indented, one value per line.
    pub fn pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, indent: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(n) => {
                let _ = write!(out, "{n}");
            }
            Json::Str(s) => write_str(out, s),
            Json::Arr(items) if items.is_empty() => out.push_str("[]"),
            Json::Obj(fields) if fields.is_empty() => out.push_str("{}"),
            Json::Arr(items) => {
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    push_indent(out, indent + 1);
                    item.write(out, indent + 1);
                    out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
                }
                push_indent(out, indent);
                out.push(']');
            }
            Json::Obj(fields) => {
                out.push_str("{\n");
                for (i, (k, v)) in fields.iter().enumerate() {
                    push_indent(out, indent + 1);
                    write_str(out, k);
                    out.push_str(": ");
                    v.write(out, indent + 1);
                    out.push_str(if i + 1 < fields.len() { ",\n" } else { "\n" });
                }
                push_indent(out, indent);
                out.push('}');
            }
        }
    }

    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser { s: text.as_bytes(), i: 0 };
        let v = p.value()?;
        p.ws();
        if p.i != p.s.len() {
            return Err(format!("trailing characters at byte {}", p.i));
        }
        Ok(v)
    }
}

fn push_indent(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push_str("  ");
    }
}

/// `s` as a JSON string, quoted and escaped.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    write_str(&mut out, s);
    out
}

fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &str) -> bool {
        if self.s[self.i..].starts_with(lit.as_bytes()) {
            self.i += lit.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        let Some(&c) = self.s.get(self.i) else { return Err("unexpected end of JSON".into()) };
        match c {
            b'n' if self.eat("null") => Ok(Json::Null),
            b't' if self.eat("true") => Ok(Json::Bool(true)),
            b'f' if self.eat("false") => Ok(Json::Bool(false)),
            b'"' => Ok(Json::Str(self.string()?)),
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.eat("]") {
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value()?);
                    self.ws();
                    if self.eat("]") {
                        return Ok(Json::Arr(items));
                    }
                    if !self.eat(",") {
                        return Err(format!("expected , or ] at byte {}", self.i));
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut fields = Vec::new();
                self.ws();
                if self.eat("}") {
                    return Ok(Json::Obj(fields));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.ws();
                    if !self.eat(":") {
                        return Err(format!("expected : at byte {}", self.i));
                    }
                    fields.push((k, self.value()?));
                    self.ws();
                    if self.eat("}") {
                        return Ok(Json::Obj(fields));
                    }
                    if !self.eat(",") {
                        return Err(format!("expected , or }} at byte {}", self.i));
                    }
                }
            }
            b'-' | b'0'..=b'9' => {
                let start = self.i;
                self.i += 1;
                while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                    self.i += 1;
                }
                let text = std::str::from_utf8(&self.s[start..self.i]).unwrap();
                text.parse().map(Json::Int).map_err(|e| e.to_string())
            }
            _ => Err(format!("unexpected character at byte {}", self.i)),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if !self.eat("\"") {
            return Err(format!("expected a string at byte {}", self.i));
        }
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.i) else { return Err("unterminated string".into()) };
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|e| e.to_string()),
                b'\\' => {
                    let e = *self.s.get(self.i).ok_or("bad escape")?;
                    self.i += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'u' => {
                            let hex =
                                std::str::from_utf8(self.s.get(self.i..self.i + 4).ok_or("bad escape")?).map_err(|e| e.to_string())?;
                            self.i += 4;
                            let cp = u32::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
                            let ch = char::from_u32(cp).ok_or("bad code point")?;
                            let mut buf = [0; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        other => out.push(other),
                    }
                }
                c => out.push(c),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_escape_control_characters() {
        assert_eq!(json_string("a\u{1b}b\0\"\n"), "\"a\\u001bb\\u0000\\\"\\n\"");
    }

    #[test]
    fn round_trip() {
        let v = Json::obj([("a", Json::Int(-3)), ("b", Json::Arr(vec![Json::str("x\n\"y\""), Json::Null]))]);
        assert_eq!(Json::parse(&v.pretty()).unwrap(), v);
    }
}
