# dat

dat is a statically typed language with TypeScript-style syntax, an
interpreter for development and Rust code generation for production. The
language is defined in [`reference/`](reference/README.md); the plan and the
research behind it are in [`reports/`](reports/).

This tree holds the **M0 walking skeleton**: a real, tiny version of every
component, proven on [`examples/fib/fib.dat`](examples/fib/fib.dat) in both run
modes. [`reports/M0 status.md`](<reports/M0 status.md>) checks it against the
17 exit criteria.

## Using it

```
cargo build --release
target/release/dat run examples/fib/fib.dat              # on the interpreter
target/release/dat run --compiled examples/fib/fib.dat   # generated Rust
target/release/dat test examples/fib/fib.dat
target/release/dat check examples/fib/bad_fib.dat        # one error, with its exact fix
target/release/dat fix examples/fib/bad_fib.dat          # applies the safe fixes
target/release/dat build examples/fib/fib.dat -o fib     # a native executable
```

The first compiled run on a machine builds dat-rt and its dependencies,
which takes about 10 seconds; later runs reuse them.

`reference/21-command-line.md` describes the commands, their output and
their exit statuses. Two commands exist for looking inside: `dat ir <file>`
prints the typed IR, and `dat rust <file>` prints the generated Rust.

M0 implements `int`, `bool` and `string`, functions, `let`/`const`, `if`,
`for`, `while`, `break`/`continue`, `throw`, `try`/`catch` with the built-in
error types, `print`, `assert`, `fail`, template strings, `toString()` and
`test` blocks. Anything else is reported as `E0002: not supported yet`.

## How it fits together

```
src.dat -> dat-syntax -> dat-check -> dat-ir -+-> dat-interp ----------> dat-rt
           lexer,        names,       typed   |   walks the IR           runtime
           parser,       types,       IR      +-> dat-codegen -> rustc -> dat-rt
           arena AST     lowering                  emits Rust
           (dat-diag: the diagnostic record, its terminal and JSON forms)
```

| Crate | Role |
| --- | --- |
| `dat-diag` | The diagnostic record of `reference/21-command-line.md`: code, byte span, message, label, `fixes[]` with applicability and `before` text; the terminal and `--json` renderers; applying fixes. |
| `dat-syntax` | A lossless lexer (comments kept as trivia), a recursive-descent parser with Pratt expressions, and an arena AST with a byte span on every node. It parses more than M0 checks, so later constructs get "not supported yet" instead of syntax errors. |
| `dat-check` | Name resolution, block scope, types and lowering to the IR in one pass per body; one diagnostic per root cause, each with a fix where one is certain. [`CODES.md`](crates/dat-check/CODES.md) lists the codes. |
| `dat-ir` | The typed IR both backends read: slot-indexed locals, checked integer operations with locations, structured control flow and try/catch regions. Evaluation order is the tree order. |
| `dat-rt` | Every observable operation, once: checked arithmetic and its messages, error values, `print`, `toString`, `assert` rendering, the uncaught-error printer, the test reporter and the stack guard. |
| `dat-interp` | Walks the IR, calling dat-rt. Errors propagate as `Result`. |
| `dat-codegen` | Emits one Rust function per dat function and builds it with Cargo against dat-rt, with the toolchain pinned. Runtime errors unwind. |
| `dat` | The command line. |
| `dat-conformance` | Runs the examples and the spec's code blocks in both modes and compares them. |

Neither backend may depend on the parser or the checker;
`dat-conformance structure` enforces it.

### Errors and traces

Runtime errors are `DatError` values. The interpreter returns them as `Err`.
Generated code throws them with `resume_unwind`, and `try` becomes
`dat_rt::catch` around a closure, so the error path costs nothing until
something is thrown. Generated functions are plain Rust and keep no trace
bookkeeping. When an error is thrown and no running `try` handles its type,
dat-rt records the return addresses on the native stack with the
[`backtrace`](https://github.com/rust-lang/backtrace-rs) crate. Printing the
trace resolves each distinct address once and maps its line and column in
the generated `main.rs` to a dat call site, through a table dat-codegen
writes into the crate. The interpreter builds the same trace while an error
propagates, so both modes print the same dat-only trace:

```
RangeError: int overflow: 4660046610375530309 + 7540113804746346429
  at fibIter (overflow_uncaught.dat:9:18)
  at main (overflow_uncaught.dat:17:9)
```

Every dat call has to stay a Rust frame with its own location, so generated
crates turn off LLVM's function merging, branch merging and tail calls.
Function merging is only switchable with `-Zmerge-functions=disabled`, which
`RUSTC_BOOTSTRAP=1` allows on the pinned stable toolchain. `dat build`
keeps line tables in the executable: compressed on Linux, and in a `.dSYM`
next to the executable on macOS.

Deep recursion throws a catchable `StackOverflowError`. dat code runs on a
thread dat-rt creates with a known stack (32 MiB compiled, 512 MiB in the
interpreter), and recursive functions compare the address of a local with a
limit before their first call. Every program can nest at least 10,000 calls;
measured, the interpreter goes past 160,000, and compiled code past two
million simple calls and 128,000 of the largest kind measured.

## Testing

```
cargo test --release                              # unit tests
target/release/dat-conformance all                # everything below, in both modes
target/release/dat-conformance examples           # examples/**/*.dat against their .golden files
target/release/dat-conformance self-test          # a wrong `+` in the interpreter must be caught
target/release/dat-conformance spec               # the code blocks in reference/
target/release/dat-conformance structure          # crate graph; no unsafe or RefCell in generated Rust
target/release/dat-conformance concurrency        # two compiled runs of one file at once
target/release/dat-conformance perf               # latency budgets and speed gates
```

- **Examples.** Each `examples/**/X.dat` has its expected results in
  `X.golden`, one section per command. `run` and `test` sections must give
  the same stdout, stderr and exit status on the interpreter and compiled.
  `X.fixed` is the expected text after applying the safe fixes from
  `dat check --json`, and `X.reviewed` the text after applying each
  diagnostic's preferred safe or review fix; either result must check clean.
  `--bless` rewrites the goldens from the interpreter.
- **Spec blocks.** The runner extracts all 179 code blocks in `reference/`
  and reads the reference's own comments as expectations: `// ❌ E0301:` with
  its `fix:` lines, `print(x); // output`, `expr // value` and
  `// throws RangeError`. Each block becomes one case per ❌ line plus a
  positive case. `conformance/spec-pass.txt` lists the cases that pass, and
  that list may only grow; `conformance/preludes.txt` declares names a block
  uses without declaring them.
- **Perf.** Interpreter-path latency budgets (`dat check fib.dat` ≤ 10 ms,
  `dat test fib.dat` ≤ 20 ms, `dat run hello.dat` ≤ 5 ms), the compiled-path
  budget, compiled `fib(35)` against hand-written Rust, the stack guard's cost,
  and interpreted `fib(30)` against CPython.

Generated crates live in `target/dat-cache` (override with `DAT_CACHE_DIR`).
`DAT_FAULT=interp-plus-one` makes the interpreter compute `+` wrong; it exists
only for the self-test.
