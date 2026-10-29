//! Diagnostic codes. The first two digits name the spec section that defines
//! the rule (`reference/README.md`). Codes ending in 01–49 are the spec's own.
//! Codes ending in 50–99 are provisional: the compiler needs them, but the
//! spec doesn't define them yet, so they wait for the owner's approval.
//! `CODES.md` lists them all.

/// A syntax error (dat-syntax).
pub const SYNTAX: &str = dat_syntax::E_SYNTAX;
/// A construct the current milestone doesn't implement yet.
pub const UNSUPPORTED: &str = "E0002";

/// §02: `x` is const and can't be modified or reassigned.
pub const CONST_ASSIGN: &str = "E0201";
/// Provisional §02: a value doesn't have the type that's expected.
pub const MISMATCH: &str = "E0250";
/// §02: a non-nullable variable needs a value.
pub const LET_NO_VALUE: &str = "E0203";
/// §02: no type and no value.
pub const NO_TYPE_NO_VALUE: &str = "E0204";
/// §02: const must be initialized.
pub const CONST_NO_VALUE: &str = "E0205";
/// Provisional §02: a variable can't hold `void`.
pub const VOID_VALUE: &str = "E0252";

/// §03: can't add string and int.
pub const STRING_PLUS: &str = "E0301";
/// §03: `++` is a statement.
pub const INCDEC_EXPR: &str = "E0303";
/// §03: no prefix form.
pub const INCDEC_PREFIX: &str = "E0304";
/// Provisional §03: an operator doesn't apply to these operand types.
pub const BAD_OPERANDS: &str = "E0350";

/// §06: missing return.
pub const MISSING_RETURN: &str = "E0601";
/// Provisional §06: wrong number of arguments.
pub const ARITY: &str = "E0650";
/// Provisional §06: `return` with a value in a `void` function.
pub const RETURN_IN_VOID: &str = "E0652";
/// Provisional §06: `return;` in a function that returns a value.
pub const RETURN_NO_VALUE: &str = "E0653";
/// Provisional §06: a `never` function that can return.
pub const NEVER_RETURNS: &str = "E0654";
/// Provisional §06: an exported function without a declared return type.
pub const EXPORT_NEEDS_TYPE: &str = "E0655";
/// Provisional §06: no `main`, for `dat run` and `dat build`.
pub const NO_MAIN: &str = "E0660";
/// Provisional §06: `main` has parameters or doesn't return `void`.
pub const BAD_MAIN: &str = "E0661";

/// §07: condition must be bool.
pub const NOT_BOOL: &str = "E0701";
/// Provisional §07: `break` or `continue` outside a loop.
pub const OUTSIDE_LOOP: &str = "E0750";

/// Provisional §08: a struct literal is missing a field.
pub const MISSING_FIELD: &str = "E0850";
/// Provisional §08: a struct has no field of that name.
pub const UNKNOWN_FIELD: &str = "E0851";

/// Provisional §12: only errors can be thrown.
pub const THROW_NON_ERROR: &str = "E1250";
/// Provisional §12: a `catch` type that isn't an error type.
pub const CATCH_NON_ERROR: &str = "E1251";
/// Provisional §12: a catch-all clause that isn't the last one.
pub const CATCH_ALL_NOT_LAST: &str = "E1252";

/// §14: `y` is not defined.
pub const UNDEFINED: &str = "E1401";
/// §14: `x` is already declared.
pub const REDECLARED: &str = "E1402";

/// Provisional §20: two tests in one module have the same name.
pub const DUPLICATE_TEST: &str = "E2050";
