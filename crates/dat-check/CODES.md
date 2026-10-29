# Diagnostic codes

The first two digits name the spec section that defines the rule, as
`reference/README.md` says. Codes ending in 01–49 are the spec's own and come
from its ❌ lines. Codes ending in 50–99 are **provisional**: the compiler needs
them, but no ❌ line in `reference/` defines them yet. They need the owner's
approval before they're frozen, and may be renumbered until then.

| Code | Status | Rule | Fix |
| --- | --- | --- | --- |
| E0001 | general | Syntax error, including nesting deeper than 8000 levels | `;` inserted (safe); `===` → `==` (safe); bad escape (review) |
| E0002 | general | Not supported yet in this milestone | — |
| E0201 | spec §02 | `x` is const (or a parameter, or the caught error) and can't be modified or reassigned | `const` → `let` in the declaration (review) |
| E0203 | spec §02 | A non-nullable variable needs a value | options: `= _`, or a nullable type |
| E0204 | spec §02 | No type and no value | `let x = _;` (review) |
| E0205 | spec §02 | const must be initialized | `const x: T = _;` (review) |
| E0250 | provisional §02 | A value doesn't have the expected type | `${x}` for an int or bool where a string is expected (safe) |
| E0252 | provisional §02 | A variable or parameter can't have type `void` or `never` | — |
| E0301 | spec §03 | Can't add string and int (or bool, or an error) | a template string for the whole `+` chain (safe) |
| E0303 | spec §03 | `++` / `--` is a statement | `let y = i; i++;` (safe) |
| E0304 | spec §03 | No prefix form | `i++` (safe) |
| E0350 | provisional §03 | An operator doesn't apply to these operand types | — |
| E0601 | spec §06 | Missing return | `return _;` at the end of the function (review) |
| E0650 | provisional §06 | Wrong number of arguments | — |
| E0652 | provisional §06 | `return` with a value in a `void` function | `return;` (review) |
| E0653 | provisional §06 | `return;` in a function that returns a value | `return _;` (review) |
| E0654 | provisional §06 | A `never` function can return or reach its end | `fail(_);` at the end (review) |
| E0655 | provisional §06 | An exported function without a declared return type | the inferred type (safe) |
| E0660 | provisional §06 | `dat run` or `dat build` on a file without `main` | an empty `main` at the end (review) |
| E0661 | provisional §06 | `main` has parameters or doesn't return `void` | `: void` (review) |
| E0701 | spec §07 | Condition must be bool | `!= 0` for an int, `!= ""` for a string (safe) |
| E0750 | provisional §07 | `break` or `continue` outside a loop | — |
| E0850 | provisional §08 | A struct literal is missing a field | `message: _` (review) |
| E0851 | provisional §08 | No field of that name | — |
| E1250 | provisional §12 | Only errors can be thrown | — |
| E1251 | provisional §12 | A `catch` type that isn't an error type | — |
| E1252 | provisional §12 | A catch-all clause that isn't the last one | — |
| E1401 | spec §14 | `y` is not defined | move a declaration out of a block that ended (review); a similar name (review) |
| E1402 | spec §14 | `x` is already declared, in the same scope or as a parameter of the function | assign to the existing variable (review) |
| E2050 | provisional §20 | Two tests in one module have the same name | — |

The spec's other codes (E0101–E0103, E0202, E0206, E0302, E0501, E0602,
E0603, E0702, E0703, E0801, E0802, E1501, E1701, E1702 and E1801) belong to
constructs that later milestones implement: collections, floats, narrowing,
closures, nullable types, `mut` loops, structs, interfaces, imports and
enums. The conformance
runner reports their ❌ lines as `unsupported` until then.
