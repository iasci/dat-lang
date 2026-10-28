# Error model: catchable runtime errors in the interpreter and in generated Rust

Scope: how dat can keep the AGENTS.md rule "every runtime error can be caught; never turn a failure into an uncatchable crash" in both run modes, and what each approach costs. Inputs: [AGENTS.md](../../AGENTS.md), [reference/12-errors.md](../../reference/12-errors.md) and the runtime-error mentions in §01, §02, §03, §04, §05, §09, §10, §16, §17 and §20, plus the draft roadmap (M0 spike "runtime errors as unwinding", M4 "Errors", open questions Q1 and Q2).

**Owner answers received during this research (relayed by the coordinator, 2026-10-28):**
- Q1, stack overflow: "if we can catch these errors cheap let's do it and have a StackOverflowError type, otherwise it's fine that we crash". The question is therefore the measured cost of making it catchable, and the point at which crashing is the better choice. A `StackOverflowError` row in §12 is a spec addition that still needs the owner's approval.
- Q2, out of memory: accepted as the one failure that can't be caught. Both modes abort. Section 4 covers only how to make the abort clean and identical in both modes.

**How the numbers were measured.** All "local benchmark" numbers come from my own micro-benchmarks, saved with source and raw output in [error_model_bench/](./error_model_bench/) ([src/main.rs](./error_model_bench/src/main.rs), [results.txt](./error_model_bench/results.txt)). Setup: rustc 1.99.0 (2026-09-28), Apple M4 Pro, macOS (Darwin 24.6), `opt-level=3, lto="fat", codegen-units=1, panic="unwind"`. Each row is the fastest of 9 repetitions, and I ran the whole suite 3 to 4 times as separate processes. Results moved by up to ±20% between processes, probably because macOS schedules the thread on performance or efficiency cores, so I give ranges. **I made no Linux or x86 measurements.** The Docker daemon wasn't running, and I didn't start a VM on the owner's machine. macOS is atypical for thread-local storage and unwinding cost, so Linux numbers come only from published sources.

---

## 1. Runtime errors in generated Rust: unwinding, Result everywhere, or hybrid

### Takeaway
The hybrid model is the cheapest on the path where nothing fails: declared errors become `Result`, and runtime errors unwind with a typed payload. When nothing is thrown, `catch_unwind` costs nothing measurable. Returning a `Result` from a non-inlined call costs 0.4 to 0.7 ns more per call than a plain return: +9% to +17% on a call chain that returns a struct, and +25% to +66% on call-dominated checked recursion. The hybrid pays when an error is actually thrown. On macOS a caught throw costs about 6 µs plus about 1 µs per frame, or about 2.4 µs per frame when the frame holds a value with a destructor. Propagating an `Err` costs about 11 ns per frame, so throwing is 200 to 300 times more expensive. That trade fits dat, where runtime errors mean bugs or rare conditions, as long as the expected-failure paths (`try?` on built-ins, declared errors) never unwind.

### Cited Findings
- Spec split: declared errors need `throws` and `try`. Runtime errors (`RangeError`, `KeyError`, `TypeError`, `AssertionError`) need no marking, can come from anywhere including called functions, and are caught by the same `try`/`catch`. — [12-errors.md](../../reference/12-errors.md)
- `try?` covers only the failure of the expression it marks. A runtime error inside a function that the expression calls passes through. This allows `try? xs[i]` to compile to a non-unwinding `xs.get(i)`, as the roadmap proposes. — [12-errors.md](../../reference/12-errors.md); roadmap M4 "Rust that lands"
- **`catch_unwind` costs nothing measurable when nothing is thrown.** A non-inlined call alone took 0.742, 1.007, 0.724 and 0.751 ns. The same call inside `catch_unwind(AssertUnwindSafe(..))` took 0.742, 1.003, 0.723 and 0.744 ns (4 runs). — [local benchmark](./error_model_bench/results.txt)
- **Cost of `Result` when nothing fails, call-dominated worst case** (checked `fib(32)`, 7.05M logical calls; baseline with wrapping arithmetic: 0.506–0.551 ns per call):
  - checked add, unwind on overflow, plain `i64` return: +0.12 to +0.23 ns per call (+24% to +41%)
  - checked add, `Result<i64, Box<E>>` (16 bytes, returned in two registers) with `?`: +0.42 to +0.57 ns (+76% to +112%)
  - checked add, `Result<i64, E{message: String}>` (24 bytes, returned through memory): +0.42 to +0.61 ns (+76% to +121%)
  - Unwinding vs `Result` directly: `Result` is +25% to +66% slower per call
  - With a hidden depth argument added (see section 3): unwinding +0.15 to +0.17 ns (+28% to +33%); `Result` +0.55 to +0.76 ns (+99% to +150%)
  - — [local benchmark](./error_model_bench/results.txt)
- **Struct-returning call chain** (20M non-inlined calls returning a 24-byte struct): unwinding 3.79–4.18 ns per call; `Result<P3, Box<E>>` with `?` adds +0.38 to +0.66 ns (+9% to +17%). `size_of::<Result<P3, Box<E>>>()` is 32 bytes. — [local benchmark](./error_model_bench/results.txt)
- **The panic strategy changes the `Result` cost.** In a `panic = "abort"` build of the same source, checked fib with `Result` took 0.678 ns against 0.585 ns for the unwinding variant (+16%). In the `panic = "unwind"` build it took 0.94–1.08 ns against 0.63–0.78 ns. Because dat must pin `unwind` for runtime errors, the declared-error `Result` paths also run at the slower `unwind`-build speed. — [local benchmark](./error_model_bench/results.txt)
- **Cost of one caught throw on macOS:**
  - `panic_any(RangeError{..})` with a silent hook, then `catch_unwind` and `downcast_ref`: 6.50–6.59 µs
  - `resume_unwind(Box::new(RangeError{..}))`, which never calls the hook: 5.78–5.97 µs
  - Unwinding with no destructors in the frames: 6.7 µs at 0 frames, 16.8 µs at 10, 54.9 µs at 50, 102.5 µs at 100. That is about 0.96 µs per frame.
  - Unwinding through frames that each hold a `String`, so each frame has a landing pad: 30.6–31.4 µs at 10 frames, 239–243 µs at 100. That is about 2.4 µs per frame.
  - The same 10 and 100 frames propagating `Err(Box<E>)` with `?`, with a `String` allocated and freed in each frame: 107–112 ns and 1,211–1,217 ns, about 11 ns per frame.
  - — [local benchmark](./error_model_bench/results.txt)
