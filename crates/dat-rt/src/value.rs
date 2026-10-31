//! The traits every dat value implements: printing, `==`, hashing, the
//! natural order and a placeholder. `==`, `includes`, `unique`, `sort()`,
//! `print` and `assert` all go through them, so the two run modes can't
//! differ (`reference/01-builtin-types.md`, "Equality, Hashing and Ordering").

use crate::fmt::{bool_text, write_debug_str, write_float, write_int};
use crate::{DatError, ErrorKind, Str};
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

/// How a value appears inside a collection or an assert's sides: strings
/// quoted (`reference/04-primitive-methods.md`).
pub trait Show {
    fn show(&self, out: &mut String);

    /// `toString()`: the same, except that a string is its own text.
    fn text(&self, out: &mut String) {
        self.show(out)
    }
}

/// `==` and `!=`. Floats follow IEEE 754: `NaN != NaN`, so `[NaN] != [NaN]`.
pub trait Equal {
    fn equal(&self, other: &Self) -> bool;
}

/// Set elements and map keys. Equal values hash the same.
pub trait DatHash: Equal {
    fn dat_hash<H: Hasher>(&self, h: &mut H);
}

/// The natural order `sort()` uses: a total order. Floats order as `<`
/// does, with `NaN` after every number; `-0.0` and `0.0` are equal, so a
/// stable sort keeps their order.
pub trait Order {
    fn order(&self, other: &Self) -> Ordering;
}

/// A cheap value to leave in a slot when a value is moved out of it, as
/// `shift` and take-and-restore do. Safe Rust can't leave a slot empty.
pub trait Placeholder {
    fn placeholder() -> Self;
}

/// `toString()` of any value, as a dat string.
pub fn text<T: Show + ?Sized>(v: &T) -> Str {
    let mut out = String::new();
    v.text(&mut out);
    Str::from_string(out)
}

/// A value as it appears inside a collection: strings quoted.
pub fn show<T: Show + ?Sized>(v: &T) -> Str {
    let mut out = String::new();
    v.show(&mut out);
    Str::from_string(out)
}

/// One side of a failed `assert`, as [`show`] writes it.
pub fn debug<T: Show + ?Sized>(v: &T) -> String {
    let mut out = String::new();
    v.show(&mut out);
    out
}

// ---- int ----

impl Show for i64 {
    fn show(&self, out: &mut String) {
        write_int(out, *self)
    }
}

impl Equal for i64 {
    #[inline(always)]
    fn equal(&self, other: &i64) -> bool {
        self == other
    }
}

impl DatHash for i64 {
    #[inline(always)]
    fn dat_hash<H: Hasher>(&self, h: &mut H) {
        h.write_i64(*self)
    }
}

impl Order for i64 {
    #[inline(always)]
    fn order(&self, other: &i64) -> Ordering {
        self.cmp(other)
    }
}

impl Placeholder for i64 {
    #[inline(always)]
    fn placeholder() -> i64 {
        0
    }
}

// ---- float ----

impl Show for f64 {
    fn show(&self, out: &mut String) {
        write_float(out, *self)
    }
}

impl Equal for f64 {
    #[inline(always)]
    fn equal(&self, other: &f64) -> bool {
        self == other
    }
}

impl Order for f64 {
    #[inline(always)]
    fn order(&self, other: &f64) -> Ordering {
        // Ordering by the bits instead would depend on the processor: x86
        // and ARM give `0.0 / 0.0` different signs.
        match self.partial_cmp(other) {
            Some(o) => o,
            None => self.is_nan().cmp(&other.is_nan()),
        }
    }
}

impl Placeholder for f64 {
    #[inline(always)]
    fn placeholder() -> f64 {
        0.0
    }
}

// ---- bool ----

impl Show for bool {
    fn show(&self, out: &mut String) {
        out.push_str(bool_text(*self))
    }
}

impl Equal for bool {
    #[inline(always)]
    fn equal(&self, other: &bool) -> bool {
        self == other
    }
}

impl DatHash for bool {
    #[inline(always)]
    fn dat_hash<H: Hasher>(&self, h: &mut H) {
        h.write_u8(*self as u8)
    }
}

impl Order for bool {
    #[inline(always)]
    fn order(&self, other: &bool) -> Ordering {
        self.cmp(other)
    }
}

impl Placeholder for bool {
    #[inline(always)]
    fn placeholder() -> bool {
        false
    }
}

// ---- string ----

impl Show for Str {
    fn show(&self, out: &mut String) {
        write_debug_str(out, self.as_str())
    }

    fn text(&self, out: &mut String) {
        out.push_str(self.as_str())
    }
}

impl Equal for Str {
    #[inline]
    fn equal(&self, other: &Str) -> bool {
        self.as_str() == other.as_str()
    }
}

impl DatHash for Str {
    #[inline]
    fn dat_hash<H: Hasher>(&self, h: &mut H) {
        // `str`'s own hash ends with a marker byte, so the strings of a
        // tuple or list can't run into each other: ("ab", "c") vs ("a", "bc").
        self.as_str().hash(h)
    }
}

