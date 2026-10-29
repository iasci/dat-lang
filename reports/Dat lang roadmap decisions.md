# Sharpen dat's roadmap, starting with fibonacci

**The draft roadmap's architecture holds up: one front end, a typed IR feeding both a tree-walking interpreter and a Rust code generator, one shared runtime crate, declared errors as `Result`, runtime errors as unwinding, and copy-on-write collections. About a dozen of its specifics are wrong, though, in ways that change a complexity class, abort the process or miss a budget, and its Hello World first milestone proves almost nothing.** The biggest fix is ordering. Last-use moves and owned-parameter inference must land in M2, not M8. Without them the spec's own recommended style (`acc = f(acc, x)`) runs **2,200× slower at 40k calls**, goes quadratic, and runs at a different complexity in the interpreter than in compiled code. Three Rust mappings need replacing. `Option<Box<T>>` for recursive fields **aborts the process on drop at 1M nodes, even inside `catch_unwind`**. `Rc<str>` makes `s += t` quadratic (**298 ms against 49 µs** for 40k appends). And lazily initialized `static` module constants can't hold `Rc` values at all. The owner made a catchable `StackOverflowError` conditional on it being cheap, and the condition is met: a stack-pointer check costs **0–4% on compiled `fib(35)`**, so the guard goes in at M0, and the owner has since approved the §12 wording. The interpreter path's millisecond budgets are reachable with an in-process test runner. The compiled path needs tiered budgets, because rustc's incremental floor alone is **0.27 s at 7k generated lines and 2.6 s at 110k**. M0 should become a fibonacci walking skeleton: a 68-line program, five companion programs and 17 exit criteria. Together they push a real, tiny version of every component through both run modes and show each AGENTS.md rule holding at small scale, using only constructs the spec already defines. The research raised 39 questions that touch the spec or project policy. This report settles none of them. The owner has since answered the eleven that M0 needs, and 28 remain open.

## Five owner decisions already narrow the design

The owner answered five questions while this research was under way, and this report treats them as settled. Each one removes a branch the draft roadmap ([roadmap]) left open, and each has a concrete consequence in the sections below.

| Settled decision | What it means in this report |
|---|---|
| Stack overflow becomes a catchable `StackOverflowError` if that can be done cheaply; otherwise crashing is acceptable. The §12 row is a spec addition, which the owner approved later (Q1). | The cost was measured and is cheap: 0–4% on `fib(35)` with a stack-pointer check. The guard is recommended for M0. The owner later approved the row's text and a guarantee of at least 10,000 nested calls (Q1, Q2). |
| Out of memory is the one uncatchable failure and aborts in both modes. | dat-rt installs one global-allocator wrapper, so both modes print the same message and exit the same way. Only the wording in §12 and AGENTS.md is open (Q3). |
| The millisecond goals apply only to the interpreter path (`dat check`, `dat run`, `dat test`). Anything that goes through rustc has its own budget. | The interpreter is the default test runner. `--compiled` runs in CI and on request, against the tiered budgets proposed below. |
| Compile-time type tags for interface values are accepted, and tricks that make things faster or cheaper are welcome. General reflection stays out. | Interfaces lower to closed-world enums with generated `__tag`/`__as_X` helpers instead of `Any`. Pure module constants are evaluated at compile time, and map-entry updates are rewritten to take-and-restore. |
| The terminal diagnostic format is the one for people and agents; `--json` is for tools. | One compact text layout of about six lines per diagnostic, and a JSON schema built for `dat fix`, the LSP, CI and harnesses. The draft roadmap's diagnostics panel already uses these labels. |

On 29 October 2026 the owner also answered the eleven questions M0 needs. They're marked as answered in [Questions for the owner](#questions-for-the-owner) and written into `reference/`.

Every local measurement in this report comes from one Apple M4 Pro running macOS (Darwin 24.6) with rustc 1.99.0. Source code and raw output are in the research folder: the [fib and interpreter benchmarks][b-fib], the [codegen and tooling benchmarks][b-cg], the [cargo rebuild case][b-cargo], the [value-semantics micro-benchmarks][b-val], the [error-model micro-benchmarks][b-err] and the [diagnostic size script][b-diag]. No Linux or x86 numbers were taken. macOS is unusually slow for thread-local storage and unwinding, and the interpreter numbers come from a 100-line toy walker, not from a dat interpreter. Read the ratios as indicative and the absolute times as orders of magnitude. That is why the budgets below name reference machines and ask to be recalibrated on the CI runner.

## Fast tooling: laziness and a resident checker beat faster hashing

dat's spec already has most of the rules that make the fastest checkers fast. What remains to gain is in how much gets recomputed and where the checker runs, not in how fast it hashes. Exported functions must declare return types "so the compiler can check each module without reading the bodies of the modules it imports" ([17-imports][spec-17]). That is the same firewall as TypeScript's `isolatedDeclarations`, and the same as rust-analyzer's rule that "typing inside a function's body never invalidates global derived data" ([rust-analyzer architecture](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)). With no overloading, operator resolution is one lookup. That avoids the disjunction blowups that made a 12-line Swift expression take **42 s** to type-check ([Hooper](https://danielchasehooper.com/posts/why-swift-is-slow/)). Explicit `implements`, methods inside the struct body, named re-exports instead of `export *`, and declaration-only modules all help as well ([tooling notes][n-tool]). The clearest evidence about what matters next is one PyTorch edit: **ty recomputed diagnostics in 4.7 ms, Pyright in 386 ms and Pyrefly in 2.38 s** ([Astral](https://astral.sh/blog/ty)). The gap comes from how much each tool recomputes. Every single-digit-millisecond incremental result in the survey (ty, Sorbet, Hack, Zig `--watch`) came from a long-lived process, never from a fresh CLI run.

The roadmap's line "modules check in parallel against their imports' signatures and cache by signature hash" has four holes. First, unannotated exported `const`s and exported arrow-function consts let a function body leak into an importer's types, which breaks the spec's own promise (Q18, Q19). Second, private functions with inferred return types make one body's type depend on another's inside a module, and the spec doesn't say what recursion through them does (Q20). Third, circular imports need strongly-connected-component handling for type aliases and inferred consts (Q21). Fourth, the canonical order of union members must come from their content, not from the order types were created, or parallel runs will print different messages. TypeScript 7 warns that changing its checker count "may rarely surface order-dependent results" ([TS 7.0](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)).

