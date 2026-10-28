# Interpreter design, backend equivalence and the fast test loop for dat-lang

Scope: the development-mode interpreter (`dat run`, `dat test`), how it stays identical to the generated Rust, and how `dat test` finishes in milliseconds. Written 28 Oct 2026. It includes owner decisions received during the research: the millisecond budgets apply only to the interpreter path (`dat check`, `dat run`, `dat test`), and compiled runs through rustc get their own budget and run in CI and on request. Stack overflow should become a catchable `StackOverflowError` if that is cheap, and is otherwise allowed to crash. Out of memory is accepted as uncatchable. Items marked † come from background knowledge with a canonical URL that I did not re-fetch in this session; the writer should treat them as lower confidence than fetched items.

Spec context used: AGENTS.md; reference/01, 02, 03, 06, 12, 19, 20 read in full; 04, 07, 08, 10, 11, 16, 17, 18 grepped for ordering, formatting, hashing and evaluation-order rules. Also used: the draft roadmap (pipeline: front end → typed IR → "interpreter walks the typed IR" plus Rust codegen, shared `dat-rt`; budgets: interpreter start to first statement < 5 ms, 1,000 small tests interpreted < 250 ms).

## 1. Execution strategies compared with measured numbers

### Takeaway
On call-heavy code, a naive tree-walker is about 4–6x slower than a register or bytecode VM. The gap shrinks to 1.1–1.4x once the tree-walker resolves names to slots and specialises nodes, or compiles to closures. Register VMs win on compute loops but cost months of work and usually unsafe code. Rust has no guaranteed tail calls (`become` is still unstable as of October 2026), so the fastest dispatch tricks rely on LLVM optimisations that are not guaranteed. For a statically typed language, the type checker already provides most of what CPython 3.11 and Luau gain from runtime specialisation.

### Cited Findings

