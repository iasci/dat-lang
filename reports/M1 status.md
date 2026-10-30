# M1 status: the core language

**M1 is built, and all 13 of its exit criteria pass: 12 on the reference machine, and CI on Linux and macOS.** dat now has floats, the methods of numbers and strings, the `@dat/math` module with imports from `@dat/...`, default parameter values, every syntax error in one run, and the M1 rules for literals and statements. Compiled float code runs at 1.01–1.03× hand-written Rust, inside the 4% aim. Interpreted, it runs at 0.79–0.81× CPython. A probe of about 230 programs in both modes found one difference between them and seven checker and parser bugs, all fixed (see [The probe](#the-probe)). Everything is built from the [M1 plan](./M1%20plan.md), on top of M0. It adds about 2,200 lines of Rust, for about 13,100 in all, and no outside dependency.

Measurements come from the reference machine named in the [M0 report](./M0%20status.md): an 8-vCPU Intel Xeon at 2.2 GHz (a shared GCP VM), Linux 7.0, rustc 1.99.0. Times are the median of 20 runs after 3 warmups (40 for the compiled comparisons), wall time including process start, with the programs compared run interleaved.

## Exit criteria

| # | Area | Result | Evidence |
|---|---|---|---|
| 1 | Floats | **pass** | `examples/numbers/floats.dat` covers every float rule of §01, §03 and §04: literals, an integer literal where a float is expected, mixed arithmetic, `/`, IEEE division by zero, `NaN != NaN`, exact int-to-float comparison, compound assignment, and the printing table (`2.0`, `0.30000000000000004`, `1e21`, `1.5e-7`, `-0.0`, `NaN`, `Infinity`, the largest and smallest floats). The golden matches in both modes. `examples/numbers/float_assert.dat` shows a failed float comparison. |
| 2 | Number methods | **pass** | `examples/numbers/number_methods.dat` covers every method of `int` and `float` in both modes. The exact cases: `(9007199254740993 / 1).floor()` is `9007199254740993`, `10.pow(3)` is `1000`, halves round up. Each `RangeError` is reproduced: `int.MIN.abs()`, `pow` overflow and a negative exponent, `NaN` and out-of-range rounding, `toFixed(101)`, exact division by zero and `int.MIN / -1`. E0302 is reproduced with both options in `examples/errors/numbers.dat` and in the spec case `03-operators.md:55#E0302@59`. |
| 3 | String methods | **pass** | `examples/text/string_methods.dat` covers every method of step 3 in both modes, including byte positions in `"héllo"`, negative and clamped `slice` positions, `s[i]`, `in`, and the `RangeError` for a position inside `é`. |
| 4 | `@dat/math` | **pass** | `examples/numbers/math.dat` imports it by name and with `import * as math`, and prints the same results as the methods. E1401's import fix is reproduced in `examples/errors/imports.dat` and `first_import.dat`, whose `.fixed` files `dat fix` and the JSON fixes both produce, and which then run. E1703's fix (`"math"` to `"@dat/math"`) is in `imports.dat`. |
| 5 | Default values | **pass** | `examples/semantics/defaults.dat`: calls that leave out arguments get the defaults in both modes, each default is worked out at each call, a default reads the parameters before it, and an error in a default is traced to its function in both modes. `examples/errors/defaults.dat` shows E0650's counts (`takes 1 to 3 arguments`), and the spec's two E0650 cases still pass. |
| 6 | Syntax errors | **pass** | In `examples/errors/syntax_recovery.dat`, `three` has three syntax errors and gets exactly three diagnostics, with nothing reported that follows from them. A broken `let` keeps its name, so its later uses aren't reported as undefined, and a function with a syntax error isn't reported as missing a `return`. A function whose header has a syntax error is still declared, and its body is still read for errors, so `pair(1, 2)` isn't reported. `if (ready {` reports the missing `)` once, instead of reading `ready { ... }` as a struct literal. |
| 7 | Safe fixes | **pass** | The examples runner now applies each example's safe fixes, in rounds as `dat fix` does, and checks that they add no syntax error; all 55 examples pass. `var c = 1;` no longer gets the `;` fix (`syntax_recovery.dat`), and `examples/errors/semicolons.dat` shows the fix where it's right, through `.fixed`. |
| 8 | Literals | **pass** | E0150 has its ❌ example in §01, and `01-builtin-types.md:114#E0150@117` passes, as does the block's positive case, which runs `-9223372036854775808` in both modes. |
| 9 | No-effect statements | **pass** | E0305 is reproduced with its fix (`03-operators.md:68#E0305@70`), and the case is off the expected-failure list. `examples/errors/statements.dat` shows the other shapes. |
| 10 | Spec ratchet | **pass** | 76 of 272 cases pass in both modes, up from 64, with no regression. Two §01 and §06 blocks moved when the new ❌ examples were added; every case that passed before passes at its new line. The cases still blocked need a later milestone: lists, nullable types, imports of files, or `@dat/tasks`. |
| 11 | Exit check | **pass** | `examples/numbers/exit_check.dat` is the roadmap's test, and passes in both modes. |
| 12 | Speed | **pass** | Float code (`bench/floats.dat`, the Mandelbrot set on a 400×400 grid): 26.8 ms against 26.1 ms hand-written, 1.03×, against a gate of 1.04×. Three runs on the same day, with byte-identical generated code, gave 1.01×, 1.02× and 1.03×, so the margin on this shared VM is thin. M0's gates and budgets still pass; see [Speed](#speed). |
| 13 | CI | **pass** | `.github/workflows/ci.yml`, unchanged, ran everything above on Linux and macOS for the `m1` branch, and the speed job on Linux, whose limits are doubled on a shared runner. All three jobs passed. |

## How to check it

```
cargo build --release
target/release/dat-conformance all     # examples, self-test, spec blocks, structure, concurrency; both modes
target/release/dat-conformance perf    # latency budgets and speed gates
cargo test --release
```

## Speed

| Measurement | Result | Limit |
|---|---|---|
| `dat check fib.dat` | 2.2 ms | 10 ms |
| `dat test fib.dat` | 2.7 ms | 20 ms |
| `dat run hello.dat` | 2.5 ms | 5 ms |
| `dat run --compiled fib.dat`, warm | 49.6 ms | 1 s |
| `dat test --compiled fib.dat`, warm | 47.4 ms | 2 s |
| Compiled `fib(35)` against hand-written Rust | 1.12× | 1.2× |
| Compiled float code against hand-written Rust (new) | 1.03× | 1.04× |
| Interpreted `fib(30)` against CPython | 0.76× | 2× |

Shown but not judged: the stack guard costs 1.10× on `fib(35)`; Ackermann runs at 1.98×, mutual recursion at 1.04×, the `primes` loop at 1.01× and the `rows` string building at 0.76× hand-written Rust; interpreted, the loop runs at 0.41×, the string building at 0.54× and the float code at 0.79× CPython.

Every change after the first measurement was checked against the generated Rust as well as `perf`: the standard library refactor and the probe fixes leave the Rust for every benchmark byte-identical. The one fix that adds code, the stack check in a default's wrapper, adds it only to wrappers of recursive functions whose defaults make a call.

`perf` now also shows how many lines of Rust each line of dat becomes, so later build budgets can be stated in dat lines. Over the six benchmarks, 107 lines of dat become 346 lines of Rust, 3.2 per line. Short recursive functions come out highest (4.1 for `fib35`), and the float loops lowest (2.6). The count leaves out blank lines and comments, and includes the per-crate parts: the location statics, the site table and `main`.

The generated float code is the Rust a person would write: `let l6_xx: f64 = (l3_x * l3_x);`, with no calls into dat-rt.

## How it's built

- **Floats.** `float` is `f64` throughout: `Type::Float` in the IR, `Value::Float` in the interpreter, `f64` in generated Rust. Float arithmetic is its own IR node (`FloatArith`), which never throws, so generated code is plain Rust operators. The interpreter compiles float expressions to closures that return `f64`, with closures of their own for a local and a constant, two locals, and comparisons, as for `int`.
- **Mixing ints and floats.** In arithmetic, an `int` operand converts to a float (an integer literal becomes a float constant at compile time). Elsewhere only an integer literal converts, as §01 says: an `int` variable passed where a float is expected is E0250, with `x.toFloat()` as its safe fix. An `int` and a `float` compare by their exact values through `dat_rt::num::cmp_int_float`, so `9007199254740993 > 9007199254740992.0`. A literal a float holds exactly (up to 2^53) is compared as a float, since that's exact too and faster.
- **Printing floats.** `dat_rt::fmt::write_float` takes the shortest digits from Rust's `{:e}` and lays them out as §04 says. It writes into a stack buffer, so printing doesn't allocate.
- **One node for every method.** Each method of a number or a string, and each standard library function, is one `ExprKind::Builtin` with an operation from `dat_ir::Builtin`. The IR gives each operation's signature, whether it throws, and the name of its one implementation in `dat_rt::num` or `dat_rt::text`. The interpreter calls the `try_` form, and generated code calls the throwing form with its location, as for `int` arithmetic. The checker has one table of methods, and `@dat/math`'s table names the same operations, so a function and its method can't differ. Which rounding has an exact form for `a / b` is a property of the operation (`Builtin::on_int_division`), so methods and functions both get it without matching on names.
- **Exact division.** When `floor`, `ceil`, `round` or `toInt` (or `trunc` in `@dat/math`) is applied to `a / b` with two ints, parentheses aside, the checker lowers it to `DivFloor` and the others, which dat-rt computes in 128-bit integers. A division by zero there is a `RangeError` (`int division by zero: (7 / 0).floor()`), where the float division would give `Infinity` and then fail to convert.
- **`@dat/math` and imports.** The parser reads `import { a, b as c } from "..."` and `import * as m from "..."`. The standard library's interfaces are a table in `dat-check/src/stdlib.rs`, so nothing is read at startup. Each module there lists its members: a constant, a function, or a name a later milestone implements. A function lists its forms, each an operation from `dat_ir::Builtin`, and a call uses the first form that takes its argument types, where a `float` parameter also takes an `int`. A function can take any number of arguments and combine them left to right, as `max` does. The checker has no code for any one module: imports, `ns.member`, calls, E1401's import fix, E1701 and E1703 all read the table, and its consistency is a unit test. A new module is a new table entry plus its operations in dat-rt; the backends already handle any `Builtin`. `@dat/math` is implemented; `@dat/tasks` lists its names only, so a use of one gets the import as a fix. Imports of files and packages are E0002 until M6.
- **How the standard library grows past M1.** The table is the M1 form of roadmap decision 10 (a precompiled interface embedded in the binary). Once files can import files (M6), modules written in dat can get their interface entries from their own declarations instead of by hand, and only operations that need Rust stay as `Builtin`s. Each `Builtin` costs a variant, a signature, a dat-rt function and an interpreter arm. That's fine for the primitive operations on numbers and strings, but not for dozens of modules, so it shouldn't become the way every module is built.
- **Default values.** The IR keeps each function's defaults as expressions over the parameters before them. The interpreter works them out in the callee's frame when a call leaves arguments out. Generated Rust gets a function for each shorter way of calling, such as `f1_connect_with1(l0_host)`, which works out the defaults and calls the full function. Its call isn't a trace site, so it never shows in a trace, and an error inside a default is traced to the function the default belongs to, as in the interpreter.
- **Syntax errors.** A statement that fails to parse becomes `StmtKind::Error`, and parsing resumes at the next `;` or `}` at its own depth, or at a statement keyword that starts a line. A `let`, or a word followed by a name and `=`, keeps the name with an unknown value. `===` and a missing `;` that has its fix don't stop the statement: parsing goes on as if the fix were applied, so nothing is lost. A function that had a syntax error isn't checked for a missing `return`.
- **The `;` fix.** It's offered only when the token after the gap starts a new line or closes the block, and the code from there parses: as a statement, or as a declaration when it starts one. The parser tries that parse and undoes it. So `return 1 }` gets the fix, and `var x = 1;` and a stray `function` don't.
- **Broken function headers.** A syntax error after a function's name records the name, then the parser skips to the body's `{` and reads the body for its syntax errors. The checker treats a recorded name as declared, so calls to it aren't reported again.

## The probe

An agent wrote about 230 programs against the M1 features and ran each in both modes, with `check --json`, `ir`, `rust` and `dat fix` on copies. The things a probe is most likely to break all held: the shortest digits of 78 printed floats, checked against an independent implementation, exact `(a / b).floor()` and the others on 176 pairs of ints against Python, `toFixed` ties against Python's `Decimal`, byte positions and clamping in `slice`, and traces through defaults with tabs, CRLF and multi-byte text. Nothing panicked, and no generated Rust failed to compile. It found:

| Bug | Fix |
|---|---|
| Recursion through a default (`d: int = f(n + 1)`) overflowed the stack in compiled code, uncatchably; the interpreter threw `StackOverflowError` | A default's wrapper gets the stack check when its function is recursive and a default calls (`examples/semantics/defaults.dat`) |
| `PI = 3.0` said `PI is not defined` and offered to import `PI` again, which `dat fix` repeated | E0201: `` `PI` is imported and can't be reassigned ``; functions likewise (`examples/errors/import_errors.dat`) |
| A syntax error in a function header left E1401 at every call | The name is recorded and treated as declared (`syntax_recovery.dat`) |
| `if (ok {` read `ok { ... }` as a struct literal and left the function | `Name {` is a struct literal only when a field list follows (`syntax_recovery.dat`) |
| `0x` and `0b2` got E0150, "doesn't fit in an int" | A syntax error naming the bad digit (`examples/errors/literals.dat`) |
| `1 in "abc"` said the right side should be a string | `` `in` a string needs a string on its left, not an int `` (`examples/errors/types.dat`) |
| A stray `function` after a missing `;` got the `;` fix, which didn't parse | A declaration after the gap must parse too (`examples/errors/syntax.dat`) |
| An unclosed comment or template quoted the rest of the file | The error points at the opening (`unclosed_comment.dat`, `unclosed_template.dat`) |

Its smaller findings that were fixed are under the decisions below. Left as they are:

- **Errors from `@dat/math` functions name the method**: `trunc(1e300)` throws `int overflow: 1e300.toInt()`. The message comes from the one implementation the function and the method share; naming the function needs the call's spelling carried to dat-rt.
- **`min()` and `max()` with no arguments** are E0650 at compile time. §04's `RangeError` for no values is for a spread of an empty list, which comes with lists.
- **Fixes not offered yet**: `!("a" in s)` for `!"a" in s`, `(-9223372036854775808).abs()` for the unparenthesized form, and the template fix for `s += 1.5`. `let x: flot` is E0002 for a user-defined type rather than a suggestion of `float`.
- **`2.pow(0.5)`** gets the options `0.5.floor()` and `0.5.round()`, which compile but likely aren't meant; `2.0.pow(0.5)` is.

## Decisions made while building

These are implementation choices. The ones that state behaviour the spec didn't pin down are now in the spec's tables, written into the cells that were there, so no spec block moved.

- **`float.min` and `max` keep `NaN`**: if either side is `NaN`, so is the result, and `-0.0` is smaller than `0.0`. Rust's `f64::min` ignores `NaN`, which hides it.
- **`toFixed` rounds the float's exact value.** A tie is a float whose exact value ends in a 5 at the next place, such as 0.125, and it goes away from zero (`"0.13"`). `1.005.toFixed(2)` is `"1.00"`, because the float is just below 1.005. `digits` must be from 0 to 100. `NaN` and the infinities give their `toString()`. `(-0.0).toFixed(2)` is `"-0.00"`, consistent with `-0.0` printing as `-0.0`.
- **`slice` checks both positions**, even when `start` is past `end` and the result is empty: `"héllo".slice(4, 2)` throws, because 2 is inside `é`.
- **`padStart` and `padEnd` never split a character of `fill`**, so with a multi-byte fill the result can be shorter than `length`. An empty fill leaves the string as it is.
- **`repeat` and the padding methods throw a `RangeError`** for a negative count, or a result that can't be allocated (`string too long: 2 bytes repeated 9223372036854775807 times`), instead of the allocation failure that would end the program.
- **`(a / b).floor()` on ints by zero** throws `int division by zero` rather than the float path's `int overflow: Infinity.floor()`. Both are `RangeError`s.
- **`math.floor(n)` on an `int` is `n`**, exactly, rather than converting it to a float and back, which would round integers above 2^53.
- **A failed `assert(x == 3)` with a float `x`** shows `right: 3`, as written, not `3.0`.
- **E0302 also covers `+=`, `-=`, `*=` and `%=`** with a float on the right of an `int` variable. Its two options are to round the result down, or to make the variable a float where it's declared: `0` becomes `0.0`, a type `int` becomes `float`, and another value gets `.toFloat()`.
- **E0305's fixes are both `review`.** `total == 5;` becomes `total = 5;`, as the spec says. For the other shapes, the fix removes the statement, also as review: the statement most likely stands for a mistake, such as `total + 5;` written for `total += 5;`, and removing it without a look would hide that.
- **E0150 also covers a float literal too large for a float**, such as `1e400`, which would otherwise be `Infinity`.
- **A new code, E0656**, for a parameter without a default after one with a default, such as `function between(low: int = 0, high: int)`: no call could leave out `low` and give `high`. It has a ❌ example in §06.
- **`in` with a string on the right works** (§03): `"lo" in "hello"`. The item is evaluated first.
- **Several missing names from one module get one import fix.** Two fixes that each add an import at the same place would conflict when applied together. So the first use's fix imports all of them, and the others say which line's fix imports them.
- **`Math.floor(x)` is no longer a special case.** M0 reported `Math` as "not supported yet", which wrongly marked it as valid dat. Now `Math` is an undefined name like any other (E1401), so code from other languages gets the same errors and fixes as any other mistake (AGENTS.md). A rule that offered to import `floor` for any undefined `X` before `.floor(...)` was tried and dropped: it also fired on a misspelled variable such as `totl.floor()`.
- **An unknown member gets a similar name only when it's close**: one edit for names under six letters, two for longer ones, or the same name in another case (`toUppercase` → `toUpperCase`). `s.length()` gets the safe fix `s.length`, but not `s.length(1)`, whose argument the fix would drop. Standard library names use the same rule: `import { flor }` and `math.Pi` get `floor` and `PI`.
- **Review fixes are offered only where they compile.** `cap == 4;` with a `const cap` gets "remove the statement" rather than `cap = 4;`, `total == total;` doesn't become `total = total;`, and E1402 on a `const` no longer offers to assign to it.
- **A method used as a value** (`s.trim` without a call) is E0002 until function values come in M2. `s[i] = ...` is E0350.
- **An import after a declaration** is a syntax error without a fix. The import is still read, so its names don't cause more errors.
- **An unknown `@dat/` module** is E0002, like the known ones this milestone doesn't implement, unless it's close to a module the standard library has: `"@dat/maths"` is E1703 with `"@dat/math"` as a review fix. `"@std/math"` is E1703 with the safe fix `"@dat/math"`, since §17 keeps `std` from every package owner.
- **Assigning to an import or a function** is E0201, like a `const`: `` `PI` is imported and can't be reassigned ``. It used to say `PI is not defined` and offer to import `PI` again, which `dat fix` repeated without end.
- **A literal that isn't a number** (`0x`, `0b2`, `0xfg`) is a syntax error that says which digit is wrong, not E0150. An unclosed comment or template string is reported at its start, without quoting the rest of the file, and a UTF-8 byte order mark at the start of a file is skipped.
- **An integer literal in every branch of `?:`** is a float where one is expected: `const ratio: float = ready ? 1 : 2;`. So is `-(3)`, like `-3`.
- **The spec runner** now reads `expr // throws RangeError: message` on a bare expression line and float expected values (`10 / 4 // 2.5`). When a case's safe fix leads to something a later milestone implements, such as `@dat/tasks`, the case counts as unsupported rather than failing.

## What M2 builds on

- **`Builtin`** takes the list methods in M2 the same way: a signature in the IR and one implementation in dat-rt.
- **Imports** already parse every form. M6 adds files, and `@dat/tasks` needs only its implementation.
- **The default-value functions** are what nullable parameters need in M3: a nullable parameter is one whose default is `null`.
- **Statement-level recovery** works in every block, so later constructs get it for free.
