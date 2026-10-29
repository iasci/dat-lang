# M0 status: fibonacci, end to end

**M0 is built, and all 17 exit criteria passed in CI on Linux and macOS ([run 40864115409](https://github.com/iasci/dat-lang/actions/runs/40864115409)), including the speed gates.** Compiled code now reads its trace from the native stack when an error is thrown, instead of keeping bookkeeping on every call. That took compiled `fib(35)` from 1.40× hand-written Rust to 1.07×. The commits after that run read the stack with the `backtrace` crate instead of std, and need another CI run. Everything in the [roadmap's M0](https://claude.ai/artifact/JNuKMm6CfFmLtBKFjjF4hG) exists as real code, which later milestones extend. That covers both run modes, the shared runtime, the diagnostic record, the five companion programs and the conformance runner over all 179 spec blocks. The work is about 9,700 lines of Rust. Its one outside dependency is `backtrace`, under the rule in `AGENTS.md`.

Measurements come from two machines. "CI" is GitHub's `ubuntu-latest` runner. "Here" is an 8-vCPU Intel Xeon at 2.2 GHz (a shared GCP VM) running Linux 7.0, which is too noisy to judge a 5% gate, so the gates are judged in CI. Both use rustc 1.99.0. Times are wall time including process start, the median of 20 runs after 3 warmups.

## Exit criteria

| # | Component | Result | Evidence |
|---|---|---|---|
| 1 | Parser | **pass** | Parses every example. All 179 spec blocks parse and check with 0 panics (`dat-conformance spec` catches panics per block). Constructs outside M0 parse into `unsupported` nodes, so they get `E0002: not supported yet` instead of syntax errors. |
| 2 | Checker | **pass** | `fib.dat` is clean. `bad_fib.dat` gets exactly one error: E0301 at 10:9–10:31, bytes 145–167, as the report predicted. Every ❌ line inside the M0 subset is reproduced with its code, line and fix, and no extra diagnostics. The spec's message wording isn't binding, so E0601's "missing return" plus the `return _;` fix matches. |
| 3 | Diagnostics | **pass** | Terminal and `--json` goldens in `examples/fib/bad_fib.golden`. The harness applies the JSON fix mechanically and gets `bad_fib.fixed`, as `dat fix` does. The fixed file checks clean and prints `fib(10) = 55` in both modes. `.reviewed` files check review fixes the same way. |
| 4 | Typed IR | **pass** | `dat-conformance structure` reads `cargo metadata`: `dat-interp` depends only on `dat-ir` and `dat-rt`, and `dat-codegen` only on `dat-ir`. The `dat ir fib.dat` dump is a golden section. |
| 5 | Interpreter | **pass** | 34 example programs, 121 golden checks of stdout, stderr and exit status. |
| 6 | Codegen | **pass** | Generated crates carry `#![forbid(unsafe_code)]` and `#![deny(warnings)]`, so a warning fails the build. A short allow-list covers lints that flag valid dat, such as `unconditional_recursion` for a function meant to overflow. `structure` checks the generated Rust for `unsafe`, `RefCell`, `Any` and macro calls. All goldens match compiled. |
| 7 | Runtime | **pass** | The overflow check, every error message, `print`, `toString`, `assert` rendering, the uncaught-error printer, the test reporter and the stack guard exist once, in `dat-rt`. The overflow messages are byte-identical in both modes (`examples/semantics/int_errors.golden`). |
| 8 | Error model | **pass** | Overflow and the user-thrown `RangeError` are caught in both modes. `overflow_uncaught.dat` exits 1 with the report's exact trace in both modes, `at fibIter (overflow_uncaught.dat:9:18)` then `at main (overflow_uncaught.dat:17:9)`. A `dat build` executable moved to another folder prints the same trace (a CI step). |
| 9 | Stack guard | **pass** | `deep.dat` catches `depth(100_000_000)` in both modes. The guard's cost on compiled `fib(35)` is 1.04× in CI, against a gate of ≤ 1.05×; here it measured 1.02–1.16× across runs. The interpreter overflows between 160,000 and 320,000 nested calls. Compiled code overflows between 2.0 and 3.1 million simple calls, and between 128,000 and 192,000 of the largest kind measured (a function with a `try` and strings). The spec guarantees 10,000. |
| 10 | `dat test` | **pass** | `0 failed, 4 passed` in both modes. `fib_fail.dat` prints the spec's `FAIL` block and exits 1 in both. |
| 11 | Differential harness | **pass** | `dat-conformance self-test`: with `DAT_FAULT=interp-plus-one`, the run differs from both the compiled run and the golden, and the harness reports it. |
| 12 | Spec extraction | **pass** | 179 of 179 blocks classified into 234 cases: 55 pass in both modes, 156 unsupported (with the reason), 23 doc-only, 0 fail. `conformance/spec-pass.txt` is the ratchet. |
| 13 | Interpreter-path latency | **pass** | `dat check fib.dat` 0.8 ms in CI and 2.1 ms here (budget 10), `dat test fib.dat` 1.0 and 2.8 ms (20), `dat run hello.dat` 0.8 and 2.3 ms (5). |
| 14 | Compiled-path budget | **pass** | Warm `dat run --compiled fib.dat` 17 ms in CI and 49 ms here (budget 1 s); warm `dat test --compiled fib.dat` 17 ms in CI (2 s). The first compiled build on a machine takes about 10 s, up from about 1.2 s, because it builds `backtrace` and its dependencies once. |
| 15 | Compiled speed | **pass** | `fib(35)`: 42.8 ms against 40.1 ms hand-written with `overflow-checks=on`, so 1.07× in CI against the 1.2× gate; 1.08–1.20× here. It was 1.40× with the first trace design. |
| 16 | Interpreter speed | **pass** | `fib(30)` against CPython 3.12: 1.78× in CI and 1.38× here, against the 2× gate. The M1 stretch goal of ≤ 1× isn't met yet. |
| 17 | CI | **pass** | `.github/workflows/ci.yml` runs build, unit tests, `fmt`, `clippy -D warnings` and `dat-conformance all` on Linux and macOS, plus a perf job. Run 40864115409 passed all three jobs. The step that moves a `dat build` executable and checks its trace was added after that run, so it hasn't run on macOS yet. |

## How to check it

```
cargo build --release
target/release/dat-conformance all     # examples, self-test, spec blocks, structure, concurrency; both modes
target/release/dat-conformance perf    # criteria 9 and 13–16
cargo test --release
```

## The trace design

A throw has to know the dat call chain, because §12 asks for "each call that led there" in every mode, and Rust has no `#line`. The first M0 build kept the chain itself: each function built a two-word record (its caller's record and the call's location) and passed it to the functions it called. That cost 1.4× on `fib(35)`. The owner chose the third option (Q40): read the native stack at the throw and map it back to dat.

- **Generated functions are plain Rust**, such as `fn f0_fib(l0_n: i64) -> i64`.
- **Capture.** A throw that no running `try` handles records the return addresses on the stack with `backtrace::trace`, without resolving them. A thread-local count of the error types that running `try`s handle makes that check, so a caught error costs what it did before, about 4.6 µs.
- **Printing.** Each distinct address is resolved once with `backtrace::resolve`, inlined frames included. Its line and column in the generated `main.rs` are looked up in a `SITES` table that dat-codegen appends to the crate. Frames of dat-rt, std and the harness drop out. The 32 innermost and 32 outermost frames are kept and the rest counted (`... 1234 more calls`).
- **Optimizations that would lose or mislabel a frame are off** in generated crates:
  - Tail calls: `dat_rt::no_tail_call()` goes before a `return f(...)`. It is an empty barrier that compiles to no instructions.
  - Function merging, which named frames after the wrong function: `-Zmerge-functions=disabled`, allowed on the pinned stable toolchain by `RUSTC_BOOTSTRAP=1` (Q50).
  - Branch merging, which merged calls from two branches into one call with no usable location: SimplifyCFG's sink and hoist, and codegen tail merging, through stable `-C llvm-args`.
- **The stack guard** compares the address of a local with a limit static in the generated crate. It sits before the first statement that makes a call, so a recursion's base case skips it.
- **`dat build` keeps line tables.** On Linux they're zlib-compressed: the executable is 2.2 MB, against 0.4 MB without line tables, and printing a trace from it takes about 40 ms. On macOS they go in a `.dSYM` next to the executable. A stripped executable still runs but prints only the innermost frame.

The designs measured on `fib(35)`, against 56–61 ms hand-written here:

| Design | `fib(35)` | Ratio | Traces |
|---|---|---|---|
| Checked arithmetic and unwinding only | 59–61 ms | 1.0× | innermost location only |
| Plus the stack guard (stack pointer against a static) | 63–65 ms | 1.05× | innermost location only |
| Plus a drop guard per frame that records itself while unwinding | 101–105 ms | 1.7× | exact |
| Plus a 3-word frame record passed as 2 extra arguments | 82–85 ms | 1.4× | exact |
| Plus a 2-word record built by the callee, with the stack guard on its address (the first M0 build) | 74–86 ms | 1.33–1.51× | exact |
| The native stack read at the throw, plus the stack guard (**built**) | | 1.07× in CI, 1.08–1.20× here | exact |

Deep stacks decided the stack size. Unwinding costs grow with depth, so a bigger stack goes deeper but makes a caught overflow slower. Before the trace redesign, a caught overflow took 0.79 s and 65 MB at 64 MiB:

| | std's backtrace, 16 MiB | `backtrace`, 64 MiB | `backtrace`, 32 MiB (**built**) |
|---|---|---|---|
| Uncaught overflow, simple frames | 3.6–4.0 s, 613 MB | 2.5–2.8 s, 107 MB | 1.2 s, 58 MB |
| Uncaught overflow, the largest frames | 1.3 s, 166 MB | 1.9–2.0 s, 95 MB | 0.93 s, 51 MB |
| Caught overflow (`deep.dat`) | 0.37–0.42 s, 18 MB | 1.49–1.55 s, 68 MB | 0.75 s, 35 MB |
| Maximum depth, simple frames | 1.0–1.5 million | 4.1–6.1 million | 2.0–3.1 million |
| Maximum depth, the largest frames | 64,000–96,000 | 256,000–384,000 | 128,000–192,000 |

std's backtrace captures and resolves every frame, at about 3.5 µs and 500 bytes each, with no bounded API, which is why it was replaced (Q51). `backtrace` and its dependencies, pinned in `Cargo.lock`:

| Crate | Version | Maintainer |
|---|---|---|
| backtrace | 0.3.77 | rust-lang/backtrace-rs |
| addr2line | 0.27.1 | gimli-rs |
| gimli | 0.34.0 | gimli-rs |
| object | 0.40.0 | gimli-rs |
| rustc-demangle | 0.1.28 | rust-lang |
| cfg-if | 1.0.5 | rust-lang |
| libc | 0.2.191 | rust-lang |
| memchr | 2.8.3 | BurntSushi |
| miniz_oxide | 0.9.1 | Frommi/miniz_oxide |
| adler2 | 2.0.1 | oyvindln |
| windows-link | 0.2.1 | microsoft/windows-rs (built only on Windows) |

Each generated crate gets a `Cargo.lock` with exactly dat-rt's dependencies at these versions and builds with `--locked`, so once dat is built a machine needs no network for compiled programs. The dependencies build at opt-level 1 without debug info, which keeps the first build near 10 s; opt-level 0 builds faster but prints traces twice as slowly.

Still open:

- **macOS.** Traces from `dat run --compiled` and `dat test --compiled` pass in CI. A moved `dat build` executable with its `.dSYM` hasn't been checked on macOS yet.
- **Windows** isn't built or tested. Addresses are resolved without their frame, which may lose inlined frames there.
- **The toolchain.** `RUSTC_BOOTSTRAP` and the LLVM flags must keep working when the pinned toolchain moves. An unknown flag fails the build loudly, and `examples/semantics/traces.dat` checks 25 trace shapes.
- **Other merging optimizations** could still drop a frame. The examples cover the ones found so far.

## Review

A separate review agent probed the build with about 140 programs, comparing the modes. It found seven bugs. All seven are fixed, and each now has an example with a golden:

- A statement that is only a value (`x;`, `s < "t";`) made the generated Rust fail rustc's lints. Discarded values are now emitted as `let _ = ...;`. Whether dat should allow them is still Q16.
- A `never` value in a string slot, such as `assert(false, fail("boom"))`, made the checker drop the whole statement in both modes. A `never` expression now takes the type of the place it's used in, through a `Diverge` IR node, as §01 says it can.
- A `never` function that could reach its end was accepted, and the modes then differed. It is now E0654.
- A parameter of type `never` or `void` compiled to Rust that rustc rejects. It is now E0252.
- A syntax error inside `main` also reported "no `main`", with a fix that added a second one. E0660 now appears only when nothing else is wrong.
- Nesting thousands of levels deep crashed rustc, and 20,000 levels crashed `dat` itself. Now `dat` runs the compiler on a 256 MiB stack, rustc gets the same, and more than 8,000 levels is a syntax error. 7,000 nested parentheses run in both modes.
- `export function f(x: int) { ... }` was accepted without a return type, which §06 requires. It is now E0655, with the inferred type as a safe fix.

The review also changed two smaller behaviours. A `let` that redeclares a parameter is now E1402, because the parameters and the function body share one scope. A failed `assert` with a message now shows `message:` in its `FAIL` block.

The review's last suspicion was real. Generated crates are keyed by path, so a second `--compiled` run of a file could replace the first one's `main.rs` mid-build. A lock now holds from writing the source until the program starts, and `dat-conformance concurrency` checks it.

Later work found five more bugs, all fixed with examples:

- `dat rust` panicked on a file with tests and no `main`. It now prints the test crate.
- E0661's `: void` fix for `function main(): int { return 0; }` left `return 0;`, which was then E0652. The fix now rewrites the returns too.
- `function main(): int { }` reported E0601 as well as E0661. It now reports only E0661.
- `let x: void;` reported E0203. It is now E0252.
- A `+` chain with two adjacent non-string operands got a safe E0301 fix, although its meaning is ambiguous (Q48).

## Decisions made while building

These are implementation choices. None of them changes the spec.

- **Compiled traces come from the native stack, and the interpreter builds the identical trace while an error propagates.** [The trace design](#the-trace-design) has the measurements.
- **Unwinding carries the error, and the trace is captured at the throw only when no running `try` will catch it.** `catch` is `catch_unwind` plus a downcast, and a Rust panic (a dat bug) passes through every dat `catch` and exits with status 2 as an internal error (the interim answer to Q4).
- **The compiled stack is 32 MiB**, and the interpreter's 512 MiB. [The trace design](#the-trace-design) shows the trade-off.
- **The interpreter has typed fast paths**: `int` and `bool` expressions evaluate without the generic `Value`, which took `fib(30)` from 422 ms to 315 ms here. It still walks the IR; closure compilation stays the M1 upgrade if the stretch goal matters.
- **One generated crate per entry file and mode**, in `target/dat-cache`, all sharing one target directory, so dat-rt builds once and an unchanged program is a Cargo no-op. Files are rewritten only when their content changes. The toolchain is pinned to 1.99.0, the one in `rust-toolchain.toml`.
- **Tests run sequentially on one large-stack thread** (decision 16 says sequential in M1). Each test's output is captured, and the report goes through dat-rt in both modes.
- **`dat fix` exists already.** The harness needed fix application anyway. It applies the preferred safe fix of each diagnostic, re-checks, and repeats up to four rounds.
- **Parser recovery works per declaration.** After a syntax error, parsing resumes at the next top-level declaration, so a file with three broken functions gets three errors. Inside a declaration it stops at the first one, as the plan allowed for M0.
- **The checker infers unannotated return types on demand.** A recursive function needs an annotation (the interim answer to Q20) and gets an `E0002` that says so.
- **The extractor needs one prelude** (`const name = "Ada";` for a block in §03). Spec blocks with a `}` as their ❌ line, like `sign` in §06, run only their ❌ case.

## Answers

Questions 1–39 of the research report still stand. Questions 45–49 were settled without asking the owner, who asked not to be asked questions with obvious answers.

| # | Question | Answer |
|---|---|---|
| 40 | Compiled speed: which option for the failing gate? | The owner chose reading the native stack at the throw. It's built, at 1.07× in CI. |
| 41 | The provisional codes | The owner approved all 19 and asked for a ❌ example of each in the spec. Each now has one, except E0002 ("not supported yet"), which §21 describes because it disappears once every feature exists. None is provisional now. |
| 42 | The test summary when nothing fails | `0 failed, 4 passed (2 ms)`, the same shape every time. |
| 43 | `--verbose` lines for passing tests | `PASS fib.dat:41  small values`, where 41 is the line the `test` starts on. A `FAIL` line names the line that failed. |
| 44 | The `FAIL` block for an error other than an `assert` | The error, then the frames below the test, under a `FAIL` line naming the line in the test where it failed. |
| 45 | The message of `StackOverflowError` | `too many nested calls`. It leaves out the depth, which differs between the modes and between machines. |
| 46 | `assert(cond, message)` | `e.message` is the given message, and `assertion failed: <condition as written>` without one. |
| 47 | The overflow message with a negative right operand | `-9223372036854775808 + -1`, following the template. |
| 48 | The E0301 fix for `"Total: " + count + 1` | Two options and no safe fix, because the meaning is ambiguous: `` `Total: ${count + 1}` ``, the likelier intent, first, and `` `Total: ${count}${1}` `` second. Chains that alternate strings and values keep their safe fix. |
| 49 | A warning for a file with neither `main` nor tests | No warning: such files are libraries. |
| 50 | Function merging needs `RUSTC_BOOTSTRAP=1` on stable | The owner chose to keep it on the pinned stable toolchain, over a pinned nightly or dropping the switch. |
| 51 | Outside libraries | Allowed when secure, without significant performance cost, and usable as they are without workarounds; otherwise fork or write our own (`AGENTS.md`). `backtrace` is the first. |

## What M1 builds on

- **The parser** already accepts lists, maps, arrow functions, `match`, member calls, `?.`, `??`, `is`/`as`, generics and struct declarations, so M1 adds checker and IR support without new syntax work.
- **The diagnostic record, codes and fixes** are in their final schema. Every code has a ❌ example in the spec. M1's diagnostics for syntax dat doesn't have, like `===` (already one), follow the same pattern.
- **`dat_rt::Str`** is the opaque string type the plan recommends: `Rc<String>` with append in place. The small-string representation can replace it without touching generated code.
- **The interpreter's typed fast paths** extend to `float`.
- **The spec ratchet** turns M1's features into passing cases automatically, because the extractor already reads expressions with expected values (`10 % 3 // 1`) and `// throws` lines.
