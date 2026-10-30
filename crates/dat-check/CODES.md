# Diagnostic codes

The first two digits name the spec section that defines the rule, as
`reference/README.md` says; `E00` codes are general (§21). Each code has a ❌
example in that section, which `dat-conformance spec` checks. Within a
section, the codes that came with the spec end in 01–49, and the ones the
compiler added, approved with their ❌ examples at M0, end in 50–99.

| Code | Status | Rule | Fix |
| --- | --- | --- | --- |
| E0001 | general §21 | Syntax error, including nesting deeper than 8000 levels | `;` inserted (safe); `===` → `==` (safe); bad escape (review) |
| E0002 | general §21 | Not supported yet in this milestone (no ❌ example: it marks valid dat) | — |
| E0201 | spec §02 | `x` is const (or a parameter, or the caught error) and can't be modified or reassigned | `const` → `let` in the declaration (review) |
| E0203 | spec §02 | A non-nullable variable needs a value | options: `= _`, or a nullable type |
| E0204 | spec §02 | No type and no value | `let x = _;` (review) |
| E0205 | spec §02 | const must be initialized | `const x: T = _;` (review) |
| E0250 | spec §02 | A value doesn't have the expected type | `${x}` for an int or bool where a string is expected (safe) |
| E0252 | spec §02 | A variable or parameter can't have type `void` or `never` | — |
| E0301 | spec §03 | Can't add string and int (or bool, or an error) | a template string for the whole `+` chain (safe); when two ints are next to each other after a string, two options: add them first, or join them as text |
| E0303 | spec §03 | `++` / `--` is a statement | `let y = i; i++;` (safe) |
| E0304 | spec §03 | No prefix form | `i++` (safe) |
| E0350 | spec §03 | An operator doesn't apply to these operand types | — |
| E0601 | spec §06 | Missing return | `return _;` at the end of the function (review) |
| E0650 | spec §06 | Wrong number of arguments | — |
| E0652 | spec §06 | `return` with a value in a `void` function | `return;` (review) |
| E0653 | spec §06 | `return;` in a function that returns a value | `return _;` (review) |
| E0654 | spec §06 | A `never` function can return or reach its end | `fail(_);` at the end (review) |
| E0655 | spec §06 | An exported function without a declared return type | the inferred type (safe) |
| E0660 | spec §06 | `dat run` or `dat build` on a file without `main` | an empty `main` at the end (review) |
| E0661 | spec §06 | `main` has parameters or doesn't return `void` | `: void`, and each `return x;` becomes `return;` (a returned call stays as a statement) (review) |
| E0701 | spec §07 | Condition must be bool | `!= 0` for an int, `!= ""` for a string (safe) |
| E0750 | spec §07 | `break` or `continue` outside a loop | — |
| E0850 | spec §08 | A struct literal is missing a field | the field with `_`, at the end of the literal (review) |
| E0851 | spec §08 | No field of that name | — |
| E1250 | spec §12 | Only errors can be thrown | — |
| E1251 | spec §12 | A `catch` type that isn't an error type | — |
| E1252 | spec §12 | A catch-all clause that isn't the last one | — |
| E1401 | spec §14 | `y` is not defined | move a declaration out of a block that ended (review); a similar name (review) |
| E1402 | spec §14 | `x` is already declared, in the same scope or as a parameter of the function | assign to the existing variable (review) |
| E2050 | spec §20 | Two tests in one module have the same name | — |

The spec's other codes (E0101–E0104, E0202, E0206, E0302, E0305, E0501, E0602,
E0603, E0702, E0703, E0801, E0802, E1501, E1701–E1703, E1801, E1802 and E2201–E2206) belong to
constructs that later milestones implement: collections, floats, narrowing,
closures, nullable types, `mut` loops, structs, interfaces, imports, enums
and concurrency. The conformance
runner reports their ❌ lines as `unsupported` until then.
