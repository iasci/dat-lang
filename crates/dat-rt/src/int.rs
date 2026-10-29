//! Checked `int` arithmetic. Overflow and `% 0` are `RangeError`s whose
//! messages name the operation and its operands (`reference/12-errors.md`).
//!
//! `try_*` functions are for the interpreter. The others are for generated
//! code: they take the operation's location and throw.

use crate::{DatError, Frame, Loc, throw};

#[inline]
pub fn try_add(a: i64, b: i64) -> Result<i64, Box<DatError>> {
    a.checked_add(b).ok_or_else(|| DatError::int_overflow(a, "+", b))
}

#[inline]
pub fn try_sub(a: i64, b: i64) -> Result<i64, Box<DatError>> {
    a.checked_sub(b).ok_or_else(|| DatError::int_overflow(a, "-", b))
}

#[inline]
pub fn try_mul(a: i64, b: i64) -> Result<i64, Box<DatError>> {
    a.checked_mul(b).ok_or_else(|| DatError::int_overflow(a, "*", b))
}

/// `%` keeps the sign of the left operand. `int.MIN % -1` overflows; the spec
/// doesn't settle it yet (owner question 9), so it throws.
#[inline]
pub fn try_rem(a: i64, b: i64) -> Result<i64, Box<DatError>> {
    if b == 0 {
        return Err(DatError::rem_by_zero(a));
    }
    a.checked_rem(b).ok_or_else(|| DatError::int_overflow(a, "%", b))
}

#[inline]
pub fn try_neg(a: i64) -> Result<i64, Box<DatError>> {
    a.checked_neg().ok_or_else(|| DatError::int_neg_overflow(a))
}

#[inline(always)]
pub fn add(a: i64, b: i64, loc: &'static Loc, frame: &Frame<'_>) -> i64 {
    match a.checked_add(b) {
        Some(v) => v,
        None => throw(DatError::int_overflow(a, "+", b), loc, frame),
    }
}

#[inline(always)]
pub fn sub(a: i64, b: i64, loc: &'static Loc, frame: &Frame<'_>) -> i64 {
    match a.checked_sub(b) {
        Some(v) => v,
        None => throw(DatError::int_overflow(a, "-", b), loc, frame),
    }
}

#[inline(always)]
pub fn mul(a: i64, b: i64, loc: &'static Loc, frame: &Frame<'_>) -> i64 {
    match a.checked_mul(b) {
        Some(v) => v,
        None => throw(DatError::int_overflow(a, "*", b), loc, frame),
    }
}

#[inline(always)]
pub fn rem(a: i64, b: i64, loc: &'static Loc, frame: &Frame<'_>) -> i64 {
    match try_rem(a, b) {
        Ok(v) => v,
        Err(e) => throw(e, loc, frame),
    }
}

#[inline(always)]
pub fn neg(a: i64, loc: &'static Loc, frame: &Frame<'_>) -> i64 {
    match a.checked_neg() {
        Some(v) => v,
        None => throw(DatError::int_neg_overflow(a), loc, frame),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_follow_the_spec() {
        assert_eq!(try_add(i64::MAX, 1).unwrap_err().message, "int overflow: 9223372036854775807 + 1");
        assert_eq!(try_neg(i64::MIN).unwrap_err().message, "int overflow: -(-9223372036854775808)");
        assert_eq!(try_rem(7, 0).unwrap_err().message, "int remainder by zero: 7 % 0");
        assert_eq!(try_sub(i64::MIN, 1).unwrap_err().message, "int overflow: -9223372036854775808 - 1");
        assert_eq!(try_add(i64::MIN, -1).unwrap_err().message, "int overflow: -9223372036854775808 + -1");
        assert_eq!(try_rem(-7, 3).unwrap(), -1);
    }
}
