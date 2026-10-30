//! `toString()` and the quoted form used when a value is shown inside other
//! output, such as an `assert`'s `left:` and `right:` lines.

use crate::Str;

pub fn int_to_str(n: i64) -> Str {
    let mut s = String::new();
    write_int(&mut s, n);
    Str::from_string(s)
}

/// The text of an `int`, for `toString()` and for `${n}` in a template.
pub(crate) fn write_int(out: &mut String, n: i64) {
    use std::fmt::Write as _;
    let _ = write!(out, "{n}");
}

pub fn float_to_str(x: f64) -> Str {
    let mut s = String::new();
    write_float(&mut s, x);
    Str::from_string(s)
}

/// The text of a `float` (`reference/04-primitive-methods.md`): the
/// shortest digits that read back as the same number, always with a
/// decimal point or an exponent. The exponent form is used from `1e21` up
/// and from `1e-7` down. Rust's `{:e}` gives the shortest digits.
pub(crate) fn write_float(out: &mut String, x: f64) {
    use std::fmt::Write as _;
    if x.is_nan() {
        out.push_str("NaN");
        return;
    }
    if x.is_infinite() {
        out.push_str(if x > 0.0 { "Infinity" } else { "-Infinity" });
        return;
    }
    if x == 0.0 {
        // `-0.0` reads back as negative zero; `0.0` wouldn't.
        out.push_str(if x.is_sign_negative() { "-0.0" } else { "0.0" });
        return;
    }
    if x < 0.0 {
        out.push('-');
    }
    // At most 24 bytes, such as `2.2250738585072014e-308`.
    let mut buf = [0u8; 32];
    let sci = {
        let mut w = Buf { buf: &mut buf, len: 0 };
        let _ = write!(w, "{:e}", x.abs());
        let len = w.len;
        std::str::from_utf8(&buf[..len]).expect("float digits are ASCII")
    };
    let (mantissa, exp) = sci.split_once('e').expect("`{:e}` writes an exponent");
    let exp: i32 = exp.parse().expect("the exponent is a number");
    if !(-7 < exp && exp < 21) {
        out.push_str(mantissa);
        out.push('e');
        let _ = write!(out, "{exp}");
        return;
    }
    let (first, rest) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if exp < 0 {
        out.push_str("0.");
        for _ in 0..(-exp - 1) {
            out.push('0');
        }
        out.push_str(first);
        out.push_str(rest);
        return;
    }
    // `exp` digits go before the point after the first one.
    let exp = exp as usize;
    out.push_str(first);
    if rest.len() <= exp {
        out.push_str(rest);
        for _ in rest.len()..exp {
            out.push('0');
        }
        out.push_str(".0");
    } else {
        out.push_str(&rest[..exp]);
        out.push('.');
        out.push_str(&rest[exp..]);
    }
}

/// A fixed buffer that `write!` can fill without allocating.
struct Buf<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl std::fmt::Write for Buf<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let end = self.len + s.len();
        if end > self.buf.len() {
            return Err(std::fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

pub fn bool_to_str(b: bool) -> Str {
    Str::new(bool_text(b))
}

pub(crate) fn bool_text(b: bool) -> &'static str {
    if b { "true" } else { "false" }
}

pub fn debug_int(n: i64) -> String {
    n.to_string()
}

pub fn debug_float(x: f64) -> String {
    let mut s = String::new();
    write_float(&mut s, x);
    s
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

// The spec's examples use 3.14 and 3.14159, which aren't meant as π.
#[cfg(test)]
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;

    fn text(x: f64) -> String {
        debug_float(x)
    }

    #[test]
    fn floats_print_as_the_spec_says() {
        assert_eq!(text(2.0), "2.0");
        assert_eq!(text(0.1), "0.1");
        assert_eq!(text(3.14), "3.14");
        assert_eq!(text(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(text(1e21), "1e21");
        assert_eq!(text(1e20), "100000000000000000000.0");
        assert_eq!(text(1.5e-7), "1.5e-7");
        assert_eq!(text(1e-7), "1e-7");
        assert_eq!(text(0.000001), "0.000001");
        assert_eq!(text(123.456), "123.456");
        assert_eq!(text(-2.5), "-2.5");
        assert_eq!(text(-0.0), "-0.0");
        assert_eq!(text(0.0), "0.0");
        assert_eq!(text(f64::NAN), "NaN");
        assert_eq!(text(f64::INFINITY), "Infinity");
        assert_eq!(text(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(text(1.7976931348623157e308), "1.7976931348623157e308");
        assert_eq!(text(5e-324), "5e-324");
        assert_eq!(text(-1.25e25), "-1.25e25");
        assert_eq!(text(12.566370614359172), "12.566370614359172");
        assert_eq!(text(31.622776601683793), "31.622776601683793");
        for x in [0.1, 1.0 / 3.0, 2e-8, 123456789.123, 9007199254740993.0, 1e100, 4.35, 0.7] {
            assert_eq!(text(x).parse::<f64>().unwrap(), x, "{x} reads back");
        }
    }
}