impl Order for Str {
    #[inline]
    fn order(&self, other: &Str) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

thread_local! {
    /// The empty string every placeholder shares, so leaving one costs a
    /// count increment rather than an allocation.
    static EMPTY_STR: Str = Str::new("");
}

impl Placeholder for Str {
    #[inline]
    fn placeholder() -> Str {
        EMPTY_STR.with(Str::clone)
    }
}

// ---- errors and void ----

impl Show for DatError {
    fn show(&self, out: &mut String) {
        out.push_str(self.kind.name());
        out.push_str(": ");
        out.push_str(&self.message);
    }
}

impl Placeholder for DatError {
    fn placeholder() -> DatError {
        DatError::new(ErrorKind::Error, String::new())
    }
}

impl Placeholder for () {
    #[inline(always)]
    fn placeholder() {}
}

// ---- tuples ----

macro_rules! tuples {
    ($( ($t0:ident $i0:tt $(, $t:ident $i:tt)+) )*) => {$(
        tuples!(@impl ($t0 $i0 $(, $t $i)+) first: $i0, rest: $($i)+);
    )*};
    (@impl ($($t:ident $i:tt),+) first: $first:tt, rest: $($rest:tt)+) => {
        impl<$($t: Show),+> Show for ($($t,)+) {
            fn show(&self, out: &mut String) {
                out.push('[');
                self.$first.show(out);
                $(
                    out.push_str(", ");
                    self.$rest.show(out);
                )+
                out.push(']');
            }
        }

        impl<$($t: Equal),+> Equal for ($($t,)+) {
            #[inline]
            fn equal(&self, other: &Self) -> bool {
                $(self.$i.equal(&other.$i))&&+
            }
        }

        impl<$($t: DatHash),+> DatHash for ($($t,)+) {
            #[inline]
            fn dat_hash<Z: Hasher>(&self, h: &mut Z) {
                $(self.$i.dat_hash(h);)+
            }
        }

        impl<$($t: Placeholder),+> Placeholder for ($($t,)+) {
            #[inline]
            fn placeholder() -> Self {
                ($($t::placeholder(),)+)
            }
        }
    };
}

tuples! {
    (A 0, B 1)
    (A 0, B 1, C 2)
    (A 0, B 1, C 2, D 3)
    (A 0, B 1, C 2, D 3, E 4)
    (A 0, B 1, C 2, D 3, E 4, F 5)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10)
    (A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11)
}

// A tuple type of one position, `[int]`, and of none, `[]`.

impl<A: Show> Show for (A,) {
    fn show(&self, out: &mut String) {
        out.push('[');
        self.0.show(out);
        out.push(']');
    }
}

impl<A: Equal> Equal for (A,) {
    #[inline]
    fn equal(&self, other: &Self) -> bool {
        self.0.equal(&other.0)
    }
}

impl<A: DatHash> DatHash for (A,) {
    #[inline]
    fn dat_hash<Z: Hasher>(&self, h: &mut Z) {
        self.0.dat_hash(h);
    }
}

impl<A: Placeholder> Placeholder for (A,) {
    #[inline]
    fn placeholder() -> Self {
        (A::placeholder(),)
    }
}

impl Show for () {
    fn show(&self, out: &mut String) {
        out.push_str("[]");
    }
}

impl Equal for () {
    #[inline]
    fn equal(&self, _: &Self) -> bool {
        true
    }
}

impl DatHash for () {
    #[inline]
    fn dat_hash<Z: Hasher>(&self, _: &mut Z) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown<T: Show>(v: &T) -> String {
        debug(v)
    }

    #[test]
    fn primitives_print_as_to_string_does() {
        assert_eq!(shown(&42i64), "42");
        assert_eq!(shown(&2.0f64), "2.0");
        assert_eq!(shown(&true), "true");
        assert_eq!(shown(&Str::new("a\"b")), "\"a\\\"b\"");
        assert_eq!(text(&Str::new("a\"b")).as_str(), "a\"b");
        assert_eq!(shown(&(1i64, Str::new("x"), 0.5f64)), "[1, \"x\", 0.5]");
        assert_eq!(text(&(1i64, Str::new("x"))).as_str(), "[1, \"x\"]");
        assert_eq!(
            shown(&DatError::new(ErrorKind::RangeError, "index 5 out of range for length 3")),
            "RangeError: index 5 out of range for length 3"
        );
    }

    #[test]
    fn floats_compare_as_ieee_and_order_nan_last() {
        assert!(!f64::NAN.equal(&f64::NAN));
        assert!((-0.0f64).equal(&0.0));
        assert!(!(1.0f64, f64::NAN).equal(&(1.0, f64::NAN)));
        assert_eq!(f64::NAN.order(&f64::INFINITY), Ordering::Greater);
        assert_eq!(f64::NEG_INFINITY.order(&f64::NAN), Ordering::Less);
        assert_eq!(f64::NAN.order(&-f64::NAN), Ordering::Equal);
        assert_eq!((-0.0f64).order(&0.0), Ordering::Equal);
        assert_eq!(false.order(&true), Ordering::Less);
    }

    #[test]
    fn placeholders_are_cheap_and_empty() {
        let a = Str::placeholder();
        let b = Str::placeholder();
        assert_eq!(a.as_str(), "");
        assert_eq!(a, b);
        assert_eq!(<(i64, bool, Str)>::placeholder().0, 0);
        assert_eq!(DatError::placeholder().message, "");
    }
}
