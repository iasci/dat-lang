//! `toString()` and the quoted form used when a value is shown inside other
//! output, such as an `assert`'s `left:` and `right:` lines.

use crate::Str;

pub fn int_to_str(n: i64) -> Str {
    Str::from_string(n.to_string())
}

pub fn bool_to_str(b: bool) -> Str {
    Str::new(if b { "true" } else { "false" })
}

pub fn debug_int(n: i64) -> String {
    n.to_string()
}

pub fn debug_bool(b: bool) -> String {
    b.to_string()
}

/// A string as a dat literal: quoted, with the spec's escapes.
pub fn debug_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:X}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
