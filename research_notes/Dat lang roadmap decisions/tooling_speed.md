# Checker and tooling speed for dat-lang (check, LSP, formatter): techniques, evidence, decisions

Scope: how the fastest type checkers and compilers reach millisecond responses, cold and incremental; which language rules make that possible; how the dat spec (`reference/`) helps or hurts; what dat should adopt. Current as of 28 October 2026. Older numbers are dated inline. Spec files are cited as `reference/NN-name.md`. Local measurements were taken in this session on an Apple M4 Pro (14 cores), macOS (Darwin 24.6), Rust 1.99, release build. Their source is in `/private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/src/main.rs` and `.../bench/src/par.rs`, run on a synthetic corpus of 500 dat-like files × 100 lines (50,000 lines, 1,549,848 bytes). The scratchpad is temporary, so the method is described inline wherever a number is used.

## 1. How the fastest checkers and compilers get their speed (with measured numbers)

### Takeaway
Most of the speed comes from three things. In order of leverage: (1) **not doing work**: laziness, plus a firewall that keeps a body edit from re-checking anything outside that body (rust-analyzer, ty, Sorbet, Go export data). (2) **A type system with no global constraint solving or backtracking**: Sorbet's single-shot local inference, against Swift's exponential overload solver. (3) **Native code with compact data plus shared-memory parallelism**: TypeScript 7 got 8–12x by porting the same algorithm to Go and running it in parallel. Every system that reports single-digit-millisecond incremental results measures a long-lived process (LSP server, daemon or `--watch`), not a fresh CLI run.

### Cited Findings