The recommended design adds a second level of early cutoff and moves the checker into a resident process. Both should be hand-rolled rather than built on salsa. The first level works per module. Each module gets a resolved interface: exported names, struct fields (including private fields and whether they have defaults), method and operator signatures, `implements` lists, `throws` lists and alias expansions. The interface is hashed after resolution, and spans are kept in a side table so that an edit above a declaration doesn't invalidate importers. If the hash is unchanged after a re-check, importers aren't touched. The second level works inside the module. Each function or test body caches its diagnostics and typed IR, keyed by its body hash and the signatures it uses. When a private function's return type is re-inferred, it is compared by value, and unchanged callers stay cached. That is salsa's "backdating" done by hand ([salsa algorithm](https://salsa-rs.github.io/salsa/reference/algorithm.html)). Salsa is the best-proven route to low LSP latency. But rust-analyzer's port to the new salsa ran **about 250 MB over its old memory use** in January 2025 ([salsa notes](https://hackmd.io/@salsa/SkNtN8Lv1e)), and Pyrefly, which avoids salsa, used less memory than ty on 31 of 53 packages ([Pyrefly](https://pyrefly.org/blog/speed-and-memory-comparison/)). An internal API shaped like queries keeps a later move to salsa mechanical. The process model follows from the evidence. M6 ships the in-process CLI with a memory-mapped, zero-copy cache and change detection by file metadata. M7 adds a server, shared by `dat check` and the LSP, that re-checks on file events before the agent asks. When gopls moved to an on-disk summary cache, its memory use fell **about 75%** ([Go blog](https://go.dev/blog/gopls-scalability)).

Local measurements show why both levels are needed. Spawning and waiting on an empty Rust binary took **1.23–1.40 ms**. `stat` on 500 files took 0.46–0.93 ms, and reading every file of a 500-file, 50k-line corpus took 3.2–7.5 ms (the probe is `src/main.rs` and `src/par.rs` in the [codegen and tooling benchmarks][b-cg]). A CLI whose cache trusts file metadata should land at about **4–6 ms** for a body-only edit (an estimate). A CLI that reads and hashes every file to detect changes lands at 9–15 ms and breaks a 10 ms budget. A server that has already re-checked on the file-watcher event answers in an estimated 2–3 ms. An edit that changes a module's interface, when many modules import it, can't meet 10 ms in any mode. That is why the budget has to be split by edit kind (see the corrections section).

Six more moves round out "beyond signature hashing", roughly in order of value per cost:

1. **Embed a precompiled standard-library interface in the `dat` binary.** Then nothing in std is parsed or checked at startup, which also protects the 5 ms interpreter-start budget. This is the idea behind Go's export data and salsa's "durability".
2. **Use an arena AST with `u32` IDs plus a lossless token-and-trivia vector per file**, not a rowan-style red-green tree. oxc's arena AST parses typescript.js in **26.4 ms against 139.3 ms for Biome**, with 40% less memory ([oxc benchmark](https://github.com/oxc-project/bench-javascript-parser-written-in-rust)). rust-analyzer's own docs say that memoizing red nodes "more than doubles" memory, and that incremental reparsing "doesn't actually matter much" ([syntax.md](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/syntax.md)).
3. **Check in parallel by phase** (parse, then interfaces, then all bodies with work stealing), on a persistent thread pool. Lexing 50k lines took 1.5–1.9 ms on one thread and 0.4–0.6 ms on 14 freshly started threads, so on small inputs thread startup eats most of the gain. TypeScript 7's fixed checkers duplicate work, and going **from 4 checkers to 8 gained only 1.41×** ([TS 7.0](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)).
4. **Make inference cheap by construction.** Check callbacks after the non-function arguments, so `reduce((sum, x) => sum + x, 0.0)` never backtracks, and check each expression once against its expected type. Sorbet's single-shot inference checks about 100k lines per second per core this way ([Sorbet internals](https://github.com/sorbet/sorbet/blob/master/docs/internals.md)).
5. **Re-print cached diagnostics from unchanged modules on incremental runs**, because agents need the complete error list from every run.
6. **Key the interpreter's IR cache and the generated Rust by the same per-module hashes**, so one invalidation scheme serves all three paths. Add a perf harness with a synthetic corpus that measures cold, warm, body-edit and interface-edit latency as p50 and p95 from M0.

## The interpreter: a typed-IR walker fits the budgets, and tests stay in one process

A tree walker over a typed IR with resolved slots is the cheapest design that meets the interpreter budgets. The data also shows that dispatch style matters much less than how values and variables are represented. In one controlled Rust experiment, the slowdown against native code was **6.4× for walking the AST, 4.8× for closure compilation, and 1.4× for a register VM** that needed unsafe code and a register allocator ([Mitchell](https://ndmitchell.com/downloads/slides-cheaply_writing_a_fast_interpreter-23_feb_2021.pdf)). Representation fixes beat dispatch changes elsewhere too. Replacing a `HashMap` environment with a `Vec` took one Rust tree walker from about 39 s to 3.7 s on `fib(35)` ([van Kooten](https://www.dannyvankooten.com/blog/2022/rewriting-interpreter-rust/)). Safe-Rust bytecode (loxido, 5.03 s) ran slower than Java's tree-walking jlox (4.28 s), until raw pointers brought it down to 1.20 s ([loxido](https://github.com/ceronman/loxido)). Locally, a 100-line toy walker with resolved slots ran `fib(30)` in **100.6 ms: about 37 ns per call, or 1.7× CPython 3.15**. A naive walker that looks variables up by name took 321.9 ms ([fib benchmarks][b-fib]). Static types narrow the gap further. The IR can emit `AddInt` and `LtIntConst` nodes that need no type-tag checks, which is the specialization CPython and Luau only get at runtime.

| Strategy | Evidence | Verdict |
|---|---|---|
| Walk the parsed AST, looking names up | naive local walker: `fib(30)` in 322 ms, 5.4× CPython | Reject |
| Walk a typed IR with resolved slots and explicit evaluation order | local resolved walker: 1.7× CPython; AST interpreters match bytecode ones ([Larose et al.](https://stefan-marr.de/papers/oopsla-larose-et-al-ast-vs-bytecode-interpreters-in-the-age-of-meta-compilation/)) | **Adopt in M0/M1** |
| Compile the same IR to closures | 6.4× → 4.8× (Mitchell); +10–15% at Cloudflare ([Cloudflare](https://blog.cloudflare.com/building-fast-interpreters-in-rust/)) | Upgrade only if a speed gate fails |
| Register bytecode VM | about 4–5× faster on calls; Wasmi's rewrite took "many months" ([Wasmi v0.32](https://wasmi-labs.github.io/blog/posts/wasmi-v0.32/)) | Only if benchmarks demand it |
| Threaded or tail-call VM | 7.4 ns per call, but unsafe, unaudited and relying on optimizations Rust doesn't guarantee ([Stitch](https://github.com/makepad/stitch)) | Reject: a VM bug would corrupt memory instead of raising a catchable error |
| Cranelift JIT | can't instantiate dat-rt's Rust generics at runtime | Reject |

The interpreter should share the runtime crate with compiled code, not mirror it. Concretely:

- **Values.** A tagged `Value` of 16 bytes, using thin-pointer `Rc`s.
- **Places.** Assignable expressions evaluate to places, so `xs.push(1)` mutates the frame slot directly.
- **Propagation.** `Result<Value, Unwind>`, where `Unwind` carries throws as well as `return`, `break` and `continue`.
- **Shared collections.** Collections are generic Rust (`List<T>`, `Map<K, V>`). The interpreter instantiates them with `Value`, and generated code instantiates them with concrete types.
- **Shared primitive operations.** `int_add`, `int_rem`, `float_to_string`, `str_index` and the like are `#[inline]` functions that return `Result`, and both modes call them. Built-ins that take callbacks are generic over a fallible `FnMut`.

Two alternatives fail. Using trait objects as the shared interface would add a vtable call to every collection operation in compiled code. Keeping separate implementations per mode is how divergence gets in: dart2wasm's integer parsing was "not compatible with the VM" until it reused the shared parser ([Dart SDK](https://dart.googlesource.com/sdk/+/c91fd18cc4341d5de99fd8484487d1a1f91fb235)). The interpreter must also execute the move and copy marks that the M2 ownership pass writes into the IR. Otherwise it runs `acc = f(acc, x)` in quadratic time while compiled code runs it in linear time ([interpreter notes][n-interp], [value-semantics notes][n-val]).

The test runner has to stay in one process. cargo-nextest runs each test in its own process for isolation ([nextest](https://nexte.st/docs/design/why-process-per-test/)). At **0.5 ms or more per process spawn** ([startup-time](https://github.com/bdrung/startup-time)), that model costs at least 500 ms per 1,000 tests, twice the roadmap's 250 ms budget, and macOS spawns are slower still. dat can isolate tests inside one process because tests "share no state" ([20-testing][spec-20]):

- Each test gets fresh frames on a pool thread.
- An internal panic is caught per test and reported as a compiler bug.
- An infinite loop is stopped by a cooperative flag checked on loop back-edges and calls.
- Each test's `print` output is buffered and emitted in file-and-line order, so both modes produce identical, diffable results.

The estimate is **15–60 ms per 1,000 small tests cold, and under 20 ms warm**. That is inferred from per-call costs; no benchmark of an in-process test runner was found. dat has no I/O, time or randomness yet, so a test's result depends only on its transitive IR closure, and a Go-style result cache keyed by that hash is exact today. Go's history shows the cache key "needed significant design work" once external inputs arrived ([golang/go#11193](https://golang.org/issue/11193)). Whether `dat test` skips cached tests by default is a spec question (Q30), because the spec says it "finds and runs every test".

Keeping two backends equal takes more machinery than the roadmap lists. The known sources of divergence all apply to dat:

- **Evaluation order.** For compound assignment on primitive types, Rust evaluates the right-hand side first, which its own Reference calls "unusual" ([Rust Reference](https://doc.rust-lang.org/reference/expressions/operator-expr.html)). So `xs[i()] += f()` would call `f` before `i` unless the shared IR fixes the evaluation order. The owner chose left to right (Q14).
- **Integer edge cases.** `i64::MIN.checked_rem(-1)` returns `None`, although the mathematical answer is 0 (Q9).
- **Float formatting.** Rust's `Display` for floats never switches to exponent notation, so dat's float formatter must live in dat-rt.
- **Standard-library drift.** Rust 1.81 replaced the standard sorts with versions that "may panic" when the comparator isn't a total order ([Rust 1.81](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/)). The std built into the interpreter and the user's rustc can therefore disagree, unless the toolchain is pinned or sorting, case mapping and number parsing are implemented in dat-rt (Q17).
- **Hash order.** Insertion-ordered maps and sets remove this source of divergence.

The countermeasures are:

- an IR that makes evaluation order explicit;
- primitive operations implemented only in dat-rt;
- a pinned toolchain in generated crates;
- per-test result comparison;
- a fault-injection self-test for the differential harness;
- a type-directed program generator by M4. Csmith's differential testing found **more than 325 previously unknown bugs** in C compilers ([Regehr](https://blog.regehr.org/?p=492)).

Two tempting upgrades should stay off the roadmap. A Cranelift JIT compiles about ten times faster than LLVM ([Cranelift](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/README.md)). But JIT-compiled code can't instantiate dat-rt's Rust generics for new types at runtime, so it would need a third, type-erased runtime, which breaks the argument that one runtime keeps the modes equal. Tail-call threaded dispatch gains only **1–5%** on modern CPUs once a compiler regression is accounted for ([Elhage](https://blog.nelhage.com/post/cpython-tail-call/)), and Rust's `become` is unstable, with stabilization hoped for in 2027 ([Rust goals 2026](https://goals.rust-lang.org/2026/tail-call-loop-match.html)). Startup time is dominated by checking, not by process creation: an empty Rust binary started in 1.2–1.7 ms locally. The 5 ms budget therefore holds for single-module and cached programs, provided that `dat run` checks only the modules reachable from the entry file, lowers function bodies on first call as wasmi does ([Wasmi v0.32](https://wasmi-labs.github.io/blog/posts/wasmi-v0.32/)), and reads std from the embedded interface.

## Rust codegen: rustc's incremental floor sets a tiered compiled budget

rustc sets the latency of the compiled path. Its incremental cost grows with crate size, because parsing, macro expansion, name resolution and dependency-graph bookkeeping run over the whole crate after every edit. A generator that emits dat-shaped Rust (structs, unions, interfaces, checked arithmetic, module constants and tests) measured the following on the M4 Pro ([raw results][b-cg-results]):

| Generated Rust | Clean build | Rebuild after one module changes | First launch (macOS) |
|---|---|---|---|
| 7.4k lines, one crate | 1.01 s | 0.27 s | 0.14 s |
| 36.8k lines, one crate | 4.27 s | 0.89 s | 0.15 s |
| 110k lines, one crate | 12.88 s | 2.62 s | 0.24 s |
| 110k lines, 10 crates in a star | 4.67 s | 0.51 s (leaf edit) / 1.21 s (base edit) | — |
| 110k lines, 30 crates in a chain | 12.75 s | 0.33 s (top edit) / 3.11 s (bottom edit) | — |
| 36.8k lines, incremental off | 3.68 s | 3.54 s | — |
| 36.8k lines, `codegen-units = 1` | 5.36 s | 2.48 s | — |
| 36.8k lines, `-Zthreads=8` (nightly only) | 2.24 s | 0.92 s | — |

Three conclusions follow:

1. **One crate holds a 3 s one-module budget up to roughly 100k generated lines, and a 1 s budget up to roughly 40k.** Beyond that, generated code must be split across crates. Splitting only helps edits near the top of the crate graph until Rust's "Relink, Don't Rebuild" goal lands; it targets 5–10× faster rebuilds when only function bodies change ([Fast Builds roadmap](https://goals.rust-lang.org/2026/roadmap-fast-builds.html)).
2. **Most of the popular levers do nothing for the edit loop.** The debuginfo level, the linker (0.07 s of a 0.85 s rebuild on macOS) and the parallel front end (1.9× faster clean builds, but nightly-only in 1.99) all left the one-module rebuild unchanged. Turning incremental compilation off made it 4× slower, and `codegen-units = 1` made it 2.8× slower.
3. **Cranelift is out for now, for a different reason.** It is nightly-only, and "unwinding on panics is not yet supported" on macOS or Windows ([cg_clif README](https://github.com/rust-lang/rustc_codegen_cranelift)), which breaks dat's runtime errors. Its end-to-end gain on Zed was only 5–12% ([Rust goal](https://goals.rust-lang.org/2026/improve-cg_clif-performance.html)).

The closest precedent agrees with these measurements. Feldera's SQL-to-Rust compiler cut a 100k-line release build from **1,618 s to 150 s** by emitting 1,106 crates named by content hash, after changing `codegen-units` "made no meaningful difference" ([Feldera](https://www.feldera.com/blog/cutting-down-rust-compile-times-from-30-to-2-minutes-with-one-thousand-crates)). For scale, 55% of Rust users wait more than 10 s for a rebuild ([Rust survey 2025](https://blog.rust-lang.org/2025/09/10/rust-compiler-performance-survey-2025-results)), so sub-second rebuilds would put dat well ahead of typical Rust.

The compiled budget should therefore be tiered and stated for named reference machines:

| Compiled-path measure | Budget |
|---|---|
| Warm edit to one module, until test results appear | ≤ 1 s up to 40k generated Rust lines; ≤ 3 s up to 150k (needs the crate split) |
| Clean `dat test --compiled`, runtime prebuilt | ≤ 5 s at 40k lines |
| `dat build --release` | ≤ 30 s at 40k lines |

Track the ratio of generated Rust lines to dat lines from M1, so these budgets can later be stated in dat lines. The build plan:

- **Crate layout.** Generate one crate through M5, with each dat module as a Rust module addressed by full paths, so a later split is mechanical. At M6, group the strongly connected components of the import graph into crates, above a bottom "types" crate that holds all structs, unions, interfaces and closed-world enums. One crate per dat module isn't possible: dat allows circular imports, Rust crates can't depend on each other in a cycle, and Rust's orphan and inherent-impl rules keep impls next to their types.
- **Driver.** Build through a generated Cargo workspace. A no-op Cargo run costs only 17–34 ms.
- **Dev profile.** Incremental on, default codegen units, `debug = "line-tables-only"`, `panic = "unwind"`.
- **Release profile.** Default to thin-local LTO. The roadmap's fat LTO with one codegen unit took **18.1 s against 5.9 s** on 37k lines, and its runtime benefit is unmeasured, so it belongs behind something like `dat build --max` until the M8 benchmarks justify it.
- **Test binary.** Link one test binary per project. On macOS each freshly linked binary costs about **0.15 s** on first launch while it is scanned.

The style of the generated code matters as much as the build settings:

- **No macros.** Macro expansion alone was 0.126 s of a 0.85 s incremental rebuild, and it isn't incremental. Emit `List::from_vec(Vec::from([..]))` instead of `vec![..]`, and direct `fmt::Write` calls instead of `println!`.
- **One Rust function per dat function**, with `#[cold] #[inline(never)]` error paths in dat-rt.
- **Plain loops** instead of iterator chains, and **`&dyn Fn`** at user-function boundaries.
- **`#![forbid(unsafe_code)]`** and crate-level lint allows.
- **Rewrite only files whose content changed**, because Cargo fingerprints by modification time.

Rust has no `#line` directive; the 2024 pre-RFC closed without becoming an RFC ([internals](https://internals.rust-lang.org/t/pre-rfc-line-attribute/21069)). So three things replace it:

- every runtime-checked operation carries a `&'static Loc`, which gives exact dat locations at near-zero cost;
- the generator writes a line map from `.rs` lines to `.dat` lines;
- rustc's JSON diagnostics on generated code are mapped back to dat locations and reported as internal compiler errors.

Every hard feature has a safe, direct Rust form; sketches of each compile on rustc 1.99 ([sketches][b-sketch]). Several of the roadmap's mappings change, because of newer Rust features or because the owner's decision on type tags allows a faster path:

| dat feature | Draft roadmap | Recommended | Evidence |
|---|---|---|---|
| Narrowing on field paths and after an early exit | `if let`, falling back to a checked unwrap; risk "high" | Bind the narrowed path to a fresh reference with let-else, if-let, let-chains, or a labelled block that breaks with the reference. Risk drops to medium. | let-else and labelled-block breaks are stable since 1.65 ([Rust 1.65](https://blog.rust-lang.org/2022/11/03/Rust-1.65.0.html)), let-chains since 1.88 in edition 2024 ([Rust 1.88](https://blog.rust-lang.org/2025/06/26/Rust-1.88.0)) |
| Unions and nullable types | a generated enum with one `From` per member; `Option<T>` | One enum per canonical, sorted member set, with `null` as an outer `Option`. Aliases become `pub type`, and `From` widening impls are generated only for pairs the program uses. | spec 18 makes unions flattened and order-free ([18-enums-and-unions][spec-18]) |
| Interface values | `Vec<Box<dyn I>>`, copied through a generated clone method | A closed-world enum when the whole program is visible. Fallback: `Rc<dyn I>` cloned on mutation. `__tag` and `__as_X` helpers instead of `Any`. | The enum was 1.9× faster on copy-heavy code (0.86 s against 1.61 s) and equal on pure dispatch; debug builds were 18–20% faster and binaries 2–9% smaller ([codegen notes][n-cg]) |
| Closures | move closures; `Rc<dyn Fn>` when stored | `&dyn Fn` for parameters, `Rc<dyn Fn>` for stored values. For dat-rt built-ins, choose between generic `F: Fn` and `&dyn Fn` by measurement in M2. | monomorphization was Feldera's main compile-time cost |
| Generics | monomorphized | Emit Rust generics with trait bounds; never monomorphize in the dat compiler | borrow checking plus type checking were 59% of a clean debug build |
| Module constants | a lazily initialized `static` with a re-entry guard | `thread_local!` with `OnceCell` and dat's own re-entry flag; evaluate pure initializers at compile time | `LazyLock` needs `Sync` values and poisons forever if its initializer panics ([LazyLock](https://doc.rust-lang.org/std/sync/struct.LazyLock.html)) |

The closed-world enum has limits:

- An enum is as large as its largest implementor, so variants above about 64 bytes should be boxed.
- A generic interface needs one enum per instantiation.
- Once code is split into crates, the enum has to live where it can see every implementor.
- Packages would open the world (Q24). So the `Rc<dyn I>` fallback stays, and the M5 conformance suite should run under both lowerings.

For module constants, `OnceCell` retries after an initializer fails while `LazyLock` poisons forever. Whether a failed initializer is retried or rethrown is therefore a spec choice, not a library accident (Q28).

## Runtime errors: unwinding is free until thrown, and stack overflow costs 0–4%

The roadmap's hybrid model is the right one, and it is cheaper than the alternatives exactly where cost matters. Measured on the M4 Pro with `panic = "unwind"` ([error-model benchmarks][b-err], [fib benchmarks][b-fib]):

| | When nothing fails | When an error is thrown |
|---|---|---|
| Runtime errors as unwinding (`catch_unwind` at each `try`) | no measurable cost: 0.742 ns per call with or without `catch_unwind` | 5.8–6.6 µs per caught throw on macOS, plus about 0.96 µs per unwound frame (about 2.4 µs if the frame has a destructor); 1.84 µs on Linux, from a published figure ([purplesyringa](https://purplesyringa.moe/blog/bringing-faster-exceptions-to-rust/)) |
| `Result` everywhere | +0.38–0.66 ns per non-inlined call: +9–17% on a call chain returning a struct, +25–66% against unwinding on checked recursion | about 11 ns per frame |
| Compiled `fib(35)` against checked hand-written Rust | 0.98–1.07× with unwinding | about 1.6× with `Result` |

A throw costs 200–300 times more than returning an error, and that trade fits dat's split between the two kinds. Declared errors are expected failures, so they stay `Result` with `?`. Runtime errors signal bugs or rare conditions, so they unwind. Hot loops already have a path that doesn't unwind (`try? xs[i]` lowers to `xs.get(i)`). Catching a `RangeError` for each element of a million-element list would take roughly 6–30 s on macOS, but that is a matter for a lint and `dat explain`, not a design flaw.

Three implementation choices remove most of the pitfalls of unwinding:

- **Throw with `resume_unwind(Box<DatError>)`.** It never calls the panic hook ([std](https://doc.rust-lang.org/std/panic/fn.resume_unwind.html)), and it measured 0.6–0.7 µs cheaper than `panic_any` with a silenced hook. Keep the single process-wide hook for genuine Rust panics, such as std's "capacity overflow". Which dat error type those become is an open question (Q4).
- **Lower `finally` to plain code that runs after `catch_unwind` returns**, never to a `Drop` guard. A panic inside `Drop` during unwinding aborts the process with status 134, which was verified locally.
- **Carry `return`, `break` and `continue` out of the `try` closure as a `Flow` value.**

M1 already makes `int` overflow and `% 0` throw, so this machinery has to exist in M0 or M1, not M4 ([error-model notes][n-err]). The PoC builds it.

Stack overflow can be caught cheaply, which meets the owner's condition. Rust aborts on stack overflow even inside `catch_unwind` (exit status 134, verified locally). Catching it with a guard page and a signal handler that unwinds isn't sound in Rust, because the fault can land in a function prologue or inside `malloc`. The sound and cheap option is the one Wasmtime and CPython 3.14 use: compare the stack pointer with a limit when a function is entered ([CPython stack protection](https://chromium.googlesource.com/external/github.com/python/cpython/+/03503dccd0aa6d8895614e80ae9c78c03c813d45/InternalDocs/stack_protection.md), [wasmtime Trap](https://docs.wasmtime.dev/api/wasmtime/enum.Trap.html)). Added cost per guarded call, measured locally:

| Guard | Recursive `fib`: added ns per call | Binary-tree sum: added ns per call |
|---|---|---|
| Stack pointer against a limit in a plain static | +0.02 to +0.10 | +0.03 to +0.17 |
| Depth passed as a hidden argument | +0.06 to +0.22 | +0.05 to +0.13 |
| Stack pointer against a limit in thread-local storage | +0.28 to +0.42 | +0.26 to +0.33 |
| Depth counter in thread-local storage | +0.80 to +0.90 | +0.74 to +0.95 |
| `stacker::maybe_grow` | +0.87 to +1.05 | +0.84 to +0.94 |

The recommended setup has three parts:

- **Guard only recursive functions.** Build a conservative call graph in which every indirect call counts as an edge to every compatible target, and guard only functions in non-trivial strongly connected components. Code without recursion then pays nothing.
- **Run dat code on threads that dat-rt creates.** `main`, each test and, later, each task get a large reserved stack (for example 64 MiB) and a 256 KiB red zone, which covers the dat-rt and std calls made between checks.
- **Use the same check in the interpreter's call path.**

On compiled `fib(35)`, the worst case for a per-call guard, the stack-pointer check cost **0–4%**. A depth counter in thread-local storage cost about 2×, and a global atomic counter about 3.3× ([fib benchmarks][b-fib]).

The catch is determinism. A limit in bytes fires at different depths in a tree walker and in compiled code (a minimal compiled frame is about 32 bytes), which is why the interpreter notes argued for a deterministic depth counter. The way to reconcile the two is one spec sentence: guarantee a minimum depth, for example at least 10,000 nested calls, and leave the exact overflow depth implementation-defined above it. That keeps one cheap check per call. If the owner wants identical depths in both modes, the affordable deterministic form is a hidden depth argument (+0.05 to +0.22 ns per call), with the stack-pointer check kept as a backstop. The owner chose the guaranteed minimum, 10,000 nested calls (Q2). Other languages split on this: the JVM and JavaScript engines make stack exhaustion catchable, while .NET and Go treat it as fatal.

The bigger hole is not recursion in dat code but recursion in Rust code that dat never sees. Dropping a 1M-node `Option<Box<Node>>` chain, the roadmap's mapping for nullable self-references, overflowed an 8 MiB stack and **aborted the process even inside `catch_unwind`**. The derived `Clone` aborted at 200k nodes ([value-semantics benchmarks][b-val], [error-model benchmarks][b-err]). No guard on dat functions sees this. It also fires during ordinary unwinding, when unwound frames drop their values. The compiler must therefore generate an iterative `Drop` for every type that can contain itself, including through `list` and `map`, and dat-rt's clone, equality, hashing, comparison and `toString` must be iterative or check the stack. This belongs in M3, together with recursive structs, and in the M4 torture suite, whatever is decided about the stack-overflow row.

Out of memory is settled as an abort; the only work left is making the abort clean and identical in both modes. Rust's `-Zoom=panic` was removed in December 2025, and `set_alloc_error_hook` is still nightly-only ([rust#43596](https://github.com/rust-lang/rust/issues/43596), [std docs](https://doc.rust-lang.org/std/alloc/fn.set_alloc_error_hook.html)). So dat-rt should define a `#[global_allocator]` that wraps `System`. When an allocation fails, it prints one dat-formatted line and calls `std::process::abort()`, which gives both modes the same message and Unix exit status 134. Collection growth should go through `try_reserve` into the same path. Otherwise `Vec::with_capacity(usize::MAX / 4)` raises a "capacity overflow" panic, which is catchable in one mode and possibly handled differently in the other. On Linux with memory overcommit, the kernel's OOM killer usually sends SIGKILL first, and no runtime can intercept that. An optional byte cap in the allocator would make OOM deterministic in tests while keeping the abort semantics.

Checked arithmetic is required by the spec, and its cost decides whether M8's 1.2× target is realistic. Locally it ranged from **+1% to +250%** on integer kernels. The losses concentrate where checks block vectorization: +236–249% on a vectorizable sum and +132–140% on an `i64` matrix multiply. For comparison, checks cost **+8.7–11.8%** on SPEC CINT2006 in C ([Regehr](https://blog.regehr.org/archives/1384)). Generated code should call explicit `checked_*` operations with a cold, typed throw, rather than turn on the `overflow-checks` profile flag, which would also check dat-rt and would panic with a string message. M8 then adds range analysis, hoisting of checks out of loops, and deferred checks for pure reductions.

Even with all of this, five failures stay uncatchable:

- out of memory (accepted by the owner);
- an OS SIGKILL or a hardware fault;
- a native frame larger than the red zone;
- targets without unwinding: wasm still needs nightly with `-Zbuild-std` ([rustc book](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html));
- foreign exceptions, once FFI exists.

No system surveyed catches everything, so §12 and AGENTS.md should name these five once (Q3).

## Value semantics: copy-on-write holds only if last-use moves land in M2

The spec's value model is coherent and fast in Rust, but only with an ownership analysis that the roadmap schedules six milestones too late. The spec's own guarantee settles the representation: "assigning or passing a collection never copies its elements by itself" ([02-variable-declaration][spec-02]). So do the measurements ([value-semantics benchmarks][b-val]):

| Operation | `Vec` with deep copies | `Rc<Vec>` with `make_mut` | Persistent vectors (imbl / rpds) |
|---|---|---|---|
| Copy a 100k-element list | 9.6 µs | 2.6 ns | 7.2 / 2.4 ns |
| Push 1M ints | 0.88 ms | 1.61 ms (1.84×) | 7–42× slower than `Vec` |
| Sum 1M ints | 76 µs | 75 µs | 10–27× slower |
| `xs[i] += 1` over 1M elements | 114–117 µs | 518 µs–1.23 ms when checked on every write; 116 µs when checked once, then `iter_mut` | 161–252× slower |
| Copy a 1000×1000 nested list and change one element | 190.8 µs | 0.76 µs | 0.94 µs (imbl) |

The recommendation is `Rc<Vec<T>>` behind an opaque `dat_rt::List<T>` (and `Rc<IndexMap>` for maps), with two codegen rules that recover plain `Vec` speed. First, check uniqueness once per statement or loop, not once per element. Second, lower `for (mut x of xs)` to `make_mut(xs).iter_mut()`, which is legal because the spec locks `xs` for the duration of the loop.

The ownership analysis decides complexity, not just constant factors. The spec recommends returning a new value over using `mut`. With parameters always passed as `&T` (the roadmap's mapping), `function append(xs, x) { let r = xs; r.push(x); return r; }` called as `acc = append(acc, i)` copies the whole list on every call. That took **210 ms for 40k calls, against 95 µs** when the parameter is owned and the caller moves `acc` at its last use: a 2,200× gap that grows quadratically. The spec's own grouping example in §16 (get the list, push, store it back; [16-map-methods][spec-16]) runs **21× slower at 50k items** as written (19.9 ms against 943 µs), because the map still holds the old list when it is pushed. The literature points the same way:

- Koka without its reuse and drop optimizations ran 2.45× slower on rbtree ([Perceus](https://www.microsoft.com/en-us/research/uploads/prod/2020/11/perceus-tr-v4.pdf)).
- Lean's geometric mean rose from 1.24 to 1.74 without reuse ([Counting Immutable Beans](https://arxiv.org/pdf/1908.05647)).
- Swiftlet, a research model of Swift's value semantics, lost most of its gap to Swift through "unnecessary reference counter checks" ([JOT 2022](https://www.jot.fm/issues/issue_2022_02/article2.pdf)).

One backward liveness pass over the shared typed IR in M2 produces four things at once:

1. the spec's "modified copy is never read" error, which is already an M2 item;
2. a move-or-clone decision at every copy site;
3. owned or borrowed parameters. Following Lean's heuristic, a parameter is owned when the body stores it, returns it, or copies it into a variable that is then mutated;
4. drop placement.

The same pass enables take-and-restore for get-copy-modify-store on map entries and fields. Both modes execute the IR's marks, so they share the same complexity. Codegen-only moves in M8 would leave the interpreter quadratic where compiled code is linear.

Strings need a different representation from the roadmap's `Rc<str>`. dat strings are immutable, but `s += t` rebinds `s`, so building a string in a loop is ordinary code. With `Rc<str>`, every `+=` allocates a new string: **298 ms for 40k appends, against 49 µs** for `String::push_str`, 54 µs for `Rc<String>` with `make_mut`, and 160 µs for `EcoString`. Small-string types copy in 0.3–0.4 ns when the string is stored inline. The recommendation is an opaque `dat_rt::Str` that stores small strings inline and larger ones in a refcounted heap buffer that appends in place when it is the only reference. In M1 this can wrap `EcoString` or `Rc<String>`. Later, a custom non-atomic buffer can replace it, with a niche so that `string?` stays 16 bytes. Every spec string operation maps directly onto Rust's `str`, because UTF-8 byte order equals code-point order ([RFC 3629](https://www.rfc-editor.org/rfc/rfc3629)).

Sets and maps should build on indexmap. Its lookups are as fast as `HashMap` with the same hasher (7.2 against 7.6 ms per 1M hits), it iterates ten times faster, and its equality already ignores order, as the spec requires. Its weak point is deletion. `swap_remove` is O(1) but moves the last entry into the hole, and the order-preserving `shift_remove` took **1.39 s for 10k removals** from a 100k-entry map, against 62 µs for `swap_remove`. The spec doesn't say which order survives a `delete` (Q25). If the answer is JavaScript's and Python's ("keys added after deletion are inserted at the end", [Python docs](https://docs.python.org/3/library/stdtypes.html#mapping-types-dict)), dat-rt needs a CPython-style map with tombstones, with `shift_remove` as a stopgap in M2 that has the same semantics. Hashing should use foldhash with a random seed per process; it is about 2× faster than SipHash on lookups. Because iteration follows insertion order, the seed never reaches program output in either mode.

Structs stay unboxed: copying one with five reference fields costs 4.0 ns, and `Copy` is derived when every field is a scalar. Recursive fields, though, should become copy-on-write boxes, `Option<Rc<T>>` with `make_mut`, like Swift's `indirect`. Copying a 10k-node list cost 226 µs with `Option<Box<T>>` and 2.9 ns with `Option<Rc<T>>`. Dropping a deep chain still needs the generated iterative `Drop` from the error-model section.

The roadmap's plan to "turn Rc into Arc inside dat-rt only" once concurrency arrives works only if every refcount in dat-rt goes through one `dat_rt::Shared<T>` from M0. The decision also comes earlier than the roadmap assumes. Module constants "computed once" plus tests that "run in parallel" already share values across threads, and Rust can't put an `Rc` in a `static`. Using `Arc` everywhere costs:

- 1.25× per clone, and 5× in batches;
- 3.8× on per-element writes when the uniqueness check isn't hoisted;
- **439 ns per operation when 8 threads contend for one count**.

The literature reports whole-program slowdowns of 5–59% from atomic-only counts in Koka. The long-term answer is the signed refcount header that Koka and Lean use inside their runtimes: positive for thread-local, negative for shared, and a sentinel for immortal objects. For M6 the choice is between that header and initializing constants once per thread, and it depends on whether a constant's initializer may have observable effects (Q28).

Some copies remain by design, so agents need to see them. Swift needed a language feature, yielding accessors (SE-0474), because get/set properties made appends "quadratic", "a frequent performance pitfall" ([SE-0474](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0474-yielding-accessors.md)). dat's `operator get`/`mut operator set` has exactly that shape, and no compiler rewrite can fix it without a spec change (Q26). Lean ships `dbgTraceIfShared` to find accidental sharing ([Lean reference](https://lean-lang.org/doc/reference/4.34.0-rc1/Run-Time-Code/Reference-Counting/)). dat needs three equivalents:

- dat-rt counts copy-on-write copies and bytes by source location in both modes, and reports them through `dat test --report-copies` and the JSON diagnostics;
- conformance cases check complexity by running at n and 4n;
- an M2 benchmark decides whether `dat_rt::List` uses a ring buffer, since `shift()` on a `Vec` makes the BFS loops agents write with it quadratic.

## Diagnostics: one compact text format, and an agent benchmark by M2

The empirical work on feedback to LLMs says that the first repair rounds and the quality of the feedback matter most. That makes the output of the first `dat check` the most important thing the toolchain produces. The findings:

- **Noisy static feedback is worse than none.** FeedbackEval measured single-round repair success of **57.9% with test feedback, 53.1% with a generic "please fix", and only 49.2% with pylint output**. Gains "stabiliz[e] after two to three iterations of repair" ([FeedbackEval](https://arxiv.org/html/2504.06939v2)).
- **Feedback quality limits repair.** Replacing a model's own feedback with an expert programmer's raised the number of repaired programs 1.58× ([Olausson et al.](https://arxiv.org/abs/2306.09896)).
- **Type errors dominate.** About 94% of LLM compile errors in TypeScript are type errors ([Mündler et al.](https://arxiv.org/abs/2504.09246v1)), so dat's type diagnostics will carry most of the load.
- **Unfamiliar languages cost iterations, not code.** Agents spent 1.16–1.69× Python's tokens in Rust, Java and OCaml, and 92% of one model's failed OCaml snapshots were compile errors ([tokenmaxxing study](https://arxiv.org/html/2607.22807)).

No study isolates the effect of adding an exact-fix line to a compiler error ([diagnostics notes][n-diag]). So the core bet in AGENTS.md is unmeasured, and dat has to measure it itself.

On format, the owner's decision matches the indirect evidence:

- **Tokens.** A rustc-style text block for the roadmap's own example came to about 59 word-and-punctuation pieces, against 110 for the same content in JSON: roughly **1.9× the tokens** ([size script][b-diag]; a proxy count, not a real tokenizer).
- **Reading accuracy.** Frontier models still show accuracy differences of 3–50% when extracting facts from JSON tool output, depending on prompting strategy ([Kate et al.](https://arxiv.org/abs/2510.15955)).
- **Editing.** Agents edit by string replacement, so a corrected source line is directly usable, while offsets only help programs ([Anthropic text editor tool](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/text-editor-tool)).

The recommended layout:

- **Each diagnostic is about six lines:** a header with the code, the location, the source line with a caret and a short label, a `fix:` title, and the full corrected line.
- **Uncertain or multiple fixes** are marked `fix (review):` or listed as `option:` lines.
- **No decorative gutter lines**, and colors only on a TTY.
- **Diagnostics are sorted** by file, position and code, and capped at 50 with "and N more".
- **The output ends with a summary** such as `2 errors, 1 fixable with dat fix`.

The PoC section shows this layout on `bad_fib.dat`.

The JSON record is cheap to get right at M0 and expensive to change once `dat fix`, the LSP and the benchmark depend on it. The roadmap's example lacks fields that every mature format has:

- **Spans** carry byte offsets plus 1-based line and column counted in Unicode scalar values, as rustc does ([rustc JSON](https://doc.rust-lang.org/rustc/json.html)). The LSP server negotiates UTF-8 positions ([LSP 3.17](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)).
- **Fixes.** Each diagnostic has `fixes[]`, and each fix has an applicability level and `isPreferred`.
- **Edits.** Each edit has a file, a byte range and the exact `before` text. That lets tools detect stale offsets and lets agents use `str_replace`.
- **One document per run**, with `schemaVersion`, `diagnostics[]`, a `summary`, and each diagnostic's `rendered` text.

Applicability should have three levels, `safe`, `review` and `option`, following rustc, Ruff, ESLint and Clang, and `dat fix` applies only `safe` fixes. Clang's rule that the compiler "must recover from errors as if the fix-it had been applied" ([Clang](https://clang.llvm.org/docs/InternalsManual.html)) gives two things: it suppresses follow-on errors, and it gives a testable property. Apply every safe fix, and the root cause disappears without new errors. Error codes should be stable, never reused, and grouped by spec section, with `dat explain` generated from the text in `reference/`. Error recovery follows the standard recipe: a resilient parser that wraps bad input in error nodes, an error type that absorbs further checks, and "tainting" to suppress follow-on errors. The goal is one diagnostic per root cause, and the conformance suite checks that on every ❌ case in scope.

`dat test` should check the whole project first and print compile diagnostics before test results. The owner set its exit codes (Q32): 0 for success, 1 for failing tests, and 2 when the tests couldn't run, which covers compile errors, unknown options and a `--filter` that matches nothing. Ruff's scheme ([Ruff](https://docs.astral.sh/ruff/linter/#exit-codes)), which this report first proposed, would have counted compile errors as 1. Whether tests in healthy modules still run when another module fails to check is a spec question (Q31).

Teaching dat to agents should rely on the compiler more than on documents. Repository context files raised agent cost by more than 20% with no gain in success ([AGENTS.md study](https://arxiv.org/pdf/2602.11988)). In unfamiliar languages, distilled text guidance didn't help weaker agents, while working helper code did ([esolang study](https://arxiv.org/abs/2606.10933)). The spec is about 95 KB, roughly 20–25k tokens. A better package has two parts:

- a 1–2k-token dat sheet of ❌/✅ pairs, shipped in the binary;
- a dedicated diagnostic, with a fix, for each TypeScript habit: `===`, `class`, `function` expressions, `any`, `undefined`, `var`, truthiness, and `x++` used as an expression.

The literature can't settle any of these questions for a new language: fix lines or not, all errors or only the first, text or JSON, docs in context or not. So the agent benchmark has to move from M7 to a pilot at M1–M2. The design borrows the 2026 tokenmaxxing method:

- **Size.** About 20 tasks, growing to 40–60.
- **Five task families:** spec-feature tasks; repair tasks seeded from ❌ cases and TypeScript habits; failing-test tasks; multi-file tasks from M6 on; and ports of small TypeScript programs.
- **Checking.** Hidden tests run in both modes, with TypeScript twins of each task as baselines.
- **Harnesses.** Two or three different agent harnesses with pinned models, capped at 40 turns and 10–15 minutes per task.
- **Statistics.** At least 5 runs per condition (at least 10 for a decision), paired designs, and thresholds registered before running.
- **Metrics.** Success, pass^k ([τ-bench](https://arxiv.org/abs/2406.12045)), turns to the first green `dat test`, tokens per solved task relative to TypeScript, and how often agents adopt the suggested fix.

A sweep of 40 tasks × 5 runs × 3 conditions is about 600 sessions, on the order of 10⁷ tokens (an estimate). The LSP stays at M7 as a thin adapter over the same diagnostic record, since agents mostly work through the CLI.

## The draft roadmap is wrong on budgets, order and five mappings

Measured against the evidence, the draft roadmap gets its architecture right and its details wrong in three recurring ways. Its budgets ignore the difference between a fresh process and a resident one, and between body edits and interface edits. It schedules analyses after the features that depend on them. And it wrote its Rust mappings without checking what Rust 1.65–1.99 offers and what dat's value model requires.

### Budgets

| Draft budget | Verdict | Replace with | Evidence |
|---|---|---|---|
| Interpreter start to first statement < 5 ms (M1) | Holds only for small or cached programs; otherwise it equals check time | ≤ 5 ms for single-module or cached programs, measured with `hyperfine -N` | Rust process start is 1.2–1.7 ms locally ([tooling notes][n-tool]) |
| `dat test`, 1,000 small tests < 250 ms (M1) | Loose with an in-process runner; impossible with one process per test | Keep 250 ms as the ceiling and target ≤ 60 ms cold. PoC: 4 tests ≤ 20 ms | ≥ 0.5 ms per spawn; 15–60 ms estimated in-process ([interpreter notes][n-interp]) |
| `dat check` after editing one file, 50k lines < 10 ms (M6) | Fails for interface edits with many importers in any mode; tight for a fresh CLI | Body edit ≤ 10 ms p95 for CLI plus cache and ≤ 5 ms for daemon or LSP. Interface edit ≤ 5 ms plus the importers' re-check, never above cold. No change, warm: ≤ 5 ms | 4–6 ms best case; 9–15 ms if every file is read |
| `dat check` cold, 50k lines < 300 ms (M6) | Realistic, probably loose | Target ≤ 100 ms on an 8-core laptop, with a 300 ms ceiling that includes a 4-vCPU CI runner | Sorbet-class throughput gives 40–60 ms |
| `dat test --compiled`, warm, one module changed < 3 s (M6) | Too loose for small programs; fails above about 100k generated lines in one crate | The tiered budget above; PoC ≤ 2 s | 0.27 / 0.89 / 2.62 s at 7k / 37k / 110k lines ([raw results][b-cg-results]) |
| ≤ 1.2× hand-written Rust, geometric mean, overflow checks on (M8) | Plausible for call-heavy and mixed code; fails on integer loops that could vectorize unless checks are removed | Keep it, with checked hand-written Rust as the baseline. Report vectorizable kernels separately. Add a `fib(35)` gate at M0 | `fib(35)` at about 1.0×; vectorizable sum +236–249% |
| Release profile with fat LTO and `codegen-units = 1` (M8) | Triples build time with unmeasured benefit | Thin-local LTO by default; fat LTO opt-in | 18.1 s against 5.9 s at 37k lines |
| (missing) | No budget names hardware or statistics | Name reference machines; report p50 and p95 over ≥ 30 runs; use ratios for speed; hard-fail at 2× the budget | noise on shared CI runners |

### Milestone order

| Item | Draft roadmap | Recommended | Why |
|---|---|---|---|
| First milestone's exit check | Hello World | A fibonacci walking skeleton with 17 exit criteria | Hello World exercises no calls, locals, checked arithmetic or catchable errors |
| Throw and catch for runtime errors; interpreter `Unwind` | M4 | M0 (the PoC) | M1 overflow and `% 0` must throw |
| Last-use moves, owned-parameter inference, take-and-restore | M8 ("move on last use") | M2, as one liveness pass that also emits the M2 "modified copy" error | 2,200× and 21× gaps, both quadratic; the modes must share complexity |
| Iterative `Drop`, clone, equality, hash and `toString` for recursive types | not planned | M3, then the M4 torture suite | a 1M-node drop aborts inside `catch_unwind` |
| Agent benchmark | M7 | Pilot of about 20 tasks at M1–M2; full suite as M7's exit metric | no literature answers dat's diagnostics questions |
| `dat fix` | M7 | Any time from M1 (cheap; helps the benchmark) | applies `safe` edits from the record |
| Diagnostic schema | M0 record with `line`, `col`, `endCol` and one `fix` | M0 record with byte spans, `fixes[]`, applicability and `before` text | expensive to change once tools depend on it |
| How module constants are shared, and the `Shared<T>` abstraction | `Rc`→`Arc` "after concurrency"; constants as `static` in M6 | `Shared<T>` from M0; decide constant sharing before M6 | `Rc` can't live in a `static`, and parallel tests share constants |
| Closing the exported-const and exported-arrow typing gaps | not planned | M1 (shapes the module interface) | the only ways a body leaks into an importer |
| Order of sets and maps after `delete` | implemented in M2 | Owner answer before M2 | `swap_remove` and `shift_remove` differ in order and in complexity class |
| Differential program generator | not planned | M4 | Csmith-class bug yield |
| Crate splitting | the whole program as one crate (M6) | One crate until about 40k lines; SCC groups over a types crate at M6 | 2.62 s per edit at 110k lines |
| Spikes | unwinding, copy-on-write lists, compiled-test turnaround | Unwinding and turnaround become real PoC code (already measured); the copy-on-write spike stays throwaway | the walking skeleton keeps its code |

### Risk ratings and mappings

| Mapping | Draft | Research |
|---|---|---|
| Narrowing on a field path | risk high; checked unwrap after an early exit | Medium: let-else and labelled blocks handle early exits with no unwrap |
| Runtime errors and `catch` | risk high; "call-depth guard" for recursion | Unwinding measured at about 1.0× of checked hand-written Rust; use a stack-pointer guard (0–4%), not a depth counter (about 2× in thread-local storage). The real high risk is recursive drop, which the table doesn't list |
| `string` → `Rc<str>` | risk low | Wrong representation: `+=` is quadratic. Use an opaque `Str` with inline small strings and an appendable copy-on-write buffer |
| parameter → `&T` | risk low | Incomplete: needs owned-parameter inference (2,200×). Overlapping `mut` and non-`mut` arguments need a clone, or Rust reports E0502 |
| nullable self-reference → `Option<Box<T>>` | under structs, risk low | Wrong: every copy is deep (226 µs per 10k nodes), and drop or clone aborts. Use `Option<Rc<T>>` plus an iterative `Drop` |
| interface → `Box<dyn I>` | risk medium | A closed-world enum by default (1.9× faster on copy-heavy code), with `Rc<dyn I>` as the fallback |
| module const → lazily initialized `static` | risk medium | Wrong mechanism: `LazyLock` needs `Sync` and poisons forever. Use thread-local `OnceCell` plus dat's own guard, and compile-time evaluation |
| list, set, map → `Rc<Vec>` with `make_mut` | risk medium | Right representation, but the uniqueness check must be hoisted (4.5–10× otherwise), and delete order is unresolved |
| `int` → checked `i64` | risk low | The semantics are right; the cost (up to +249% on vectorizable loops) is an M8 risk |

A few other details are outdated:

- **Syntax tree.** "Lossless syntax tree" should become an arena AST plus a lossless token and trivia vector; the formatter and fixes need lossless tokens, not a red-green tree.
- **Fix text in the spec.** "The 37 ❌ lines become expected diagnostics" understates the work. Only three of the 37 state a fix, and two have no message at all, so fix text has to be written in the compiler, or added to the spec with the owner's approval. The owner approved adding it (Q37), and all 37 lines now carry a code and a fix.
- **Panic hook.** The "silent panic hook" becomes `resume_unwind`, which skips the hook entirely.
- **Open questions.** The roadmap's Q1–Q3 are now answered by the owner.
- **Spec error.** The `FAIL math.dat:9` sample in [20-testing][spec-20] points at the wrong line and shows an assertion that can't fail. The owner approved the fix (Q34), and the sample now points at `math.dat:8`.

## M0 redefined: fibonacci through every component in both modes

### Every successful skeleton compiled a tiny program end to end on day one

The strongest precedents agree on one shape for a first milestone: get a tiny subset of the language through every stage at once, run it under a driver that compares real program output, and grow the subset in small steps. Ghuloum's incremental compiler has 24 steps, and "every step yields a fully working compiler for a progressively expanding subset"; the first step only compiles a program that returns 42 ([Ghuloum](http://scheme2006.cs.uchicago.edu/11-ghuloum.pdf)). Cockburn's walking skeleton is "a tiny implementation of the system that performs a small end-to-end function", and it is kept and evolved, not thrown away ([Blodgett](https://www.mattblodgett.com/2020/09/start-with-walking-skeleton.html)). Zig and Roc each run one test corpus against every backend, with explicit skip markers. Carbon is the counterexample. Its separate interpreter, built as an "executable specification", fell behind the design and was archived ([Carbon explorer](https://docs.carbon-lang.dev/explorer)). That supports dat's plan to split the two run modes only after a shared typed IR.

Hello World touches only `print` and a string literal. Fibonacci forces calls, recursion, locals, loops, checked integer arithmetic, a catchable runtime error and `test` blocks. That is enough to prove the typed IR, the shared runtime and the error model in both modes, while needing only about fifteen grammar productions ([PoC notes][n-poc]). The PoC code is real but tiny, and later milestones grow it. My rough estimate is 5–8k lines of Rust, almost none of it thrown away.

### The program

Every construct below is in the spec today. `fibIter(92)` = 7,540,113,804,746,346,429 is the largest Fibonacci number that fits in a 64-bit `int`, and `fibIter(93)` overflows. The loop must not compute one step ahead, or `fibIter(92)` itself would overflow. A recursive `fib(93)` would take about 3.9 × 10¹⁹ calls, so the overflow comes from the iterative version.

```
// fib.dat: the M0 walking skeleton. Every construct is plain dat as specified in reference/.

// Recursive: calls, recursion, `<`, checked `+` and `-`.
function fib(n: int): int {
  if (n < 2) {
    return n;
  }
  return fib(n - 1) + fib(n - 2);
}

// Iterative: let, const, for, `<=`, `++`, assignment, a user-thrown runtime error.
// fibIter(92) is the largest Fibonacci number that fits in int; fibIter(93) overflows.
function fibIter(n: int): int {
  if (n < 0) {
    throw RangeError { message: `fibIter: n must be >= 0, got ${n}` };
  }
  if (n < 2) {
    return n;
  }
  let prev = 0;
  let curr = 1;
  for (let i = 2; i <= n; i++) {
    const next = prev + curr;   // RangeError when the sum doesn't fit in int
    prev = curr;
    curr = next;
  }
  return curr;
}

function main(): void {
  print(`fib(25) = ${fib(25)}`);
  print("fibIter(92) =", fibIter(92));
  try {
    print(fibIter(93));
    print("not reached");
  } catch (e: RangeError) {
    print("fibIter(93) overflows int: caught RangeError");
  }
}

test "small values" {
  assert(fib(0) == 0);
  assert(fib(1) == 1);
  assert(fib(10) == 55);
  assert(fibIter(10) == fib(10));
}

test "largest value that fits in int" {
  assert(fibIter(92) == 7540113804746346429);
}

test "overflow is a catchable RangeError" {
  try {
    const f = fibIter(93);
    fail(`expected a RangeError, got ${f}`);
  } catch (e: RangeError) {
    assert(e.message != "");
  }
}

test "negative input is rejected" {
  try {
    const f = fibIter(-1);
    fail(`expected a RangeError, got ${f}`);
  } catch (e: RangeError) {
    assert(e.message == "fibIter: n must be >= 0, got -1");
  }
}
```

Expected stdout of `dat run examples/fib/fib.dat`, identical in both modes, with exit status 0:

```
fib(25) = 75025
fibIter(92) = 7540113804746346429
fibIter(93) overflows int: caught RangeError
```

The program is written to avoid every behaviour the spec leaves open:

- `catch (e: RangeError)` is legal. The rule that makes "a `catch` clause for a declared error type that nothing in the block can throw" a compile error covers only declared types, and `RangeError` is a runtime type ([12-errors][spec-12]).
- The tests bind results with `const f = …` instead of writing `fibIter(93);` alone, because the spec doesn't say whether a non-`void` result may be discarded (Q16).
- The overflow test checks only that the message isn't empty. §12 now defines the text (Q5), and the `overflow_uncaught.dat` golden checks it byte for byte.

### Five companion programs

**`examples/fib/bad_fib.dat`** proves the "exact fix" rule. The error sits on line 10, columns 9–30 (bytes 145–167):

```
// bad_fib.dat
function fib(n: int): int {
  if (n < 2) {
    return n;
  }
  return fib(n - 1) + fib(n - 2);
}

function main(): void {
  print("fib(10) = " + fib(10));
}
```

This is the terminal rendering in the compact layout recommended above. It is the form people and agents read; `E0301` is the code §03 gives this error.

```
error[E0301]: can't add string and int
  --> bad_fib.dat:10:9
10 |   print("fib(10) = " + fib(10));
   |         ^^^^^^^^^^^^^^^^^^^^^^ `fib(10)` is an int
fix: use a template string
10 |   print(`fib(10) = ${fib(10)}`);
1 error, 1 fixable with `dat fix`
```

This is the `--json` rendering for `dat fix`, the LSP and the harness, in the recommended schema (abridged):

```json
{ "schemaVersion": 1,
  "diagnostics": [{
    "code": "E0301", "severity": "error", "message": "can't add string and int",
    "file": "bad_fib.dat",
    "span": { "byteStart": 145, "byteEnd": 167, "line": 10, "col": 9, "endLine": 10, "endCol": 31 },
    "labels": [{ "message": "`fib(10)` is an int" }],
    "fixes": [{ "title": "use a template string", "applicability": "safe", "isPreferred": true,
                "edits": [{ "file": "bad_fib.dat", "byteStart": 145, "byteEnd": 167,
                            "before": "\"fib(10) = \" + fib(10)",
                            "after": "`fib(10) = ${fib(10)}`" }] }],
    "rendered": "error[E0301]: can't add string and int\n  --> bad_fib.dat:10:9\n..." }],
  "summary": { "errors": 1, "warnings": 0, "fixable": 1 } }
```

The harness applies the edit the way rustc's `run-rustfix` does ([rustc UI tests](https://rustc-dev-guide.rust-lang.org/tests/ui.html)). It compares the result with `bad_fib.dat.fixed`, checks that the fixed file has no diagnostics, and checks that it prints `fib(10) = 55` in both modes.

**`examples/fib/overflow_uncaught.dat`** lets an error escape `main`, so both modes must exit with status 1:

```
// overflow_uncaught.dat
function fibIter(n: int): int {
  if (n < 2) {
    return n;
  }
  let prev = 0;
  let curr = 1;
  for (let i = 2; i <= n; i++) {
    const next = prev + curr;
    prev = curr;
    curr = next;
  }
  return curr;
}

function main(): void {
  print(fibIter(93));
}
```

Its stderr follows §12 (Q5, Q6). The operands at the overflow are F(91) and F(92).

```
RangeError: int overflow: 4660046610375530309 + 7540113804746346429
  at fibIter (overflow_uncaught.dat:9:18)
  at main (overflow_uncaught.dat:17:9)
```

**`examples/fib/fib_fail.dat`** is a deliberately failing test in the format of [20-testing][spec-20]:

```
function fib(n: int): int {
  if (n < 2) {
    return n;
  }
  return fib(n - 1) + fib(n - 2);
}

test "deliberately wrong" {
  assert(fib(10) == 56);
}
```

Expected stdout in both modes, with exit status 1. The harness normalizes the time in the summary line:

```
FAIL fib_fail.dat:9  deliberately wrong
  assert(fib(10) == 56)
    left:  55
    right: 56

1 failed, 0 passed (2 ms)
```

**`examples/fib/deep.dat`** is the stack-guard case. It stays inside the guaranteed 10,000 nested calls, then catches the `StackOverflowError` that §12 now defines:

```
function depth(n: int): int {
  if (n == 0) {
    return 0;
  }
  return 1 + depth(n - 1);
}

function main(): void {
  print(depth(9_000));
  try {
    print(depth(100_000_000));
    print("not reached");
  } catch (e: StackOverflowError) {
    print("deep recursion caught");
  }
}
```

With a guard, both modes print `9000` and then `deep recursion caught`, and exit with status 0. Without one, the compiled binary aborts with status 134. A tree-walking interpreter would overflow the host stack at a far smaller dat depth than compiled code, so the interpreter needs its own check and a large dedicated thread stack.

**`bench/fib35.dat`** is `function main(): void { print(fib(35)); }` with the same `fib`. It is paired with **`bench/fib35.rs`**, the hand-written baseline: the same algorithm, built with `overflow-checks=on`. A trivial `hello.dat` serves only the startup budget.

### Commands, measurements and exit criteria

The PoC exercises:

- `dat check`, which exits 0 with no output on `fib.dat` and exits 1 with the diagnostic on `bad_fib.dat`, plus `dat check --json`;
- `dat run` (interpreter) and `dat run --compiled` (generate Rust, run rustc, run the binary);
- `dat build -o fib`;
- `dat test` and `dat test --compiled`, with `--filter`;
- a `dat-conformance` runner that executes every example and every extracted spec block in both modes.

The gates for the PoC come from these local measurements of the same recursive algorithm, timed as wall time including process start, taking the minimum over runs ([fib benchmarks][b-fib]):

| Program | `fib(30)` | `fib(35)` | `fib(35)` against checked hand-written Rust |
|---|---|---|---|
| Hand-written Rust, `overflow-checks=on` | 3.5 ms | 22.3–22.9 ms | 1.0× (baseline) |
| Hand-written Rust, wrapping arithmetic | 3.2 ms | 16.9–19.5 ms | 0.75–0.85× (LLVM turns one recursive call into a loop) |
| Generated style: `checked_add` with an unwinding `RangeError` | 3.3 ms | 21.8–24.5 ms | 0.98–1.07× |
| Same, plus the stack-pointer guard | — | 22.7–23.0 ms | 1.0–1.03× |
| Generated style: `Result` with `?` everywhere | 4.9 ms | 35.0–36.8 ms | about 1.6× |
| Same as unwinding, plus a thread-local depth counter | 5.5 ms | 45.3–47.0 ms | about 2.0× |
| Toy tree walker with resolved slots | 100.6 ms | 1,124 ms | about 50× |
| Toy tree walker with a naive `HashMap` environment | 321.9 ms | 3,567 ms | about 160× |
| Lua 5.5.1 / CPython 3.15.1 | 31.9 / 59.7 ms | 321.6 / 532.9 ms | — |

On the compiled path for a program this small, rustc on the single file took 78–103 ms, a cold `cargo build --release` with a separate runtime crate took 248 ms, and a rebuild after editing the program took 122–135 ms ([cargo rebuild case][b-cargo]).

"Both modes" below means the interpreter and the compiled binary. Absolute millisecond budgets apply only to the interpreter path; the compiled path has its own.

| # | Component | Criterion | How measured | Pass threshold |
|---|---|---|---|---|
| 1 | Lexer and parser (`dat-syntax`) | Parses all PoC programs and every spec block in the subset, with byte spans on every node. A syntax error reports `file:line:col` and never panics | Unit tests; parsing all 167 blocks never panics (blocks outside the subset report "unsupported") | 0 panics; every subset block parses |
| 2 | Checker (`dat-check`) | `fib.dat` is clean. `bad_fib.dat` reports exactly one error, with the spec message "can't add string and int", at 10:9–10:31. The subset's ❌ lines in `03-operators` (`++` forms) and `14-scope` are reproduced with their spec text. The `06-functions` "missing return" case may be `xfail` | Golden diagnostics; a matcher for spec lines | 100% of in-subset ❌ lines matched, apart from listed `xfail`s; 0 extra diagnostics |
| 3 | Diagnostics | The terminal form (primary) and the `--json` form are golden-tested. The JSON fix applies mechanically, and the fixed file checks clean and runs | `.diag`, `.diag.json` and `.fixed` goldens | Byte-identical goldens; the fixed program prints `fib(10) = 55` in both modes |
| 4 | Typed IR (`dat-ir`) | Both backends consume only the IR. A `dat ir fib.dat` text dump is snapshot-tested | The crate graph forbids `dat-interp` or `dat-codegen` depending on `dat-syntax` or `dat-check`, checked in CI with `cargo metadata`; snapshot test | No forbidden edges; snapshot stable |
| 5 | Interpreter (`dat-interp`) | Runs every PoC program with the expected stdout, stderr and exit status | Conformance runner | All goldens match |
| 6 | Codegen and rustc (`dat-codegen`) | Generated Rust compiles with no warnings, has `#![forbid(unsafe_code)]` and no `RefCell`, and passes the same goldens | Conformance runner with `--compiled` | 0 rustc warnings; all goldens match |
| 7 | Runtime (`dat-rt`) | The overflow check, the `RangeError` message, `toString`, `print`, `assert`'s left/right rendering and the uncaught-error printer exist once, in dat-rt, and both modes call them | Code-review rule, plus a test that the overflow message is byte-identical in both modes | Identical bytes |
| 8 | Error model | Overflow and the user-thrown `RangeError` are catchable in both modes. An uncaught error exits with status 1 and a trace with only dat frames, no Rust frames | `fib.dat` tests; `overflow_uncaught.dat` golden | Identical exit statuses and stderr |
| 9 | Stack guard | `deep.dat` catches the deep recursion in both modes; otherwise the guard's cost is recorded and a crash is accepted | `bench/fib35` with and without the guard | Adopt if the guard costs ≤ 5% on compiled `fib(35)` (measured 0–4% locally) |
| 10 | `dat test` | `fib.dat`: 4 passed, 0 failed, with identical per-test results in both modes. `fib_fail.dat` prints the spec's `FAIL` block in both modes and exits 1 | Conformance runner | Identical result lists |
| 11 | Differential harness (`dat-conformance`) | Compares stdout, normalized stderr, exit status, the uncaught error's type and message, and the per-test result list. Detects an injected fault | Self-test with `DAT_FAULT=interp-plus-one`, which makes the interpreter compute `+` wrong | The harness reports the difference; otherwise CI fails |
| 12 | Spec extraction | Extracts all 167 blocks and classifies every annotation as checked, unsupported or doc-only, with counts. Subset cases pass in both modes | Runner summary | 167/167 classified; ≥ 10 subset cases passing; the `pass` count becomes a ratchet that can only grow |
| 13 | Interpreter-path latency | `dat check fib.dat` ≤ 10 ms; `dat test fib.dat` (4 tests, interpreted) ≤ 20 ms; `dat run hello.dat` ≤ 5 ms; all wall time including process start | `hyperfine -N --warmup 3 --runs 20` in CI | Median under budget; CI hard-fails at 2× |
| 14 | Compiled-path budget (separate) | Warm `dat run --compiled fib.dat` with dat-rt prebuilt ≤ 1 s; warm `dat test --compiled fib.dat` ≤ 2 s; cold builds only recorded | hyperfine | Under budget |
| 15 | Compiled speed | `bench/fib35` generated against hand-written, same profile, `overflow-checks=on` | `hyperfine -N`, 20 runs, same CI job | ≤ 1.2× (the unwinding design measured about 1.0×) |
| 16 | Interpreter speed | Interpreted `fib(30)` against `python3 fib.py 30` on the same runner | hyperfine, same job | ≤ 2× CPython at the PoC (the toy walker measured 1.7×); stretch goal for M1: ≤ 1× |
| 17 | CI | All of the above on Linux and macOS, on every push | GitHub Actions matrix | Green on both OSes |

Ratio gates are used instead of absolute times because shared CI runners are noisy. A gate of ≤ 1.2× still catches a `Result`-everywhere design (1.6×) or a depth counter in thread-local storage (2×). A gate of ≤ 2× CPython fails a naive walker (5.4×), as it should.

### What each AGENTS.md rule looks like at fib scale

| AGENTS.md rule ([AGENTS.md][agents]) | What the PoC proves | Criteria |
|---|---|---|
| Fewest turns: familiar syntax, and errors that state the exact fix | `fib.dat` reads as plain TypeScript-shaped code. `bad_fib.dat` gets exactly one diagnostic whose fix, applied mechanically, checks clean and prints the right answer in both modes | 2, 3 |
| Super fast tooling | Check, test and run each finish within milliseconds on the interpreter path, including process start | 13 |
| Production speed | Generated `fib(35)` is within 1.2× of checked hand-written Rust | 15 (14 for the separate compiled budget) |
| Two run modes with the same semantics; every test in both | Both backends read only the IR; every observable built-in exists once in dat-rt; per-test results are identical; the harness catches an injected fault; the interpreter is fast enough to be the default | 4, 5, 6, 7, 10, 11, 16 |
| Rust compatibility: no GC, no `Rc<RefCell>`, no inheritance, no reflection | The generated crate builds warning-free under `#![forbid(unsafe_code)]`, with no `RefCell` and no reflection | 6 |
| Error tolerance: every runtime error can be caught | Overflow and user-thrown errors are caught in both modes; an uncaught error exits 1 with a dat-only trace; deep recursion is caught, or the cost of catching it is recorded | 8, 9 |
| `reference/` is the spec; don't replace designed features without asking | All 167 blocks are extracted and classified, and subset blocks pass in both modes. Every unspecified behaviour becomes an owner question, and the program avoids it | 12 |

### Real versus stubbed, and the CI that enforces it

Under the walking-skeleton principle, every box is real but tiny. Stubs are acceptable for breadth, but never for the interfaces that later milestones extend.

| Component | Real in the PoC | Stubbed or deferred |
|---|---|---|
| `dat-syntax` | Hand-written lexer that keeps comments as trivia; Pratt parser for the subset; byte spans on every node | Parser error recovery (stops at the first syntax error); the formatter's needs (M7) |
| `dat-check` | Name resolution, block scope, types for the subset, several type errors per run, diagnostics that carry a fix | Inference beyond literals and locals, narrowing, generics, the ownership pass |
| `dat-ir` | Typed IR: functions, slot-indexed locals, checked integer ops as distinct opcodes, structured control flow, try/catch regions, source locations; text dump | Optimizations |
| `dat-interp` | Walks the IR, not the AST; calls dat-rt for every observable built-in; propagates `Unwind`; has its own depth check; runs on a large-stack thread | Closure compilation or bytecode |
| `dat-codegen` | Emits Rust from the same IR; `#![forbid(unsafe_code)]`; drives Cargo; one crate for all tests | Release-profile tuning, LTO |
| `dat-rt` | Checked arithmetic, error structs, the `resume_unwind` throw and catch helpers, `print` and `toString` for int, bool and string, `assert` rendering, the uncaught-error reporter, the test harness, the stack guard | Collections, float formatting, strings beyond concatenation and templates |
| Diagnostics | One record in the recommended schema; terminal renderer (primary); `--json` | `dat fix`, `dat explain`, LSP |
| `dat-conformance` | Extractor for all 167 blocks, classifier, preludes in a sidecar file, example goldens, mode diff, fault-injection self-test, ratchet file | Program generator, test-case reducer |
| Benchmarks | hyperfine scripts with startup measured separately, the hand-written Rust baseline, CPython and Lua reference runs, ratio gates | Instruction-count job (Gungraun), which can follow soon |

Spec-as-tests needs no spec edits for the PoC. The extractor treats the reference's existing comments as the annotation language:

- `// ❌ Error: <msg>` is an expected diagnostic, matched exactly against the one-line form.
- `// throws <Type>[: <msg>]` makes the runner wrap that line in a `try`.
- `print(...); // <text>` is expected stdout.
- `<expr> // <literal>` becomes an `assert`.

The work is real, because the blocks were written for readers. Of the 158 blocks at the time of this report (the M0 answers later added 9, for 167, and one more ❌ line: the README's example of the format), 88 are fragments of function bodies, the 37 ❌ lines sit in 26 blocks, 8 blocks redeclare names to show alternatives, and 8 use `{ ... }` placeholder bodies. So the runner splits blocks into one variant per ❌ line, keeps hidden setup ("preludes") in a sidecar file keyed by a hash of each block, and reports `pass`, `fail`, `unsupported`, `doc-only` or `xfail` for each case. This follows the precedent of Astral's mdtest ([ty_test README](https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md)). Fence tags and invisible directives in `reference/` are left as an owner question (Q39).

CI runs a Linux and macOS matrix with the toolchain pinned in `rust-toolchain.toml` and `Swatinem/rust-cache`. The steps are `cargo build`, `cargo test`, and the conformance runner in both modes. When a run fails, the generated `.rs` files and diffs are uploaded as artifacts. A separate perf job on Linux runs the hyperfine budgets and ratios, hard-fails only beyond 2× a budget, and posts its numbers as a job summary.

## Decision register

Status values: **Settled** means the owner decided it. **Adopt** means it is recommended with no spec change and is ready to use. **Measure** means it gets decided by measurement at the milestone shown. **Owner Q#** means it waits on a spec or policy answer.

| # | Decision | Recommendation | Key evidence | Needed by | Status |
|---|---|---|---|---|---|
| 1 | First milestone | A fib walking skeleton replaces Hello World; the copy-on-write spike stays throwaway | Ghuloum; Hello World exercises no calls, arithmetic or errors | M0 | Adopt |
| 2 | Spec-as-tests | Infer expectations from existing comments, use sidecar preludes and per-line splitting, and keep a status ratchet | 158 blocks before the M0 answers: 88 fragments; 37 ❌ lines in 26 blocks | M0 | Adopt (Q39) |
| 3 | Differential harness | Compare outcomes plus goldens; fault-injection self-test; program generator by M4 | Csmith: 325+ bugs | M0, M4 | Adopt |
| 4 | Benchmark gates | Ratios measured in the same job: compiled ≤ 1.2× checked hand-written Rust, interpreter ≤ 2× CPython; ms budgets on the interpreter path only | about 1.0× and 1.7× measured | M0 | Adopt |
| 5 | Incremental granularity | Per-module interface hash plus per-body cache with backdating; hand-rolled behind a query-shaped API | ty 4.7 ms, Pyright 386 ms, Pyrefly 2.38 s; salsa port about 250 MB over | M1 model, M6 cache | Adopt |
| 6 | Process model | Checker as a library with no global state; CLI with an mmap cache (M6); resident server shared with the LSP (M7) | spawn 1.3 ms; reading all files 3–7 ms | M0, M6, M7 | Adopt |
| 7 | Syntax representation | Arena AST with `u32` IDs plus a lossless token and trivia vector | oxc 26.4 ms, Biome 139.3 ms | M0 | Adopt |
| 8 | Types and determinism | Interned types; unions ordered by content | TS 7 warns of order-dependent results | M1, M3 | Adopt (Q22) |
| 9 | Parallelism | Phase-parallel on a persistent pool; sequential below a size threshold | 4→8 TS checkers: 1.41×; import cycles rule out DAG order | M1 rule, M6 | Adopt |
| 10 | Standard library | Precompiled interface embedded in the binary | Go export data | M1 | Adopt |
| 11 | Interfaces that leak bodies | Close the exported-const and exported-arrow-function gaps | the only ways a body leaks into an importer | M1 | Owner Q18–19 |
| 12 | Tooling budgets | Split by process mode and edit kind; p50/p95 over ≥ 30 runs on named hardware; synthetic corpus | 4–6 ms against 9–15 ms CLI arithmetic | M0 | Adopt |
| 13 | Interpreter strategy | Walk a typed IR with resolved slots and explicit evaluation order; upgrade only if a gate fails | 101 ms against 322 ms on `fib(30)` | M0 | Adopt |
| 14 | Interpreter values and runtime sharing | 16-byte tagged `Value`; generic dat-rt; `#[inline]` primitive ops returning `Result`; no trait objects | dart2wasm divergence through separate implementations | M0–M2 | Adopt |
| 15 | Interpreter propagation | `Result<Value, Unwind>` carrying Throw, Return, Break and Continue | about 11 ns per frame, against microseconds per throw | M0 | Adopt |
| 16 | Test runner | In-process pool; fresh state per test; cooperative timeouts; per-test output capture | ≥ 0.5 ms per spawn means ≥ 500 ms per 1,000 tests | M1 sequential, M6 parallel | Adopt (Q12; Q32 answered) |
| 17 | Test caching and selection | Exact cache keyed by transitive IR hash (M6); function-level selection (M7) | dat has no I/O yet | M6, M7 | Adopt (Q30) |
| 18 | Equivalence machinery | Order-explicit IR, primitive ops only in dat-rt, pinned toolchain, per-test diff | Rust evaluates compound-assignment RHS first; sorts changed in 1.81 | M0–M2 | Adopt (Q14 answered; Q17) |
| 19 | JIT and dispatch tricks | None; revisit `become` after it stabilizes | 1–5% dispatch gains; dat-rt generics can't be JIT-instantiated | after M8 | Adopt |
| 20 | Compiled budget | Tiered: ≤ 1 s up to 40k lines, ≤ 3 s up to 150k with the split, clean ≤ 5 s, release ≤ 30 s | 0.27 / 0.89 / 2.62 s | M0 measure, M6 enforce | Settled (separate budget); numbers Adopt |
| 21 | Crate layout | One crate until about 40k generated lines; then SCC groups over a types crate, named by content hash | star leaf edit 0.51 s; chain bottom edit 3.11 s; Feldera 1,618→150 s | M6 | Measure |
| 22 | Build driver and profiles | Cargo workspace. Dev: incremental, default codegen units, line tables, unwind. Release: thin-local LTO, fat LTO via `--max` | incremental off 4× slower; 1 codegen unit 2.8×; fat LTO 18.1 s against 5.9 s | M0, M8 | Adopt |
| 23 | Backend and toolchain | LLVM only; pinned rustc; dat-rt built from source, prebuilt later; no `RUSTC_BOOTSTRAP` | Cranelift has no unwinding on macOS | M0 | Adopt |
| 24 | Generated-code style | No macros; loops; `&dyn` at boundaries; one Rust function per dat function; cold slow paths; write only changed files; forbid unsafe; one test binary | macro expansion 0.126 of 0.85 s; 0.15 s macOS first launch | M0 on | Adopt |
| 25 | Narrowing lowering | let-else, if-let, let-chains, labelled block; edition 2024 | all sketches compile on 1.99 | M3 | Adopt (Q23) |
| 26 | Unions | One canonical enum per sorted member set; `null` as an outer `Option` | spec 18 makes unions order-free | M3 | Adopt (Q22) |
| 27 | Interface values | Closed-world enum with `__tag`/`__as_X`; `Rc<dyn>` fallback; conformance under both | 1.9× on copy-heavy code; builds no slower | M5 | Settled (tags); lowering Adopt (Q24) |
| 28 | Closures and generics | `&dyn Fn` parameters and `Rc<dyn Fn>` stored values; emit Rust generics | Feldera; borrowck plus typeck 59% of a clean build | M2, M5 | Adopt; dat-rt built-ins Measure at M2 |
| 29 | Module constants | Thread-local `OnceCell` plus dat's own re-entry guard; compile-time evaluation of pure initializers | `LazyLock` needs `Sync` and poisons | M6 | Owner Q28 |
| 30 | Locations and traces | `&'static Loc` per checked op; line map; rustc JSON reported as internal errors; traces captured at the throw unless identical traces are required | Rust has no `#line`; capture costs 3.7 µs | M0, M4 | Adopt (Q7) |
| 31 | Error propagation | Hybrid: declared errors as `Result`, runtime errors unwind | 0 ns when nothing is thrown; `fib(35)` 1.0× against 1.6× | M0 | Adopt (moves from M4) |
| 32 | Throw primitive and `finally` | `resume_unwind(Box<DatError>)`; hook only for Rust panics; `finally` after `catch_unwind` with a `Flow` value | skips the hook, 0.6–0.7 µs cheaper; a panic in `Drop` aborts | M0, M4 | Adopt (Q4) |
| 33 | Stack overflow | Stack-pointer check against a static limit in recursive SCCs; large-stack threads; 256 KiB red zone; interpreter check | +0.02 to +0.17 ns per call; 0–4% on `fib(35)` | M0 | Settled; wording (Q1) and depth (Q2) answered |
| 34 | Deep recursive values | Generated iterative `Drop`; iterative or stack-checked clone, eq, hash and `toString` | 1M-node drop aborts inside `catch_unwind` | M3 | Adopt |
| 35 | Out of memory | Global-allocator wrapper in dat-rt that prints a dat message and aborts; `try_reserve` for capacity overflow | `-Zoom=panic` removed in Dec 2025 | M2 | Settled; wording Q3 |
| 36 | Integer overflow checks | Explicit `checked_*` with a cold throw; M8 range analysis, hoisting, deferred checks; report vectorizable kernels separately | +132–249% on vectorizable kernels; SPEC +8.7–11.8% | M0, M8 | Adopt (Q9) |
| 37 | Collections | `Rc<Vec>` and `Rc<IndexMap>` behind opaque dat-rt types | 2.6 ns copies; persistent vectors 7–252× slower | M2 | Adopt |
| 38 | Ownership analysis | One liveness pass in M2: moves, the modified-copy error, take-and-restore, owned-parameter inference | 2,200× and 21× gaps, quadratic | M2 | Adopt (Q27) |
| 39 | Element writes | Hoist `make_mut`; `for (mut x of xs)` becomes `iter_mut` | 518 µs–1.23 ms against 116 µs | M2, M8 | Adopt |
| 40 | Sets and maps | Tombstone ordered map (`shift_remove` as a stopgap); foldhash with a random seed | `shift_remove` 1.39 s; foldhash 2× faster than SipHash | M2 | Owner Q25 |
| 41 | Strings | Opaque `Str`: inline small strings plus an appendable refcounted buffer; wrap `EcoString` or `Rc<String>` in M1 | 298 ms against 49 µs | M1 | Adopt |
| 42 | Structs and recursive types | Unboxed structs, `Copy` when all fields are scalars; recursive fields as `Option<Rc<T>>` with iterative `Drop` | 226 µs against 2.9 ns per copy | M3 | Adopt |
| 43 | Refcount header | One `Shared<T>` from M0; `Rc` now; signed thread-local/shared/immortal header later | Arc 1.25–5×; 439 ns per op under 8-thread contention | M0, M6 | Adopt (Q28, Q29) |
| 44 | Copy visibility | Copy counter (`--report-copies`); complexity-ratio conformance cases; ring-buffer decision for `shift` | Lean's `dbgTraceIfShared`; 14.3× against 4.9× growth for 5× more items | M2, M8 | Adopt (Q26) |
| 45 | Output format | Compact text, about six lines per diagnostic plus a summary; `--json` with `rendered` for tools | JSON about 1.9× the tokens | M0 | Settled; layout Adopt |
| 46 | Record schema | Byte spans plus 1-based scalar-value columns; `fixes[]` with applicability; edits with `before`; `schemaVersion` | rustc JSON; LSP UTF-8 negotiation | M0 | Adopt |
| 47 | Fix levels | `safe`, `review`, `option`; `dat fix` applies `safe`; recover-as-if-fixed property tested | rustc, Ruff, ESLint, Clang | M0 | Settled (Q36 answered) |
| 48 | Codes and `dat explain` | Stable category codes; `dat explain` generated from `reference/` | rustc `--explain` | M0 scheme, M7 freeze | Settled (Q37 answered): codes numbered by spec section |
| 49 | Error recovery | One diagnostic per root cause; cap of 50; deterministic order | pylint feedback 49.2% against 53.1% generic | M1–M6 | Adopt |
| 50 | `dat test` behaviour | Checks first; exit codes 0/1/2 | `go test` runs vet; Ruff exit codes | M1 | Adopt (Q31 open; Q32 answered) |
| 51 | Teaching agents | Compact ❌/✅ sheet plus TypeScript-habit diagnostics | context files +20% cost, no gain | M1–M3 | Adopt (Q38) |
| 52 | Agent benchmark | Pilot of about 20 tasks at M1–M2; full suite at M7 | no study isolates fix lines | M1–M2 | Adopt |
| 53 | LSP | CLI first; LSP as a thin adapter at M7 | agents loop through the CLI | M7 | Adopt |

## Questions for the owner

These 39 questions are collected from all seven notes and deduplicated. Each would add to, change or clarify `reference/` or AGENTS.md, or set project policy. On 29 October 2026 the owner answered the eleven that M0 needs (1, 2, 5, 6, 11, 14, 32, 34, 35, 36 and 37), and the answers are now in `reference/`. The other 28 remain open, and none of them is treated as decided here. "Raised in" names the notes: tooling, interpreter, codegen, errors, values, diagnostics, PoC. The last column gives the owner's answer, or says what the implementation does in the meantime without settling anything in the spec.

| # | Question | Raised in | Needed by | Answer, or until answered |
|---|---|---|---|---|
| 1 | Approve the §12 row: `StackOverflowError`, kind Runtime, thrown by "a call nests deeper than the stack allows", plus a sentence that `try?` doesn't cover it | errors, interpreter, PoC | M0 goldens; M4 spec | **Answered:** approved as proposed (§12) |
| 2 | Stack-overflow depth: implementation-defined above a guaranteed minimum (for example ≥ 10,000 nested calls), a fixed limit identical in both modes, or configurable? | interpreter, errors, PoC | M0, M4 | **Answered:** at least 10,000 nested calls in both modes; above that the limit is implementation-defined (§12) |
| 3 | Wording for out of memory in §12 ("prints `fatal: out of memory`, aborts, Unix status 134"), and an AGENTS.md note listing the remaining uncatchable failures (OOM, OS SIGKILL, oversized native frames, abort-only targets, later FFI) | errors | M4 | behaviour implemented; only the wording waits |
| 4 | What does a genuine Rust panic (a dat-rt bug, or a "capacity overflow" not routed to the abort) become: `AssertionError` with "internal error:", or a new internal-error type? | errors, interpreter | M1 | reported as an internal error, exit status 2 |
| 5 | Text of the `int` overflow `RangeError` message (proposed: `int overflow: <a> <op> <b>`) | PoC | M0 | **Answered:** as proposed, with dat's own operators; `int % 0` gives `int remainder by zero: <a> % 0` (§12) |
| 6 | Streams and layout: `print` to stdout; uncaught errors to stderr with message, location and a dat-only trace; exit status 1 | PoC | M0 | **Answered:** as proposed, with stdout flushed before the error (§09, §12). Whether traces must match in release builds stays open (Q7) |
| 7 | Must uncaught-error stack traces be identical between the interpreter and compiled release builds (which needs a shadow stack, +0.80–0.88 ns per call), or only type, message and location? | codegen, errors | M4 | traces compared only in test builds |
| 8 | State after a callback throws inside a built-in (`sort`, `map`): all-or-nothing, same elements in unspecified order, or the exact algorithm? What does an inconsistent comparator throw, and is the order of comparator calls observable? | interpreter, errors | M2 | one shared dat-rt algorithm; conformance cases avoid the question |
| 9 | Is `int.MIN % -1` equal to `0` or a `RangeError`? | interpreter, errors | M1 | dat-rt throws `RangeError`, flagged |
| 10 | Does `try?` apply to arithmetic (`try? a + b`) or to calls of functions that don't throw? | PoC | M1 | rejected as "not supported yet" |
| 11 | `catch (e: Error)` around a block that can throw only runtime errors seems to break the rule "a catch for a declared type nothing can throw is a compile error", since `Error` is listed as Declared. Which wins? | PoC | M0, M4 | **Answered:** the catch-all wins; the check covers specific declared types only (§12) |
| 12 | Confirm that a test timeout is an action of the test runner, not a catchable runtime error | interpreter | M6 | — |
| 13 | Policy: are wasm or other targets without unwinding in scope? | errors | after M8 | no wasm target |
| 14 | Are operands, call arguments, and the target and value of a compound assignment evaluated left to right? | interpreter, PoC | M0 | **Answered:** left to right, with a compound assignment's target evaluated before its value (§03) |
| 15 | Are out-of-range integer literals (`9223372036854775808`) valid, and is `-9223372036854775808`? | PoC | M1 | compile error "not supported yet" |
| 16 | May an expression statement discard a non-`void` result? May `_` name a catch variable? | PoC | M1 | PoC binds results with `const` |
| 17 | Confirm that observable algorithms (sort, case mapping, Unicode tables, number parsing) follow the dat version, not the user's rustc | interpreter | M1 | toolchain pinned in generated crates |
| 18 | May an exported `const` omit its type, and if so, must the type be evident without reading function bodies? | tooling | M1 | — |
| 19 | Must an exported arrow-function `const` declare its return type and `throws`? | tooling | M1 | — |
| 20 | When private functions with inferred return types recurse, is that a compile error with a fix, or is the type inferred? | tooling | M1 | PoC requires explicit return types |
| 21 | Are type-level cycles (inferred consts that depend on each other; alias cycles across circular imports) compile errors with a fix? | tooling | M6 | — |
| 22 | In what order are union members printed in messages and docs? | tooling | M3 | content order (qualified names) |
| 23 | Does assigning a sibling field (`user.name = …`) end the narrowing of `user.address`? | codegen | M3 | — |
| 24 | Will packages or separately compiled code ever add interface implementors after the program is compiled? | codegen | M5 | closed-world enum by default; `Rc<dyn>` fallback kept |
| 25 | After `delete`, do the remaining set and map entries keep their relative order, and does a re-added key go to the end (as in JavaScript and Python)? | values | before M2 | — |
| 26 | Should `operator get`/`mut operator set` gain an in-place form, or is the copy in `h[k].push(x)` a documented cost? | values | M3 | — |
| 27 | Should the reference document which patterns are guaranteed to stay in place (`acc = f(acc, x)`, `s += t`, map-entry updates)? Documentation only | values | M2 | — |
| 28 | Module constants: may initializers have observable effects? Is "computed once" per program or per thread? Shared across tests or recomputed per test? After an initializer fails, retry or rethrow? | values, codegen, interpreter | before M6 | — |
| 29 | Later: when concurrency arrives, are values sent to another task shared or deep-copied? Must a networking standard library use a DoS-resistant hasher? How do tests that use I/O, time or randomness interact with result caching? | values, interpreter | after M8 | — |
| 30 | Should `dat test` skip unchanged tests by default, or only with `--changed`? | tooling | M6 | runs everything |
| 31 | Should tests run when some modules fail to type-check, and if so which ones? | diagnostics | M1 | stops after printing the compile errors |
| 32 | Test output: lines for passing tests, a summary line, per-test capture of `print` shown only on failure, and exit statuses for `dat test` and `dat check` (proposed 0/1/2) | interpreter, PoC, diagnostics | M0, M1 | **Answered:** no line per passing test unless `--verbose`; captured `print` shown on failure; a summary line; exit 0 / 1 / 2, with 2 when tests couldn't run (§20, §21) |
| 33 | Should test-failure output adopt error codes or rerun hints? This would change 20-testing.md | diagnostics | M1 | spec's `FAIL` format unchanged |
| 34 | Correct the `FAIL math.dat:9` example (wrong line, and an assertion that can't fail) | PoC | M0 | **Answered:** fixed; the sample now plants a bug and points at `math.dat:8` (§20) |
| 35 | Where does the CLI reference (`check`, `build`, `run --compiled`) live, given that the spec names only `dat run` and `dat test`? | PoC | M0 | **Answered:** a new section, `reference/21-command-line.md` |
| 36 | Does "errors state the exact fix" allow several alternatives? The spec's own `items.length` example lists two | diagnostics | M0 | **Answered:** one fix when the alternatives mean the same thing; up to three `option:` lines, never applied by `dat fix`, when the right one depends on intent (`reference/README.md`) |
| 37 | Should the spec's ❌ lines carry stable codes and fix text, and is their wording normative or illustrative? | diagnostics, PoC | M0 | **Answered:** every ❌ line carries a stable code, numbered by spec section, and a fix. The code, the line and the fix are binding; the message wording isn't (`reference/README.md`) |
| 38 | Should the spec gain ❌ examples for TypeScript habits (`===`, `class`, `any`, `undefined`), and where should the compact agent sheet live? | diagnostics | M1 | sheet ships with the tool |
| 39 | May `reference/` gain fence tags (`dat`, `dat ignore`) and invisible prelude directives, so that blocks become runnable tests? Presentation only | PoC | after M0 | preludes kept in a sidecar file |

## Conclusion

The research changes where the risk sits. The roadmap rated narrowing and runtime errors as its hardest Rust problems. With let-else and a measured 1.0× unwinding cost, both are now routine. The real hazards were ones the roadmap rated low or didn't list: copy semantics that go quadratic without a liveness pass, recursive drop that aborts below every guard, string and constant representations that don't survive dat's own idioms, and a budget table that can't tell a resident checker from a fresh process. Each of these is cheap to fix early and expensive to retrofit. That is the strongest argument for moving the ownership analysis to M2, the schema to M0, and the agent benchmark to M1–M2.

The fibonacci skeleton is also the cheapest way to turn this report's estimates into facts. Its 17 criteria run the actual pipeline on Linux and macOS and replace the toy-interpreter and macOS-only numbers with real ones. Its fault-injection self-test checks that the two-mode promise is enforced rather than assumed. Its golden outputs make about a dozen of the owner's open questions concrete enough to answer in one sitting, and the owner answered eleven of them on 29 October. The biggest bet in AGENTS.md, that compiler errors stating the exact fix save agent turns, has no published evidence either way. The milestone plan should treat it as an experiment to run by M2, not as a premise.

[n-tool]: <../research_notes/Dat lang roadmap decisions/tooling_speed.md>
[n-interp]: <../research_notes/Dat lang roadmap decisions/interpreter.md>
[n-cg]: <../research_notes/Dat lang roadmap decisions/rust_codegen.md>
[n-err]: <../research_notes/Dat lang roadmap decisions/error_model.md>
[n-val]: <../research_notes/Dat lang roadmap decisions/value_semantics.md>
[n-diag]: <../research_notes/Dat lang roadmap decisions/diagnostics_agents.md>
[n-poc]: <../research_notes/Dat lang roadmap decisions/poc_milestone.md>
[b-fib]: <../research_notes/Dat lang roadmap decisions/benchmarks/fib/>
[b-cargo]: <../research_notes/Dat lang roadmap decisions/benchmarks/cargo_rebuild/>
[b-cg]: <../research_notes/Dat lang roadmap decisions/benchmarks/codegen_and_tooling/>
[b-cg-results]: <../research_notes/Dat lang roadmap decisions/benchmarks/codegen_and_tooling/results.jsonl>
[b-sketch]: <../research_notes/Dat lang roadmap decisions/benchmarks/codegen_and_tooling/sketches/sketch.rs>
[b-val]: <../research_notes/Dat lang roadmap decisions/benchmarks/value_semantics/results.txt>
[b-err]: <../research_notes/Dat lang roadmap decisions/error_model_bench/results.txt>
[b-diag]: <../research_notes/Dat lang roadmap decisions/benchmarks/diagnostics/sizes.py>
[agents]: <../AGENTS.md>
[roadmap]: <https://claude.ai/artifact/JNuKMm6CfFmLtBKFjjF4hG>
[spec-02]: <../reference/02-variable-declaration.md>
[spec-12]: <../reference/12-errors.md>
[spec-16]: <../reference/16-map-methods.md>
[spec-17]: <../reference/17-imports.md>
[spec-18]: <../reference/18-enums-and-unions.md>
[spec-20]: <../reference/20-testing.md>
