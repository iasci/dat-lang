//! The methods of `int` and `float` (`reference/04-primitive-methods.md`),
//! which `@dat/math` shares, and the exact comparison of an `int` with a
//! `float`.
//!
//! `try_*` functions are for the interpreter. The others are for generated
//! code: the ones that can fail take the operation's location and throw.

use crate::fmt::write_float;
use crate::{DatError, ErrorKind, List, Loc, Str, throw};
use std::cmp::Ordering;

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

/// A receiver as written before a method: `7`, or `(-7)` when it's negative.
fn receiver_int(n: i64) -> String {
    if n < 0 { format!("({n})") } else { n.to_string() }
}

fn receiver_float(x: f64) -> String {
    let mut s = String::new();
    write_float(&mut s, x);
    if s.starts_with('-') { format!("({s})") } else { s }
}

// ---- int ----

#[inline]
pub fn try_int_abs(a: i64) -> R<i64> {
    a.checked_abs().ok_or_else(|| range_error(format!("int overflow: {}.abs()", receiver_int(a))))
}

#[inline(always)]
pub fn int_abs(a: i64, loc: &'static Loc) -> i64 {
    or_throw(try_int_abs(a), loc)
}

#[inline(always)]
pub fn int_min(a: i64, b: i64) -> i64 {
    a.min(b)
}

#[inline(always)]
pub fn int_max(a: i64, b: i64) -> i64 {
    a.max(b)
}

#[inline(always)]
pub fn int_sqrt(a: i64) -> f64 {
    (a as f64).sqrt()
}

#[inline(always)]
pub fn int_to_float(a: i64) -> f64 {
    a as f64
}

/// `a.pow(e)`, exact. A negative exponent or a result outside `int` throws.
pub fn try_int_pow(a: i64, e: i64) -> R<i64> {
    if e < 0 {
        return Err(range_error(format!("negative exponent: {}.pow({e})", receiver_int(a))));
    }
    let r = match u32::try_from(e) {
        Ok(e) => a.checked_pow(e),
        // Only 0, 1 and -1 survive an exponent this large.
        Err(_) => match a {
            0 | 1 => Some(a),
            -1 => Some(if e % 2 == 0 { 1 } else { -1 }),
            _ => None,
        },
    };
    r.ok_or_else(|| range_error(format!("int overflow: {}.pow({e})", receiver_int(a))))
}

#[inline(always)]
pub fn int_pow(a: i64, e: i64, loc: &'static Loc) -> i64 {
    or_throw(try_int_pow(a, e), loc)
}

// ---- float to int ----

const TWO_63: f64 = 9_223_372_036_854_775_808.0;

/// `r`, a whole number that `x.method()` rounded `x` to, as an `int`.
#[inline]
fn fit(r: f64, x: f64, method: &str) -> R<i64> {
    // NaN fails both comparisons.
    if (-TWO_63..TWO_63).contains(&r) { Ok(r as i64) } else { Err(bad_float_to_int(x, method)) }
}

#[cold]
#[inline(never)]
fn bad_float_to_int(x: f64, method: &str) -> Box<DatError> {
    let call = format!("{}.{method}()", receiver_float(x));
    if x.is_nan() { range_error(format!("can't convert NaN to an int: {call}")) } else { range_error(format!("int overflow: {call}")) }
}

/// `x.toInt()`: toward zero.
#[inline]
pub fn try_float_to_int(x: f64) -> R<i64> {
    fit(x.trunc(), x, "toInt")
}

#[inline]
pub fn try_float_floor(x: f64) -> R<i64> {
    fit(x.floor(), x, "floor")
}

#[inline]
pub fn try_float_ceil(x: f64) -> R<i64> {
    fit(x.ceil(), x, "ceil")
}

/// `x.round()`: to the nearest whole number, with halves going up, so
/// `(-2.5).round()` is `-2`. `x - x.floor()` is exact, so values just
/// below a half, such as 0.49999999999999994, round down.
#[inline]
pub fn try_float_round(x: f64) -> R<i64> {
    let f = x.floor();
    let r = if x - f >= 0.5 { f + 1.0 } else { f };
    fit(r, x, "round")
}

#[inline(always)]
pub fn float_to_int(x: f64, loc: &'static Loc) -> i64 {
    or_throw(try_float_to_int(x), loc)
}

#[inline(always)]
pub fn float_floor(x: f64, loc: &'static Loc) -> i64 {
    or_throw(try_float_floor(x), loc)
}

#[inline(always)]
pub fn float_ceil(x: f64, loc: &'static Loc) -> i64 {
    or_throw(try_float_ceil(x), loc)
}

#[inline(always)]
pub fn float_round(x: f64, loc: &'static Loc) -> i64 {
    or_throw(try_float_round(x), loc)
}

// ---- float ----

