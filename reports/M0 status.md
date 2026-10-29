# M0 status: fibonacci, end to end

**M0 is built. Of its 17 exit criteria, 14 pass on this machine, two fail and one hasn't run.** Compiled `fib(35)` runs at 1.33–1.51× hand-written Rust against a 1.2× gate; the cost is the bookkeeping that gives compiled code the same dat-only traces as the interpreter. The stack guard measured 0.97–0.98× before the final rebuild and 1.12× after it, against a 1.05× gate. The generated code didn't change, so this looks like code alignment on this VM, but the final build fails the gate. The CI workflow is written but has never run, because nothing has been pushed. Everything else in the [roadmap's M0](https://claude.ai/artifact/JNuKMm6CfFmLtBKFjjF4hG) exists as real code, which later milestones extend. That covers both run modes, the shared runtime, the diagnostic record, the five companion programs and the conformance runner over all 167 spec blocks. The work is about 8,800 lines of Rust with no external dependencies.

Measurements come from an 8-vCPU Intel Xeon at 2.2 GHz (a GCP VM) running Linux 7.0, with rustc 1.99.0. Times are wall time including process start, the median of 20 runs after 3 warmups.

## Exit criteria

| # | Component | Result | Evidence |
|---|---|---|---|
| 1 | Parser | **pass** | Parses every example. All 167 spec blocks parse and check with 0 panics (`dat-conformance spec` catches panics per block). Constructs outside M0 parse into `unsupported` nodes, so they get `E0002: not supported yet` instead of syntax errors. |
| 2 | Checker | **pass** | `fib.dat` is clean. `bad_fib.dat` gets exactly one error: E0301 at 10:9–10:31, bytes 145–167, as the report predicted. 10 in-subset ❌ lines are reproduced with their code, line and fix, and no extra diagnostics: E0203, E0204, E0205, E0301 (twice), E0303, E0304, E0601, E1401 and E1402. E0601 isn't an `xfail`: the spec's message wording isn't binding, so "missing return" plus the `return _;` fix matches. |
| 3 | Diagnostics | **pass** | Terminal and `--json` goldens in `examples/fib/bad_fib.golden`. The harness applies the JSON fix mechanically and gets `bad_fib.fixed`, as `dat fix` does. The fixed file checks clean and prints `fib(10) = 55` in both modes. |
| 4 | Typed IR | **pass** | `dat-conformance structure` reads `cargo metadata`: `dat-interp` depends only on `dat-ir` and `dat-rt`, and `dat-codegen` only on `dat-ir`. The `dat ir fib.dat` dump is a golden section. |
| 5 | Interpreter | **pass** | 27 example programs, 94 golden checks of stdout, stderr and exit status. |
| 6 | Codegen | **pass** | Generated crates carry `#![forbid(unsafe_code)]` and `#![deny(warnings)]`, so a warning fails the build. A short allow-list covers lints that flag valid dat, such as `unconditional_recursion` for a function meant to overflow. `structure` checks the generated Rust for `unsafe`, `RefCell`, `Any` and macro calls. All goldens match compiled. |
| 7 | Runtime | **pass** | The overflow check, every error message, `print`, `toString`, `assert` rendering, the uncaught-error printer, the test reporter and the stack guard exist once, in `dat-rt`. The overflow messages are byte-identical in both modes (`examples/semantics/int_errors.golden`). |
| 8 | Error model | **pass** | Overflow and the user-thrown `RangeError` are caught in both modes. `overflow_uncaught.dat` exits 1 with the report's exact trace, `at fibIter (overflow_uncaught.dat:9:18)` then `at main (overflow_uncaught.dat:17:9)`, in both modes. |
| 9 | Stack guard | **fail** | `deep.dat` catches `depth(100_000_000)` in both modes. The guard's cost on compiled `fib(35)` measured 0.97–0.98× in three runs before the final rebuild, then 1.12× (80–82 ms against 71–73 ms) after it, against a gate of ≤ 1.05×. The generated Rust didn't change; only dat-rt did. The unguarded binary got faster, while the guarded one stayed at 80–82 ms. So the swing looks like alignment, but this VM can't separate the guard's cost from that noise. The interpreter overflows between 160,000 and 320,000 nested calls, compiled code between 1.28 and 2.56 million, both above the 10,000 the spec guarantees. |
| 10 | `dat test` | **pass** | `0 failed, 4 passed` in both modes. `fib_fail.dat` prints the spec's `FAIL` block and exits 1 in both. |
| 11 | Differential harness | **pass** | `dat-conformance self-test`: with `DAT_FAULT=interp-plus-one`, the run differs from both the compiled run and the golden, and the harness reports it. |
| 12 | Spec extraction | **pass** | 167 of 167 blocks classified into 203 cases: 27 pass in both modes, 154 unsupported (with the reason), 22 doc-only, 0 fail. `conformance/spec-pass.txt` is the ratchet. |
| 13 | Interpreter-path latency | **pass** | `dat check fib.dat` 2.2 ms (budget 10), `dat test fib.dat` 2.6 ms (20), `dat run hello.dat` 2.3 ms (5). Starting the 256 MiB compiler thread added about 0.5 ms; before that the numbers were 1.6, 2.2 and 1.9 ms. |
| 14 | Compiled-path budget | **pass** | Warm `dat run --compiled fib.dat` 41 ms (budget 1 s), warm `dat test --compiled fib.dat` 40 ms (2 s). A cold run of the whole conformance suite, building dat-rt and about 40 generated crates, took 9.4 s. |
| 15 | Compiled speed | **fail** | `fib(35)`: 80–86 ms against 56–59 ms hand-written with `overflow-checks=on`, so 1.33–1.51× against the 1.2× gate (1.40× on the final build). See [the failing gate](#the-failing-gate). |
| 16 | Interpreter speed | **pass** | `fib(30)`: 315–326 ms against CPython 3.12's 208–225 ms, so 1.40–1.50× against the 2× gate. The M1 stretch goal of ≤ 1× isn't met yet. |
| 17 | CI | **not run** | `.github/workflows/ci.yml` runs build, unit tests, `fmt`, `clippy -D warnings` and `dat-conformance all` on Linux and macOS, plus a perf job. It needs a push to run, and the perf job will fail on criterion 15, and possibly 9. Nothing was run on macOS. |

## How to check it

```
cargo build --release
target/release/dat-conformance all     # examples, self-test, spec blocks, structure; both modes
target/release/dat-conformance perf    # criteria 9 and 13–16
cargo test --release
```

## The failing gate

Compiled code pays for exact traces on every call. A throw has to know the dat call chain. Rust has no `#line`, so generated code keeps the chain itself: each function builds a two-word `Frame` record (its caller's record and the call's location) and passes it to the functions it calls, and a throw walks the records. The variants measured on `fib(35)`, against 56–61 ms hand-written:

| Design | `fib(35)` | Ratio | Traces |
|---|---|---|---|
| Checked arithmetic and unwinding only | 59–61 ms | 1.0× | innermost location only |
| Plus the stack guard (stack pointer against a static) | 63–65 ms | 1.05× | innermost location only |
| Plus a drop guard per frame that records itself while unwinding (the first design) | 101–105 ms | 1.7× | exact |
| Plus a 3-word frame record passed as 2 extra arguments | 82–85 ms | 1.4× | exact |
| Plus a 2-word record built by the callee and the stack guard on its address (**built**) | 74–86 ms | 1.33–1.51× | exact |

Two findings surprised me. First, the drop guard's cost is mostly the landing pad, not the `panicking()` check. Second, reading the stack pointer next to the record stores cost 1 ns per call. Comparing the address of the caller's record, which arrives in a register, made the guard free.

The plan expected 1.0× because its measurement had no trace bookkeeping, and the spec asks for "each call that led there" in every mode. The options are for you to choose:

1. **Accept about 1.4× for M0.** `fib` is the worst case: a function that does almost nothing but call. Revisit at M8 together with range analysis.
2. **Let release builds print only the innermost frame** (Q7), keeping exact traces in `dat run --compiled` and `dat test --compiled`. This measured 1.05×, but `dat build` output would no longer show each call, which §12 asks for.
3. **Rebuild traces from native backtraces at the throw.** Map Rust frames to dat functions and call sites with a table the generator writes. This is zero-cost until a throw and keeps exact traces, but it depends on debug info and symbolization on each platform. A probe on this machine found it workable on Linux: `std::backtrace` with `debug = "line-tables-only"` reported every call site's line and column in the generated `main.rs`, including frames inlined into their callers. Capturing took about 40 µs at the throw, and symbolizing took about 9 ms when the trace is printed. Neither macOS nor very deep stacks were tried. I estimate a few days of work.

## Review

A separate review agent probed the build with about 140 programs, comparing the modes. It found seven bugs. All seven are fixed, and each now has an example with a golden:

- A statement that is only a value (`x;`, `s < "t";`) made the generated Rust fail rustc's lints. Discarded values are now emitted as `let _ = ...;`. Whether dat should allow them is still Q16.
- A `never` value in a string slot, such as `assert(false, fail("boom"))`, made the checker drop the whole statement in both modes. A `never` expression now takes the type of the place it's used in, through a `Diverge` IR node, as §01 says it can.
- A `never` function that could reach its end was accepted, and the modes then differed. It is now E0654.
- A parameter of type `never` or `void` compiled to Rust that rustc rejects. It is now E0252.
- A syntax error inside `main` also reported "no `main`", with a fix that added a second one. E0660 now appears only when nothing else is wrong.
- Nesting thousands of levels deep crashed rustc, and 20,000 levels crashed `dat` itself. Now `dat` runs the compiler on a 256 MiB stack, rustc gets the same, and more than 8,000 levels is a syntax error. 7,000 nested parentheses run in both modes.
- `export function f(x: int) { ... }` was accepted without a return type, which §06 requires. It is now E0655, with the inferred type as a safe fix.

The review also changed two smaller behaviours. A `let` that redeclares a parameter is now E1402, as in TypeScript. A failed `assert` with a message now shows `message:` in its `FAIL` block. The review's last suspicion stays open: generated crates are keyed by path, not content, so two concurrent `--compiled` runs of the same file share a crate directory.

## Decisions made while building

These are implementation choices. None of them changes the spec.

- **Traces from frame records in both modes, not a thread-local trace filled by drop guards.** The measurements are above. The interpreter builds the identical trace while an error propagates. Traces keep their 32 innermost and 32 outermost frames and count the rest (`... 1234 more calls`).
- **Unwinding carries the trace inside the error.** No thread-local state remains, so `catch` is `catch_unwind` plus a downcast, and a Rust panic (a dat bug) passes through every dat `catch` and exits with status 2 as an internal error (the interim answer to Q4).
- **The interpreter has typed fast paths**: `int` and `bool` expressions evaluate without the generic `Value`, which took `fib(30)` from 422 ms to 315 ms. It still walks the IR; closure compilation stays the M1 upgrade if the stretch goal matters.
- **One generated crate per entry file and mode**, in `target/dat-cache`, all sharing one target directory, so dat-rt builds once and an unchanged program is a Cargo no-op. Files are rewritten only when their content changes. The toolchain is pinned to 1.99.0, the one in `rust-toolchain.toml`.
- **Tests run sequentially on one large-stack thread** (decision 16 says sequential in M1). Each test's output is captured, and the report goes through dat-rt in both modes.
- **`dat fix` exists already.** The harness needed fix application anyway. It applies the preferred safe fix of each diagnostic, re-checks, and repeats up to four rounds.
- **Parser recovery works per declaration.** After a syntax error, parsing resumes at the next top-level declaration, so a file with three broken functions gets three errors. Inside a declaration it stops at the first one, as the plan allowed for M0.
- **The checker infers unannotated return types on demand.** A recursive function needs an annotation (the interim answer to Q20) and gets an `E0002` that says so.
- **The extractor needs one prelude** (`const name = "Ada";` for a block in §03). Spec blocks with a `}` as their ❌ line, like `sign` in §06, run only their ❌ case.

## Questions for the owner

These are new; questions 1–39 of the research report still stand.

| # | Question | What M0 does meanwhile |
|---|---|---|
| 40 | Compiled speed: which option in [the failing gate](#the-failing-gate)? | exact traces everywhere, at about 1.4× on `fib(35)` |
| 41 | Approve the provisional codes in [`CODES.md`](../crates/dat-check/CODES.md): E0001 syntax error, E0002 not supported yet, and 17 codes ending in 50–99 for rules the spec states without a ❌ line (type mismatch E0250, arity E0650, `break` outside a loop E0750, throwing a non-error E1250, and others) | uses them; they may be renumbered |
| 42 | The test summary when nothing fails: `0 failed, 4 passed (2 ms)` or `4 passed (2 ms)`? The spec shows only `1 failed, 0 passed` | `0 failed, 4 passed (2 ms)` |
| 43 | `--verbose` lines for passing tests: `PASS fib.dat:41  small values`, where 41 is the line of the `test` | as shown |
| 44 | When a test fails with an error other than an `assert`, its `FAIL` block shows the error and the frames below the test (`at helper (tests.dat:8:10)`), and the `FAIL` line names the test's own line. Is that the format? | as described |
| 45 | The message of `StackOverflowError` | `too many nested calls` |
| 46 | `assert(cond, message)`: is `e.message` the given message, and `assertion failed: <condition as written>` without one? | yes to both |
| 47 | The overflow message with a negative right operand: `-9223372036854775808 + -1`, following the `<a> <op> <b>` template literally, or `+ (-1)`? | `+ -1` |
| 48 | The E0301 fix for a longer chain: `"Total: " + count + 1` becomes `` `Total: ${count}${1}` ``, which keeps the TypeScript meaning ("Total: 31"). Is that what agents should see? | as shown |
| 49 | `dat run` on a file without `main` gets E0660 with a fix that adds an empty `main` at the end. Should `dat check` warn when a file has neither `main` nor tests? | no warning |

## What M1 builds on

- **The parser** already accepts lists, maps, arrow functions, `match`, member calls, `?.`, `??`, `is`/`as`, generics and struct declarations, so M1 adds checker and IR support without new syntax work.
- **The diagnostic record, codes and fixes** are in their final schema. M1's TypeScript-habit diagnostics (`===` is already one) follow the same pattern.
- **`dat_rt::Str`** is the opaque string type the plan recommends: `Rc<String>` with append in place. The small-string representation can replace it without touching generated code.
- **The interpreter's typed fast paths** extend to `float`.
- **The spec ratchet** turns M1's features into passing cases automatically, because the extractor already reads expressions with expected values (`10 % 3 // 1`) and `// throws` lines.