- Capturing a native backtrace at the throw site (`Backtrace::force_capture()`, unresolved, about 5 frames deep) costs 3.68–3.79 µs. — [local benchmark](./error_model_bench/results.txt)
- Published Linux numbers (Nov 2024, CPU not stated): `panic!` + `catch_unwind` 2.3814 µs; `resume_unwind` instead of `panic!` 1.8379 µs; the author's custom mechanism, which skips the hook, the panic counter and the `Box`, 556.32 ns. That is a 4.3× speedup and became the Lithium crate. — [purplesyringa, "Bringing faster exceptions to Rust"](https://purplesyringa.moe/blog/bringing-faster-exceptions-to-rust/)
- Published comparison on a real workload, success path only (Aug 2024): serde JSON deserialization using the author's panic-based `#[iex]` against plain `Result` ran 0% to +40% faster (canada struct: 404.2 → 565.0 MB/s; citm_catalog DOM: 363.8 → 439.4 MB/s; twitter DOM: +5%). The author's stated reasons: `Result` is larger than the value, often spills to the stack, needs a branch at every call site, and inhibits inlining. — [purplesyringa, "You might want to use panics for error handling"](https://purplesyringa.moe/blog/you-might-want-to-use-panics-for-error-handling/)
- Swift's design rationale says table-based unwinding makes the error path expensive ("This lookup process is quite expensive, especially since it has to repeat all the way up the stack"). It says explicit propagation "hurts runtime performance along the non-error path more than the alternatives", although "branches involved in testing for errors are usually very easy to predict", and that the code-size cost can be considerable. — [Swift ErrorHandlingRationale](https://github.com/swiftlang/swift/blob/main/docs/ErrorHandlingRationale.md)
- Midori measured the cost of return codes (an extra return value plus a branch at each call site) and found it "nontrivial". The post gives no numbers. — [Joe Duffy, "The Error Model" (2016)](https://joeduffyblog.com/2016/02/07/the-error-model/)
- **Size cost of `panic = "unwind"`** on the benchmark binary: the file is 518,320 B against 497,792 B with `abort` (+4.1%); `__text` is 292,552 B against 285,920 B (+2.3%); `__eh_frame` is 31,968 B against 24,792 B. The binary is small and mostly std. — [local benchmark](./error_model_bench/results.txt)
- The Rust Reference: "When compiling code with the `abort` panic strategy, the optimizer may assume that unwinding across Rust frames is impossible, which can result in both code-size and runtime speed improvements." — [Rust Reference, Panic](https://doc.rust-lang.org/reference/panic.html)
- Khalil Estell argues that on Cortex-M4, C++ exceptions produce smaller binaries than result types beyond some program size. I saw only the talk abstract, not his numbers. — [ACCU 2024 talk page](https://accu.org/video/spring-2024-day-2/estell)
- Rust std: "It is **not** recommended to use this function for a general try/catch mechanism. The `Result` type is more appropriate to use for functions that can fail on a regular basis." — [std::panic::catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)

### Inferences
- **(b) `Result` everywhere** would make almost every dat function fallible, because any index, `int` add, `!` or `as` can raise a runtime error. Every call site would then pay the +0.4 to +0.7 ns branch-and-spill cost measured above. It also spreads through types: closures become `Rc<dyn Fn(..) -> Result<..>>`, trait methods return `Result`, and dat-rt's higher-order built-ins must take fallible callbacks. Its gains are a throw that costs nanoseconds, and no dependency on unwinding, which matters for wasm (section 2).
- **(a) Unwinding for declared errors too** contradicts the spec's intent. Declared errors are expected failures, such as a parse error, and would pay 6 µs or more per throw on macOS instead of about 11 ns per frame. It also loses the direct `try` → `?` mapping.
- **(c) Hybrid** matches the spec's split between declared and runtime errors, which is also how Swift and Midori draw the line, and it measured fastest on the success path. Its weak point is programs that use runtime errors as control flow in a hot loop: catching a `RangeError` per element of a 1M-element list would take roughly 6 to 30 s on macOS. The language already offers the cheap alternative (`try? xs[i]`), so this is guidance plus a lint, not a design flaw.
- macOS unwinding (about 6.8 µs base) appears to be 3 to 4 times slower than the published Linux figure (1.84 µs). The machines differ, so treat the ratio as indicative only.

### Gaps
- No Linux or x86-64 measurements of my own, and no code-size comparison on a realistic program. Estell's size numbers weren't verified.

---

## 2. Unwinding pitfalls: panic hook, UnwindSafe, double panics, abort-only targets, FFI

### Takeaway
Most pitfalls go away if dat-rt owns the throw:
- throw with `resume_unwind`, which skips the hook
- keep one process-wide hook for genuine Rust panics only
- lower `finally` to ordinary code after `catch_unwind`, never to a `Drop` guard
- keep `panic = "unwind"` pinned

Two hard limits remain. Wasm targets default to `abort`, and unwinding on wasm still needs nightly with `-Zbuild-std` as of April 2026. And recursive drop glue on deep values overflows the native stack and aborts even inside `catch_unwind`.

### Cited Findings
- `catch_unwind` "only catches unwinding panics, not those that abort the process." — [std::panic::catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)
- "If a custom panic hook has been set, it will be invoked before the panic is caught, before unwinding." — [std::panic::catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)
- `resume_unwind` "Triggers a panic without invoking the panic hook." If panics are configured to abort, it aborts instead. — [std::panic::resume_unwind](https://doc.rust-lang.org/std/panic/fn.resume_unwind.html). Measured: 0.6–0.7 µs cheaper per throw than `panic_any` with a silent hook. — [local benchmark](./error_model_bench/results.txt)
- The closure passed to `catch_unwind` must be `UnwindSafe`. `AssertUnwindSafe` asserts it. "If it is `Err`, it contains the panic payload, and dropping that may in turn panic!" — [std::panic::catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)
- **A panic inside `Drop` while unwinding aborts.** Verified locally: a `Drop` that panics during an unwind inside `catch_unwind` prints "panic in a destructor during cleanup / thread caused non-unwinding panic. aborting." and exits with status 134. — [local check](./error_model_bench/results.txt)
- **Recursive drop glue overflows the stack.** A 1M-node `Option<Box<Node>>` chain (the roadmap maps nullable self-references to `Option<Box<T>>`), dropped inside `catch_unwind` on an 8 MiB main thread, gives "has overflowed its stack / fatal runtime error: stack overflow, aborting" and exit status 134. — [local check](./error_model_bench/results.txt); roadmap M3 mapping
- Rust-originated panics exist even when dat code never panics. `Vec::with_capacity(usize::MAX / 4)` panics with "capacity overflow", is printed by the default hook, and is caught by `catch_unwind`. — [local check](./error_model_bench/results.txt)
- Panic strategy: "Not all targets may provide the `unwind` handler." Crates built with `unwind` can use the `abort` handler, but not the reverse. — [Rust Reference, Panic](https://doc.rust-lang.org/reference/panic.html)
- FFI: unwinding into Rust through a non-unwinding ABI such as `"C"`, or calling an unwinding Rust function from code built with `-fno-exceptions`, is undefined behavior. A foreign exception caught by `catch_unwind` either aborts or returns an opaque `Err`, and which one is unspecified. — [Rust Reference, Panic](https://doc.rust-lang.org/reference/panic.html). Rust 1.71 (July 2023) stabilized the `-unwind` ABIs ([Rust 1.71.0 announcement](https://blog.rust-lang.org/2023/07/13/Rust-1.71.0/)). Rust 1.81 (Sept 5, 2024) made uncaught panics in `extern "C"` functions abort ([releases.rs 1.81.0](https://releases.rs/docs/1.81.0/)).
- **Wasm:** "By default the `wasm32-unknown-unknown` target is compiled with `-Cpanic=abort`." The Wasm exception-handling proposal reached stabilization in mid-2025, but the rustup std is built with `abort`, so unwinding needs `RUSTFLAGS='-Cpanic=unwind' cargo +nightly build -Zbuild-std`. "As of 2025-10-03 LLVM is still using the 'legacy exception instructions' by default". "There are no concrete plans for adding new targets ... which have `-Cpanic=unwind` enabled-by-default." — [rustc book, wasm32-unknown-unknown](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html)
- Cloudflare, April 22, 2026: "Building for Wasm with `panic=unwind` still requires an experimental nightly Rust target". Engines with modern Wasm exception handling (exnref): Chrome 138, Firefox 131, Safari 18.4, Node.js 25.0.0; Cloudflare backported it to Node 22 and 24. Rust's Wasm targets still default to the legacy variant, "now deprecated". Aborts can't be recovered in place: "there is no possibility of state recovery at all." — [Cloudflare, "Making Rust Workers reliable"](https://blog.cloudflare.com/making-rust-workers-reliable)

### Inferences
- **Hook policy.** As far as I know, Rust's panic hook is process-global. I didn't open the `set_hook` page, so treat this as unverified. "Per-thread" suppression therefore has to happen inside the single hook, by checking the payload type or a thread-local flag. If dat throws only through `resume_unwind`, the hook never sees dat errors. It sees only genuine Rust panics (a dat-rt bug, std's "capacity overflow", arithmetic inside a dependency), which is exactly where a Rust-level message and backtrace are useful.
- **Double panic.** dat has no user-defined destructors: grepping `reference/` finds no drop or deinit feature. Code that runs during unwinding is therefore compiler-generated drop glue plus dat-rt `Drop` impls. If those never panic, and `finally` runs after `catch_unwind` returns instead of inside a landing pad, a runtime error inside `finally` is an ordinary new throw, not a double panic. The spec rule that "it replaces any error that was passing through" then amounts to dropping the old payload and throwing the new one.
- `AssertUnwindSafe` is correct for dat, not a shortcut. The spec says changes made before the error stay, so observing partially mutated state after a catch is the intended semantics. This requires every dat-rt structure to stay valid when a callback throws mid-operation (for example `sort` with a throwing comparator); see section 6.
- **Recursive drop is the biggest hole in "every runtime error can be caught".** It isn't a dat call, so no guard on dat functions sees it. It also fires during unwinding, when the values in unwound frames are dropped. The usual Rust fix is a hand-written iterative `Drop` for self-referential types. The dat compiler can generate one for every struct that contains `T?` or `list<T>` of itself. Clone, equality, hashing, `toString` and comparison of recursive values have the same problem and need iterative or stack-checked implementations in dat-rt.

### Gaps
- I didn't open the `std::panic::set_hook` documentation, so the process-global nature of the hook comes from my knowledge, not a fetched source.

---

## 3. Stack overflow: options, measured guard cost, precedents

### Takeaway
Catching stack overflow is cheap if done the way Go and Wasmtime do it: compare the stack pointer with a limit when entering a function, and only in functions that can recurse. The limit has to be in a plain static or a passed context, not in thread-local storage on macOS. That costs +0.02 to +0.17 ns per guarded call (+2% to +19% on tiny recursive functions) and nothing in non-recursive functions. Keeping the limit in TLS costs +0.26 to +0.42 ns. A depth counter in TLS, a shadow stack, or `stacker` costs +0.74 to +1.05 ns per call. Guard page plus signal handler isn't sound in Rust. The guard covers only dat calls: deep recursive values still overflow in Rust drop, clone, eq, hash and `toString` code unless dat-rt handles them (section 2).

### Cited Findings
**Rust today**
- An unguarded deep recursion inside `catch_unwind` prints "thread 'main' (…) has overflowed its stack / fatal runtime error: stack overflow, aborting" and exits with status 134 (SIGABRT). It isn't caught. — [local check](./error_model_bench/results.txt)
- std's Unix handler checks whether the faulting address is in the thread's guard page, prints the message and aborts. It runs on an alternate signal stack (`sigaltstack`). — [std/sys/pal/unix/stack_overflow.rs](https://doc.rust-lang.org/src/std/sys/pal/unix/stack_overflow.rs.html). Detection isn't guaranteed on every platform; without stack probes a program can just segfault. — [users.rust-lang.org thread](https://users.rust-lang.org/t/is-rust-guaranteed-to-detect-stack-overflows/52593)

**Measured guard cost** (per guarded call, against an unguarded baseline; `fib(32)`: 7.05M logical calls with almost no work per call, baseline 0.50–0.65 ns; binary-tree sum: 8.4M calls with a little work per call, baseline 0.88–1.25 ns; 3 to 4 runs) — [local benchmark](./error_model_bench/results.txt)

| Guard | fib: added ns/call (%) | tree: added ns/call (%) |
|---|---|---|
| SP (inline asm) vs limit in a plain static | +0.02 to +0.10 (+4% to +19%) | +0.03 to +0.17 (+2% to +19%) |
| depth passed as a hidden argument | +0.06 to +0.22 (+10% to +41%) | +0.05 to +0.13 (+4% to +15%) |
| SP (inline asm) vs limit in thread_local | +0.28 to +0.42 (+46% to +79%) | +0.26 to +0.33 (+21% to +37%) |
| SP (address of a local, safe Rust) vs thread_local limit | +0.56 to +0.67 (+88% to +125%) | not measured |
| depth counter in thread_local (inc/dec) | +0.80 to +0.90 (+123% to +167%) | +0.74 to +0.95 (+63% to +84%) |
| same, decremented by a `Drop` guard | +0.76 to +0.89 | not measured |
| shadow stack push/pop (thread_local index) | +0.80 to +0.88 | not measured |
| `stacker::remaining_stack()` check | +0.92 to +1.03 (+146% to +192%) | not measured |
| `stacker::maybe_grow` (heap stack segments) | +0.87 to +1.05 (+141% to +196%) | +0.84 to +0.94 (+68% to +107%) |

- Caveat: LLVM turned one of fib's two recursive calls into a loop (the unguarded `fib_plain` has one `bl` instruction), so its baseline is unusually fast. Guards that do work after the call (decrementing a counter) block that transformation, and that is part of their measured cost.
- A safe guard works: an SP check against a limit set at thread entry, on a thread with an 8 MiB stack, caught a runaway recursion as a typed `StackOverflowError` twice in a row, and the process carried on. — [local check](./error_model_bench/results.txt)
- A minimal compiled frame uses about 32 bytes: a small guarded function reached depth 30,720 in a 960 KiB budget. — [local check](./error_model_bench/results.txt)

**Precedents**
- Go: `debug.SetMaxStack` defaults to 1 GB on 64-bit and 250 MB on 32-bit systems. If a goroutine exceeds the limit while growing its stack, "the program crashes"; the output is "goroutine stack exceeds N-byte limit" followed by "fatal error: stack overflow". — [pkg.go.dev runtime/debug](https://pkg.go.dev/runtime/debug), [golang.org/issue/41228](https://golang.org/issue/41228). Other run-time errors in Go are panics that `recover` can catch (next item).
- Go spec: "Execution errors such as attempting to index an array out of bounds trigger a run-time panic equivalent to a call of the built-in function panic with a value of the implementation-defined interface type runtime.Error", and `recover` in a deferred function stops the panicking sequence. — [Go spec, Run-time panics / Handling panics](https://go.dev/ref/spec#Run_time_panics)
- JVM: `StackOverflowError` extends `VirtualMachineError` → `Error` → `Throwable`, so `catch` can catch it. — [Java SE 21 API](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/lang/StackOverflowError.html). HotSpot protects a yellow zone and a red zone at the end of each thread stack. JEP 270 (Java 9) added a reserved zone so that critical sections can finish, but it explicitly doesn't aim to make non-privileged code robust against stack overflow. — [JEP 270](https://openjdk.org/jeps/270)
- .NET: a `StackOverflowException` can't be caught with try/catch and terminates the process; catching was possible in .NET Framework 1.0/1.1 and removed in 2.0. — [Microsoft docs](https://learn.microsoft.com/dotnet/api/system.stackoverflowexception); version history per [Aleph Security](https://alephsecurity.com/2018/10/22/StackOverflowException)
- CPython: C-stack protection arrived in 3.12 (a counter) and was changed in 3.14 to check the actual stack pointer against bounds obtained from the OS. Between the soft and hard limits it raises `RecursionError` ("Stack overflow (used … kB)"); past the hard limit it is a fatal error ("Unrecoverable stack overflow"). Stack used between two checks must stay below `_PyOS_STACK_MARGIN_BYTES`. — [CPython InternalDocs/stack_protection.md](https://chromium.googlesource.com/external/github.com/python/cpython/+/03503dccd0aa6d8895614e80ae9c78c03c813d45/InternalDocs/stack_protection.md)
- JavaScript engines: deep recursion throws a catchable `RangeError: Maximum call stack size exceeded`. — [WebKit deep-recursion test](https://webkit.googlesource.com/WebKit/+/master/LayoutTests/js/dom/deep-recursion-test-expected.txt)
- Wasmtime/Cranelift: on entry to a wasm function, the generated code checks the stack pointer against `stack_limit` in `VMRuntimeLimits` (reached through the context pointer), without counting the function's own frame. Host calls aren't checked, and the limit is described as a resource limit rather than a safety feature. — [wasmfx/wasmfxtime#122](https://github.com/wasmfx/wasmfxtime/issues/122). It surfaces as a catchable `Trap::StackOverflow` — [wasmtime Trap docs](https://docs.wasmtime.dev/api/wasmtime/enum.Trap.html). A frame-size accounting bug in the check was reported in [wasmtime#8331](https://github.com/bytecodealliance/wasmtime/pull/8331).
- Swift's rationale classes stack overflow and running out of memory as "universal errors" and argues against making them recoverable: "It is inherently dangerous to implicitly volunteer functions for unwinding from an arbitrary point." — [Swift ErrorHandlingRationale](https://github.com/swiftlang/swift/blob/main/docs/ErrorHandlingRationale.md)
- Midori: stack overflow means abandonment. Because Midori used linked stacks, running out of stack was "physically identical" to running out of heap. — [Duffy](https://joeduffyblog.com/2016/02/07/the-error-model/)
- Zig: no runtime detection. An open request asks for detection with a partial stack trace (issue #1616), and a static bound on stack use is planned (issue #157). — [ziggit thread](https://ziggit.dev/t/should-i-be-getting-a-stack-trace-on-stack-overflow/11860), [llvm-dev 2018](https://lists.llvm.org/pipermail/llvm-dev/2018-July/124585.html)
- Rust interpreters: Rhai limits call depth to 64 by default (8 in debug builds) and returns an error result when it is exceeded; nesting depth is limited separately because deep nesting can overflow inside the call limit. — [Rhai book: max call stack](https://rhai.rs/book/safety/max-call-stack.html), [max statement depth](https://rhai.rs/book/safety/max-stmt-depth.html). Boa's default recursion limit was raised from 400 to 512, with a stack-size limit of 1024 (third-party mirror, unverified). — [Boa commit mirror](https://code.fanruan.com/github/boa/commit/cec98927f32684497f6b73c5a5a6b192d999a540)
- rustc protects its own deep recursion with `ensure_sufficient_stack` = `stacker::maybe_grow(RED_ZONE = 100 KiB, STACK_PER_RECURSION = 1 MiB)` and warns that it "causes a little bit of overhead", so it shouldn't be sprinkled around carelessly. — [rustc_data_structures/stack.rs](https://doc.rust-lang.org/stable/nightly-rustc/src/rustc_data_structures/stack.rs.html)

### Inferences
- **How sound each option is:**
  - *Guard page plus a signal handler that unwinds:* not sound. The fault can happen at any instruction, including in a prologue or inside `malloc` while it holds a lock. Rust doesn't support unwinding out of a signal handler, and std itself aborts. The JVM can do it because its JIT controls every instruction and probes the stack ahead; Rust code generated through LLVM can't.
  - *Depth counter alone:* only sound if (maximum frame size × limit) is smaller than the stack. Compiled frame sizes vary per function and with inlining, and dat-rt and std frames add to them, so a counter can't promise it never aborts.
  - *SP check against a limit:* sound if a red zone below the limit covers the largest stack use between two checks. That includes non-recursive callees and dat-rt/std calls (CPython's margin rule). A red zone of 256 KiB should cover normal code. Pathological frames, such as a huge tuple held by value, would still reach the guard page and abort.
  - *`stacker::maybe_grow`:* sound (rustc ships it), but about 1 ns per call, and it allocates a new segment through `mmap` whenever the red zone is reached.
- **Which functions to guard:** build a conservative call graph in which an indirect call (closure through `Rc<dyn Fn>`, an interface method through `dyn Trait`) is an edge to every compatible address-taken target. Guard the functions in non-trivial strongly connected components; leaf functions are never guarded. Programs without recursion pay nothing, and recursive programs pay the per-call cost from the table on recursive calls only.
- **Known limit:** dat-rt runs `main`, each test and (later) each task on a thread it creates with a fixed stack size. The limit is the stack pointer at thread entry minus the size, plus the red zone. On 64-bit systems a large reserved stack (64 MiB or more) only commits pages that are touched. That is my general knowledge, not measured here. A plain static limit works while dat is single-threaded. Once concurrency arrives, the limit moves to thread-local storage (measured +0.26 to +0.42 ns on macOS; Linux should be cheaper but I didn't measure it) or into a context pointer passed as an argument, the way Wasmtime does.
- **Same depth in both modes?** Not with an SP-based limit. A tree-walking interpreter uses much more native stack per dat call than compiled code (about 32 bytes for a minimal compiled frame; the interpreter's frame size wasn't measured). Two conformant positions:
  - (i) The depth at which the error fires is implementation-defined. The conformance runner compares only that both modes raise `StackOverflowError`, and test programs don't print once per recursion level.
  - (ii) A fixed logical depth limit, kept by a hidden depth argument (+0.05 to +0.22 ns per call), with the SP check as a backstop. Both modes then fail at the same depth, on every OS.

  (ii) gives agents reproducible behaviour but costs two checks per guarded call.
- **Interpreter:** the same SP check goes in its call routine and in its recursive evaluation of deeply nested expressions. A bytecode VM with frames on the heap avoids native recursion for dat-to-dat calls, but callbacks from dat-rt built-ins (`map`, `sort`) still recurse on the native stack.
- **Catch sites near the limit:** `catch` and `finally` run in the frame that holds the `try`, after the deeper frames have unwound. They run at depth d−1 or shallower, so the red zone gives them room, as CPython's soft and hard limits do.

### Gaps
- No Linux or x86 numbers for thread-local storage or SP checks. I found no published per-call cost of Go's or Wasmtime's prologue checks. Interpreter frame size depends on a design that doesn't exist yet. The Boa defaults come from a third-party mirror.

---

## 4. Out of memory (settled: abort in both modes)

### Takeaway
Rust's default on allocation failure is "print a message and abort", and the only stable way to change it is to replace the global allocator. The `-Zoom=panic` route was removed in December 2025, and `set_alloc_error_hook` is still nightly-only. To keep the abort clean and identical, dat-rt can provide one global-allocator wrapper that both modes link: on failure it prints a dat-formatted message and calls `std::process::abort()`. Rust's "capacity overflow" panic, raised when a collection would exceed `isize::MAX` bytes, should go to the same fatal path.

### Cited Findings
- `handle_alloc_error` with std: "print a message to standard error and abort the process. This behavior can be replaced with `set_alloc_error_hook` and `take_alloc_error_hook`. Future versions of Rust may panic by default instead." — [std::alloc::handle_alloc_error](https://doc.rust-lang.org/std/alloc/fn.handle_alloc_error.html)
- `set_alloc_error_hook` is a nightly-only experimental API (tracking issue #51245). — [std docs](https://doc.rust-lang.org/std/alloc/fn.set_alloc_error_hook.html), [rust#51245](https://github.com/rust-lang/rust/issues/51245)
- The `oom=panic` tracking issue (RFC 2116) was closed on Dec 10, 2025 via PR #147725 ("remove_oom_panic"). A maintainer noted that `-Zoom` is silently ignored when libstd is dynamically linked, while `set_alloc_error_hook` needs no special compiler integration. — [rust#43596](https://github.com/rust-lang/rust/issues/43596), [rust#147725](https://github.com/rust-lang/rust/pull/147725)
- Verified locally: `try_reserve(usize::MAX)` returns `Err("memory allocation failed because the computed capacity exceeded the collection's maximum")`, while `Vec::with_capacity(usize::MAX / 4)` raises a "capacity overflow" panic that `catch_unwind` catches. — [local check](./error_model_bench/results.txt)
- Midori: OOM is abandonment, with a rare opt-in recoverable form (`try new byte[...] else catch`). — [Duffy](https://joeduffyblog.com/2016/02/07/the-error-model/)
- Erlang: the `max_heap_size` process flag (2016) can kill a process that exceeds its heap. The thread reports that raising an exception was rejected because catch-all code could swallow it; an untrappable exit signal is used instead. — [erlang-questions, "Max heap size", April 2016](https://erlang.org/pipermail/erlang-questions/2016-April/088960.html)

### Inferences
- **Identical in both modes:** dat-rt defines `#[global_allocator]` as a thin wrapper over `System`. On a null return it writes one line, for example `fatal: out of memory (allocating N bytes)`, and calls `std::process::abort()`, which is stable. The interpreter binary links the same dat-rt, so message and exit status match (134 from SIGABRT on Unix). This needs no nightly API, but a thin wrapper may cost something on every allocation (not measured).
- **Capacity overflow:** dat-rt's list, map and set growth should call `try_reserve` and send its `Err` to the same fatal path. Otherwise, a huge `list` request becomes a catchable Rust panic in compiled mode but may behave differently in the interpreter, depending on how each mode sizes its buffers.
- **Exit statuses to document:** an uncaught dat error exits with 1 (§12); an OOM abort gives 134. On Linux with memory overcommit, the kernel OOM killer usually sends SIGKILL (137) before `malloc` ever fails, and no runtime can intercept that. That is general knowledge, not checked against a source here.
- An optional memory cap, where the dat-rt allocator counts bytes and treats a configured limit as OOM, would make OOM deterministic in tests and the same on every OS. It keeps "abort" semantics, so it matches the owner's Q2 answer.

### Gaps
- Go's and Swift's OOM behaviour wasn't fetched. The cost of a global-allocator wrapper wasn't measured.

---

## 5. Integer overflow checks: cost

### Takeaway
The spec already requires checked `int` arithmetic, so the open question is cost and mitigation. On integer kernels, checked arithmetic that unwinds on overflow costs +1% to +250%. The big losses come from lost auto-vectorization (+236% to +249% on a vectorizable sum, +132% to +140% on i64 matrix multiply). Published whole-program figures for C are +8.7% to +11.8% on SPEC CINT2006 (2016). The M8 target of ≤1.2× hand-written Rust with checks on is plausible for mixed programs, but integer-vectorizable loops will exceed it unless the compiler removes checks there.

### Cited Findings
- Spec: `int` is a 64-bit signed integer; overflow throws `RangeError` and doesn't wrap; `int % 0` throws `RangeError`. — [03-operators.md](../../reference/03-operators.md), [01-builtin-types.md](../../reference/01-builtin-types.md)
- **Local measurements**, wrapping versus checked arithmetic that unwinds on overflow, 3 runs:
  - sum of 1M `i64` (vectorizable): +236% to +249%
  - 256×256 `i64` matrix multiply: +132% to +140%
  - Collatz step count for 1..300k: +40% to +63%
  - sieve to 10M: +5% to +29%
  - polynomial hash mod p over 1M values: +1% to +13%
  - — [local benchmark](./error_model_bench/results.txt)
- John Regehr (April 2016, Clang 3.8, trapping checks): signed-overflow checking slows SPEC CINT2006 by 11.8% overall. Per benchmark it ranges from negligible (gcc, perl, omnetpp) to about 20% (sjeng, h264ref) and about 40% (hmmer). Patches by Sanjoy Das brought it to 8.7%. Code size grew by under 1%. "Most of the slowdown comes from integer overflow checks interfering with loop optimizations"; hmmer's hot loop isn't vectorized at all once checked. — [Regehr, "Efficient Integer Overflow Checking in LLVM"](https://blog.regehr.org/archives/1384)
- Dan Luu (about 2014, older): bzip2 compression was 1.28× slower with sanitizer diagnostics but 1.01× with trap-on-error. A simple sum loop was "about 6x slower" because SSE adds became scalar adds. His estimate was about 3% for SPECint-like code. — [danluu.com/integer-overflow](https://danluu.com/integer-overflow/)
- Swift: "arithmetic operators in Swift don't overflow by default. Overflow behavior is trapped and reported as an error"; `&+`, `&-` and `&*` opt into wrapping. — [The Swift Programming Language, Advanced Operators](https://raw.githubusercontent.com/swiftlang/swift-book/main/TSPL.docc/LanguageGuide/AdvancedOperators.md)
- Midori treated unintended overflow as a bug that leads to abandonment, with explicit `unchecked` opt-outs, and its compiler "aggressively optimized redundant checks away". — [Duffy](https://joeduffyblog.com/2016/02/07/the-error-model/)
- Verified locally: `i64::MIN.checked_rem(-1)` is `None`, so Rust treats `MIN % -1` as overflow even though the mathematical result is 0. — [local check](./error_model_bench/results.txt)

### Inferences
- The generated code should use explicit `checked_*` operations with a `#[cold]` throw of `RangeError`, not the `overflow-checks = true` profile flag. The flag would also check dat-rt and every dependency, and it panics with a `&str` message instead of a typed dat error.
- Mitigations for M8 that keep the semantics:
  - range analysis for loop induction variables and indexes
  - widening or deferred checks for **pure** reductions: accumulate with an overflow flag or in `i128`, and throw after the loop, only when no observable effect lies between the overflow and the deferred throw, because §12 keeps partial mutations visible
  - hoisting checks out of loops with known bounds, which the roadmap already lists
- `Math.floor(a / b)` on ints is "computed exactly" (§09), so floor division `MIN / -1` must also become a `RangeError`.

### Gaps
- I found no rigorous published benchmark of Rust's `overflow-checks` in release builds on real programs. The search turned up only a rustc source comment saying checks are "performance-neutral in almost all of the compiler" ([int_overflow.rs, search snippet only, not opened](https://rust.googlesource.com/rust/+/121f01b133571d2aa8ce0618bfee182ecdddfdba/compiler/rustc_serialize/src/int_overflow.rs)).

---

## 6. The interpreter: identical semantics

### Takeaway
The interpreter should propagate everything with a value: `eval` returns `Result<Value, Unwind>`, and `Unwind` carries thrown errors (declared and runtime alike) and `return`/`break`/`continue`. It shouldn't use Rust panics. Propagation then costs nanoseconds, partial mutations stay where they happened, and the dat stack trace comes from frames the interpreter already has. The hard part is the boundary with dat-rt. Built-ins that call back into dat (`map`, `filter`, `sort`) must leave the same state in both modes when a callback fails, and messages, error types and locations must come from shared dat-rt code.

### Cited Findings
- Spec requirements both modes must meet: changes made in the block before the error stay; a runtime error in `finally` replaces the passing error; an uncaught error prints its message, source location and a stack trace, exits with status 1, or fails the current test. — [12-errors.md](../../reference/12-errors.md), [20-testing.md](../../reference/20-testing.md)
- Error propagation by `Result` costs about 11 ns per frame, including a `String` allocated and freed in each frame (107–112 ns for 10 frames, 1,211–1,217 ns for 100). — [local benchmark](./error_model_bench/results.txt)
- Rhai, a Rust-hosted interpreter, ends a script that exceeds its call-depth limit with an error result instead of crashing. — [Rhai book](https://rhai.rs/book/safety/max-call-stack.html)
- Zig's error return traces are built while the error propagates, and only when the call is passed directly to `try`, `catch`, `if` or `return`. That is a model for collecting frames during `Result` propagation without per-call cost. — [Zig commit by Cody Tapscott](https://git.jakstys.lt/motiejus/zig/src/commit/3007fdde45868142654d0bfa59bc0e17e5f24a1c)
- The roadmap puts float printing, sort order, map order, string methods and error types in the shared dat-rt crate, so the modes differ only in how they step through the IR. — roadmap, "One front end, one runtime, two ways to run"

### Inferences
- **Shape:** `enum Unwind { Throw(DatError), Return(Value), Break(LabelId), Continue(LabelId) }`. `DatError` holds the type tag, the error value, the throw location and a trace. `try`/`catch` is a `match` on `Err(Unwind::Throw(e))`; `finally` runs before the pending `Unwind` is re-raised. Partial mutation happens naturally because the interpreter mutates its frame slots in place.
- **Callbacks across dat-rt:** dat-rt's higher-order built-ins take fallible callbacks, `FnMut(..) -> Result<R, Unwind>`. The interpreter returns `Err` from them. Compiled code passes closures whose error type can't occur, and its runtime errors unwind through dat-rt instead. So that both modes leave the same state when a callback fails, each such built-in must reach the same state through an early `Err` return as through an unwind passing through it. Writing them as explicit loops with no `Drop`-based fix-ups in dat-rt achieves that. Whether a list is left half-sorted after a throwing comparator is a semantics question for the owner (Decision D8).
- **Stack traces in dat terms:** the interpreter has the frames. Compiled code can (a) capture raw return addresses at the throw (about 3.7 µs on macOS, roughly 60% on top of a 6 µs throw) and map them to dat function names and lines only when printing an uncaught error, (b) keep a shadow stack (+0.80 to +0.88 ns per call with thread-local storage on macOS), or (c) add frames during propagation (only possible on the `Result` path). (a) puts no cost on calls. Release builds must keep line tables, and inlined frames need inline debug info to appear.
- **Messages:** every runtime error is built by one dat-rt constructor per kind (for example `RangeError: index 5 out of range for length 3`, from §10), so the text can't differ between modes.

### Gaps
- No measurements of an interpreter prototype. Whether LTO-inlined frames show up correctly in release-mode backtraces with line-tables-only debug info wasn't verified.

---

## 7. Mapping `try`/`catch`/`finally` and `return`/`break`/`continue` onto `catch_unwind`

### Takeaway
Lower each `try` block to a closure run under `catch_unwind(AssertUnwindSafe(..))`. The closure returns `Result<Flow<T>, DeclaredErrors>`, where `Flow` carries normal completion, `return`, labeled `break` and labeled `continue` out as values. `finally` becomes plain code after the call. This costs nothing on the success path (measured), avoids double-panic aborts, and keeps borrows simple because the closure is called at once.

### Cited Findings
- `catch_unwind` with nothing thrown: no measurable cost. — [local benchmark](./error_model_bench/results.txt)
- Spec restrictions that make the lowering easy: a `try` needs at least one `catch` or a `finally`; `finally` can't contain `return`, `break`, `continue`, `throw` or `try` calls; `catch (e: Error)` and `catch (e)` must be last; a `catch` for a declared error that nothing can throw is a compile error. — [12-errors.md](../../reference/12-errors.md)
- `AssertUnwindSafe` is the standard way to pass closures that capture `&mut` into `catch_unwind`. — [std::panic::catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)

### Inferences
- **Lowering sketch** (illustrative, not compiled):
  ```rust
  enum Flow<T> { Normal, Return(T), Break(u32), Continue(u32) }
  let r = std::panic::catch_unwind(AssertUnwindSafe(|| -> Result<Flow<R>, Declared> {
      /* try body: `return x` => return Ok(Flow::Return(x)); `break 'l` => return Ok(Flow::Break(L));
         `try f()` => f()?  (declared errors leave as Err) */
      Ok(Flow::Normal)
  }));
  // catch clauses: Err(payload) => match payload.downcast::<DatError>() { runtime kinds ... },
  //                Ok(Err(declared)) => match declared { ParseError(e) => ..., }
  // finally: plain statements here, outside any landing pad
  // then re-raise: resume_unwind(payload) / return Err(d) / return v / break 'l / continue 'l
  ```
- Catch clauses must run outside the closure, after it returns. Then a `throw e` inside `catch` is an ordinary throw, and `finally` still runs after it: store the pending outcome, run `finally`, then re-raise.
- Conversion to `Error` (§12) happens at `catch (e: Error)` and at the boundary of a function declared `throws Error`. For runtime errors it reads the payload's `toString()`.
- The test harness wraps each test body in the same machinery on a dat-rt thread. An escaping error fails the test without aborting the runner, consistent with the "torture suite" exit check.
- Nested `try` blocks become nested closures. Generic code size grows with nesting depth, but each level costs nothing at run time when nothing is thrown.

### Gaps
- The sketch wasn't compiled against Rust's borrow checker for every case: closures capturing `mut` parameters together with locals the `catch` clauses use. M0 should spike it.

---

## 8. Precedents and design philosophy

### Takeaway
No mainstream system makes every failure catchable. They differ only in where they draw the line:

| Language | Bugs (overflow, index, null) | Stack overflow | Out of memory |
|---|---|---|---|
| JVM/Kotlin | catchable | catchable (`StackOverflowError`) | catchable |
| Go | recoverable panics | fatal | — |
| .NET | catchable | fatal | — |
| Swift | trap (fatal) | crash | — |
| Python | catchable | `RecursionError` up to a hard limit | — |
| Midori | abandonment | abandonment | abandonment |
| Rust | panics that are catchable only with `panic = "unwind"` | abort | abort |
| Erlang | process isolation | — | heap limit kills the process |

("—" means not covered by the sources fetched for this note.)

dat's position (catchable runtime errors, a cheap catchable `StackOverflowError`, OOM as the documented exception) is close to JavaScript and Python, and stricter than Go and .NET on stack overflow. It is achievable in Rust, but only with documented exceptions.

### Cited Findings
- Midori sorted bugs (incorrect cast, null dereference, out-of-bounds index, divide by zero, unintended overflow, OOM, stack overflow, contract and assertion failures) into **abandonment**, which tears down the context without running more user code. Its argument: after a bug, "All data structures reachable by this code are now suspect", and catching everything "encourages very bad behavior". — [Duffy, "The Error Model"](https://joeduffyblog.com/2016/02/07/the-error-model/)
- Swift's four classes: simple domain errors (optionals), recoverable errors (`throws`/`try`), universal errors (OOM, stack overflow; not made recoverable) and logic failures (out-of-bounds, forced `nil` unwrap; trap). — [Swift ErrorHandlingRationale](https://github.com/swiftlang/swift/blob/main/docs/ErrorHandlingRationale.md)
- Go: run-time errors are panics of type `runtime.Error`, recoverable with `recover` — [Go spec](https://go.dev/ref/spec#Run_time_panics). Exceeding the stack maximum crashes the program — [runtime/debug](https://pkg.go.dev/runtime/debug).
- JVM: `StackOverflowError` is a catchable `Error` — [Java SE 21](https://docs.oracle.com/en/java/javase/21/docs/api/java.base/java/lang/StackOverflowError.html). .NET: a stack overflow can't be caught since 2.0 — [Microsoft docs](https://learn.microsoft.com/dotnet/api/system.stackoverflowexception). Python 3.14: `RecursionError` at the soft limit, fatal at the hard limit — [CPython stack_protection.md](https://chromium.googlesource.com/external/github.com/python/cpython/+/03503dccd0aa6d8895614e80ae9c78c03c813d45/InternalDocs/stack_protection.md).
- Erlang's runtime team chose to kill a process that exceeds `max_heap_size` with an untrappable signal rather than raise an exception, so catch-all code can't hide it. — [erlang-questions, April 2016](https://erlang.org/pipermail/erlang-questions/2016-April/088960.html)
- Rust: `catch_unwind` is not meant as a general try/catch ([std docs](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)); with the `unwind` handler, panics from `panic!` and std are recoverable, but not every failure is ([Rust Reference](https://doc.rust-lang.org/reference/panic.html)).

### Inferences
- dat already sits between Swift and Midori on one side (bugs are catchable, but are a separate category from declared errors and need no marking) and Java on the other (everything catchable). Its reason, "one bad input doesn't have to stop the program", is the request-handler argument that Go's `recover` and Java servlet containers rely on.
- Even with every measure in this note, these failures remain uncatchable:
  1. OOM: accepted by the owner
  2. SIGKILL from the OS OOM killer, and hardware faults
  3. stack overflow inside Rust code that has no check, if a frame is larger than the red zone
  4. targets without unwinding (wasm today)
  5. foreign exceptions and FFI, once Rust crates can be called

  AGENTS.md could name these explicitly (see D11).
- The Midori and Erlang argument that catch-all handlers hide bugs applies to `catch (e: Error)` in dat. A mitigation that changes no semantics: the uncaught-error printer and a lint could flag `catch (e: Error)` blocks that discard `AssertionError`. That is an option for the owner, not a recommendation to change the spec.

### Gaps
- Kotlin-specific docs weren't fetched; I inferred Kotlin's behaviour from the JVM. Go's OOM behaviour wasn't fetched.

---

## Decisions for dat-lang

Each decision lists options, evidence with numbers, a recommendation, its cost, the milestone that needs it, and whether it changes the spec. Spec changes are **questions for the owner**, not assumptions.

### D1. How runtime errors propagate in generated Rust
- **Options:**
  - (a) unwinding for both declared and runtime errors
  - (b) `Result` everywhere
  - (c) hybrid: declared errors as `Result` with `?`, runtime errors as unwinding with a typed payload
- **Evidence:**
  - Option (b): the success path costs +0.38 to +0.66 ns per non-inlined call (+9% to +17% on a struct-returning chain) and +25% to +66% against unwinding on call-dominated checked recursion. A failing call costs about 11 ns per frame. It needs no unwinding, which matters for wasm.
  - Options (a) and (c): `catch_unwind` costs 0 ns when nothing is thrown. A caught throw costs 5.8–6.6 µs plus about 0.96 µs per frame on macOS (about 2.4 µs per frame with destructors); on Linux 1.84 µs (published).
  - Option (a) also makes expected declared errors pay microseconds.
  - Real workload: success-path throughput 0% to +40% in favour of panics in serde (published).
- **Recommendation:** (c), as the roadmap proposes. Keep `try?` on built-ins non-unwinding (`xs.get(i)`), and add a lint or `dat explain` guidance against catching runtime errors in hot loops.
- **Cost:** `panic = "unwind"` must stay pinned (+2.3% text and +4.1% file size on the test binary). Each throw costs microseconds.
- **Milestone:** M0 spike. Settled by **M1, not M4**: M1 already makes `int` overflow and `% 0` throw `RangeError`, so the throw primitive must exist then.
- **Spec change:** none.

### D2. Throw primitive, payload and panic hook
- **Options:**
  - (a) `panic_any(payload)` plus a silent global hook
  - (b) `resume_unwind(Box<DatError>)`, which never calls the hook, plus a hook kept for genuine Rust panics only
  - (c) a custom throw in the style of Lithium (published at 556 ns against 1.84 µs on Linux)
- **Evidence:**
  - (b) is 0.6–0.7 µs cheaper per throw than (a) on macOS and is documented to skip the hook.
  - (c) is a third-party crate built on unstable internals (published as a proof of concept, then a crate).
  - Rust-originated panics exist ("capacity overflow"), and only the hook sees their message and location.
- **Recommendation:** (b). Use one concrete payload type, `DatError` (kind, value, location, raw trace). Install one process-wide hook at startup that records the message and location of non-dat panics in a thread-local, so the catch site can convert them, and that prints only when nothing catches them. Revisit (c) in M8 if throw cost shows up in benchmarks.
- **Cost:** about 5.8 µs per throw on macOS, about 1.8 µs on Linux.
- **Milestone:** M0/M1.
- **Spec change:** **question for the owner**. What dat error type should a Rust-level panic from dat-rt or std become (a dat-rt bug, or "capacity overflow" if D4 doesn't route it to the OOM abort)? Options: `AssertionError` with an "internal error:" message (no spec change), or a new internal-error type (spec addition).

### D3. Stack overflow: how to make it catchable (Q1, answered "yes if cheap")
- **Options:**
  - (a) crash, as Rust does
  - (b) depth counter in thread-local storage
  - (c) depth as a hidden argument
  - (d) SP check against a limit at function entry, with the limit in a plain static, a context argument, or thread-local storage
  - (e) `stacker::maybe_grow` heap segments
  - (f) guard page plus signal handler that unwinds
  - (g) only a big thread stack (lowers the odds, not catchable)
- **Evidence, added cost per guarded call:**
  - (d) with a static limit: +0.02 to +0.17 ns (+2% to +19%)
  - (c): +0.05 to +0.22 ns (+4% to +41%)
  - (d) with a thread-local limit: +0.26 to +0.42 ns (+21% to +79%)
  - (b): +0.74 to +0.95 ns (+63% to +167%)
  - (e): +0.84 to +1.05 ns (+68% to +196%)
  - (f) is unsound in Rust; std aborts from its own handler.
  - Precedents for (d): Wasmtime and CPython 3.14.
  - Guarding only functions in recursive SCCs makes non-recursive code cost 0%.
  - A guarded deep recursion was caught twice as a typed error in the local test.
- **Recommendation:** (d) with the limit in a static while dat is single-threaded, moved to a context argument or TLS when concurrency is specified. Add to it:
  - dat-rt runs `main`, tests and later tasks on threads it creates with a fixed stack (for example 64 MiB reserved) and a 256 KiB red zone
  - SCC-only guarding over a conservative call graph that includes indirect calls
  - the same check in the interpreter's call path
  - **iterative drop glue generated for recursive structs, and iterative or stack-checked clone, eq, hash, compare and `toString` in dat-rt**, because a 1M-node list drop aborts today even inside `catch_unwind`
- **Threshold for choosing (a):** proposed: choose crashing if, with SCC-only guarding, the M8 suite's geomean overhead exceeds 1% or any single benchmark exceeds 5%, or on a target where the stack pointer and limit can't be read. Microbenchmarks suggest the geomean will land well under 1% unless the suite is dominated by tiny recursive functions. The recursive-drop fix is needed either way, because it also prevents aborts during ordinary unwinding.
- **Cost:** below 0.2 ns per recursive call. One `unsafe` inline-asm read of SP in dat-rt (generated code stays safe; the safe address-of-local variant costs +0.56 to +0.67 ns with a TLS limit). Iterative `Drop` code for recursive types.
- **Milestone:** M0 spike (already listed), M3 for the recursive-struct drop, M4 for the torture suite.
- **Spec change:** **question for the owner**. Add a `StackOverflowError` row to the §12 table (Kind: Runtime; thrown by: "a call nests deeper than the stack allows"), plus one sentence that `try?` doesn't cover it. The owner agreed to the type in principle; the row wording still needs approval.

### D4. Must stack overflow happen at the same depth in both modes?
- **Options:**
  - (i) depth is implementation-defined; conformance checks only that both modes throw `StackOverflowError`
  - (ii) a fixed logical depth limit (hidden depth argument, +0.05 to +0.22 ns per call) with the SP check as a backstop
  - (iii) (i) plus a guaranteed minimum depth, for example "at least 10,000 nested calls"
- **Evidence:** about 32 bytes for a minimal compiled frame (30,720 frames in 960 KiB), while interpreter frames are larger (not measured), so SP-based limits fire at different depths. Behaviour differs across engines in practice (Rhai 64, Boa 512, JS engines by stack size).
- **Recommendation:** (iii). Agents get a depth they can rely on, conformance cases avoid printing once per recursion level, and only one check per call is needed. Choose (ii) only if the owner wants bit-identical output for runaway recursion.
- **Cost:** (iii) needs a big enough interpreter thread stack for the guaranteed depth (for example 10,000 × a few KiB, or less than 64 MiB reserved).
- **Milestone:** M4.
- **Spec change:** **question for the owner** under (ii) or (iii): add a sentence to §12 or §06.

### D5. Out of memory: clean, identical abort (Q2, answered "abort")
- **Options:**
  - (a) Rust default ("memory allocation of N bytes failed", abort)
  - (b) `set_alloc_error_hook` (nightly-only)
  - (c) a dat-rt `#[global_allocator]` wrapper that prints a dat-formatted line and calls `process::abort()`, with "capacity overflow" routed to the same path through `try_reserve`
- **Evidence:** `-Zoom=panic` was removed in December 2025; the hook is unstable; `try_reserve` returns `Err` on stable (verified); "capacity overflow" is otherwise a catchable panic (verified). Erlang and Midori also make OOM fatal.
- **Recommendation:** (c).
- **Cost:** a thin wrapper on every allocation (not measured).
- **Milestone:** M2 (dat-rt collections).
- **Spec change:** **question for the owner**, only to approve wording. One sentence in §12, "Uncaught Errors", and an AGENTS.md exception: OOM prints `fatal: out of memory` and aborts (Unix status 134). Note that the OS OOM killer (SIGKILL) can end the process first.

### D6. Integer overflow checking
- **Options:**
  - (a) `overflow-checks = true` in the profile
  - (b) explicit `checked_*` operations with a cold `RangeError` throw in generated code
  - (c) (b) plus M8 work: range analysis, hoisting, deferred checks for pure reductions
- **Evidence:** kernels +1% to +250% (vectorization loss dominates); SPEC CINT2006 +8.7% to +11.8% in C (2016); code size under 1%.
- **Recommendation:** (b) now, (c) in M8. Track integer-vectorizable benchmarks separately against the 1.2× target.
- **Cost:** see the numbers above.
- **Milestone:** M1, then M8.
- **Spec change:** **question for the owner**. Should `int.MIN % -1` be `0` (mathematically exact) or a `RangeError` (Rust's `checked_rem` treats it as overflow)? §03 doesn't say.

### D7. Interpreter propagation
- **Options:**
  - (a) Rust panics in the interpreter too
  - (b) `Result<Value, Unwind>` with `Throw`, `Return`, `Break` and `Continue`
- **Evidence:** about 11 ns per frame against microseconds per throw. With (b) no hook or `UnwindSafe` issues arise, and frames for the dat trace are free. Rhai and CPython return errors as values.
- **Recommendation:** (b). Use panics in the interpreter only as a backstop for dat-rt bugs.
- **Cost:** a `match` per evaluation step (not measured).
- **Milestone:** M1.
- **Spec change:** none.

### D8. State after a throwing callback inside a built-in (sort, map and the like)
- **Options:**
  - (a) specify all-or-nothing: the built-in works on a copy and commits at the end, costing a copy when the value is uniquely owned
  - (b) "same elements, unspecified order", with a shared dat-rt algorithm so both modes match in practice
  - (c) specify the exact algorithm
- **Evidence:** §12 says "Changes made in the block before the error stay", which doesn't say what happens inside a single built-in call. For conformance, dat-rt must reach the same state through an `Err` return and through an unwind (section 6).
- **Recommendation:** decide before the conformance suite covers callbacks. (b) costs nothing; (a) is cleanest for agents.
- **Milestone:** M2 (callbacks) or M4.
- **Spec change:** **question for the owner**.

### D9. `try`/`catch`/`finally` lowering and stack traces in compiled mode
- **Options for lowering:**
  - (a) the closure, `Flow` value and `finally`-after-call scheme in section 7
  - (b) `Drop` guards for `finally`
- **Evidence:** a runtime error inside `finally` under (b) would be a panic during unwinding, which aborts (verified: exit 134). (a) costs 0 ns on the success path.
- **Options for traces:** capture raw addresses at the throw (about 3.7 µs) against a shadow stack (+0.80 to +0.88 ns per call).
- **Recommendation:** (a), and capture at the throw. Symbolize and map to dat names only when printing an uncaught error. Release builds keep line tables.
- **Milestone:** M0 spike, M4.
- **Spec change:** none.

### D10. Targets without unwinding (wasm)
- **Options:**
  - (a) no wasm target until Rust's wasm targets unwind by default
  - (b) nightly with `-Zbuild-std`
  - (c) a second codegen strategy using `Result` everywhere for abort-only targets
- **Evidence:** as of April 2026 wasm unwinding needs nightly, and the rustc book says there are no concrete plans for unwinding by default. Engines (Chrome 138, Firefox 131, Safari 18.4, Node 25) support modern exception handling.
- **Recommendation:** (a) until the owner names wasm as a target. If needed, (c) is the only stable path, and it costs +25% to +66% on call-heavy code.
- **Milestone:** after M8.
- **Spec change:** **question for the owner**: the spec doesn't list targets.

### D11. Documenting the exceptions to "every runtime error can be caught"
- **Evidence:** no surveyed system catches everything. Even with D3 and D5 these remain uncatchable: OOM (accepted), SIGKILL and hardware faults, oversized native frames beyond the red zone, abort-only targets, and foreign exceptions once FFI exists.
- **Recommendation:** list them once in §12 "Uncaught Errors" and as a parenthetical in AGENTS.md.
- **Milestone:** M4.
- **Spec change:** **question for the owner**, because it rewords an AGENTS.md rule.