#[inline(always)]
pub fn float_abs(x: f64) -> f64 {
    x.abs()
}

#[inline(always)]
pub fn float_sqrt(x: f64) -> f64 {
    x.sqrt()
}

#[inline(always)]
pub fn float_pow(x: f64, e: f64) -> f64 {
    x.powf(e)
}

/// The smaller value. A `NaN` on either side gives `NaN`, and `-0.0` is
/// smaller than `0.0`.
#[inline]
pub fn float_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else if b < a {
        b
    } else if a == b {
        if a.is_sign_negative() { a } else { b }
    } else {
        f64::NAN
    }
}

/// The larger value. A `NaN` on either side gives `NaN`, and `0.0` is
/// larger than `-0.0`.
#[inline]
pub fn float_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else if b > a {
        b
    } else if a == b {
        if a.is_sign_negative() { b } else { a }
    } else {
        f64::NAN
    }
}

// ---- `max(...values)` and `min(...values)` from `@dat/math` ----

#[cold]
#[inline(never)]
fn no_values(function: &str) -> Box<DatError> {
    range_error(format!("{function}() of no values"))
}

pub fn try_max_int(values: &List<i64>) -> R<i64> {
    values.iter().copied().max().ok_or_else(|| no_values("max"))
}

pub fn try_min_int(values: &List<i64>) -> R<i64> {
    values.iter().copied().min().ok_or_else(|| no_values("min"))
}

/// As [`float_max`] does for two: a `NaN` anywhere gives `NaN`.
pub fn try_max_float(values: &List<f64>) -> R<f64> {
    values.iter().copied().reduce(float_max).ok_or_else(|| no_values("max"))
}

pub fn try_min_float(values: &List<f64>) -> R<f64> {
    values.iter().copied().reduce(float_min).ok_or_else(|| no_values("min"))
}

#[inline(always)]
pub fn max_int(values: &List<i64>, loc: &'static Loc) -> i64 {
    or_throw(try_max_int(values), loc)
}

#[inline(always)]
pub fn min_int(values: &List<i64>, loc: &'static Loc) -> i64 {
    or_throw(try_min_int(values), loc)
}

#[inline(always)]
pub fn max_float(values: &List<f64>, loc: &'static Loc) -> f64 {
    or_throw(try_max_float(values), loc)
}

#[inline(always)]
pub fn min_float(values: &List<f64>, loc: &'static Loc) -> f64 {
    or_throw(try_min_float(values), loc)
}

#[inline(always)]
pub fn float_is_nan(x: f64) -> bool {
    x.is_nan()
}

/// The most decimals `toFixed` writes.
const MAX_FIXED_DIGITS: i64 = 100;

/// `x.toFixed(digits)`: exactly `digits` decimals, rounded to the nearest,
/// with exact halves going away from zero.
pub fn try_float_to_fixed(x: f64, digits: i64) -> R<Str> {
    if !(0..=MAX_FIXED_DIGITS).contains(&digits) {
        return Err(range_error(format!("toFixed digits must be from 0 to {MAX_FIXED_DIGITS}: {}.toFixed({digits})", receiver_float(x))));
    }
    if !x.is_finite() {
        let mut s = String::new();
        write_float(&mut s, x);
        return Ok(Str::from_string(s));
    }
    let d = digits as usize;
    // Rust rounds the exact value to nearest, with exact halves going to
    // even. An exact half has exactly `d + 1` decimals, the last one a 5,
    // which happens when `x * 2^(d + 1)` is an odd whole number.
    let y = x * 2f64.powi(digits as i32 + 1);
    if !(y.is_finite() && y.fract() == 0.0 && (y % 2.0).abs() == 1.0) {
        return Ok(Str::from_string(format!("{x:.d$}")));
    }
    let mut s = format!("{x:.*}", d + 1).into_bytes();
    s.pop();
    if s.last() == Some(&b'.') {
        s.pop();
    }
    // Add one in the last place, away from zero.
    let mut i = s.len();
    loop {
        if i == 0 || s[i - 1] == b'-' {
            s.insert(i, b'1');
            break;
        }
        i -= 1;
        match s[i] {
            b'.' => {}
            b'9' => s[i] = b'0',
            c => {
                s[i] = c + 1;
                break;
            }
        }
    }
    Ok(Str::from_string(String::from_utf8(s).expect("digits are ASCII")))
}

#[inline(always)]
pub fn float_to_fixed(x: f64, digits: i64, loc: &'static Loc) -> Str {
    or_throw(try_float_to_fixed(x, digits), loc)
}

// ---- exact division of two ints ----

/// `(a / b).method()`, computed on whole numbers. `b` isn't zero.
#[inline]
fn div(a: i64, b: i64, method: &str, q: impl FnOnce(i128, i128) -> i128) -> R<i64> {
    if b == 0 {
        return Err(range_error(format!("int division by zero: ({a} / 0).{method}()")));
    }
    let v = q(a as i128, b as i128);
    i64::try_from(v).map_err(|_| range_error(format!("int overflow: ({a} / {b}).{method}()")))
}

