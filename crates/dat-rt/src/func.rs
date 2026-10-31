//! Stored function values: `const f = (x: int) => x + 1;`, a callback kept in
//! a list, a named function passed as a value. `FuncN` takes `N` arguments.
//!
//! These hold an `Rc<dyn Fn>` directly instead of a [`crate::Shared`]: stable
//! Rust can only turn a closure into a `dyn Fn` behind one of std's own
//! pointers. A later switch of `Shared` to a thread-safe count switches these
//! too.
//!
//! A parameter that the function only calls, or lends on, takes a `FnRefN`
//! instead: the caller's function, borrowed for the call, so an arrow
//! function passed to it is never allocated.

use crate::{Placeholder, Show};
use std::rc::Rc;

macro_rules! funcs {
    ($( $name:ident ($($a:ident: $t:ident),*); )*) => {$(
        /// A stored function value.
        pub struct $name<$($t,)* R>(Rc<dyn Fn($($t),*) -> R>);

        impl<$($t,)* R> $name<$($t,)* R> {
            pub fn new(f: impl Fn($($t),*) -> R + 'static) -> Self {
                $name(Rc::new(f))
            }

            // As many arguments as the dat function takes.
            #[allow(clippy::too_many_arguments)]
            #[inline(always)]
            pub fn call(&self, $($a: $t),*) -> R {
                (self.0)($($a),*)
            }
        }

        impl<$($t,)* R> Clone for $name<$($t,)* R> {
            #[inline]
            fn clone(&self) -> Self {
                $name(Rc::clone(&self.0))
            }
        }

        impl<$($t,)* R> Show for $name<$($t,)* R> {
            fn show(&self, out: &mut String) {
                out.push_str("<function>")
            }
        }

        impl<$($t: 'static,)* R: 'static> Placeholder for $name<$($t,)* R> {
            fn placeholder() -> Self {
                $name::new(|$(_: $t),*| crate::unreachable())
            }
        }
    )*};
}

macro_rules! lent {
    ($( $name:ident $func:ident ($($a:ident: $t:ident),*); )*) => {$(
        /// A function value lent to a parameter for one call.
        pub struct $name<'f, $($t,)* R>(&'f dyn Fn($($t),*) -> R);

        impl<'f, $($t,)* R> $name<'f, $($t,)* R> {
            #[inline(always)]
            pub fn new(f: &'f dyn Fn($($t),*) -> R) -> Self {
                $name(f)
            }

            // As many arguments as the dat function takes.
            #[allow(clippy::too_many_arguments)]
            #[inline(always)]
            pub fn call(&self, $($a: $t),*) -> R {
                (self.0)($($a),*)
            }
        }

        impl<$($t,)* R> Clone for $name<'_, $($t,)* R> {
            #[inline]
            fn clone(&self) -> Self {
                *self
            }
        }

        impl<$($t,)* R> Copy for $name<'_, $($t,)* R> {}

        impl<$($t,)* R> $func<$($t,)* R> {
            /// Lends this function to a parameter for one call.
            #[inline(always)]
            pub fn lend(&self) -> $name<'_, $($t,)* R> {
                $name(&*self.0)
            }
        }
    )*};
}

funcs! {
    Func0();
    Func1(a: A);
    Func2(a: A, b: B);
    Func3(a: A, b: B, c: C);
    Func4(a: A, b: B, c: C, d: D);
    Func5(a: A, b: B, c: C, d: D, e: E);
    Func6(a: A, b: B, c: C, d: D, e: E, f: F);
    Func7(a: A, b: B, c: C, d: D, e: E, f: F, g: G);
    Func8(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H);
    Func9(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I);
    Func10(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I, j: J);
    Func11(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I, j: J, k: K);
    Func12(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I, j: J, k: K, l: L);
}

lent! {
    FnRef0 Func0();
    FnRef1 Func1(a: A);
    FnRef2 Func2(a: A, b: B);
    FnRef3 Func3(a: A, b: B, c: C);
    FnRef4 Func4(a: A, b: B, c: C, d: D);
    FnRef5 Func5(a: A, b: B, c: C, d: D, e: E);
    FnRef6 Func6(a: A, b: B, c: C, d: D, e: E, f: F);
    FnRef7 Func7(a: A, b: B, c: C, d: D, e: E, f: F, g: G);
    FnRef8 Func8(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H);
    FnRef9 Func9(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I);
    FnRef10 Func10(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I, j: J);
    FnRef11 Func11(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I, j: J, k: K);
    FnRef12 Func12(a: A, b: B, c: C, d: D, e: E, f: F, g: G, h: H, i: I, j: J, k: K, l: L);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn function_values_call_clone_and_print() {
        let base = 10;
        let add = Func2::new(move |a: i64, b: i64| a + b + base);
        let copy = add.clone();
        assert_eq!(copy.call(1, 2), 13);
        assert_eq!(Func0::new(|| 7i64).call(), 7);
        assert_eq!(crate::debug(&add), "<function>");
        assert_eq!(crate::debug(&crate::List::from_vec(vec![Func1::new(|x: i64| x)])), "[<function>]");
    }

    #[test]
    fn lent_functions_call_and_copy() {
        fn twice(f: FnRef1<'_, i64, i64>, x: i64) -> i64 {
            let g = f;
            f.call(g.call(x))
        }
        let base = 10;
        assert_eq!(twice(FnRef1::new(&move |x: i64| x + base), 1), 21);
        let stored = Func1::new(|x: i64| x * 3);
        assert_eq!(twice(stored.lend(), 2), 18);
        assert_eq!(FnRef0::new(&|| 7i64).call(), 7);
    }
}
