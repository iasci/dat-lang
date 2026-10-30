# M1 plan: the core language

M1 finishes the parts of dat that work on single values: numbers, text and `bool`, with the functions, control flow, built-ins and tests around them. In the [roadmap](https://claude.ai/artifact/JNuKMm6CfFmLtBKFjjF4hG) it delivers §01 (the primitive types), §02, §03, §04, §06, §07, §09, §13, §14 and §20. Lists, sets and maps come in M2; structs, nullable types and `match` in M3; declared errors in M4.

M0 already built most of what the roadmap listed for M1. What's left is floats, the methods on numbers and strings, the first standard library module (`@dat/math`), default parameter values, finding every syntax error in one run, and one wrong fix. All of it is now built; [M1 status](./M1%20status.md) checks it against the exit criteria.

## Already done in M0

From the roadmap's list for M1:

- The parser reads every construct in the spec, at all precedence levels.
- `int` is Rust's `i64`, and overflow and `% 0` throw a `RangeError`.
- Return types are inferred, except on exported functions (E0655), and a path that can end without a `return` is reported (E0601).
- `assert` shows both sides of a failed comparison.
- Errors with fixes for a condition that isn't a `bool` (E0701), `"a" + 1` (E0301), `++i` (E0304), `++` inside an expression (E0303), and `===` and `!==` (E0001).
- `dat fix` applies the safe fixes.
- `string` is `dat_rt::Str`, and `s += t` appends in place.
- `test` blocks run in both modes. 1,000 small tests take about 10 ms, against the roadmap's target of 60 ms.
- The interpreter runs `fib(30)` faster than CPython, which was M1's stretch goal.

## What the spec can prove

The spec ratchet barely moves in M1. Of the 176 spec cases that can't run yet, M1 alone unblocks about three, because almost every example also uses a list, a struct or a nullable type. M2 unblocks about 57 more and M3 about 64. So M1 proves itself the way M0 did: with its own example programs and goldens in `examples/`.

## What M1 builds

In this order, since most steps need floats. A step is done when it parses, reproduces the spec's ❌ lines, gives the same output in both modes, and stays inside the speed budgets.

1. **Floats.** `float` becomes Rust's `f64`. A literal like `19.99` stays a float; exact decimals are opt-in and come after M1 ([Money and decimals design](./Money%20and%20decimals%20design.md)).
   - Literals such as `3.14`, `2.0` and `1e9`. An integer literal can stand where a float is expected (`const ratio: float = 1`); an `int` variable needs `.toFloat()`.
   - Arithmetic with a float gives a float, and `/` always does. `total /= 4` on an `int` is E0302, with its two options. Division by zero gives `Infinity`, `-Infinity` or `NaN`, as IEEE 754 says. An `int` can be compared with a `float`.
   - Printing follows §04: the shortest text that reads back as the same number, always with a decimal point or an exponent, as in `2.0`, `0.30000000000000004`, `1e21` and `1.5e-7`. dat-rt builds it from the shortest digits Rust already computes, so no outside library is needed.
   - The interpreter gets float closures of their own, as `int` and `bool` have, so float code never goes through the generic value.
2. **Methods on numbers (§04).**
   - `int`: `abs`, `min`, `max`, `sqrt`, `pow`, `toFloat` and `toString`. `1000.sqrt()` gives a float; `10.pow(3)` gives an exact `int` and throws a `RangeError` when the result doesn't fit.
   - `float`: `floor`, `ceil`, `round`, `toInt`, `abs`, `sqrt`, `pow`, `min`, `max`, `toFixed`, `isNaN` and `toString`.
   - Rounding throws a `RangeError` on `NaN` or a result outside `int`. `toInt()`, `floor()`, `ceil()` and `round()` applied to a division of two ints, as in `(a / b).floor()`, are computed in integer arithmetic, so they're exact for every `int`.
3. **Methods on strings (§04).**
   - `length`, `isEmpty`, `includes`, `startsWith`, `endsWith`, `indexOf`, `lastIndexOf`, `slice`, `trim`, `trimStart`, `trimEnd`, `toUpperCase`, `toLowerCase`, `replace`, `replaceAll`, `repeat`, `padStart`, `padEnd` and `toString`, and `s[i]`.
   - Three wait for later features: `split` returns a list (M2), and `toInt` and `toFloat` on a string throw a declared `ValueError` (M4). Going through a string's characters comes with `for...of` in M2.
   - Every method on numbers and strings exists once, in dat-rt.
4. **The first standard library module, `@dat/math`.** It has `PI`, `E`, and the number methods as functions, for code that prefers calling them: `floor(x)`, `sqrt(x)`, `max(a, b, c)` and the rest. This brings imports from `@dat/...`, by name or with `import * as math`; imports of files stay in M6. The module's interface is built into the `dat` binary, as the roadmap planned for M1, so nothing in it is checked at startup. A name the module exports, used without its import, gets E1401 with the import as its fix, and a path like `"math"` gets E1703 with `"@dat/math"` as its fix. `max(...scores)` waits for lists.
5. **Default parameter values**, as in `port: int = 5432`. A default can be any expression, including one that uses the parameters before it, and it's worked out each time a call leaves the argument out, as decided by the owner (§06). Rust has no default arguments, so each shorter way of calling becomes a small generated Rust function next to the original; a default can then use the module's private functions. A nullable parameter that defaults to `null` waits for nullable types (M3).
6. **Every syntax error in one run.** Today parsing skips to the next function after a syntax error, so a function with three mistakes reports one. M1 resumes at the next statement, without follow-on errors caused by the first mistake.
7. **A wrong safe fix.** `var x = 1;` gets "add `;`", which turns it into `var; x = 1;`. `dat fix` applies safe fixes without review, so the parser offers the `;` only where the code after it then parses.
8. **Integer literals that don't fit in an `int`**, such as `9223372036854775808`, are a compile error with a code of their own. `-9223372036854775808`, the smallest `int`, is valid. Today both are "not supported yet".
9. **Statements with no effect** are E0305, decided by the owner (§03). A line that's only a value with no call in it, such as `x;` or `total == 5;`, is an error; its fix removes it, or for `==`, turns it into `=` for review. A call can still drop its result. M0 accepts these lines today, so `examples/errors/statements.dat` changes.
10. **Speed.** `perf` gets a float program, shown against hand-written Rust and CPython. The aim is within about 4% of hand-written Rust, compiled. `perf` also shows how many lines of Rust each line of dat becomes, which the roadmap asks for from M1 so that later build budgets can be stated in dat lines.

## Exit criteria

The roadmap's exit check, which must pass in both modes:

```
test "division and remainder" {
  assert((7 / 2).floor() == 3);
  assert(10 / 4 == 2.5);
  assert(-7 % 3 == -1);
}
```

| # | Area | Passes when |
|---|---|---|
| 1 | Floats | An example covers every float rule in §01, §03 and §04, including the printing table, and its golden matches in both modes. |
| 2 | Number methods | Every method in step 2 is covered in both modes, including the exact integer results and each `RangeError`. E0302 is reproduced with both options. |
| 3 | String methods | Every method in step 3 is covered in both modes, including byte positions, negative `slice` positions and the `RangeError` for a position inside a multi-byte character. |
| 4 | `@dat/math` | An example imports it by name and with `import * as`, and gives the same output as the methods in both modes. E1401's import fix and E1703 are reproduced. |
| 5 | Default values | A call that leaves out a defaulted argument gets the default in both modes, and E0650 still counts arguments correctly. |
| 6 | Syntax errors | A function with three syntax errors gets exactly three diagnostics. |
| 7 | Safe fixes | Every safe fix in the examples, applied by `dat fix`, gives code that parses. |
| 8 | Literals | The too-large literal has its ❌ example and code, and the smallest `int` is accepted in both modes. |
| 9 | No-effect statements | E0305 is reproduced with its fix, and the spec case comes off the expected-failure list. |
| 10 | Spec ratchet | No case regresses, and the cases that needed only M1 pass. |
| 11 | Exit check | The test above passes in both modes. |
| 12 | Speed | The float program runs within about 4% of hand-written Rust, compiled. M0's gates and latency budgets still pass. |
| 13 | CI | All of the above pass on Linux and macOS. |

## Settled without asking

These follow from the spec or from earlier answers. Each can still be changed.

- **`int.MIN % -1` is `0`.** The result fits in an `int`, so nothing overflows. Rust's checked remainder reports an overflow here, so dat-rt handles that case itself. (Research question 9.)
- **Negative zero prints as `-0.0`.** §04's rule says a printed float reads back as the same number, and `0.0` would read back as positive zero.
- **An integer literal outside the `int` range is a compile error**, and `-9223372036854775808` is valid, as in Rust. (Research question 15.)
- **Case mapping, number parsing and number printing follow the dat version, not the machine.** They're written in dat-rt or come from Rust's standard library, and the toolchain is pinned. (Research question 17.)
- **Code from other languages gets no special errors.** It gets the same errors and fixes as any other mistake. (Research question 38.)
- **A Rust panic inside dat is an internal error with exit status 2**, as M0 already does. (Research question 4.)

## Left for later

- Lists, sets, maps, arrow functions, closures, `mut`, `for...of` and `range`: M2.
- Structs, nullable types, `??`, `?.`, unions, `match` and enums: M3.
- Declared errors, `try` expressions, `try?` and `finally`: M4. That's also when to decide whether `try?` applies to arithmetic (research question 10).
- Whether exported constants and arrow functions must declare their types: with arrow functions, in M2 (research questions 18 and 19).
- Whether a recursive function's return type can be inferred: today it must be declared (research question 20).
- Which tests run when some modules fail to check: with imports, in M6 (research question 31).
