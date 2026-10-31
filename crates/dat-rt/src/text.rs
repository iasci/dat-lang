//! The methods of `string` (`reference/04-primitive-methods.md`). Lengths
//! and positions count bytes, and every position a method returns is a
//! character boundary.
//!
//! `try_*` functions are for the interpreter. The others are for generated
//! code: the ones that can fail take the operation's location and throw.

use crate::{DatError, ErrorKind, List, Loc, Str, throw};

type R<T> = Result<T, Box<DatError>>;

#[inline(always)]
fn or_throw<T>(r: R<T>, loc: &'static Loc) -> T {
    match r {
        Ok(v) => v,
        Err(e) => throw(e, loc),
    }
}

#[cold]
#[inline(never)]
fn range_error(message: String) -> Box<DatError> {
    Box::new(DatError::new(ErrorKind::RangeError, message))
}

/// The character that byte `i`, not a boundary, falls inside, and its start.
fn char_around(s: &str, i: usize) -> (usize, char) {
    let start = (0..i).rev().find(|&j| s.is_char_boundary(j)).unwrap_or(0);
    (start, s[start..].chars().next().unwrap_or('\u{fffd}'))
}

#[cold]
#[inline(never)]
fn inside_char(s: &str, what: &str, i: usize) -> Box<DatError> {
    let (start, c) = char_around(s, i);
    range_error(format!("{what} {i} is inside the character {} at {start}", crate::debug_str(&c.to_string())))
}

#[inline]
pub fn length(s: &str) -> i64 {
    s.len() as i64
}

#[inline]
pub fn is_empty(s: &str) -> bool {
    s.is_empty()
}

#[inline]
pub fn includes(s: &str, t: &str) -> bool {
    s.contains(t)
}

/// `item in s`.
#[inline]
pub fn is_in(item: &str, s: &str) -> bool {
    s.contains(item)
}

#[inline]
pub fn starts_with(s: &str, t: &str) -> bool {
    s.starts_with(t)
}

#[inline]
pub fn ends_with(s: &str, t: &str) -> bool {
    s.ends_with(t)
}

#[inline]
pub fn index_of(s: &str, t: &str) -> i64 {
    s.find(t).map_or(-1, |i| i as i64)
}

#[inline]
pub fn last_index_of(s: &str, t: &str) -> i64 {
    s.rfind(t).map_or(-1, |i| i as i64)
}

/// A `slice` position: negative counts from the end, and out-of-range
/// positions are clamped.
fn position(p: i64, len: usize) -> usize {
    if p < 0 { len.saturating_sub(p.unsigned_abs().min(len as u64) as usize) } else { (p as u64).min(len as u64) as usize }
}

/// `s.slice(start, end)`. A call without `end` passes `int.MAX`.
pub fn try_slice(s: &str, start: i64, end: i64) -> R<Str> {
    let (a, b) = (position(start, s.len()), position(end, s.len()));
    for p in [a, b] {
        if !s.is_char_boundary(p) {
            return Err(inside_char(s, "slice position", p));
        }
    }
    Ok(if a < b { Str::new(&s[a..b]) } else { Str::default() })
}

#[inline(always)]
pub fn slice(s: &str, start: i64, end: i64, loc: &'static Loc) -> Str {
    or_throw(try_slice(s, start, end), loc)
}

/// `s[i]`: the character that starts at byte `i`.
pub fn try_at(s: &str, i: i64) -> R<Str> {
    let len = s.len();
    if i < 0 || i as u64 >= len as u64 {
        return Err(range_error(format!("index {i} out of range for length {len}")));
    }
    let i = i as usize;
    if !s.is_char_boundary(i) {
        return Err(inside_char(s, "index", i));
    }
    let c = s[i..].chars().next().expect("i is inside the string");
    Ok(Str::new(&s[i..i + c.len_utf8()]))
}

#[inline(always)]
pub fn at(s: &str, i: i64, loc: &'static Loc) -> Str {
    or_throw(try_at(s, i), loc)
}

pub fn trim(s: &str) -> Str {
    Str::new(s.trim())
}

pub fn trim_start(s: &str) -> Str {
    Str::new(s.trim_start())
}

pub fn trim_end(s: &str) -> Str {
    Str::new(s.trim_end())
}

pub fn to_upper_case(s: &str) -> Str {
    Str::from_string(s.to_uppercase())
}

pub fn to_lower_case(s: &str) -> Str {
    Str::from_string(s.to_lowercase())
}

pub fn replace(s: &str, from: &str, to: &str) -> Str {
    Str::from_string(s.replacen(from, to, 1))
}

pub fn replace_all(s: &str, from: &str, to: &str) -> Str {
    Str::from_string(s.replace(from, to))
}

/// A buffer for a result of `bytes` bytes, or a `RangeError` if that's
/// longer than [`crate::MAX_LEN`].
fn buffer(bytes: Option<usize>, call: impl FnOnce() -> String) -> R<String> {
    match bytes {
        Some(n) if n <= crate::MAX_LEN => Ok(String::with_capacity(n)),
        _ => Err(range_error(format!("string too long: {}", call()))),
    }
}

pub fn try_repeat(s: &str, count: i64) -> R<Str> {
    if count < 0 {
        return Err(range_error(format!("repeat count can't be negative: repeat({count})")));
    }
    let bytes = usize::try_from(count).ok().and_then(|n| s.len().checked_mul(n));
    let mut out = buffer(bytes, || format!("{} bytes repeated {count} times", s.len()))?;
    for _ in 0..count {
        out.push_str(s);
    }
    Ok(Str::from_string(out))
}

