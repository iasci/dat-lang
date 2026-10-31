//! Diagnostic codes. The first two digits name the spec section that defines
//! the rule (`reference/README.md`), and each code has a ❌ example there;
//! `E00` codes are general (§21). `CODES.md` lists them all.

/// A syntax error (dat-syntax).
pub const SYNTAX: &str = dat_syntax::E_SYNTAX;
/// A construct the current milestone doesn't implement yet.
pub const UNSUPPORTED: &str = "E0002";

/// §02: `x` is const and can't be modified or reassigned.
pub const CONST_ASSIGN: &str = "E0201";
/// §02: a value doesn't have the type that's expected.
pub const MISMATCH: &str = "E0250";
/// §02: a non-nullable variable needs a value.
pub const LET_NO_VALUE: &str = "E0203";
/// §02: no type and no value.
pub const NO_TYPE_NO_VALUE: &str = "E0204";
/// §01: a tuple index that isn't a constant, or is past the tuple's end.
pub const TUPLE_INDEX: &str = "E0101";
/// §01: a number literal outside the range of its type.
pub const LITERAL_RANGE: &str = "E0150";
/// §01: a set element, map key or grouping key whose type isn't hashable.
pub const NOT_HASHABLE: &str = "E0151";
/// §01: sorting or searching values whose type can't be ordered or compared.
pub const NOT_COMPARABLE: &str = "E0152";
/// §02: a `let` copy is changed and never read afterwards.
pub const COPY_UNUSED: &str = "E0202";
/// §02: `[]` or `{}` where no type says what it holds.
pub const CANT_INFER: &str = "E0206";

/// §02: const must be initialized.
pub const CONST_NO_VALUE: &str = "E0205";
/// §02: a variable can't hold `void`.
pub const VOID_VALUE: &str = "E0252";

/// §03: can't add string and int.
pub const STRING_PLUS: &str = "E0301";
/// §03: a compound assignment would make an `int` variable a float.
pub const INT_BECOMES_FLOAT: &str = "E0302";
/// §03: `++` is a statement.
pub const INCDEC_EXPR: &str = "E0303";
/// §03: no prefix form.
pub const INCDEC_PREFIX: &str = "E0304";
/// §03: a statement with no effect.
pub const NO_EFFECT: &str = "E0305";
/// §03: an operator doesn't apply to these operand types.
pub const BAD_OPERANDS: &str = "E0350";

/// §06: missing return.
pub const MISSING_RETURN: &str = "E0601";
/// §06: wrong number of arguments.
pub const ARITY: &str = "E0650";
/// §06: a `mut` parameter with a default value.
pub const MUT_DEFAULT: &str = "E0651";
/// §06: `return` with a value in a `void` function.
pub const RETURN_IN_VOID: &str = "E0652";
/// §06: `return;` in a function that returns a value.
pub const RETURN_NO_VALUE: &str = "E0653";
/// §06: a `never` function that can return.
pub const NEVER_RETURNS: &str = "E0654";
/// §06: an arrow function's parameter type can't be inferred.
pub const PARAM_TYPE_UNKNOWN: &str = "E0602";
/// §06: a closure uses a `let` variable.
pub const CAPTURES_LET: &str = "E0603";
/// §06: an exported function without a declared return type.
pub const EXPORT_NEEDS_TYPE: &str = "E0655";
/// §06: a parameter without a default after one with a default.
pub const DEFAULT_ORDER: &str = "E0656";
/// §06: one call passes the same value as `mut` twice, a value and a part of it,
/// or two parts that may be the same one.
pub const MUT_TWICE: &str = "E0657";
/// §06: an argument for a `mut` parameter without `mut`.
pub const NEEDS_MUT: &str = "E0658";
/// §06: `mut` before an argument whose parameter isn't `mut`.
pub const NOT_MUT: &str = "E0659";
/// §06: no `main`, for `dat run` and `dat build`.
pub const NO_MAIN: &str = "E0660";
/// §06: `main` has parameters or doesn't return `void`.
pub const BAD_MAIN: &str = "E0661";

/// §07: condition must be bool.
pub const NOT_BOOL: &str = "E0701";
/// §07: the list of a `for (mut x of xs)` loop used in its body.
pub const IN_USE_BY_LOOP: &str = "E0703";
/// §07: `break` or `continue` outside a loop.
pub const OUTSIDE_LOOP: &str = "E0750";

/// §08: a struct literal is missing a field.
pub const MISSING_FIELD: &str = "E0850";
/// §08: a struct has no field of that name.
pub const UNKNOWN_FIELD: &str = "E0851";

/// §12: only errors can be thrown.
pub const THROW_NON_ERROR: &str = "E1250";
/// §12: a `catch` type that isn't an error type.
pub const CATCH_NON_ERROR: &str = "E1251";
/// §12: a catch-all clause that isn't the last one.
pub const CATCH_ALL_NOT_LAST: &str = "E1252";

/// §14: `y` is not defined.
pub const UNDEFINED: &str = "E1401";
/// §14: `x` is already declared.
pub const REDECLARED: &str = "E1402";

/// §17: a module doesn't export that name.
pub const NOT_EXPORTED: &str = "E1701";
/// §17: an import path must start with `./`, `../` or `@`.
pub const IMPORT_PATH: &str = "E1703";

/// §20: two tests in one module have the same name.
pub const DUPLICATE_TEST: &str = "E2050";