fn floor_div(a: i128, b: i128) -> i128 {
    let (q, r) = (a / b, a % b);
    if r != 0 && ((r < 0) != (b < 0)) { q - 1 } else { q }
}

#[inline]
pub fn try_div_to_int(a: i64, b: i64) -> R<i64> {
    div(a, b, "toInt", |a, b| a / b)
}

#[inline]
pub fn try_div_floor(a: i64, b: i64) -> R<i64> {
    div(a, b, "floor", floor_div)
}

#[inline]
pub fn try_div_ceil(a: i64, b: i64) -> R<i64> {
    div(a, b, "ceil", |a, b| {
        let (q, r) = (a / b, a % b);
        if r != 0 && ((r < 0) == (b < 0)) { q + 1 } else { q }
    })
}

/// Halves go up, as `float.round()` does: `floor(a / b + 1/2)`.
#[inline]
pub fn try_div_round(a: i64, b: i64) -> R<i64> {
    div(a, b, "round", |a, b| {
        let (n, d) = if b < 0 { (-a, -b) } else { (a, b) };
        floor_div(2 * n + d, 2 * d)
    })
}

#[inline(always)]
pub fn div_to_int(a: i64, b: i64, loc: &'static Loc) -> i64 {
    or_throw(try_div_to_int(a, b), loc)
}

#[inline(always)]
pub fn div_floor(a: i64, b: i64, loc: &'static Loc) -> i64 {
    or_throw(try_div_floor(a, b), loc)
}

#[inline(always)]
pub fn div_ceil(a: i64, b: i64, loc: &'static Loc) -> i64 {
    or_throw(try_div_ceil(a, b), loc)
}

#[inline(always)]
pub fn div_round(a: i64, b: i64, loc: &'static Loc) -> i64 {
    or_throw(try_div_round(a, b), loc)
}

// ---- comparing an int with a float ----

/// A comparison operator, for [`holds`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

/// Whether `op` holds for an ordering; `None`, a comparison with `NaN`,
/// satisfies only `!=`.
#[inline(always)]
pub fn holds(op: Cmp, o: Option<Ordering>) -> bool {
    match op {
        Cmp::Lt => o == Some(Ordering::Less),
        Cmp::Le => matches!(o, Some(Ordering::Less | Ordering::Equal)),
        Cmp::Gt => o == Some(Ordering::Greater),
        Cmp::Ge => matches!(o, Some(Ordering::Greater | Ordering::Equal)),
        Cmp::Eq => o == Some(Ordering::Equal),
        Cmp::Ne => o != Some(Ordering::Equal),
    }
}

/// Compares the exact values of an `int` and a `float`, so
/// 9007199254740993 is greater than 9007199254740992.0, though converting
/// it to a float would make them equal.
#[inline]
pub fn cmp_int_float(a: i64, b: f64) -> Option<Ordering> {
    // Rounding to a float keeps order, so a difference after it is real.
    let fa = a as f64;
    if fa != b {
        return fa.partial_cmp(&b);
    }
    // `b` is a whole number next to `a`, and 2^63 is the one that isn't an int.
    if b >= TWO_63 { Some(Ordering::Less) } else { Some(a.cmp(&(b as i64))) }
}

#[inline]
pub fn cmp_float_int(a: f64, b: i64) -> Option<Ordering> {
    cmp_int_float(b, a).map(Ordering::reverse)
}

// The spec's examples use 3.14 and 3.14159, which aren't meant as π.
#[cfg(test)]
#[allow(clippy::approx_constant)]
mod tests {
    use super::*;

    fn fixed(x: f64, d: i64) -> String {
        try_float_to_fixed(x, d).unwrap().as_str().to_string()
    }

    #[test]
    fn to_fixed_rounds_exact_halves_away_from_zero() {
        assert_eq!(fixed(3.14159, 2), "3.14");
        assert_eq!(fixed(0.125, 2), "0.13");
        assert_eq!(fixed(-0.125, 2), "-0.13");
        assert_eq!(fixed(2.5, 0), "3");
        assert_eq!(fixed(9.5, 0), "10");
        assert_eq!(fixed(-9.5, 0), "-10");
        assert_eq!(fixed(99.5, 0), "100");
        assert_eq!(fixed(1.005, 2), "1.00");
        assert_eq!(fixed(19.99, 2), "19.99");
        assert_eq!(fixed(0.0, 3), "0.000");
        assert_eq!(fixed(f64::NAN, 2), "NaN");
        assert!(try_float_to_fixed(1.0, 101).is_err());
    }

