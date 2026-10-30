---
name: perf-check
description: Check that a change keeps dat fast. Use after any change under crates/ (checker, IR, codegen, interpreter, runtime, parser), refactors included, before calling the work done; when asked about performance; and when adding a benchmark.
---

# Performance check

## When

- Any change under `crates/`, even a refactor that isn't meant to change speed.
- Not needed for changes only to `examples/`, `reference/` or `reports/`.
- Two steps, both required: diff the generated Rust, then run `perf`.

## Before the first edit

- Keep the old compiler as a baseline: `cargo build --release --locked && cp target/release/dat <scratch>/dat-base`.
- If you forgot, build the last commit in a worktree: `git worktree add <scratch>/base HEAD`, then `cargo build --release --locked` inside it. This drops any uncommitted earlier work from the baseline, so say so.
- If another process is using `target/release` (a probe agent, a long test run), build your change into a separate `CARGO_TARGET_DIR` so its binary stays the same.

## Step 1: diff the generated Rust

- Compare old and new output for every benchmark and every example that isn't an error example:
  ```
  B=<scratch>/dat-base
  for f in bench/*.dat $(ls examples/*/*.dat | grep -v /errors/); do
    for mode in rust ir; do
      cmp -s <($B $mode $f 2>&1) <(target/release/dat $mode $f 2>&1) || echo "differs: $mode $f"
    done
  done
  ```
- No output means the generated Rust and IR are byte-identical, so compiled speed can't have changed. Any compiled-speed difference in `perf` is then noise.
- An example you edited, or one the baseline can't compile, differs for that reason alone. The `bench/*.dat` lines are the ones that matter for speed.
- Identical IR doesn't cover changes to `dat-interp` itself. For those, read the interpreter-against-CPython lines in `perf`.
- A diff you expect: read it, find out which functions it reaches, and say so in the report. Example: the stack check added to a default's wrapper is added only when the function is recursive and a default makes a call, and no benchmark changed.
- A diff you didn't expect is a bug. A change to the stdlib table, for example, must not change the Rust for code that doesn't use it.
- To read one hot function: `target/release/dat rust bench/floats.dat | sed -n '/pub fn f0_escapes/,/^    }$/p'`. Generated functions are named `f<N>_<name>`. Compare it with the hand-written `bench/<name>.rs`.

## Step 2: run perf

- `perf` isn't part of `dat-conformance all`. Run it on its own.
- `dat-conformance` uses the `dat` next to it, so build both first: `cargo build --release --locked`.
- It builds and times about a dozen programs, so run it in the background and send its output to a file: `target/release/dat-conformance perf > <scratch>/perf.log 2>&1`.
- Run it last in the gate: fmt, clippy, build, `all`, `cargo test`, then `perf`.
- `cargo fmt` changes no behavior. After a fix that was only formatting, don't run `perf` again.
- Run it on a quiet machine. Another cargo build or a probe running at the same time skews the numbers.
- Locally, run it without flags: this VM is the reference machine, where a gate fails at its limit. CI runs `perf --shared-runner`, which fails only at twice the limit.

## The gates

- Tooling (ms, including process start): `dat check fib.dat` ≤ 10 ms, `dat test fib.dat` ≤ 20 ms, `dat run hello.dat` ≤ 5 ms.
- Compiled path, warm: `dat run --compiled` ≤ 1 s, `dat test --compiled` ≤ 2 s.
- `fib(35)` against hand-written Rust ≤ 1.2×. This is the worst case: it does nothing per call but call itself.
- Float code (`bench/floats`) against hand-written Rust ≤ 1.04×. This is the everyday-code aim from AGENTS.md.
- Interpreted `fib(30)` against CPython ≤ 2×.
- Shown but not judged: the stack guard's cost on fib, ack, mutual, primes, rows, the interpreter's primes, rows and floats against CPython, and lines of Rust per line of dat.
- The last line says `perf: all gates pass` or `perf: FAILED: <names>`.

## Reading the numbers

- Each number is the median of 20 runs after 3 warmups. Programs that are compared run in turn, so drift hits both sides.
- Noise on this VM is about 2%: byte-identical floats code measured 1.01×, 1.02× and 1.03× on the same day. A result within 2% of its gate tells you nothing by itself.
- A compiled gate fails but the Rust is identical: it's the machine. Check its load and run once more.
- A compiled gate fails and the Rust changed: read the hot function against `bench/<name>.rs` before touching anything else.
- Judge by AGENTS.md: everyday code within about 4% of hand-written Rust. Don't give up simplicity, exact traces or catchable errors for speed only `fib` would show.
- Last measured numbers: the Speed section of the latest `reports/M* status.md`.

## What keeps it fast

- Work at check time, not at run time. The stdlib table is read by the checker, and generated code calls `dat_rt` functions directly.
- The stack check `dat_rt::stack_check(&super::STACK_LIMIT)` goes only into recursive functions. `DAT_NO_STACK_GUARD=1` leaves it out, only so `perf` can show its cost.
- Exact int division, as in `(a / b).floor()`, lowers to integer operations, not floats.
- New runtime work goes only where it's needed, and the report says where it runs.

## Adding a benchmark

- Three files with the same algorithm and the same output: `bench/<name>.dat`, `bench/<name>.rs` written the way a person would write it, and `bench/<name>.py` for the interpreter. Check that all three print the same thing.
- The hand-written Rust is built the way generated crates are: `rustc -C opt-level=3 -C overflow-checks=on -C llvm-args=-align-all-functions=6` (`rustc()` in `perf.rs`). dat checks int overflow, and without the alignment, fib's speed depends on where the linker puts it.
- Register it in `crates/dat-conformance/src/perf.rs`: in `others` to show it, or as a gate to judge it, for everyday code only. Add it to the lines-per-line list too.

## Reporting

- Every compiler change states both results: what the generated-Rust diff showed (identical, or what changed and where it runs) and the `perf` lines against their gates, with the noise noted when a number is near its gate.
- Update the Speed table in the milestone's status report when the numbers move.