#[inline(always)]
pub fn repeat(s: &str, count: i64, loc: &'static Loc) -> Str {
    or_throw(try_repeat(s, count), loc)
}

/// The padding that brings `s` up to `length` bytes: `fill` repeated, cut
/// at a character so it doesn't go over.
fn padding(s: &str, length: i64, fill: &str, method: &str) -> R<Option<String>> {
    let want = match usize::try_from(length) {
        Ok(n) if n > s.len() && !fill.is_empty() => n - s.len(),
        _ => return Ok(None),
    };
    let mut pad = buffer(Some(want), || format!("{method}({length})"))?;
    'fill: loop {
        for c in fill.chars() {
            if pad.len() + c.len_utf8() > want {
                break 'fill;
            }
            pad.push(c);
        }
    }
    Ok(Some(pad))
}

pub fn try_pad_start(s: &str, length: i64, fill: &str) -> R<Str> {
    Ok(match padding(s, length, fill, "padStart")? {
        Some(mut pad) => {
            pad.push_str(s);
            Str::from_string(pad)
        }
        None => Str::new(s),
    })
}

pub fn try_pad_end(s: &str, length: i64, fill: &str) -> R<Str> {
    Ok(match padding(s, length, fill, "padEnd")? {
        Some(pad) => {
            let mut out = String::with_capacity(s.len() + pad.len());
            out.push_str(s);
            out.push_str(&pad);
            Str::from_string(out)
        }
        None => Str::new(s),
    })
}

#[inline(always)]
pub fn pad_start(s: &str, length: i64, fill: &str, loc: &'static Loc) -> Str {
    or_throw(try_pad_start(s, length, fill), loc)
}

#[inline(always)]
pub fn pad_end(s: &str, length: i64, fill: &str, loc: &'static Loc) -> Str {
    or_throw(try_pad_end(s, length, fill), loc)
}

/// `s.split(separator)`. Separators at the edges give empty strings; an
/// empty separator gives the characters, as `[...s]` does.
pub fn split(s: &str, separator: &str) -> List<Str> {
    if separator.is_empty() {
        return chars(s);
    }
    s.split(separator).map(Str::new).collect()
}

/// `[...s]`: each character as a string.
pub fn chars(s: &str) -> List<Str> {
    s.char_indices().map(|(i, c)| Str::new(&s[i..i + c.len_utf8()])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(r: R<Str>) -> String {
        r.unwrap().as_str().to_string()
    }

    #[test]
    fn slice_counts_bytes_and_clamps() {
        assert_eq!(ok(try_slice("report.pdf", -3, i64::MAX)), "pdf");
        assert_eq!(ok(try_slice("hello", 1, 3)), "el");
        assert_eq!(ok(try_slice("hello", 3, 1)), "");
        assert_eq!(ok(try_slice("hello", -100, 100)), "hello");
        assert_eq!(ok(try_slice("héllo", 3, i64::MAX)), "llo");
        assert_eq!(try_slice("héllo", 2, i64::MAX).unwrap_err().message, "slice position 2 is inside the character \"é\" at 1");
    }

    #[test]
    fn index() {
        assert_eq!(ok(try_at("hello", 4)), "o");
        assert_eq!(ok(try_at("héllo", 1)), "é");
        assert_eq!(try_at("hello", 5).unwrap_err().message, "index 5 out of range for length 5");
        assert_eq!(try_at("hello", -1).unwrap_err().message, "index -1 out of range for length 5");
        assert_eq!(try_at("héllo", 2).unwrap_err().message, "index 2 is inside the character \"é\" at 1");
    }

    #[test]
    fn padding_and_repeat() {
        assert_eq!(ok(try_pad_start("7", 3, "0")), "007");
        assert_eq!(ok(try_pad_end("ab", 5, "xy")), "abxyx");
        assert_eq!(ok(try_pad_start("abc", 2, " ")), "abc");
        assert_eq!(ok(try_pad_start("a", 4, "é")), "éa");
        assert_eq!(ok(try_pad_start("a", 4, "")), "a");
        assert_eq!(ok(try_repeat("ab", 3)), "ababab");
        assert_eq!(ok(try_repeat("ab", 0)), "");
        assert!(try_repeat("ab", -1).is_err());
        assert_eq!(try_repeat("ab", i64::MAX).unwrap_err().message, "string too long: 2 bytes repeated 9223372036854775807 times");
    }

    #[test]
    fn search() {
        assert_eq!(index_of("a-b-c", "-"), 1);
        assert_eq!(last_index_of("a-b-c", "-"), 3);
        assert_eq!(index_of("abc", "x"), -1);
        assert_eq!(index_of("héllo", "l"), 3);
        assert_eq!(ok(Ok(replace("a-b-c", "-", "+"))), "a+b-c");
        assert_eq!(ok(Ok(replace_all("a-b-c", "-", "+"))), "a+b+c");
    }

    #[test]
    fn split_and_chars() {
        let shown = |l: List<Str>| crate::debug(&l);
        assert_eq!(shown(split("a,b,,c", ",")), r#"["a", "b", "", "c"]"#);
        assert_eq!(shown(split("a,", ",")), r#"["a", ""]"#);
        assert_eq!(shown(split("", ",")), r#"[""]"#);
        assert_eq!(shown(split("a--b", "--")), r#"["a", "b"]"#);
        assert_eq!(shown(split("héllo", "")), r#"["h", "é", "l", "l", "o"]"#);
        assert_eq!(shown(split("", "")), "[]");
        assert_eq!(shown(chars("ab")), r#"["a", "b"]"#);
    }
}