    #[test]
    fn rounding_to_int() {
        assert_eq!(try_float_round(-2.5).unwrap(), -2);
        assert_eq!(try_float_round(2.5).unwrap(), 3);
        assert_eq!(try_float_round(0.49999999999999994).unwrap(), 0);
        assert_eq!(try_float_floor(-0.5).unwrap(), -1);
        assert_eq!(try_float_to_int(-9.9).unwrap(), -9);
        assert_eq!(try_float_to_int(-9223372036854775808.0).unwrap(), i64::MIN);
        assert_eq!(try_float_to_int(9223372036854775808.0).unwrap_err().message, "int overflow: 9223372036854776000.0.toInt()");
        assert_eq!(try_float_floor(f64::NAN).unwrap_err().message, "can't convert NaN to an int: NaN.floor()");
        assert_eq!(try_float_ceil(f64::NEG_INFINITY).unwrap_err().message, "int overflow: (-Infinity).ceil()");
    }

    #[test]
    fn exact_division() {
        assert_eq!(try_div_floor(7, 2).unwrap(), 3);
        assert_eq!(try_div_floor(-7, 2).unwrap(), -4);
        assert_eq!(try_div_ceil(-7, 2).unwrap(), -3);
        assert_eq!(try_div_ceil(7, -2).unwrap(), -3);
        assert_eq!(try_div_round(7, 2).unwrap(), 4);
        assert_eq!(try_div_round(-7, 2).unwrap(), -3);
        assert_eq!(try_div_round(7, -2).unwrap(), -3);
        assert_eq!(try_div_to_int(-7, 2).unwrap(), -3);
        assert_eq!(try_div_floor(i64::MAX, 1).unwrap(), i64::MAX);
        assert_eq!(try_div_floor(i64::MIN, -1).unwrap_err().message, "int overflow: (-9223372036854775808 / -1).floor()");
        assert_eq!(try_div_round(7, 0).unwrap_err().message, "int division by zero: (7 / 0).round()");
    }

    #[test]
    fn int_methods() {
        assert_eq!(try_int_pow(10, 3).unwrap(), 1000);
        assert_eq!(try_int_pow(-1, 1 << 40).unwrap(), 1);
        assert_eq!(try_int_pow(10, 19).unwrap_err().message, "int overflow: 10.pow(19)");
        assert_eq!(try_int_pow(2, -1).unwrap_err().message, "negative exponent: 2.pow(-1)");
        assert_eq!(try_int_abs(i64::MIN).unwrap_err().message, "int overflow: (-9223372036854775808).abs()");
        assert_eq!(int_sqrt(1000), 31.622776601683793);
    }

    #[test]
    fn min_and_max_keep_nan_and_signed_zero() {
        assert!(float_min(1.0, f64::NAN).is_nan());
        assert!(float_max(f64::NAN, 1.0).is_nan());
        assert!(float_min(0.0, -0.0).is_sign_negative());
        assert!(float_max(-0.0, 0.0).is_sign_positive());
    }

    #[test]
    fn max_and_min_of_a_spread_list() {
        let ints: List<i64> = [3, -7, 5].into_iter().collect();
        assert_eq!(try_max_int(&ints).unwrap(), 5);
        assert_eq!(try_min_int(&ints).unwrap(), -7);
        let floats: List<f64> = [0.0, -0.0, 2.5].into_iter().collect();
        assert_eq!(try_max_float(&floats).unwrap(), 2.5);
        assert!(try_min_float(&floats).unwrap().is_sign_negative());
        let with_nan: List<f64> = [1.0, f64::NAN, 3.0].into_iter().collect();
        assert!(try_max_float(&with_nan).unwrap().is_nan());
        assert!(try_min_float(&with_nan).unwrap().is_nan());
        let e = try_max_int(&List::new()).unwrap_err();
        assert_eq!((e.kind, e.message.as_str()), (ErrorKind::RangeError, "max() of no values"));
        assert_eq!(try_min_int(&List::new()).unwrap_err().message, "min() of no values");
        assert_eq!(try_max_float(&List::new()).unwrap_err().message, "max() of no values");
        assert_eq!(try_min_float(&List::new()).unwrap_err().message, "min() of no values");
    }

    #[test]
    fn int_float_comparison_is_exact() {
        assert_eq!(cmp_int_float(9007199254740993, 9007199254740992.0), Some(Ordering::Greater));
        assert_eq!(cmp_int_float(i64::MAX, 9223372036854775808.0), Some(Ordering::Less));
        assert_eq!(cmp_int_float(2, 2.0), Some(Ordering::Equal));
        assert_eq!(cmp_int_float(2, f64::NAN), None);
        assert_eq!(cmp_float_int(2.5, 2), Some(Ordering::Greater));
        assert!(holds(Cmp::Ne, None) && !holds(Cmp::Eq, None) && !holds(Cmp::Le, None));
    }
}