**Strategy ladder in one controlled Rust experiment (Neil Mitchell, Feb 2021):**
- Mitchell implemented the same toy language as an AST walker, closures, a stack bytecode VM and a register VM, with variables resolved to indexes, unchecked array access and no allocation in every variant. Measured against native Rust with a non-inlined `add`, the penalties were **AST walk 6.4x ("lowest effort, cleanest code"), closure generation 4.8x ("more effort, but not much more"), and register machine with tail-call threading 1.4x ("requires register allocator, uses unsafe operations… much more effort")**. A first naive comparison showed 570x because rustc constant-folded the native loop. — [Mitchell, "Cheaply writing a fast interpreter" slides](https://ndmitchell.com/downloads/slides-cheaply_writing_a_fast_interpreter-23_feb_2021.pdf); companion post: [Writing a fast interpreter (2020)](http://neilmitchell.blogspot.com/2020/04/writing-fast-interpreter.html)
- The slides say tail calls in Rust "are compiled to JUMP on x86_64 with -O" but are "not guaranteed". A compact (non-word-aligned) tape was a few percent slower. Constant folding and bigger fused fragments (e.g. `add3`) are listed as further wins. — [same slides](https://ndmitchell.com/downloads/slides-cheaply_writing_a_fast_interpreter-23_feb_2021.pdf)

**Closure compilation in production Rust:**
- Cloudflare's wirefilter (Mar 2019) compiles each AST node into a `Box<dyn Fn(&ExecutionContext) -> bool>`, with field indexes resolved at parse time. It measured about **10–15% faster runtime than AST interpretation**. Closure creation cost was negligible. They rejected a JIT because of executable-memory risk and compile cost. A separate change, field access through fixed arrays, cut matching from **2,548 to 1,227 ns/iter**. Representation choices mattered more than dispatch style. — [Cloudflare blog](https://blog.cloudflare.com/building-fast-interpreters-in-rust/)
- Closures used as handlers inside a central bytecode loop are slower than a `match`: **461.57 ms for switch dispatch vs 836.79 ms for "closure-in-central-loop"**. A closure-tail variant needs tail calls that Rust does not guarantee. — [neopallium/interpreter-dispatch-research](https://github.com/neopallium/interpreter-dispatch-research)

**Tree-walking vs bytecode, Rust and others:**
- Danny van Kooten (2022, Rust): recursive fib(35) took **3.79 s on the tree-walker vs 0.758 s on a bytecode VM (5x)**. The tree-walker had already come down from ~39 s to ~3.7 s by replacing a `HashMap` environment with a `Vec` and not cloning nodes. — [dannyvankooten.com](https://www.dannyvankooten.com/blog/2022/rewriting-interpreter-rust/)
- Lox implementations on one machine (i7-8565U; no date given), fib benchmark: **clox 0.91 s, jlox (Java tree-walker) 4.28 s, loxido safe Rust bytecode 5.03 s, loxido unsafe Rust bytecode 1.20 s**, Python 3.9 2.11 s. Safe Rust bytecode was *slower than the Java tree-walker*. Raw pointers for the pc, stack and frames gave the 4x. — [ceronman/loxido README](https://github.com/ceronman/loxido)
- Rhai (Rust AST walker with precomputed hashes and variable offsets): **fib 2.25 s vs Python 3 0.6 s vs V8 0.07 s**. Rhai describes itself as "roughly 2–3x slower than Python 3" and says its work-in-progress bytecode VM "Rhai Grain" is "roughly 2x faster", about on par with Python. — [Rhai book, benchmarks](https://rhai.rs/book/about/benchmarks.html)
- Larose, Kaleba, Burchell, Marr (OOPSLA 2023) built AST and bytecode interpreters for the same language (SOM) on RPython and GraalVM/Truffle. Their AST interpreters were "on par with, or even slightly faster than their bytecode counterparts for interpreter-only performance". Bytecode is more compact in memory, but the bytecode interpreters allocated more on small programs. — [paper page](https://stefan-marr.de/papers/oopsla-larose-et-al-ast-vs-bytecode-interpreters-in-the-age-of-meta-compilation/); [Marr blog](https://stefan-marr.de/2023/10/ast-vs-bytecode-interpreters/)
- Dubroy (Mar 2024, TypeScript on V8/JSC): the tree-walking PEG interpreter was **2.04–3.69x faster** than the switch-based bytecode version. He did not explain why, and the results are not specific to Rust. — [dubroy.com](https://dubroy.com/blog/two-little-interpreters/)
- A 2025 hobby PR reports that in Rust "walking a tree is a match on an enum — already close to free", so moving to bytecode bought almost nothing. Its first measurement was distorted by allocator overhead. This is a self-reported, low-authority source. — [rithymeth/MRT PR #16](https://github.com/rithymeth/MRT/pull/16)

**Register VMs and the wasm interpreters (closest analogue: statically typed input):**
- Wasmi v0.32 (2024) moved from a stack-based to a register-based IR. It reports execution "up to 5x" faster and startup "several orders of magnitude" faster through lazy compilation. Coremark scores (higher is better): **Wasmi 0.31 → 0.32: 657 → 1457 (Epyc 7763), 884 → 1577 (M2 Pro); Wasm3 1309 / 2931; Stitch 1390 / 3056**. Recursive fib "has not significantly improved", because the new IR favours compute over calls. It still uses "loop-switch dispatch". Tail-call dispatch was "discussed and dismissed" pending explicit tail calls in Rust. It took "many months of research, development and QA", and other wasm proposals were paused during that time. Validation is 20–40% of startup. — [Wasmi v0.32 post](https://wasmi-labs.github.io/blog/posts/wasmi-v0.32/)
- Stitch (Rust, threaded code through LLVM sibling-call optimisation): Coremark **2950 (Mac) vs Wasm3 2911 vs Wasmi 788 (older Wasmi) vs Wasmtime JIT 12807**. The JIT is 4–8x faster than the best interpreter. fib_rec(32) takes **52.14 ms on an M2 Pro (≈7.4 ns per call, from 7.05 M calls)**. About 15k LOC. Caveats: sibling-call optimisation "is not guaranteed", 64-bit only, much unsafe and unaudited code, cannot run under Miri, experimental. — [makepad/stitch](https://github.com/makepad/stitch)
- Lua 5.0 (2005) was, by its authors' account, the first widely used register-based VM. They argue stack instructions are too low-level and register instructions are more powerful, at the cost of a more complex compiler. — [The Implementation of Lua 5.0 (JUCS 2005)](https://www.lua.org/doc/jucs05.pdf); [Ierusalimschy workshop slides](https://www.lua.org/wshop05/Ierusalimschy1.pdf). The percentage of instructions saved could not be retrieved (see Gaps).
- Luau: a "highly tuned portable bytecode interpreter" in C, "partially inspired by" LuaJIT's interpreter. Its core loop plus builtins compile to **about 16 KB on x64**. "Fastcall" builtins run from the core loop "without setting up a stack frame". It uses specialised iteration bytecodes and inline caches. It can match the LuaJIT interpreter on some workloads and is "noticeably faster" than Lua 5.x. It compiles **950K lines/second on one Ryzen 5900X core**. — [luau.org/performance](https://luau.org/performance)
- CPython 3.11's specialising adaptive interpreter (PEP 659) rewrites generic bytecodes into type-specialised ones at runtime. 3.11 averaged about 1.25x faster than 3.10 on pyperformance. — † [What's New in Python 3.11](https://docs.python.org/3/whatsnew/3.11.html); † [PEP 659](https://peps.python.org/pep-0659/)

**Dispatch technique evidence:**
- CPython 3.14's tail-call interpreter was first reported as 10–15% faster. Most of that came from "inadvertently working around a regression in LLVM 19". Against GCC, clang-18 or a tuned clang-19, the gain is **~1–5%**. Older literature claimed 20–100% from computed goto. Recent measurements show **2–4% on modern CPUs**. — [Nelson Elhage, Mar 2025](https://blog.nelhage.com/post/cpython-tail-call/)
- Rust `become` (explicit tail calls) and `loop_match` (`#[loop_match]`/`#[const_continue]`, for computed-goto-like codegen) are both unstable and "still require substantial work" under an accepted 2026 Rust project goal. Only the LLVM backend has `become` codegen (added July 2025). Other backends "will ICE". LLVM 22 improves tail calls on x86_64. Wasm needs the `tail-call` feature, and powerpc has limits. The goal's authors aim to "hopefully stabilize in 2027". — [Rust project goal 2026: tail calls & loop_match](https://goals.rust-lang.org/2026/tail-call-loop-match.html); [Trifecta Tech, "Rust should have stable tail calls"](https://trifectatech.org/blog/tail-calls-project-goal/); [rust-lang/rust PR #144232](https://github.com/rust-lang/rust/pull/144232)

**JIT as a dev backend (Cranelift):**
- Cranelift compiles "about an order of magnitude faster" than LLVM-based systems, and its code was about 2% slower than V8 TurboFan and 14% slower than WAVM/LLVM in the cited paper. — [Cranelift README](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/README.md); [cranelift.dev](https://cranelift.dev/). HPC data point (2023): Cranelift compiled HPCG in 150 ms vs 2,811 ms for LLVM, and the code ran ~14% slower. — [arXiv 2301.03982](https://arxiv.org/pdf/2301.03982)
- As a rustc backend (cg_clif), the whole-pipeline gain is small. It was 5% faster than LLVM (12% with line tables) on Zed incremental builds (Feb 2026, 16 cores), and a 2024 measurement showed 20% faster on a full debug build (29.6 s vs 37.5 s). The rustc front end dominates. — [Rust goal 2026: improve cg_clif performance](https://goals.rust-lang.org/2026/improve-cg_clif-performance.html); [LWN 2024](https://lwn.net/Articles/964735)
- Roc's old Rust compiler had a "dev backend" that emitted machine code directly to "generate Roc binaries fast". The Zig rewrite (feature parity reported July 2026, aggregator source) includes an **interpreter** used by the REPL, tests and snapshot tooling (Oct 2025 docs). This suggests Roc moved its fast path from a custom codegen toward interpretation, but I could not confirm what `roc dev` currently does. — [Roc Zig-compiler interpreter docs (commit mirror)](https://git.joshthomas.dev/language-servers/roc/src/commit/0228bf88228d8a8e66bfc9e97693e5cf6b1879af/src/eval); [zeli.app summary, 2026-07-16](https://zeli.app/de/story/48933149)

### Inferences
- Per-call costs on fib (calls(n) = 2·F(n+1) − 1, so fib(30) = 2.69 M calls and fib(35) = 29.9 M calls): optimised Rust tree-walker ≈ 127 ns/call (van Kooten), Rust bytecode ≈ 25 ns/call, Stitch threaded ≈ 7.4 ns/call. A dat tree-walker with slot resolution and typed nodes should land between the first two, roughly 30–80 ns/call (inference, not measured). That puts fib(30) at about 0.1–0.2 s interpreted, against roughly 5–10 ms in native Rust.
- Static types turn CPython-style and Luau-style runtime specialisation into a lowering-time choice. The interpreter IR can emit `AddInt` (checked i64), `AddFloat`, `ConcatStr`, `LtIntConst`, and so on, with no guards and no inline caches. Most of the "bytecode beats AST" literature measures dynamically typed languages, where much of a tree-walker's time goes to type dispatch that dat removes at check time.
- The strategy matters less for dat's stated budgets than it seems. "Start to first statement < 5 ms" and "1,000 small tests < 250 ms" are dominated by startup, check and the runner (sections 2 and 5), not by per-node dispatch. Strategy choice matters only for CPU-heavy tests and `dat run` of real workloads.
- Unsafe threaded code (Stitch, unsafe loxido) gives the largest wins but conflicts with the "every runtime error can be caught, never crash" rule, because a VM bug becomes memory corruption instead of a catchable panic. This is a strong reason to stay safe in v1.
- A Cranelift JIT for dat has a structural problem beyond effort. `dat-rt` collections are Rust generics (`List<T>`) that rustc monomorphises. JIT code cannot instantiate them for new `T` at runtime, so a JIT needs a third, type-erased runtime (like Swift's value witness tables). That would break the one-runtime equivalence argument.

### Gaps
- No fetched source gives jlox vs clox numbers from *Crafting Interpreters* itself. The loxido table is the substitute.
- The Lua 5.0 instruction-count reduction (register vs stack) was not found in fetched text.
- No fetched 2024–2026 benchmark compares closure compilation with a register VM on the same Rust codebase with fib numbers.
- No source on boa's AST-to-bytecode switch numbers or LuaJIT interpreter numbers was fetched. Gleam and MoonBit tooling were not researched (out of time budget).

## 2. Startup time: what dominates, and is < 5 ms realistic including the type check?

### Takeaway
Process start for a Rust binary costs about 0.5 ms on 2011-era x86 Linux (2018 measurement), so the 5 ms budget is spent on reading, parsing, checking and lowering. At Luau-class front-end throughput (about 1 M lines/s/core), a few-thousand-line program fits in 5 ms single-threaded. A 50k-line project does not fit cold and needs the roadmap's signature-hash cache or lazy per-function lowering. Runtime init is negligible because dat has no GC and no module-level side effects.

### Cited Findings
- Hello-world startup, run 1,000 times pinned to one core (measured 2018-02-10): **C 0.26 ms, Rust 0.51 ms, Go 0.41 ms, Lua 0.63 ms, Python 3 25.84 ms (9.31 ms with -S)** on an Intel i5-2400S. On a Raspberry Pi 3: Rust 4.42 ms. — [bdrung/startup-time](https://github.com/bdrung/startup-time)
- Hyperfine's `-N` (no shell) is recommended for commands under ~5 ms, because shell startup otherwise dominates. — [hyperfine docs](https://docs.rs/hyperfine)
- Luau compiles 950K lines/s on one core. — [luau.org/performance](https://luau.org/performance)
- Wasmi cut startup by "several orders of magnitude" with lazy translation (translate a function body on first call). Validation is 20–40% of startup, and it offers lazy-translation-with-eager-validation. — [Wasmi v0.32](https://wasmi-labs.github.io/blog/posts/wasmi-v0.32/)
- The Roc Zig rewrite reports a "very fast feedback loop" by skipping executable generation, but secondary reports of its rebuild times conflict (1.3 s vs 35 ms). — [Roc rewrite coverage](https://byteiota.com/roc-rewrites-300k-lines-from-rust-to-zig-35ms-rebuilds-fewer-bugs/); [weeklyrust](https://weeklyrust.substack.com/p/why-roc-is-moving-away-from-rust)
- dat spec facts that bound startup work: modules hold only declarations, and "importing never runs code"; module `const`s are "computed once, the first time it's used" (reference/17-imports.md). Exported functions must declare return types, so "the compiler [can] check each module independently" (reference/06-functions.md).

### Inferences
- Budget split for `dat run small.dat` (inference): exec and dynamic loading ≈ 0.5–1.5 ms (macOS dyld is likely at the high end; not measured here), read and parse ≈ 0.1 ms per 1k lines, type check ≈ 0.2–1 ms per 1k lines (assumed 2–5x parse cost; no source), lowering to the execution IR ≈ parse cost. That leaves ample room for programs under ~2–3k lines. The `< 5 ms` budget is realistic for single-file and small programs if the binary has no heavy static initialisers (no eager regex compilation, no locale tables, no tokio runtime on the `run` path).
- For larger projects, three techniques keep startup flat: (1) check only modules reachable from the entry or test file; (2) cache per-module check results keyed by source hash plus the signature hashes of imports (roadmap M6); (3) lower function bodies to the execution IR lazily on first call, as wasmi does. With these, start to first statement depends on the code that runs, not on project size.
- The budget should be measured with `hyperfine -N` on both Linux and macOS in CI. macOS process creation can be slowed by XProtect (see section 5's nextest source).

### Gaps
- No 2024–2026 measurement of Rust process startup on macOS/Apple Silicon was found. The 0.51 ms figure is 2018 Linux.
- No published type-checker throughput (lines/s) for a Rust-written checker of a TypeScript-like language was found. The oxc CodSpeed numbers lacked file sizes, and no tsgo numbers were found.

## 3. Value representation and sharing the runtime library with compiled mode

### Takeaway
Static types allow untagged 64-bit slots (the wasm-interpreter approach). In safe Rust with Rc-based copy-on-write values, though, untagged slots mean manual refcounting and unsafe drop logic. A tagged `enum Value` with type-specialised IR nodes gets most of the benefit safely. The runtime library is best shared as generic Rust code (`List<T>`, `Map<K, V>`, formatting, primitive ops) that the interpreter instantiates with `T = Value` and the generated code instantiates with concrete types. Behaviour is then defined once, and neither mode pays for dynamic dispatch it doesn't need.

### Cited Findings
- Wasm interpreters run statically typed code. wasmi exposes both a typed `RuntimeValue`/`Val` enum and an `UntypedValue` type for its engine internals. Wasmtime has a raw `ValRaw` union and notes that its public value type "is not actually the internal representation in JIT code". — [wasmi_core docs](https://docs.rs/wasmi_core/^0.13); [wasmtime ValRaw](https://docs.wasmtime.dev/api/wiggle/wasmtime_crate/union.ValRaw.html); [wasmtime Val](https://docs.wasmtime.dev/api/wasmtime/enum.Val.html)
- loxido's 4x speedup from safe to unsafe Rust came largely from representation: raw pointers for stack, frames and pc, objects as an enum instead of trait objects, and a custom hash map. — [ceronman/loxido](https://github.com/ceronman/loxido)
- Cloudflare's 2x win came from representation (fixed arrays with precomputed indexes instead of name lookups), not from the dispatch technique. — [Cloudflare blog](https://blog.cloudflare.com/building-fast-interpreters-in-rust/)
- van Kooten's 10x tree-walker improvement came from replacing a `HashMap` environment with a `Vec` and not cloning AST nodes. — [dannyvankooten.com](https://www.dannyvankooten.com/blog/2022/rewriting-interpreter-rust/)
- Rhai resolves most variables to "pre-calculated offsets into a Scope" and dispatches functions through "pre-calculated hashes". — [Rhai benchmarks](https://rhai.rs/book/about/benchmarks.html)
- Larose et al.: bytecode interpreters allocated more on small programs because "boxing avoidance is not as applicable" and the reified stack needs memory. — [Marr et al. 2023](https://stefan-marr.de/papers/oopsla-larose-et-al-ast-vs-bytecode-interpreters-in-the-age-of-meta-compilation/)
- Spec facts that shape representation: values are copied on assignment with copy-on-write for lists, sets, maps and strings (reference/02); `mut` parameters give access to the caller's value (reference/06); closures capture only immutable values by copy (reference/06); generics are monomorphised "so generics cost nothing at runtime" (reference/19). Roadmap mapping: `string` → `dat_rt::Str` over `Rc<str>`, `list<T>` → `Rc<Vec<T>>` with `make_mut`.

### Inferences
- **Option A, one tagged `enum Value`** (`Int(i64) | Float(f64) | Bool | Null | Str(Str) | List(List<Value>) | Struct(Rc<StructVal>) | Union(tag, Box<Value>) | Fn(Rc<Closure>) | …`): safe, simple, and generic functions run without monomorphisation, which helps startup. Cost: a tag check per access, but with typed IR nodes that branch is always predicted. With thin pointers (`Rc<StrInner>` instead of the fat `Rc<str>`), `Value` can be 16 bytes. With fat `Rc<str>`, it is 24 bytes (layout inference from Rust's DST pointer rules; measure with `size_of`).
- **Option B, untagged u64 slots plus per-slot static types** (wasm-style): smaller and faster for numeric code. But heap values (COW lists, strings) need refcount increments and decrements driven by static type info, which in Rust means `unsafe` raw-pointer handling and hand-written drop glue, a crash risk under the "never crash" rule. Defer it to a later optimisation, and then only for frames of scalar locals.
- **Option C, hybrid:** tagged `Value` everywhere, plus unboxed fast paths in the IR for scalar locals (`IntSlot`, `FloatSlot`). This is a later upgrade with no format change.
- **Sharing `dat-rt` without slowing either mode:**
  - Generic collections (`List<T>`, `Set<T>`, `Map<K, V>`, insertion-ordered) are written once. Behaviour traits (`DatEq`, `DatHash`, `DatCompare`, `DatToString`) are implemented by `Value` (dynamic, matching on tag) and derived for generated structs (static). Compiled code gets full monomorphisation, and the interpreter gets `List<Value>`. Same algorithm, same iteration order, same error messages.
  - Primitive operations are defined once as `#[inline]` functions returning `Result<_, RtError>`: `int_add`, `int_rem`, `int_div_floor`, `float_to_string`, `str_index`, `parse_int`, and so on. The interpreter calls them from its node handlers, and the code generator emits calls to them. LLVM inlines them, so compiled speed is unaffected, and the two modes cannot disagree on overflow or formatting.
  - Callback-taking methods (`map`, `filter`, `sort(compare)`, `groupBy`) are generic over `FnMut(&T) -> Result<U, Throw>`. The interpreter passes a Rust closure that re-enters the evaluator, and compiled code passes the generated closure. Errors thrown inside callbacks propagate the same way.
  - Avoid trait objects (`dyn DatValue`) as the shared interface. They would put a vtable call on every compiled-mode collection operation, which conflicts with the "close to hand-written Rust" goal.
- The interpreter should evaluate to borrowed references where possible (`&Value` into frame slots) instead of cloning, since compiled code passes `&T` for parameters (roadmap). Otherwise every read costs an Rc increment that compiled mode doesn't pay. Performance can differ here, but semantics cannot, because COW hides the difference.

### Gaps
- I could not fetch wasmi's `UntypedVal` definition, so its exact width (64 vs 128 bits since SIMD) is unconfirmed.
- No measured cost of `enum Value` size (16 vs 24 vs 32 bytes) on interpreter speed was found for a Rust interpreter.

## 4. Keeping two backends semantically identical

### Takeaway
Projects that keep backends in sync use three tools together: one shared test corpus run on every backend (Zig, Roc, the Wasm spec suite), differential fuzzing with generated programs (Csmith, wasmtime), and a single source of truth for semantics (Truffle derives compiled code from the interpreter). The known divergence sources are float formatting, NaN bits, hash iteration order, stack exhaustion, overflow edge cases, evaluation order and runtime-library version drift. Several of these apply directly to dat, and three are open questions in the current spec.

### Cited Findings
- **Zig:** one behaviour-test suite runs on every backend. Tests that a backend can't run yet opt out with `if (builtin.zig_backend == .stage2_x86_64) return error.SkipZigTest;`, and CI skip logic has been tightened so fewer tests are wrongly skipped. — [Zig commit adding `zig_backend`](https://git.medv.io/zig/commit/1c24ef0d0b09a12a1fe98056f2fc04de78a82df3.html); [Andrew Kelley 2023 commit](https://shale-1.onrender.com/zig/commit/3c93c1664a3c3546076b39c2d40405f5db537dae)
- **Wasmtime fuzzing:** an oracle can "compare the result of executing a Wasm file with and without optimizations enabled, and make sure that the two executions are observably identical". Known legitimate divergences: NaN bit patterns (mitigation: "canonicalize NaN values after all floating point operations"), relaxed SIMD, call-stack exhaustion ("one engine overflowing while another doesn't is not necessarily a bug"), and memory growth limits. WASIp1 is "not suitable for differential fuzzing" because its spec is too loose. — [Wasmtime fuzzing docs](https://docs.wasmtime.dev/contributing-fuzzing.html)
- **Csmith** (PLDI 2011): random program generation plus differential testing across C compilers found "more than 325 previously unknown bugs". — † [Yang, Chen, Eide, Regehr, "Finding and Understanding Bugs in C Compilers"](https://users.cs.utah.edu/~regehr/papers/pldi11-preprint.pdf)
- **Wasm reference interpreter:** the spec ships an OCaml reference interpreter and a shared `.wast` test suite that engines run. — † [WebAssembly/spec interpreter](https://github.com/WebAssembly/spec/tree/main/interpreter); † [WebAssembly/testsuite](https://github.com/WebAssembly/testsuite)
- **GraalVM Truffle:** the language is implemented once as an AST interpreter, and compiled code is derived from it by partial evaluation, so the semantics are equal by construction. — † [Truffle framework docs](https://www.graalvm.org/latest/graalvm-as-a-platform/language-implementation-framework/)
- **HotSpot:** `-Xint` runs interpreter-only, which makes interpreter-vs-JIT differential runs possible. — † [java launcher docs](https://docs.oracle.com/en/java/javase/21/docs/specs/man/java.html)
- **Miri** interprets Rust MIR to detect undefined behaviour and runs cargo test suites. It is a second backend used to check the first. Stitch notes it "cannot run under Miri" because Miri doesn't apply sibling-call optimisation. — † [rust-lang/miri](https://github.com/rust-lang/miri); [makepad/stitch](https://github.com/makepad/stitch)
- **Dart:** a dart2wasm JS-compatibility mode's integer parsing "is not compatible with the VM or dart2js semantics" and was fixed by reusing the shared parser. Web compilers keep per-backend test expectations. This is an example of divergence entering through runtime-library functions rather than codegen. — [Dart SDK commit](https://dart.googlesource.com/sdk/+/c91fd18cc4341d5de99fd8484487d1a1f91fb235); [Dart SDK commit](https://dart.googlesource.com/sdk/+/4bfb196287ed2c746382c87dee38c3f17b02ef41)
- **Evaluation order in Rust:** for compound assignment where both operand types are primitive, "the right hand side is evaluated first, the left hand side is evaluated next". The Reference calls this "unusual. Elsewhere left to right evaluation is the norm." For non-primitive types the order is left-to-right. — [Rust Reference, operator expressions](https://doc.rust-lang.org/reference/expressions/operator-expr.html)
- **Overflow edge cases in Rust:** `/` or `%` with `MIN` and `-1` is an overflow, and "these checks occur even when -C overflow-checks is disabled". Integer division by zero panics. — [Rust Reference](https://doc.rust-lang.org/reference/expressions/operator-expr.html)
- **Sort behaviour depends on the std version:** Rust 1.81 replaced the sorts with driftsort and ipnsort, which "may panic" when the comparison is not a total order, where earlier versions returned unsorted data. — [Rust 1.81.0 announcement](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/); [releases.rs 1.81.0](https://releases.rs/docs/1.81.0/)
- **Float printing:** Rust's standard `to_string` "never uses scientific notation". The `ryu-js` crate (boa-dev) implements ECMAScript Number-to-string. — [ryu-js docs](https://docs.rs/ryu-js); [ryu-js repo](https://github.com/Tropid/ryu-js). dat specifies shortest round-trip output with a decimal point or exponent, and exponent form at ≥ 1e21 or ≤ 1e-7 (reference/04), which is JavaScript's thresholds with `2.0` instead of JS's `2`.
- **Hash order:** std `HashMap` is randomly seeded per process. — † [std HashMap docs](https://doc.rust-lang.org/std/collections/struct.HashMap.html). dat requires insertion order for sets and maps (reference/01, 11, 16), which removes this source if both modes use the same `dat-rt` ordered map.
- **Stack overflow in Rust is not catchable:** `catch_unwind` catches only unwinding panics, and stack overflow aborts the process. — † [std::panic::catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html). `stacker::maybe_grow` checks the remaining stack against a red zone and switches to a heap-allocated segment, and is a no-op on unsupported platforms. — [stacker docs](https://docs.rs/stacker). Thread stack size is configurable. — † [std::thread::Builder::stack_size](https://doc.rust-lang.org/std/thread/struct.Builder.html#method.stack_size)

### Inferences (dat-specific divergence register)
1. **Evaluation order (spec gap).** reference/03 defines grouping ("left to right") and short-circuiting (reference/07) but not operand or argument evaluation order. If codegen emitted `xs[i()] += f()` as Rust `+=` on `i64`, Rust would run `f()` before `i()`. Fix: the shared IR makes order explicit (temporaries in source order) before either backend sees it. That needs a spec sentence ("operands and arguments are evaluated left to right").
2. **`MIN % -1` and `MIN / -1` (spec gap).** dat says overflow throws `RangeError` and `%` keeps the left sign. Mathematically `MIN % -1 == 0`, but Rust's `checked_rem` returns `None`. `Math.floor(MIN / -1)` on ints is a real overflow. One `dat-rt` function per op makes the two modes agree, but the spec must choose: `0` or `RangeError` for `MIN % -1`.
3. **Float to text.** Never use Rust `Display` in either mode. Use one `dat-rt` formatter (ryu-js-style plus the `.0` rule). Float to int must not use `as`, which saturates silently (†); use checked conversions that throw `RangeError` per reference/12.
4. **Std-version drift between `dat` and the user's rustc.** The interpreter uses the std compiled into the `dat` binary, while `--compiled` uses whatever rustc builds the generated crate. Sort algorithm (1.81 change), Unicode tables for case mapping, and float parsing can differ across Rust versions. Fix: pin the toolchain in the generated crate (`rust-toolchain.toml`) to the version that built `dat`, or implement observable algorithms (sort, case mapping, number parsing) inside `dat-rt` instead of delegating to std.
5. **Comparator and callback side effects.** If a `sort` comparator prints or throws, the sequence of comparator calls is observable. It is identical in both modes only if both run the same `dat-rt` sort on the same std. An inconsistent comparator can panic in Rust ≥ 1.81 and must become a catchable dat error in both modes. The spec should say what happens (unspecified order? which error?).
6. **Stack depth (owner decision: catchable `StackOverflowError` if cheap).** Native stack use per dat call differs between a tree-walker (deep Rust recursion per expression), compiled code (inlined frames) and debug vs release builds. A byte-based guard (stacker) would therefore throw at different depths in each mode, and tests that recurse deeply would pass in one mode and fail in the other. A deterministic dat-frame counter with the same limit in both modes gives identical behaviour. Run the program (and each test worker) on a thread with a large reserved stack so the native stack never overflows before the counter does. The stack is virtual memory and committed lazily (†).
7. **Native recursion inside `dat-rt`.** Drop, clone, `==`, `hash` and `toString` of deeply nested recursive structs (reference/08 "Recursive Structs", e.g. a long linked list via `Option<Box<T>>`) recurse natively in both modes and can abort the process regardless of the dat depth counter. These need iterative implementations or `stacker` in `dat-rt`. Otherwise "never crash" fails on a 1M-element linked list.
8. **Module-const initialisation order.** Lazy "computed once, the first time it's used" consts (reference/17) whose initialiser prints or fails make output depend on which test or code path touches them first. Under parallel tests this is nondeterministic in both modes. See question Q-E below.
9. **Rust panics that aren't dat errors.** A bug in `dat-rt` (an `unwrap`, an index panic) would be caught by `catch_unwind` in compiled mode under the roadmap's panic-based runtime errors, but might crash or be reported differently in the interpreter. Both modes need one rule: any non-dat panic is reported as an internal error, never silently converted into a dat `Error`.

### Gaps
- No fetched source documents how rustc's cg_clif CI checks equivalence with the LLVM backend (e.g. which rustc test suites it runs).
- Roc's `test_gen` README (multi-backend test macros) returned 404, so Roc's mechanism is unverified here.
- No measured cost of a per-call depth counter in compiled Rust code was found. It is expected to be about one increment, one compare and one decrement per call (inference), and should be measured in the M0 spike.

## 5. Fast test loops: parallelism, affected tests, caching, and realistic time per 1,000 tests

### Takeaway
Run tests in-process on a thread pool against one checked program, each with fresh interpreter state. Process-per-test (nextest's model) costs at least 0.5 ms per spawn, about 500 ms per 1,000 tests on Linux and more on macOS and Windows, which is double the 250 ms budget. Because dat tests have no global mutable state and (today) no I/O, a Go-style test-result cache keyed by the test's transitive code hash is exact. Function-level dependency tracking can later select affected tests.

### Cited Findings
- cargo-nextest runs each test in its own process for isolation of global state, fault containment and reliable timeouts and cancellation (SIGTERM, then SIGKILL after 10 s). Process creation is "very fast" on Linux, "quite slow" on Windows, and can be slow on macOS when XProtect interferes. Thread cancellation is hazardous, so in-process runners need cooperative cancellation. — [nextest: why process-per-test](https://nexte.st/docs/design/why-process-per-test/)
- Process spawn cost: Rust hello-world ≈ 0.51 ms (2018, Linux). — [bdrung/startup-time](https://github.com/bdrung/startup-time)
- Go caches test results and invalidates them when the files or environment variables a test reads change. `-count=1` bypasses the cache, `go clean -testcache` clears it, and `GODEBUG=gocachetest=1` explains misses. Designing the cache key for external inputs "needed significant design work". — [Go test_cache_inputs script](https://golang.google.cn/src/cmd/go/testdata/script/test_cache_inputs.txt?m=text); [golang/go#11193](https://golang.org/issue/11193); [golang/go#22593](https://golang.org/issue/22593); [Keboola on Go cache](https://500.keboola.com/in-depth-of-go-build-cache/)
- Vitest: `vitest related` runs "only tests that cover a list of source files" from static imports (not dynamic `import(path)`). `--changed [since]` runs tests affected by git-changed files. `experimental.recordDependencies` records the modules each test file loads. The result cache orders failed and slow files first. — [Vitest CLI](https://vitest.dev/guide/cli.html). Jest offers `--findRelatedTests` and `--onlyChanged` over its module graph. — † [Jest CLI](https://jestjs.io/docs/cli)
- Bazel caches test results per target by default (`--cache_test_results`). — † [Bazel command-line reference](https://bazel.build/reference/command-line-reference)
- salsa provides incremental recomputation with automatic dependency tracking (used by rust-analyzer). — † [salsa-rs/salsa](https://github.com/salsa-rs/salsa)
- Wasmtime offers epoch- and fuel-based interruption for stopping runaway guest code cooperatively. — † [Wasmtime Config docs](https://docs.rs/wasmtime/latest/wasmtime/struct.Config.html)
- JS test runners as a reference point (blog-quality, conflicting): a 50-test, 10-file suite ran cold in ~0.08 s with Bun, ~0.9 s with Vitest and ~1.2 s with Jest. — [pkgpulse comparison](https://www.pkgpulse.com/guides/bun-test-vs-vitest-vs-jest-test-runner-benchmark-2026). Low-authority source.
- Spec facts: tests "share no state, because dat has no global mutable state, so they run in parallel and in any order"; a test body behaves like a function declared `throws Error`; filtering is by file and `--filter` (reference/20). The coordinator relayed that the millisecond goals apply only to the interpreter path, and that compiled runs happen in CI and on request.

### Inferences
- **Realistic time for 1,000 small tests** (inference): a "small" test is 1–5 asserts and a few hundred IR nodes, so about 5–50 µs on a tree-walker. That is 5–50 ms single-threaded, or under 10 ms on 8 cores. Add startup (~1 ms), check (cold: proportional to code size, e.g. ~10–50 ms for 10–50k lines at Luau-like speeds; warm with cache: ~1–5 ms), and reporting. **Total: about 15–60 ms cold, under ~20 ms warm.** The 250 ms budget is comfortable with an in-process runner and impossible with process-per-test.
- **Isolation without processes:** each test gets fresh frames and fresh lazy-const state, or a shared once-per-run cache (see Q-E). Dat errors are values, so a failing test can't corrupt others. Internal panics are caught per test with `catch_unwind` and reported as compiler bugs. Infinite loops are handled with a cooperative interrupt: an atomic flag checked on loop back-edges and calls (one relaxed load), set by a per-test timeout. Stack overflow is handled by the deterministic depth counter (section 4, item 6).
- **Output capture:** run tests in parallel but buffer each test's `print` output and emit it per test in a stable order (by file and line) so results are deterministic and identical between modes. The CI diff in reference/20 compares results, and nondeterministic interleaving would make that diff flaky.
- **Exact caching is unusually easy for dat today.** The language has no I/O, time or randomness yet (README "Not yet specified"), so a test's result is a pure function of (its transitive IR closure, the `dat` version, the flags). Key = hash of the lowered IR of every function, const and type the test reaches, plus the interpreter version. When I/O, time or randomness arrive, tests that reach those builtins must be marked uncacheable or have their inputs hashed (Go's lesson).
- **Affected-test selection** at function granularity is possible because the checker sees the whole typed program, which is finer than Vitest and Jest (file module graph) or Bazel (target). With caching, `dat test` after editing one function reruns only tests whose closure hash changed. This is the natural job of the M7 incremental query engine (salsa-style).
- **Compiled tests** (`--compiled`): one generated harness crate with all tests, run in-process with per-test `catch_unwind` and output capture. Under the owner's decision this has its own budget and runs in CI and on request. The equivalence diff should compare per-test (status, error type, message, captured stdout), not raw interleaved output.

### Gaps
- No primary benchmark of an in-process interpreter running 1,000+ tests (e.g. Deno, Bun, Zig or MoonBit runners) with per-test overhead was found. The numbers above are inferences from per-call costs.
- MoonBit's and Gleam's test tooling was not researched.

## 6. The right first version for dat and the upgrade path

### Takeaway
Ship a safe tree-walking interpreter over a resolved, typed, order-explicit execution IR: slots instead of names, type-specialised nodes, a flat arena, and a source span on every node. Pair it with a tagged `Value`, a generic shared `dat-rt`, a deterministic call-depth guard and an in-process parallel test runner. That is the cheapest design that meets the 5 ms and 250 ms budgets. If CPU-heavy workloads later need it, the same IR can be lowered to closures (~1.1–1.3x faster) or to a register bytecode VM (~3–5x faster on calls in the cited data) without changing the front end, `dat-rt`, or semantics.

### Cited Findings
- Effort and speed ladder: AST walk is the lowest effort at 6.4x; closures are "not much more" effort at 4.8x; a register VM is "much more effort" plus unsafe at 1.4x. — [Mitchell slides](https://ndmitchell.com/downloads/slides-cheaply_writing_a_fast_interpreter-23_feb_2021.pdf)
- A production register-IR rewrite took "many months" (wasmi v0.32). — [Wasmi v0.32](https://wasmi-labs.github.io/blog/posts/wasmi-v0.32/). A threaded-code interpreter is ~15k LOC, mostly unsafe and unaudited. — [Stitch](https://github.com/makepad/stitch)
- Dispatch tricks are worth 1–5% on modern CPUs, not 20–100%. — [Elhage](https://blog.nelhage.com/post/cpython-tail-call/). Rust's `become` is unstable, with stabilisation hoped for in 2027. — [Rust goals 2026](https://goals.rust-lang.org/2026/tail-call-loop-match.html)
- Representation fixes (Vec slots instead of HashMap, no cloning, precomputed indexes) gave 2–10x in Rust interpreters, more than dispatch changes. — [van Kooten](https://www.dannyvankooten.com/blog/2022/rewriting-interpreter-rust/); [Cloudflare](https://blog.cloudflare.com/building-fast-interpreters-in-rust/)
- AST interpreters can match bytecode interpreters (Larose et al. 2023). — [paper](https://stefan-marr.de/papers/oopsla-larose-et-al-ast-vs-bytecode-interpreters-in-the-age-of-meta-compilation/)

### Inferences
- **Upgrade path, in order of cost and benefit:** (1) node specialisation, e.g. `LtLocalConst` or `AddLocalInt` (tree-walker "superinstructions"; cheap). (2) Closure compilation of the same IR (moderate; Mitchell 6.4x → 4.8x, Cloudflare +10–15%). (3) Unboxed scalar-local frames. (4) Register bytecode VM with `match` dispatch (months). (5) Tail-call threaded dispatch after `become` stabilises. (6) A JIT only if the language later needs one, which is unlikely given the Rust codegen path.
- **Trigger for upgrading:** a fixed interpreter benchmark set in CI (fib(30), 10M-iteration int loop, string building, sort of 1M ints, map word count, struct-heavy binary-trees) with targets such as fib(30) < 150 ms. Upgrade only when a target fails, not on speculation.
- **The equivalence machinery is part of v1, not an add-on:** order-explicit IR, primitive ops in `dat-rt`, a pinned toolchain for generated crates, a deterministic depth guard and per-test output capture. Retrofitting any of these later would change observable behaviour.

### Gaps
- Effort estimates for dat specifically (person-weeks) are my inference from the cited ladders, not measured data.

## Decisions for dat-lang

Each decision lists options, evidence (with numbers), a recommendation, cost, the milestone that needs it settled, and whether it needs a spec change. Spec changes are flagged as owner questions and none is assumed.

### D1. Interpreter execution strategy
- **Options:**
  - (a) Walk the parsed syntax tree.
  - (b) Tree-walk a resolved, typed execution IR (slots, type-specialised nodes, flat arena, explicit evaluation order).
  - (c) Closure compilation of that IR.
  - (d) Register bytecode VM with `match` dispatch.
  - (e) Register VM with tail-call or threaded dispatch (Stitch-style).
  - (f) Cranelift JIT.
- **Evidence:**
  - (a) is 4–6x slower than bytecode on calls (van Kooten 3.79 s vs 0.758 s fib(35)), and name lookup makes it ~10x worse (39 s before the Vec fix).
  - (b) gets most of (a)'s fixes (Rhai uses offsets and hashes and is still 2–3x slower than CPython; Larose et al. found AST at parity with bytecode).
  - (c) is 1.1–1.33x faster than AST (Cloudflare +10–15%; Mitchell 6.4x → 4.8x), but closures inside a central loop are slower than `match` (836 vs 462 ms).
  - (d) is ~4–5x faster than AST on calls but took wasmi "many months". Safe Rust bytecode can be slower than the Java tree-walker (loxido 5.03 s vs jlox 4.28 s).
  - (e) is fastest (Stitch ≈ 7.4 ns/call), but unsafe, 64-bit only, and not guaranteed by Rust.
  - (f) is 4–8x faster than the best interpreters at peak, but compile cost is per run and dat-rt generics can't be instantiated by a JIT.
- **Recommendation:** **(b)** for M1. Design the IR so (c) or (d) can be added later as a pure lowering.
- **Cost:** (b) is roughly 3–6 weeks for M1 scope on top of the checker (inference); (c) about +2–3 weeks later; (d) 2–4 months; (e) and (f) are not justified.
- **Milestone:** settle at M0 (IR shape), build in M1.
- **Spec change:** none.

### D2. Dispatch technique (only relevant if D1 moves to bytecode)
- **Options:** (a) `match` loop; (b) tail-call threading relying on LLVM sibling calls; (c) `become` / `loop_match` when stable.
- **Evidence:** dispatch tricks give 1–5% on modern CPUs (Elhage); Stitch's approach depends on non-guaranteed optimisation and unsafe code; `become` is unstable, LLVM-only, with stabilisation hoped for in 2027.
- **Recommendation:** (a). Revisit (c) after stabilisation.
- **Cost:** none now.
- **Milestone:** post-M8.
- **Spec change:** none.

### D3. Interpreter value representation
- **Options:** (a) one tagged `enum Value` (≤ 16–24 bytes) with typed IR nodes; (b) untagged 64-bit slots with static types (wasm-style); (c) hybrid: tagged, plus unboxed scalar-local fast paths.
- **Evidence:** representation changes gave 2x (Cloudflare) to 10x (van Kooten) in Rust interpreters; loxido's 4x came from unsafe raw-pointer representation; untagged slots holding Rc pointers need unsafe drop glue, which conflicts with "never crash".
- **Recommendation:** (a) now, with thin-pointer `Rc`s to keep `Value` at 16 bytes. (c) as an optimisation behind benchmarks.
- **Cost:** low.
- **Milestone:** M1 (scalars), M2 (collections).
- **Spec change:** none.

### D4. How `dat-rt` is shared by both modes
- **Options:** (a) generic Rust collections and behaviour traits instantiated with `Value` (interpreter) and concrete types (codegen), plus primitive ops as `#[inline]` functions returning `Result`; (b) trait objects (`dyn DatValue`) used by both; (c) separate implementations per mode.
- **Evidence:** (b) puts vtable calls in compiled hot paths, against the ≤ 1.2x target; (c) is how divergence enters (Dart's dart2wasm int parsing was "not compatible with the VM").
- **Recommendation:** (a). Callback-taking methods are generic over fallible `FnMut`.
- **Cost:** low. It is a design rule plus traits.
- **Milestone:** M0 (decide), M1 (primitive ops), M2 (collections).
- **Spec change:** none.

### D5. Equivalence machinery beyond "every test in both modes"
- **Options (cumulative):**
  - (a) Spec conformance cases plus every `test` in both modes (already in the roadmap).
  - (b) An order-explicit shared IR.
  - (c) Primitive ops and formatting only in `dat-rt` (D4).
  - (d) A pinned Rust toolchain for generated crates.
  - (e) Differential fuzzing with generated well-typed dat programs (Csmith-style).
  - (f) Per-test result comparison (status, error type, message, captured stdout).
- **Evidence:** Csmith found 325+ bugs; Wasmtime documents stack exhaustion, NaN bits and memory limits as divergence sources; Rust's compound assignment evaluates the RHS first for primitives; Rust 1.81 changed sort behaviour.
- **Recommendation:** all of (a)–(f). (e) can start small (expressions, then statements) and run nightly.
- **Cost:** (b)–(d) and (f) are days each; (e) is 2–4 weeks for a first generator (inference).
- **Milestone:** (a)–(d) and (f) at M0–M2; (e) by M4.
- **Spec change, owner questions:**
  - **Q-A: evaluation order.** Should the spec state that operands, arguments, and the target and value of compound assignment are evaluated left to right?
  - **Q-B: `int` edge cases.** Is `MIN % -1` equal to `0` or a `RangeError`?
  - **Q-C: sort with an inconsistent comparator.** What happens (a dat error, which type, or unspecified order)? Is the order of comparator and callback calls specified?
  - **Q-D: Unicode version.** String case mapping and similar should follow the `dat` version, not the user's rustc. Confirm.

### D6. Stack overflow (owner: catchable `StackOverflowError` if cheap)
- **Options:**
  - (a) Deterministic dat call-depth counter with the same limit in both modes, run on a large-stack thread.
  - (b) Byte-based guard (`stacker::remaining_stack`) per call.
  - (c) No guard (abort).
- **Evidence:** catching an abort is impossible (†`catch_unwind`); Wasmtime treats differing stack exhaustion across engines as legitimate divergence, which dat cannot accept because the error is catchable and observable; (b) throws at different depths per mode and build.
- **Recommendation:** (a). The interpreter cost is about one compare per call. In compiled code, pass a context `&mut` or use a thread-local counter. Also make `dat-rt`'s drop, clone, eq, hash and toString iterative, or wrap them with `stacker`, for deep recursive structs.
- **Cost:** interpreter trivial; compiled cost not measured (measure in the M0 spike); iterative drop and friends are about a week.
- **Milestone:** spike in M0, enforce in M4.
- **Spec change, owner questions:**
  - **Q-F:** add `StackOverflowError` (kind: Runtime) to the reference/12 table.
  - **Q-G:** is the depth limit a specified number (e.g. 10,000 dat frames), configurable per project, or implementation-defined? Identical behaviour in both modes needs one value.

### D7. Test runner architecture
- **Options:** (a) process per test (nextest model); (b) one process, thread pool, fresh interpreter state per test, cooperative timeouts, per-test output capture; (c) one process, sequential.
- **Evidence:** a spawn costs at least 0.5 ms, so 1,000 tests take ≥ 500 ms, which breaks the 250 ms budget; nextest notes thread cancellation is unsafe, so (b) needs cooperative interrupts (back-edge flag, as in wasmtime epochs).
- **Recommendation:** (b). Expect about 15–60 ms per 1,000 small tests cold (inference).
- **Cost:** about 1–2 weeks.
- **Milestone:** M1 (sequential is acceptable), M6 (parallel, filter, capture).
- **Spec change, owner questions:**
  - **Q-E:** are lazy module consts shared across tests in a run or recomputed per test, and how is `print` inside a const initialiser attributed under parallel tests?
  - **Q-H:** is `print` output captured per test and shown only on failure?
  - **Q-I:** what does a test timeout look like? It is a runner action, not a catchable runtime error. Confirm that this doesn't violate "every runtime error can be caught".

### D8. Affected-test selection and result caching
- **Options:** (a) always run everything; (b) Go-style result cache keyed by the hash of the test's transitive IR closure plus the `dat` version; (c) salsa-style function-level dependency tracking for `--affected` and watch mode.
- **Evidence:** Go needed significant design work for external inputs (env, files), and dat has none yet, so (b) is exact today; Vitest and Jest work at file and module-graph level, while dat can work at function level.
- **Recommendation:** (a) at M1 (fast enough); (b) at M6; (c) with the M7 query engine.
- **Cost:** (b) about 1 week; (c) comes with M7.
- **Milestone:** M6 and M7.
- **Spec change:** none now. When I/O, time or randomness are specified, the stdlib spec must say how tests that use them interact with caching (owner question for that later section).

### D9. Cranelift or other JIT dev backend
- **Options:** (a) no JIT; (b) Cranelift JIT of the dat IR; (c) Rust codegen through cg_clif for faster compiled tests.
- **Evidence:**
  - (b): Cranelift compiles ~10x faster than LLVM with 2–14% slower code, but needs a type-erased `dat-rt` (a third runtime) and native-code safety.
  - (c): only 5–20% faster rustc builds (2024–2026 data), because the front end dominates.
- **Recommendation:** (a). Optionally evaluate (c) for the separate compiled-test budget, since it changes no semantics.
- **Cost:** none.
- **Milestone:** revisit after M8.
- **Spec change:** none.

### D10. Startup budget and how to protect it
- **Options:** (a) eager whole-project check and lowering; (b) check reachable modules only, use a per-module signature-hash cache, lower lazily on first call (wasmi-style).
- **Evidence:** process start ≈ 0.5 ms; Luau-class front ends run at ~1M lines/s/core; wasmi gained "orders of magnitude" in startup from lazy translation.
- **Recommendation:** (a) is acceptable for M1 small programs; move to (b) by M6. Measure with `hyperfine -N` on Linux and macOS in CI.
- **Cost:** lazy lowering is small if the IR is per-function.
- **Milestone:** M1 (budget enforced), M6 (cache).
- **Spec change:** none.