#### rust-analyzer and salsa
- rust-analyzer's stated core invariant: "typing inside a function's body never invalidates global derived data", and "if you change the body of `foo`, all facts about `bar` should remain intact." — [rust-analyzer architecture.md](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)
- The firewall is the `ItemTree`: it "condenses a single `SyntaxTree` into a 'summary' data structure, which is stable over modifications to function bodies." — [rust-analyzer architecture.md](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)
- The model "is computed lazily (on-demand)". rust-analyzer "keeps all this input data in memory and never does any IO", and calls keeping everything in memory "OK" because input is "tens of megabytes at most". The page describes no on-disk cache. — [rust-analyzer architecture.md](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)
- Cancellation: on an input change salsa "bumps this counter and waits until all other threads using salsa finish". Stale queries are cancelled with a special panic, so "rust-analyzer requires unwinding". Requests that "might block user's typing are handled on the main thread"; others run in the background. — [rust-analyzer architecture.md](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)
- The syntax tree is built per file "to enable parallel parsing of all files". — [rust-analyzer architecture.md](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)
- matklad (rust-analyzer's original author) argues that laziness, not incrementality, is what makes IDEs fast: "it's laziness — the ability to skip huge swaths of code altogether". He lists the query-based (salsa) approach's drawbacks as "extra complexity, slower performance". (2020 post.) — [Three Architectures for a Responsive IDE](https://rust-analyzer.github.io/blog/2020/07/20/three-architectures-for-responsive-ide.html)
- Salsa mechanics. Every input `set` increments a global revision. A memoized function re-executes only if a dependency changed. **Backdating** (early cutoff): if a re-executed dependency produces the same value, salsa says "even though the inputs changed, the output didn't". Example: adding a comment re-parses to an identical AST, so `type_check` doesn't re-run. **Durability**: inputs are tagged (e.g. high for crates.io dependencies, low for the workspace). If a function depends only on high-durability inputs and a low one changes, salsa can "very easily determine that the tracked function result is still valid" without walking edges. — [Salsa book: algorithm](https://salsa-rs.github.io/salsa/reference/algorithm.html)
- The port of rust-analyzer to the redesigned ("new") salsa was costly and slow. Dec 2024: the branch passed tests, but memory and performance had regressed ([salsa meeting notes](https://hackmd.io/@salsa/B19OUlA71l)). Jan 2025: still about 250 MB worse on memory, with planned fixes estimated at roughly 300 MB and 240 MB ([salsa notes](https://hackmd.io/@salsa/SkNtN8Lv1e)). Mar 2025 call for testing: the upgrade would "slightly improve performance now and enable parallel autocomplete and persistent caches later" ([r/rust mirror](https://redlib.hackliberty.org/r/rust/comments/1jbyunp/call_for_testing_rustanalyzer)). A June 2026 nightly lists a salsa bump that saved about 70 MB on `analysis-stats self` ([release notes](https://newreleases.io/project/github/rust-lang/rust-analyzer/release/2026-06-29)). These are informal sources; I found no official write-up of the final numbers.

#### ty (Astral, Python, salsa-based) and Pyrefly (Meta, Python, not salsa)
- ty Beta, 16 December 2025: "Without caching, ty is consistently between 10x and 60x faster than mypy and Pyright." After an edit in PyTorch, "ty recomputes diagnostics in 4.7ms: 80x faster than Pyright (386ms) and 500x faster than Pyrefly (2.38 seconds)". The chart shows 4.5 ms, a small inconsistency in the source. ty is "built around 'incrementality'" and "designed from the ground up to power a language server". — [Astral: ty Beta](https://astral.sh/blog/ty)
- Early ty (then "red-knot") code split type inference into salsa queries at three granularities: a whole scope, a single definition, or a single expression. This is 2024 code seen through a mirror and may have changed. — [red_knot_python_semantic mirror](https://git.joshthomas.dev/language-servers/ruff/src/commit/7fc39ad624c0df95d62066d758fdb6fdcc6de2e3/crates/red_knot_python_semantic/src/ast_node_ref.rs)
- Pyrefly benchmark, 10 April 2026 (M4, 64 GB; Pyrefly 0.60.0, ty 0.0.29, Pyright 1.1.408, mypy 1.19.1; no caching). Cold full-check wall times: pandas ty 1.5 s / Pyrefly 1.9 s / Pyright 144.4 s; homeassistant 2.2 s / 2.2 s / 51.3 s; flask 0.2 s / 0.2 s / 1.2 s. ty has outliers: numpy 30.6 s (Pyrefly 4.8 s) and openai 4.6 s (Pyrefly 0.3 s). "Pyrefly uses less memory than Ty on 31 of 53 packages." The Pyrefly team chose not to use salsa because they wanted control over when memory is freed. — [Pyrefly: speed and memory comparison](https://pyrefly.org/blog/speed-and-memory-comparison/)
- Pyrefly's architecture: "We aim for large-scale incrementality (at the module level)", "do[es] not use fine-grained incrementality", aims "for raw performance and a simpler module-centric design", and "there's no need to solve a single binding in isolation if solving all bindings in a module is fast enough." It expects "large strongly connected components of modules". Step 1 has to resolve `import *` transitively. — [Pyrefly ARCHITECTURE.md](https://github.com/facebook/pyrefly/blob/main/ARCHITECTURE.md)

#### Pyright
- Pyright uses "lazy evaluation": "Instead of analyzing each file top to bottom, it visits nodes of the program in whatever order is necessary to resolve each type." By default the VS Code extension reports errors only for open files (`openFilesOnly`). These are old 1.1.0 release notes; the exact date isn't verified. — [Pyright 1.1.0 release notes](https://newreleases.io/project/npm/pyright/release/1.1.0)
- Flow analysis has its own cost. On sympy, functions with "complex code flow graphs and large numbers of variables" make inference "very costly". — [pyright#7159](https://github.com/microsoft/pyright/issues/7159)

#### TypeScript 7 (Go port, "Corsa"/tsgo) and `isolatedDeclarations`
- TS 7.0 went GA on 8 July 2026, with "speedups between 8x and 12x on full builds" at the default `--checkers 4`. Full builds: vscode 125.7 s → 10.6 s (11.9x); sentry 139.8 s → 15.7 s; bluesky 24.3 s → 2.8 s; playwright 12.8 s → 1.47 s; tldraw 11.2 s → 1.46 s. With `--checkers 8`: vscode 7.51 s, sentry 12.08 s, playwright 1.16 s. Memory: vscode 5.2 GB → 4.2 GB (−18%). Editor time to first error on the VS Code codebase: ~17.5 s → "under 1.3 seconds". — [Announcing TypeScript 7.0](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)
- Mechanism: parsing, checking and emitting run in parallel. Parsing and emit "parallelize well across files with little overhead". The checker uses "a fixed number of type-checker workers with their own view of the world". Workers may duplicate work, but "given the same input files, they will always divide them identically and produce the same results". Varying the count "may rarely surface order-dependent results". Project-reference builds (`--builders`) are bottlenecked by the project dependency graph, except for type-checking with `--isolatedDeclarations` and separate syntactic declaration emit. Watch mode was rebuilt on a Go port of the Parcel watcher, because "pure polling was too expensive". — [Announcing TypeScript 7.0](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)
- The March 2025 preview reported VS Code (1,505,000 LOC) at 77.8 s → 7.5 s, editor project load ~9.6 s → ~1.2 s, and memory "roughly half". The 2026 GA baseline (125.7 s) differs from the 2025 one (77.8 s). The two used different setups and should not be mixed. — [A 10x Faster TypeScript (Mar 2025)](https://devblogs.microsoft.com/typescript/typescript-native-port/)
- `--checkers` sets the number of checker workers (minimum 1). `--singleThreaded` also serializes parsing and emit. — [typescript-go PR #1985](https://github.com/microsoft/typescript-go/pull/1985)
- `isolatedDeclarations` (TS 5.5) requires explicit types on exports, so declaration emit no longer depends on type-checking other files. The error is TS9007, "Function must have an explicit return type annotation with --isolatedDeclarations". — [swc_typescript diagnostics](https://rustdoc.swc.rs/src/swc_typescript/diagnostic.rs.html); background: [Marvin Hagemeister, Speeding up the JS ecosystem: isolated declarations](https://marvinh.dev/blog/speeding-up-javascript-ecosystem-part-10/)

#### Go compiler and gopls
- Go export data is "a serialized representation of an object graph, with an index allowing lazy decoding" of parts of it. It is usually a "deep" summary, so compiling package Q "can read the export data files only for each direct import". The cost is that deep data grows up the import graph. gopls uses "shallow" data, which "demands random access to the export data files of all dependencies". — [Go compiler README](https://go.dev/src/cmd/compile/README)
- gopls v0.12 (Sept 2023) moved from holding everything in memory to separate compilation plus a file-based on-disk cache of per-package summaries and indexes. Memory and startup savings "averaged around 75%" across 28 repos. Typed syntax trees are "typically 30x larger than the source text". Memory became "proportional to the number of open packages and their direct imports", and a second start "becomes ready to serve much more quickly". The article gives no absolute latencies. — [Scaling gopls (Go blog)](https://go.dev/blog/gopls-scalability)

#### Sorbet (Stripe, Ruby)
- QCon SF 2019 (old numbers): "around 100k lines per second, per CPU core", scaling linearly "up to 64 cores"; "around 10x faster than javac per core"; about a million lines "completes in seconds"; IDE responses in "single-digit milliseconds". C++ was chosen because compiler performance depends on **memory locality**. Sorbet does not infer types across methods. — [InfoQ: Sorbet talk](https://www.infoq.com/presentations/sorbet-type-checker-ruby)
- Internals: "single-shot style of inference is fast, because we only make one decision about typing per instruction". There is no backsolving, fixed-point iteration or unification. The CFG has "~11 kinds of instructions". Method bodies are checked independently. `Ref`s are integer IDs "usually smaller than an 8-byte pointer", and names are "strings but as numbers". Design maxim: "slow operations should be longer to type". — [Sorbet internals.md](https://github.com/sorbet/sorbet/blob/master/docs/internals.md)
- Scale: over 15 million lines in 150,000 files at Stripe (undated). — [Stripe blog](https://stripe.com/blog/sorbet-stripes-type-checker-for-ruby)
- LSP fast path (Jan 2023): a changed file's old definitions are deleted and its new ones re-entered. The share of edits that fall to the full "slow path" dropped from 19% to 10%. Most of what remains is class/module definition changes (68%), "too many extra files" (13%), "too many files" (10%) and "package file" (9%). — [Making Sorbet more incremental](https://blog.jez.io/making-sorbet-more-incremental/)

#### Hack (Meta)
- The checker was built as a local server that watches the filesystem and keeps all information about the code in memory (2014). — [Facebook Engineering: Hack](https://engineering.fb.com/2014/03/20/developer-tools/hack-a-new-programming-language-for-hhvm/). Checks are "nearly instant (within the tens of milliseconds)" (2015, secondary). — [The Next Platform](https://www.nextplatform.com/code/2015/06/16/how-facebook-hack-is-upending-programming-efficiency/1658025). `hh_client` is a thin interface to the `hh_server` daemon. — [hh_client man page](https://manpages.org/hh_client)

#### Zig incremental compilation
- April 2026: incremental compilation works with the LLVM backend, so compile errors appear "in milliseconds rather than seconds". 30 May 2026, new ELF linker: Andrew's Tetris clone rebuilds in "around 30ms" per change. The Zig compiler itself takes 36 s for the first build, then 244/228/288/283 ms per incremental rebuild with `-fincremental --watch` (a long-lived process). Mar 2026: a type-resolution redesign nearly removed "over-analysis". — [Zig devlog 2026](https://ziglang.org/devlog/2026/)
- On Zig 0.15, before incremental compilation worked, a one-line change in libghostty-vt took 975 ms (2.9 s in 0.14). — [Mitchell Hashimoto](https://mitchellh.com/writing/zig-builds-getting-faster)

#### Kotlin K2
- K2 (Kotlin 2.0, 2024) is about 2x faster on average. Anki-Android clean build: 57.7 s → 29.7 s. Analysis phase up to 376% faster. Incremental analysis on Anki-Android: 0.581 s → 0.122 s. JetBrains credits a "single unified data structure" (FIR) replacing the PSI-based frontend. — [JetBrains K2 benchmarks](https://blog.jetbrains.com/kotlin/2024/04/k2-compiler-performance-benchmarks-and-how-to-measure-them-on-your-projects/)

#### Swift: the anti-example
- Chris Lattner, quoted in 2024: "we tried to make a really fancy bi-directional Hindley-Milner type checker … compile times are really bad (particularly if you have complicated expressions) … the error messages are awful because now you have a global constraint system". A 12-line string concatenation that adds an `Int` took 42 s in Swift 6 and ended with "unable to type-check this expression in reasonable time"; with `String(channel)` it took 0.19 s. `let result: Double = -(1 + 1) + -(1 + 1) - 1` takes 6.2 s. The cause is 17 overloads of `+` and 9 `ExpressibleByStringLiteral` types. — [Why Swift's type checker is so slow](https://danielchasehooper.com/posts/why-swift-is-slow/)
- The Swift team's roadmap says constraint solving with disjunctions is exponential in the worst case, so the checker must cap its total work. — [Swift Forums: roadmap for improving the type checker](https://forums.swift.org/t/roadmap-for-improving-the-type-checker/82952). The problem dates back at least to 2016. — [Cocoa with Love](https://cocoawithlove.com/blog/2016/07/12/type-checker-issues.html)

#### Low-level techniques: oxc, Ruff, Carbon
- oxc: AST in a bump arena ("O(1) allocation", "Drop entire arena at once"), "No other heap allocations", strings ≤ 24 bytes inline, SIMD whitespace skipping. The parser does "minimal semantic analysis"; scope binding and symbol resolution run in a separate semantic pass. — [oxc parser architecture](https://oxc.rs/docs/learn/architecture/parser.html)
- oxc parse benchmark on an M3 Max, single-thread: typescript.js oxc 26.4 ms / swc 91.0 ms / biome 139.3 ms; cal.com.tsx 3.4 / 14.4 / 18.3 ms. Max RSS on typescript.js: 68.8 / 92.0 / 117.4 MB. — [bench-javascript-parser-written-in-rust](https://github.com/oxc-project/bench-javascript-parser-written-in-rust)
- Ruff v0.4.0 (2024) replaced its LALRPOP parser with a hand-written recursive-descent one: 63.6 ms → 26.6 ms on a large file, and 20–40% faster lint and format overall. — [Ruff v0.4.0](https://astral.sh/blog/ruff-v0.4.0)
- Carbon's toolchain is built as distinct stages (lex → parse → check) with vectorized rather than pointer-heavy storage. I found no published lines/s target. — [Carbon toolchain docs](https://docs.carbon-lang.dev/toolchain/docs/)

#### Local measurements (this session; M4 Pro, macOS, Rust 1.99; 3 runs)
- Spawning and waiting on an empty Rust binary: 1.23 / 1.30 / 1.40 ms on average over 300 spawns. — [local bench main.rs](file:///private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/src/main.rs)
- 50k-line corpus (500 files): `stat` on all files took 0.82–0.93 ms sequentially and 0.46–0.50 ms on 14 threads. Reading all files took 4.3–7.5 ms sequentially and 3.15–3.22 ms on 14 threads warm (the first parallel round took 25.8 ms). FNV-1a hashing of all 1.55 MB took 1.53–1.65 ms. A naive tokenizer that counts 440,500 tokens took 1.48–1.90 ms single-threaded and 0.37–0.62 ms on 14 threads, including thread spawn. Reading a 20 MB file from warm page cache took 1.96–2.52 ms. — [local bench main.rs](file:///private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/src/main.rs), [local bench par.rs](file:///private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/src/par.rs)

### Inferences
- **Rough throughputs.** TS 7: 1.5M LOC (2025 count) / 10.6 s ≈ 140k LOC/s on 4 checkers, ≈ 200k LOC/s on 8. That mixes LOC and timing from different dates, so it is an order of magnitude only. Sorbet (2019): 100k lines/s per core. At those rates 50k lines costs ≈ 250–350 ms (TS 7 class) or ≈ 0.5 s single-core / ≈ 40–60 ms on 8–14 cores (Sorbet class, if scaling stays linear). dat's type system is much simpler than TypeScript's (nominal structs, no conditional types, no overloads), so Sorbet-class or better per-core throughput is a reasonable goal for a Rust implementation.
- **Doubling checkers gives diminishing returns.** Going from 4 to 8 checkers on vscode gave only 1.41x (10.6 s → 7.51 s). TS's design duplicates work per checker. A design where bodies are truly independent (Sorbet: "method bodies can be checked independently") should scale better.
- **The incremental headline numbers all come from resident processes**: ty 4.7 ms (LSP), Sorbet single-digit ms (LSP), Hack tens of ms (daemon), Zig 30–290 ms (`--watch`). I found no system that reports single-digit-millisecond incremental checking from a fresh CLI process on a large project.
- **Laziness beats raw speed.** Pyright's 386 ms vs ty's 4.7 ms on the same PyTorch edit shows that what gets recomputed matters more than how fast it is computed. Pyrefly's module-level design (2.38 s on that edit in Dec 2025) has to re-solve whole modules and their dependents, which costs a lot in Python, where inferred return types make body edits leak across modules.
- **Fine-grained is not free.** ty's numpy (30.6 s) and openai (4.6 s) outliers show that fine-grained laziness doesn't protect against pathological inference. The inference rules themselves have to be cheap.

### Gaps
- No primary source found for Hack's saved states or shallow/lazy declarations (searches returned only general hh_server material). The "< 200 ms" Hack figure comes only from an aggregator and is unverified.
- No published latency numbers for rust-analyzer edits, and no final post-migration numbers for the new salsa.
- Zig's dependency granularity (per-declaration tracking, InternPool) isn't described on the 2026 devlog page; not verified this session.
- Roslyn: no primary performance source fetched. The only Roslyn facts here come second-hand through rust-analyzer's syntax doc (see §4).
- The "up to 3x" speedup claims for `isolatedDeclarations` come from unverified blogs and are not used here.
- The local measurements are macOS-only. Linux file-open costs are usually lower but were not measured.

## 2. Which language-design rules make fast checking possible, and how the dat spec helps or hurts

### Takeaway
dat already has most of the rules the fast checkers depend on: explicit exported return types, no overloading by parameter type, explicit `implements` with methods inside the struct, explicit import paths without `export *`, no top-level code or macros, and generics checked once against constraints. There are four weak spots. (a) Exported `const`s, including exported arrow-function consts, can have inferred types, so a module's interface can depend on bodies. (b) Inferred return types on private functions create body-to-body dependencies inside a module, and the spec doesn't say what happens with recursion. (c) Circular imports need SCC handling for type aliases and inferred consts. (d) The canonical order of flattened unions must not depend on check order, or parallel checking becomes nondeterministic.

### Cited Findings
External evidence for each rule:
- **Explicit signatures at module boundaries** let modules be checked against summaries. Examples: Go deep export data (read only direct imports) ([Go README](https://go.dev/src/cmd/compile/README)); TS `isolatedDeclarations` decouples declaration emit from checking ([TS 7.0 post](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/), [swc TS9007](https://rustdoc.swc.rs/src/swc_typescript/diagnostic.rs.html)); rust-analyzer's body-edit invariant ([architecture.md](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)).
- **No global inference**: Sorbet doesn't infer across methods and uses single-shot inference with no fixed point or unification ([InfoQ](https://www.infoq.com/presentations/sorbet-type-checker-ruby), [internals.md](https://github.com/sorbet/sorbet/blob/master/docs/internals.md)). Swift's global bidirectional solver plus overloading gives exponential worst cases ([Hooper](https://danielchasehooper.com/posts/why-swift-is-slow/), [Swift Forums](https://forums.swift.org/t/roadmap-for-improving-the-type-checker/82952)).
- **No overloading**: Swift's blowups come from disjunctions (17 overloads of `+`). — [Hooper](https://danielchasehooper.com/posts/why-swift-is-slow/)
- **File-local identity, no generated top-level items, and FQNs that are enough for name resolution** make map-reduce indexing possible. Rust's macros, `#[path]` aliasing and impls-anywhere make it a "square peg". matklad's advice: "Make sure that FQNs are enough for most of the name resolution." — [Three Architectures](https://rust-analyzer.github.io/blog/2020/07/20/three-architectures-for-responsive-ide.html)
- **`import *` is costly**: Pyrefly's first phase must "solv[e] all `import *` statements transitively". — [Pyrefly ARCHITECTURE.md](https://github.com/facebook/pyrefly/blob/main/ARCHITECTURE.md)
- **Interface files can't be mutually recursive** in the header approach ("headers can't be mutually recursive"). — [Three Architectures](https://rust-analyzer.github.io/blog/2020/07/20/three-architectures-for-responsive-ide.html)
- **Order-dependence under parallelism**: TS 7 warns that varying the checker count "may rarely surface order-dependent results". — [TS 7.0 post](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)

How each dat spec rule bears on checker speed:

| Spec rule | Source | Effect | Why |
| --- | --- | --- | --- |
| Exported functions, and methods of exported structs, must declare return types "so the compiler can check each module without reading the bodies of the modules it imports" | [06-functions.md](../../reference/06-functions.md), [17-imports.md](../../reference/17-imports.md) | Helps a lot | Same idea as `isolatedDeclarations` and rust-analyzer's ItemTree firewall. A body edit never changes a function's exported signature. |
| Named functions declare `throws`; every throwing call is marked `try` | [06-functions.md](../../reference/06-functions.md), [12-errors.md](../../reference/12-errors.md) | Helps | Error sets of callees come from signatures, so "`catch` clause that nothing can throw" needs only signatures. |
| "An arrow function's errors are inferred from its body"; arrow return type "optional and inferred" | [12-errors.md](../../reference/12-errors.md), [06-functions.md](../../reference/06-functions.md) | Neutral inside bodies; **hurts if exported** | `export const f = (x: int) => ...` gives the module an exported value whose type (return and `throws`) needs the body. |
| `export` applies to `const`; consts may omit the type (`const LIMITS = { free: 10, pro: 1000 };`) | [17-imports.md](../../reference/17-imports.md), [02-variable-declaration.md](../../reference/02-variable-declaration.md) | **Hurts** | An unannotated exported const's type needs its initializer checked. If the initializer calls a private function with an inferred return type, importers transitively depend on a body. This breaks the "without reading bodies" promise in 17-imports.md. |
| Return type of non-exported functions "can be left out and inferred from the body" | [06-functions.md](../../reference/06-functions.md) | Mixed | Bounded to one module, but a caller's types then depend on the callee's body. This needs a topological order over the module's call graph, and recursion through inferred functions is unspecified. |
| No overloading by parameter type; each operator defined at most once; struct must be the left operand | [08-structs.md](../../reference/08-structs.md) | Helps a lot | Operator resolution is one lookup on the left operand's type: no disjunctions, so no Swift-style blowup. |
| Generic type arguments inferred "from the call's arguments, and from the expected type when the arguments aren't enough" | [19-generics.md](../../reference/19-generics.md) | Neutral if single-pass | Local bidirectional inference is linear when there are no overloads. Risk comes only from backtracking or argument-order dependence. |
| Callback parameter types inferred from the expected function type (`reduce((sum, x) => sum + x, 0.0)`) | [06-functions.md](../../reference/06-functions.md) | Neutral, needs a rule | `sum`'s type comes from the *later* argument `0.0`, so the checker needs a fixed argument order (e.g. non-callback arguments first). Otherwise it ends up backtracking. |
| Generics constrained by interfaces; "Without a constraint, a type parameter supports only what every type supports" | [19-generics.md](../../reference/19-generics.md) | Helps | Generic bodies are checked once at definition, not per instantiation. Monomorphization is a codegen concern only. |
| Literals take the expected type (`const ratio: float = 1`; list literal → set/tuple; record literal → struct) | [02-variable-declaration.md](../../reference/02-variable-declaration.md), [01-builtin-types.md](../../reference/01-builtin-types.md) | Neutral | Expected-type-driven, local, no search. |
| "Unions are flattened and deduplicated, and member order doesn't matter" | [18-enums-and-unions.md](../../reference/18-enums-and-unions.md) | Helps if canonicalized; risk to determinism | Interned canonical unions make type equality O(1). The canonical order must come from content (e.g. qualified names), not from the order types were created during a parallel check, or messages and codegen differ between runs (cf. TS 7 order dependence). |
| Shared fields readable on unions without narrowing | [18-enums-and-unions.md](../../reference/18-enums-and-unions.md) | Neutral | Per-union-type computation, memoizable. |
| Flow-sensitive narrowing on locals, params and field paths, ended by assignment or `mut` | [05-type-narrowing.md](../../reference/05-type-narrowing.md) | Neutral (body-local) | Never crosses a body boundary. Cost grows with CFG size and variable count (Pyright's sympy issue), so the CFG must stay small (Sorbet: ~11 instruction kinds). |
| "Modified copy never read" error, `mut` aliasing, closure capture rules | [02-variable-declaration.md](../../reference/02-variable-declaration.md), [06-functions.md](../../reference/06-functions.md) | Neutral (body-local) | Per-body liveness and alias passes; linear. |
| Circular imports allowed, "because importing never runs code" | [17-imports.md](../../reference/17-imports.md) | Mostly harmless *given* explicit signatures | Body checking needs only interfaces, so cycles don't block it. Type aliases and inferred consts across a cycle need SCC handling. Go-style DAG-ordered package parallelism isn't available. |
| Module-level `const` computed on first use; self-dependent initializer → `AssertionError` at runtime | [17-imports.md](../../reference/17-imports.md) | Runtime rule; type-level gap | The spec covers the runtime cycle, not an inference cycle (`const A = B + 1; const B = A + 1;` with no annotations). |
| Explicit `.dat` paths, folder `index.dat`, no bare-name resolution, no `export *` (only named re-exports) | [17-imports.md](../../reference/17-imports.md) | Helps a lot | Module resolution is pure path arithmetic (no node_modules search, no tsconfig paths). Re-export chains are named, so no transitive `import *` phase (cf. Pyrefly). |
| Module has only declarations; top-level names visible before their declaration | [17-imports.md](../../reference/17-imports.md), [14-scope.md](../../reference/14-scope.md) | Helps | A declaration-collection pass per file is all a map-reduce index needs. No code runs on import. |
| `implements` is explicit; methods live inside the struct body; `match` on interface values requires `_` "because other modules can add implementations" | [15-interfaces.md](../../reference/15-interfaces.md), [18-enums-and-unions.md](../../reference/18-enums-and-unions.md) | Helps a lot | A struct's whole method set is in one file (unlike Rust's impls anywhere), and exhaustiveness never needs a whole-program implementor search. |
| Private fields: "a struct with a private field that has no default can only be created by its own static functions" | [08-structs.md](../../reference/08-structs.md) | Neutral, but observable | Private fields affect what importers may write (literals), and Hashable/Equatable derivation depends on all field types. They belong in the module interface hash. |
| Equatable/Hashable/Comparable derived automatically from fields and operator methods | [01-builtin-types.md](../../reference/01-builtin-types.md), [15-interfaces.md](../../reference/15-interfaces.md) | Neutral | Computed from declarations only. Recursive structs (`TreeNode`) need a coinductive rule. |
| Tests can use the module's private declarations; tests share no state | [20-testing.md](../../reference/20-testing.md) | Helps | Test bodies are leaves: editing one invalidates nothing else, and test selection by dependency is safe. |

### Inferences
- dat's spec is closer to the "explicit interface" end of the range than TypeScript or Python. A body edit can change something outside its module in only two ways: through an unannotated exported `const`, or an exported arrow-function `const`. Closing those two gaps gives dat rust-analyzer's invariant at module granularity "for free".
- The inferred return types of private functions are a deliberate feature and should stay. They do mean that within one module a body edit can change other bodies' types. So per-body caching needs a second early-cutoff level: the inferred signature of a private function, compared by value after recomputation (salsa-style backdating done by hand).
- Because exported signatures are explicit, circular imports don't stop parallel body checking. All bodies in all modules can be checked in one parallel phase after the interfaces are collected. That is better than Go's DAG-ordered build for one large project.
- The absence of overloading means bidirectional checking stays linear. The only Swift-like risk left is if callback inference or generic inference from the expected type ever backtracks. A fixed argument-processing order avoids that.

### Gaps
- The spec doesn't say whether recursion (or mutual recursion) among private functions with inferred return types is allowed, nor what error it gives.
- The spec doesn't say what happens when inferred module `const` types form a cycle, or how type-alias cycles across circular imports are reported.
- The spec doesn't fix the order in which union members are printed (its example prints `string | int | null` for `A | null` with `A = int | string`).

## 3. Incrementality options: granularity, on-disk caches vs a daemon, how signature hashing and early cutoff work, and their costs

### Takeaway
There are three granularities on the market: per-module (Pyrefly, gopls, Sorbet's file-level fast path), per-declaration via an item-tree firewall (rust-analyzer, ty), and per-expression (ty's earlier design). Each step finer buys editor latency and costs complexity and memory. With dat's explicit export rule, per-module invalidation **with early cutoff on the module interface hash**, plus per-function-body result caching inside the module, should give most of the benefit of salsa without salsa. On-disk caches cut memory and restart time (gopls −75%). Every measured single-digit-millisecond result comes from an in-memory resident process.

### Cited Findings
- **Per-module (Pyrefly)**: "large-scale incrementality (at the module level)"; "no need to solve a single binding in isolation if solving all bindings in a module is fast enough". — [Pyrefly ARCHITECTURE.md](https://github.com/facebook/pyrefly/blob/main/ARCHITECTURE.md). In Dec 2025 Pyrefly took 2.38 s to update after a PyTorch edit, against ty's 4.7 ms. — [Astral: ty](https://astral.sh/blog/ty)
- **Per-package with summaries (gopls/Go)**: per-package results saved to files; a reference graph in memory lets gopls skip packages a change cannot affect; memory ∝ open packages + direct imports. — [Scaling gopls](https://go.dev/blog/gopls-scalability). Deep export data means a package reads only its direct imports' summaries. — [Go README](https://go.dev/src/cmd/compile/README)
- **Per-file re-definition (Sorbet fast path)**: delete a changed file's definitions and re-run definition entry; 10% of edits still take the full slow path (Jan 2023). — [Making Sorbet more incremental](https://blog.jez.io/making-sorbet-more-incremental/)
- **Per-item firewall + salsa (rust-analyzer)**: ItemTree "stable over modifications to function bodies"; everything in memory, no IO. — [architecture.md](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)
- **Fine-grained queries (ty, earlier code)**: inference queries per scope, per definition and per expression. — [red-knot mirror](https://git.joshthomas.dev/language-servers/ruff/src/commit/7fc39ad624c0df95d62066d758fdb6fdcc6de2e3/crates/red_knot_python_semantic/src/ast_node_ref.rs)
- **Early cutoff (backdating)**: recompute, compare with the old value, and if equal, mark the result unchanged so dependents don't run. **Durability**: skip validating results that depend only on rarely-changing inputs. — [Salsa algorithm](https://salsa-rs.github.io/salsa/reference/algorithm.html)
- **Memory cost of fine-grained memoization**: rust-analyzer's port to the new salsa ran ~250 MB over its old memory use in Jan 2025 ([salsa notes](https://hackmd.io/@salsa/SkNtN8Lv1e)). Pyrefly (no salsa) used less memory than ty on 31 of 53 packages ([Pyrefly blog](https://pyrefly.org/blog/speed-and-memory-comparison/)). Typed syntax trees are ~30x the source size ([gopls](https://go.dev/blog/gopls-scalability)).
- **Complexity cost**: matklad calls query-based compilers "extra complexity, slower performance" and "somewhat uncharted territory" (2020). — [Three Architectures](https://rust-analyzer.github.io/blog/2020/07/20/three-architectures-for-responsive-ide.html)
- **On-disk vs resident**: gopls's on-disk cache made the second start "much more quickly" ready ([gopls](https://go.dev/blog/gopls-scalability)). rust-analyzer has no persistent cache yet, and persistent caches were named as a future benefit of new salsa in Mar 2025 ([r/rust mirror](https://redlib.hackliberty.org/r/rust/comments/1jbyunp/call_for_testing_rustanalyzer)). Hack keeps everything in memory in a daemon ([FB 2014](https://engineering.fb.com/2014/03/20/developer-tools/hack-a-new-programming-language-for-hhvm/)). Zig's ms rebuilds use `--watch` ([Zig devlog](https://ziglang.org/devlog/2026/)).
- **Serialization**: Go's export data is indexed for "lazy decoding" ([Go README](https://go.dev/src/cmd/compile/README)). rkyv's zero-copy design accesses data "directly referencing bytes in the serialized form". It offers a validated (safe) path and an unvalidated (unsafe) fast path. — [rkyv book](https://rkyv.org/print.html)

### Inferences
**How signature hashing should work in dat.** This expands the roadmap's one line: "modules check in parallel against their imports' signatures and cache by signature hash".
1. Per module, compute a **resolved interface**, the equivalent of an ItemTree or Go export data. It covers exported names; struct fields, *including private fields and whether they have defaults* (observable via [08-structs.md](../../reference/08-structs.md) literal rules and Hashable derivation); method and static signatures; operator methods present; `implements` lists; interface method signatures; enum members; union alias expansions; exported const types; `throws` lists; and re-export maps. Types are hashed after resolution: alias-expanded, with canonical union order and qualified type identities. Raw syntax is not hashed, because a re-exported or aliased type in a third module can change meaning without the text changing.
2. **Leave spans, comments and private bodies out of the hash.** Store definition locations in a side table, so an edit above a declaration doesn't shift positions and invalidate every importer. Diagnostics that point at "defined here" read the side table.
3. **Early cutoff, level 1**: after a module is re-checked, compare the new interface hash with the old one. If equal, importers are untouched. With dat's export rules this holds for every body-only edit, once the exported-const gap is closed.
4. **Early cutoff, level 2, inside the module**: cache each function or test body's diagnostics and typed IR, keyed by (body hash, hashes of the module-local signatures it uses). A private function with an inferred return type re-infers its type. If the type is unchanged it is backdated, so its callers stay cached.
5. **Finer cutoff (optional)**: keep a per-export hash plus, per importer, the set of names it uses. Then changing `dot`'s signature re-checks only modules that import `dot`. Namespace imports (`import * as m`) record names at use sites.
6. **Deep vs shallow**: deep summaries (Go) let a module read only its direct imports, but they duplicate types upward. Shallow summaries (gopls) need random access to every dependency's summary. Inside one process the choice doesn't matter much, since everything is in memory. For an on-disk cache, a single project-wide cache file with an index (Go-style lazy decoding) avoids both problems.

Cost of each granularity for dat, estimated from the evidence above:

| Granularity | Body-edit latency (resident) | Memory | Implementation complexity | Fits dat? |
| --- | --- | --- | --- | --- |
| Whole program, no cache | = cold check (est. 30–100 ms for 50k lines, §5) | Lowest | Lowest | Good enough for CLI at ≤ 50k lines; misses 10 ms |
| Per-module + interface-hash cutoff | re-check one module (≈ module size / throughput; ~1–5 ms for a 1–2k-line module, est.) | Low (interfaces + per-module diagnostics) | Low–medium | Strong, because dat's exports are explicit |
| + per-body caching inside the module, private-signature backdating | re-check one body + its in-module dependents (sub-ms, est.) | Medium (typed IR per body) | Medium (hand-rolled dependency keys) | Strong; matches rust-analyzer's invariant |
| salsa, per-declaration/per-expression | sub-ms (ty: 4.7 ms on PyTorch) | Highest (memo tables; cf. rust-analyzer's ~250 MB regression) | High; framework churn (new-salsa port took months) | Overkill unless dat adds macros or comptime |

### Gaps
- No published measurement isolates the memory overhead of salsa memoization from the rest of ty or rust-analyzer.
- I found no source on how Go's build cache decides that importers need no recompilation when export data doesn't change. Not verified this session.
- TS `.tsbuildinfo` incremental behaviour in TS 7 wasn't covered by the sources fetched.

## 4. Data-structure choices: lossless CST vs AST with spans, arenas and IDs, interned types, parallelism

### Takeaway
The fastest parsers (oxc, Ruff) build a plain arena-allocated AST and are 3–5x faster and lighter than CST-based tools. rust-analyzer's own docs list rowan's costs: linear child lookup, no parent pointers, refcount and thread-local traffic during traversal, and doubled memory if red nodes are kept. They also say incremental reparsing "doesn't actually matter much" because full reparsing is fast enough. For dat: a lossless **token stream** (with comments and whitespace) plus an **arena AST indexed by u32 IDs** that point back at token ranges. Types interned as integer IDs. Parallelism by phase, with rayon-style work-stealing over files and bodies.

### Cited Findings
- rowan green nodes are a DST "which uses a single allocation for header and children". Tokens are interned per file: "To avoid allocating EVERY SINGLE TOKEN on the heap, syntax trees use interning". Child access means "linearly traversing the children". "there are no parent pointers!" Memoized red nodes "more than doubles the memory requirements for fully realized syntax trees", so rust-analyzer moved to cursors. Traversal "allocates and does atomic operations", which led to non-thread-safe `Rc` plus a thread-local free list: "TLS + rc bump". "frontend does not keep syntax trees for all files in memory." "incremental reparsing doesn't actually matter much for IDE use-cases … parsing from scratch seems to be fast enough." Roslyn and Swift get O(1) child access; IntelliJ uses linear traversal; "C# combats increased memory usage by using weak references." — [rust-analyzer syntax.md](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/syntax.md)
- oxc arena AST: 26.4 ms vs swc 91.0 ms vs biome 139.3 ms (typescript.js, M3 Max, single-thread); RSS 68.8 vs 92.0 vs 117.4 MB. — [oxc bench](https://github.com/oxc-project/bench-javascript-parser-written-in-rust); design — [oxc parser architecture](https://oxc.rs/docs/learn/architecture/parser.html)
- Hand-written recursive descent beat a generated LALRPOP parser 2.4x (Ruff). — [Ruff v0.4.0](https://astral.sh/blog/ruff-v0.4.0)
- Integer IDs smaller than pointers, interned names, a desugared AST (98 parser node kinds → 34 AST kinds), a small CFG and single-shot inference (Sorbet). — [Sorbet internals.md](https://github.com/sorbet/sorbet/blob/master/docs/internals.md); C++ chosen for memory locality — [InfoQ](https://www.infoq.com/presentations/sorbet-type-checker-ruby)
- Kotlin K2's speedup is credited to a "single unified data structure" (FIR) replacing PSI. — [JetBrains](https://blog.jetbrains.com/kotlin/2024/04/k2-compiler-performance-benchmarks-and-how-to-measure-them-on-your-projects/)
- Carbon: separate stages, vectorized storage rather than pointer-heavy. — [Carbon toolchain docs](https://docs.carbon-lang.dev/toolchain/docs/)
- Parallelism: TS 7 parses and emits in parallel across files with little overhead; checkers are partitioned deterministically; going 4 → 8 checkers made vscode 1.41x faster. — [TS 7.0](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/). Sorbet claims linear scaling to 64 cores. — [InfoQ](https://www.infoq.com/presentations/sorbet-type-checker-ruby). rust-analyzer builds trees per file for parallel parsing. — [architecture.md](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)
- Local: a naive tokenizer over 50k lines takes ~1.5–1.9 ms single-threaded and ~0.4–0.6 ms on 14 threads, including thread spawn. — [local bench](file:///private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/src/main.rs)

### Inferences
- **Which tree to build.** The roadmap says "lossless syntax tree". What `dat fmt` and machine-applicable fixes actually need is lossless *tokens* (comments and trivia). A full rowan-style red-green tree isn't required. The rust-analyzer team itself says incremental reparsing, a main selling point of green trees, isn't needed. An arena AST whose nodes store `TokenId` ranges, plus a per-file token vector with trivia, gives the formatter everything it needs. It keeps oxc-class parse speed and needs no refcounts during checker traversal. Fixes are emitted as text edits on spans, as the roadmap's JSON already shows.
- **Lexing and parsing are not the bottleneck at 50k lines** (a few ms in total, even before parallelism). The budget goes to name resolution, checking and IO. So effort should go to the checker's data layout (IDs, interned types, small CFG) more than to SIMD lexing.
- **Interned types with u32 IDs** let canonical unions compare in O(1) and hash cheaply. Parallel checking then needs either a concurrent (sharded) interner or per-thread interners merged at phase boundaries. In either case canonical union order must come from content, not ID order, so output is deterministic (cf. TS 7 order dependence).
- **Phase parallelism suits dat better than checker partitioning.** Bodies are independent once interfaces exist, so work-stealing over bodies avoids TS 7's duplicated per-checker work.

### Gaps
- Not verified this session: that Biome's parser builds a rowan-derived CST (that would explain its parse numbers), and that gofmt and Ruff's formatter work from an AST plus comments rather than a lossless tree. Both are widely believed, but no source was fetched.
- No published apples-to-apples benchmark of a rowan CST against an arena AST for the same language and grammar.

## 5. Does the < 10 ms incremental budget on 50k lines need a daemon? Are the roadmap budgets realistic?

### Takeaway
A fresh CLI process can meet < 10 ms for a body-only edit on macOS, but only narrowly. It has to detect changes by `stat` metadata rather than reading files, load a compact zero-copy cache, and re-check only the edited module. Process spawn alone is ~1.3 ms, and reading every source file costs 3–7 ms on macOS, which would blow the budget. Interface-changing edits with many dependents can't meet 10 ms from a CLI. A resident daemon shared by CLI and LSP brings body edits to ~2–5 ms and makes the budget robust. The roadmap's cold budget (< 300 ms for 50k lines) is realistic and probably loose. The 10 ms budget needs to be split by edit kind and process mode.

### Cited Findings
- Process spawn+wait of an empty Rust binary: 1.23–1.40 ms. `stat` on 500 files: 0.46–0.93 ms. Reading 500 files (1.55 MB): 3.2 ms in parallel, 4.3–7.5 ms sequentially (25.8 ms on a first parallel round). Reading a 20 MB file warm: ~2.0–2.5 ms. Hashing 1.55 MB with byte-wise FNV: ~1.5 ms. — [local bench main.rs](file:///private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/src/main.rs), [par.rs](file:///private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/src/par.rs)
- Resident-process incremental numbers: ty 4.7 ms (PyTorch, LSP) — [Astral](https://astral.sh/blog/ty); Sorbet "single-digit milliseconds" in IDE mode — [InfoQ](https://www.infoq.com/presentations/sorbet-type-checker-ruby); Hack "tens of milliseconds" (2015) — [Next Platform](https://www.nextplatform.com/code/2015/06/16/how-facebook-hack-is-upending-programming-efficiency/1658025); Zig 30 ms (small app) to 228–288 ms (Zig compiler) with `--watch` — [Zig devlog](https://ziglang.org/devlog/2026/).
- Cold throughput references: Sorbet 100k lines/s per core, linear to 64 cores (2019) — [InfoQ](https://www.infoq.com/presentations/sorbet-type-checker-ruby); TS 7 vscode 10.6 s with 4 checkers / 7.51 s with 8 — [TS 7.0](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/); flask in 0.2 s for both ty and Pyrefly — [Pyrefly blog](https://pyrefly.org/blog/speed-and-memory-comparison/).
- On-disk summaries trade memory for IO: gopls saw −75% memory and a faster second start. — [gopls](https://go.dev/blog/gopls-scalability). Zero-copy formats avoid a deserialize step. — [rkyv](https://rkyv.org/print.html)

### Inferences
**CLI with an on-disk cache, body-only edit to one 100–1,000-line file, 50k-line project, warm OS page cache.** Estimated from the local numbers:

| Step | Estimate |
| --- | --- |
| Process spawn + exit | ~1.3 ms |
| Change detection by `stat` (mtime+size) on 500 files, parallel | ~0.5 ms |
| Load cache (2–5 MB of interfaces + per-module diagnostics, mmap + zero-copy) | ~0.1–0.6 ms |
| Read, lex, parse the changed file; re-check its bodies; interface hash unchanged → stop | < 1 ms (est.) |
| Print all diagnostics (cached for other modules + fresh for this one) | < 0.5 ms |
| Write the cache back (only the changed module's entries; atomic rename) | ~0.5–2 ms |
| **Total** | **~4–6 ms best case** |

- Detecting changes by reading and hashing every file adds 3–7 ms (read) plus ~1.5 ms (hash) and pushes the total to ~9–15 ms, over budget. So the CLI must trust metadata and hash only files whose metadata changed. Metadata is fragile: same-mtime edits and clock skew are a known risk class, so there should be a `--no-cache` escape hatch.
- If the interface changes and k modules import it, add their re-check time. Across 50k lines this approaches the cold time, so 10 ms can't be promised for those edits from any mode. It is still bounded by cold time, estimated 30–100 ms.
- **Daemon path**: `dat check` becomes a thin client (as `hh_client` is to `hh_server`). It still pays ~1.3 ms to spawn plus an IPC round trip. Typically the server has already re-checked on the file-watcher event, so it returns cached diagnostics: ~2–3 ms total. Without the watcher head start, a body edit costs an extra < 1 ms of checking.
- **Cold budget**: reading and lexing 50k lines costs < 10 ms locally. At Sorbet-class throughput, checking costs ~40–60 ms on 8–14 cores if scaling stays linear. At TS 7-class throughput, which is a worst case for a language much heavier than dat, it costs ~250–350 ms. So 300 ms is a safe ceiling. A tighter target (≤ 100 ms on an 8-core laptop) is plausible and would let the CLI skip caching at small sizes.
- **"Interpreter start < 5 ms"** is consistent with ~1.3 ms spawn, but only if `dat run` doesn't type-check the whole program cold. The budget should say "small program, or warm cache/daemon". Otherwise `dat run` = check time + start time.

**Proposed corrected budgets.** These are proposals; the hardware must be named, and p50/p95 should be measured over ≥ 30 runs:

| Measure | Proposed budget | Why |
| --- | --- | --- |
| Cold `dat check`, 50k lines, no cache, 8-core laptop | target ≤ 100 ms; hard ceiling ≤ 300 ms (also on a 4-vCPU CI runner) | Sorbet-class throughput gives ~40–60 ms; TS 7-class ~300 ms |
| `dat check`, no changes, warm (CLI + cache, or daemon) | ≤ 5 ms | spawn + stat + cached diagnostics |
| `dat check` after a body-only edit, CLI + on-disk cache | ≤ 10 ms p95 | ~4–6 ms estimate above; leaves headroom for macOS IO |
| Same, via daemon (or LSP diagnostics after the edit) | ≤ 5 ms p95 | ty 4.7 ms on PyTorch as reference |
| Edit that changes a module interface with k importers | ≤ 5 ms + re-check of the k importers; never above the cold budget | interface cutoff can't apply |
| LSP hover / go-to-definition | ≤ 5 ms p95 (est.) | in-memory lookup |
| `dat fmt` one file (≤ 2k lines) | ≤ 5 ms incl. spawn | oxc parses a large file in ~26 ms; a typical file is far smaller |
| Interpreter start to first statement | ≤ 5 ms for a cached or single-module program | spawn ~1.3 ms |

### Gaps
- No Linux measurements were taken. The CI budgets should be calibrated on the actual runner.
- IPC round-trip cost (Unix socket) wasn't measured. The "~0.1 ms" order of magnitude is an assumption.
- rkyv-on-mmap access cost for a structure the size of a dat interface cache isn't measured in any source found.
- Cold-cache (page cache empty) IO wasn't measured systematically. The single 25.8 ms first round suggests it can dominate.

## 6. What else, beyond signature caching, could dat do?

### Takeaway
The biggest wins left are laziness (check what's asked first), a resident server shared by CLI and LSP that checks on file events before the agent asks, a precompiled standard library, phase-parallel checking, and dependency-aware test selection. Each is supported by a system above.

### Cited Findings
- Laziness is the main lever ("skip huge swaths of code altogether"). — [Three Architectures](https://rust-analyzer.github.io/blog/2020/07/20/three-architectures-for-responsive-ide.html). Pyright analyzes open files only by default. — [Pyright 1.1.0](https://newreleases.io/project/npm/pyright/release/1.1.0)
- A resident server behind a thin client (hh_server/hh_client) — [hh_client](https://manpages.org/hh_client), [FB 2014](https://engineering.fb.com/2014/03/20/developer-tools/hack-a-new-programming-language-for-hhvm/). A native file watcher rather than polling (TS 7). — [TS 7.0](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)
- Durability for rarely changing inputs (the std library). — [Salsa algorithm](https://salsa-rs.github.io/salsa/reference/algorithm.html). Precomputed package summaries (Go export data). — [Go README](https://go.dev/src/cmd/compile/README)
- Cancellation of stale work when input changes, and keeping typing-path requests on the main thread. — [rust-analyzer architecture.md](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/architecture.md)
- Deterministic work partitioning under parallelism. — [TS 7.0](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)
- Tests share no state and run in any order. — [20-testing.md](../../reference/20-testing.md)

### Inferences
Concrete additions for dat, roughly in order of value per cost:
1. **Check-on-event server shared by CLI and LSP.** `dat check` and the LSP talk to one per-project server. If none is running, `dat check` runs in-process with the on-disk cache, and optionally leaves a server behind (opt-in, e.g. `dat server` or an auto-spawn setting). Agents edit files and then run `dat check`; a watcher-driven server has usually finished before the command arrives.
2. **Precompiled std library interface** embedded in the `dat` binary, generated at compiler build time (Go export-data style, salsa "high durability"). Nothing in std is parsed or checked at startup. This also protects the 5 ms interpreter-start budget.
3. **Phase-parallel pipeline**: parse all files in parallel → collect module interfaces in parallel (SCC pass only for type aliases and inferred consts across cycles) → check all bodies in parallel with work stealing → merge diagnostics in a deterministic order.
4. **Lazy, ordered checking for focused requests.** `dat check path.dat` and LSP requests check the requested file's bodies first, using only interfaces of the rest, then the remainder in the background (LSP) or in parallel (CLI).
5. **Dependency-aware test selection.** `dat test` can skip tests whose module and transitive dependencies (by interface and body hash) are unchanged and whose last result is cached. This is safe because tests share no state ([20-testing.md](../../reference/20-testing.md)). Flag it as a CLI behavior question (§7, Q-E).
6. **Cache per-module diagnostics, not just types.** Agents need the *complete* error list from each run (the roadmap's "one run reports every error"). Incremental runs must re-emit errors of unchanged modules without re-checking them.
7. **Cheap inference rules by construction.** Fix the argument-processing order for callbacks (non-function arguments first, then arrow bodies). Check each expression once against its expected type, with no backtracking (Sorbet's single-shot approach). Keep the per-body CFG small. Inference cost then stays linear in body size, with no Swift-style work caps.
8. **Interface-hash-aware codegen and interpreter caches.** The same per-module hashes key the typed IR cache for the interpreter and the generated Rust per module, which feeds the roadmap's "compiled test, warm cache, one module changed < 3 s".
9. **Perf gates with a synthetic corpus generator** (like the local 50k-line corpus) checked into the repo, measuring cold, warm-no-change, body-edit, interface-edit and LSP latency as p50/p95 in CI from M0.
10. **Cancellation and unwinding in the compiler.** A long check is cancelled when a newer edit arrives (rust-analyzer). dat already plans `panic = "unwind"` for runtime errors.

### Gaps
- No source measures how often agents' edits are body-only vs interface-changing. Sorbet's human data (≈ 10% slow path) is the nearest proxy.

## 7. Decisions for dat-lang

### Takeaway
Ten decisions follow. Eight can be made without touching the spec. Six questions need the owner, because they would change or clarify a spec rule: exported `const` types, exported arrow consts, recursion in inferred private functions, cycles among inferred consts and type aliases, union display order, and `dat test` skipping unchanged tests.

### Cited Findings
Key numbers used below (all cited in §§1–6): ty 4.7 ms body-edit vs Pyright 386 ms vs Pyrefly 2.38 s ([Astral](https://astral.sh/blog/ty)); TS 7 8–12x, 4→8 checkers only 1.41x ([TS 7.0](https://devblogs.microsoft.com/typescript/announcing-typescript-7-0/)); Sorbet 100k lines/s/core and single-digit-ms IDE ([InfoQ](https://www.infoq.com/presentations/sorbet-type-checker-ruby)); gopls −75% memory with an on-disk cache ([gopls](https://go.dev/blog/gopls-scalability)); oxc 26 ms vs biome 139 ms parse, 69 vs 117 MB ([oxc bench](https://github.com/oxc-project/bench-javascript-parser-written-in-rust)); rowan red-node memoization "more than doubles" memory ([syntax.md](https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/syntax.md)); local spawn ~1.3 ms, 500-file read 3–7 ms ([local bench](file:///private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/src/main.rs)).

### Inferences

#### D1. Incremental architecture and granularity
- **Options**: (a) whole-program re-check, no incrementality; (b) per-module re-check with interface-hash early cutoff (Pyrefly, gopls); (c) (b) plus per-body result caching and backdating of private inferred signatures; (d) salsa per-declaration/per-expression queries (rust-analyzer, ty).
- **Evidence**: (a) is simplest. The cold path may be ≤ 100 ms (§5) but can't meet 10 ms. (b) Pyrefly chose it for "raw performance and a simpler module-centric design", but took 2.38 s on a PyTorch edit, mainly because Python body edits leak through inferred return types. dat's explicit exports stop that leak, so (b) behaves much better in dat than in Python. (c) adds rust-analyzer's invariant inside a module, so large files stay fast. (d) is the most proven for LSP latency (ty 4.7 ms), but the rust-analyzer migration took months and ran ~250 MB over its old memory use in the interim; matklad says "extra complexity, slower performance"; Pyrefly beat ty on memory in 31 of 53 packages.
- **Recommendation**: (c), hand-rolled. Explicit `ModuleInterface` and `BodyResult` types with explicit hash keys, and an internal API shaped like queries, so moving to salsa later is mechanical if dat ever adds macros, comptime or other cross-body features.
- **Cost**: medium. Interface hashing (resolved, span-free, including private fields), per-body keys, and backdating of inferred private signatures. Rough estimate: 2–4 weeks of compiler work spread over M1–M6.
- **Settle by**: M1 for the data model (bodies, interfaces, side tables for spans); M6 for the actual cache.
- **Spec change**: none (but see D7 for the gaps that make interfaces depend on bodies).

#### D2. Process model: CLI cache vs daemon
- **Options**: (a) CLI only, no cache; (b) CLI with on-disk cache; (c) resident daemon only (Hack); (d) a server shared by CLI and LSP, plus in-process fallback with the on-disk cache.
- **Evidence**: every single-digit-ms result in the survey comes from a resident process. A fresh CLI on macOS pays ~1.3 ms spawn, and reading all sources costs 3–7 ms. The estimated best case for CLI + cache is 4–6 ms for a body-only edit. gopls's disk cache cut memory 75% and sped up restarts.
- **Recommendation**: (d). M6 ships (b) with metadata-based change detection and a zero-copy cache. M7 adds the server (`dat server`/LSP) with a file watcher, and `dat check` becomes a thin client when a server is running.
- **Cost**: (b) a cache format plus invalidation tests, with a `--no-cache` flag. (d) IPC, watcher, lifecycle and stale-server handling: medium–high, but the LSP needs most of it anyway.
- **Settle by**: M6 (cache) and M7 (server). Decide in M0 that the checker core is a library with no global state, so both modes host it.
- **Spec change**: none.

#### D3. Cache format and change detection
- **Options**: (a) serde + bincode-style deserialize; (b) rkyv zero-copy on mmap; (c) a hand-rolled indexed format with lazy decoding (Go export data); (d) no cache at all at ≤ 50k lines.
- **Evidence**: the 20 MB read costs ~2 ms, and a full deserialize would add allocation proportional to size (not measured here). rkyv accesses the archive in place but needs validation for untrusted bytes. Go's index allows "lazy decoding". (d) is viable only if the cold check measures ≤ ~10 ms, which is unlikely at 50k lines.
- **Recommendation**: (b) or (c). Validate the cache once with a checksum header rather than per-field. Detect changes by (mtime, size, inode); hash content only for files whose metadata changed.
- **Cost**: low–medium. The format must be versioned with the compiler build hash.
- **Settle by**: M6.
- **Spec change**: none.

#### D4. Syntax representation
- **Options**: (a) rowan-style lossless red-green CST (the roadmap's "lossless syntax tree"); (b) arena AST with spans, comments dropped; (c) arena AST with u32 IDs plus a lossless per-file token/trivia vector that nodes reference.
- **Evidence**: oxc's arena AST parses 3–5x faster than swc and biome, with 40% less RSS than biome. rowan needs linear child lookups, has no parent pointers, and does refcount/TLS work per traversal step; memoized red nodes double memory. rust-analyzer says incremental reparsing doesn't matter. Hand-written recursive descent beat LALRPOP 2.4x (Ruff).
- **Recommendation**: (c), with a hand-written lexer and Pratt/recursive-descent parser (the roadmap already plans this), error recovery, and full reparse of the changed file. `dat fmt` and fixes work from AST + tokens + trivia.
- **Cost**: the formatter must attach comments itself (rowan gives this for free). Low–medium.
- **Settle by**: M1.
- **Spec change**: none. It changes a roadmap line, not the spec.

#### D5. Type representation and determinism
- **Options**: (a) types as trees compared structurally; (b) interned types with u32 IDs and canonical unions ordered by creation ID; (c) interned types with canonical unions ordered by content (qualified names, structural key).
- **Evidence**: TS 7 warns that results can depend on order when the checker count changes. dat requires that unions are "flattened and deduplicated, and member order doesn't matter" ([18-enums-and-unions.md](../../reference/18-enums-and-unions.md)). Sorbet's integer Refs are smaller than pointers and compare quickly.
- **Recommendation**: (c), with a concurrent or sharded interner. Error messages and generated Rust enum names use the content order.
- **Cost**: low.
- **Settle by**: M1 (interner); M3 (unions).
- **Spec change**: none, but the display order of union members is an owner question (Q-D).

#### D6. Parallelism strategy
- **Options**: (a) single-threaded; (b) Go-style DAG-ordered module parallelism; (c) TS 7-style fixed checker workers with duplicated work; (d) phase-parallel: parse ∥ → interfaces ∥ (SCC only where needed) → bodies ∥ with work stealing.
- **Evidence**: (b) can't work, because dat allows import cycles ([17-imports.md](../../reference/17-imports.md)). (c) gave only 1.41x going from 4 to 8 checkers in TS 7. Sorbet scales linearly with independent method bodies. At 50k lines, lexing takes < 2 ms single-threaded, so parsing parallelism matters less than checking parallelism.
- **Recommendation**: (d). Use a sequential fast path below a size threshold. Locally, lexing 50k lines on 14 fresh threads took 0.4–0.6 ms including thread spawn, against ~1.5 ms on one thread, so thread startup eats most of the gain on small inputs. Use a persistent pool, not per-call threads.
- **Cost**: low–medium if the checker has no global mutable state from M1 on.
- **Settle by**: architecture rule in M1; implementation in M6.
- **Spec change**: none.

#### D7. Close the "interface depends on bodies" gaps (owner questions)
- **Options for exported `const` without a type** ([17-imports.md](../../reference/17-imports.md), [02-variable-declaration.md](../../reference/02-variable-declaration.md)): (a) keep as is; importers depend on the initializer and, transitively, on private bodies whose return type is inferred. (b) Require a type annotation on exported consts (like `isolatedDeclarations`'s TS9007 family). (c) Allow inference only when the type is evident without bodies: literals, struct/collection literals, and calls to functions with declared return types.
- **Options for exported arrow-function consts** (`export const f = (x: int) => ...`; [06-functions.md](../../reference/06-functions.md), [12-errors.md](../../reference/12-errors.md)): (a) keep inference of return type and `throws`; (b) require the return type, and `throws` if any, when exported.
- **Evidence**: the spec's own justification for declared returns is to "check each module without reading the bodies of the modules it imports" (17-imports.md). These two cases are the only remaining ways a body can leak into an importer's types. TypeScript added `isolatedDeclarations` for this exact reason.
- **Recommendation**: ask the owner. My preference is (c) for consts and (b) for arrow consts, each with an error that states the exact fix, e.g. `export const LIMITS: map<string, int> = …`.
- **Cost**: tiny in the compiler. It changes spec text.
- **Settle by**: M1 (it decides the shape of `ModuleInterface`), or by M6 at the latest.
- **Spec change**: **yes**. Question for the owner, not to be decided in code.

#### D8. Recursion and cycles in inferred types (owner questions)
- **Options**: for private functions with inferred return types that recurse, (a) infer through recursion with a fixed point (rejected: that is the global-inference pattern Sorbet avoids), or (b) give a compile error that names the function and states the fix ("add `: int` to `walk`"). For inferred module consts that depend on each other, and type aliases that form a cycle across circular imports: (a) compile error with the exact fix, or (b) something else the owner prefers.
- **Evidence**: the spec covers runtime self-dependent consts (`AssertionError`, [17-imports.md](../../reference/17-imports.md)), but not type-level cycles. Sorbet's single-shot inference shows the speed benefit of never iterating.
- **Recommendation**: (b) for functions and (a) for consts/aliases: a compile error with the fix.
- **Cost**: low.
- **Settle by**: M1 (functions); M6 (cross-module consts and aliases).
- **Spec change**: **yes** (clarification). Question for the owner.

#### D9. Latency budgets (replace the roadmap's table rows)
- **Options**: (a) keep the roadmap's numbers (< 300 ms cold; < 10 ms after one edit); (b) split by mode and edit kind, as in §5.
- **Evidence**: < 300 ms cold matches TS 7-class throughput, and dat should be well under that. "< 10 ms after editing one file" fails for interface edits with many importers in any mode, and is tight for CLI + cache on macOS (~4–6 ms best case, ~9–15 ms if files are read to detect changes).
- **Recommendation**: (b). Cold ≤ 100 ms target / ≤ 300 ms ceiling; warm no-change ≤ 5 ms; body edit ≤ 10 ms CLI+cache and ≤ 5 ms daemon/LSP (p95); interface edit ≤ 5 ms + importers' re-check, never above cold; interpreter start ≤ 5 ms for cached or single-module programs. Name the hardware and measure p50/p95 over ≥ 30 runs.
- **Cost**: a synthetic corpus generator and harness in M0.
- **Settle by**: M0 (harness and corpus); enforce cold from M1 on small corpora, and incremental from M6/M7.
- **Spec change**: none (AGENTS.md says "milliseconds" without numbers).

#### D10. Test re-runs and std library
- **Options for tests**: (a) `dat test` always runs every test, as [20-testing.md](../../reference/20-testing.md) says ("finds and runs every test"); (b) by default skip tests whose dependency hashes are unchanged and replay the cached result, with `--all` to force; (c) keep (a) as the default and add `--changed`.
- **Options for std**: (a) parse and check std source at startup; (b) embed a precompiled std interface in the binary.
- **Evidence**: tests share no state (20-testing.md), so skipping is safe if hashes are complete. Embedded summaries avoid startup work (Go export data; salsa durability).
- **Recommendation**: tests (c), unless the owner prefers (b); std (b).
- **Cost**: tests low once D1 hashes exist; std low (a build step).
- **Settle by**: std in M1 (built-ins exist from M1); tests in M6.
- **Spec change**: tests (b) would change the meaning of "dat test finds and runs every test", so it is a question for the owner. Tests (c) and std (b) need no spec change.

### Gaps
Questions for the owner (each would change or clarify the spec; none should be settled in code):
- **Q-A**: May an exported `const` omit its type? If so, must its type be evident without reading function bodies? (D7)
- **Q-B**: Must an exported arrow-function `const` declare its return type and `throws`? (D7)
- **Q-C**: What happens when a private function with an inferred return type is recursive or mutually recursive? Error with fix, or inference? (D8)
- **Q-D**: In what order are union members printed in messages and docs, given that "member order doesn't matter"? The spec example prints `string | int | null`. (D5)
- **Q-E**: Should `dat test` skip unchanged tests by default, or only with `--changed`? (D10)
- **Q-F**: Are type-level cycles (inferred consts that depend on each other; type aliases that form a cycle across circular imports) compile errors? (D8)

Other open items: no Linux timings; no IPC timing; no primary source for Hack saved states; no measurement of salsa's isolated memory overhead; the agent edit mix (body-only vs interface-changing) is unknown.
