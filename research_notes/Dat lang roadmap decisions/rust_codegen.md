# Rust code generation for dat-lang: build latency, lessons from compilers that emit Rust or C, and mappings for the hardest features

Scope: how the dat→Rust backend should shape its output, how to keep `dat test --compiled` and `dat build` fast, and how to map spec 05 (narrowing), 18 (unions), 15 (interfaces), 06 (closures), 19 (generics), 17 (module consts) and source locations onto Rust. Current as of 28 Oct 2026.

**Two owner answers shape these notes (relayed by the coordinator):**
- Q4: compile-time type tags for interface values are accepted, and "all tricks to improve performance or do things cheaper" are welcome. General reflection stays out: no listing fields, no lookup by name.
- Q3: the millisecond goals apply only to the interpreter. Compiled runs get their own budget, so these notes estimate what a realistic one is.

**Local measurements used throughout.** No dat compiler exists yet, so I wrote a generator that emits dat-shaped Rust and timed real builds:
- Machine and toolchain: Apple M4 Pro (14 cores, 48 GB), macOS, rustc 1.99.0 (2026-09-28, LLVM 23.1.1), Apple ld-1230.1 (ld-prime).
- What the generator emits:
  - a runtime crate `datrt` with `Str` over `Rc<str>`, a copy-on-write `List<T>` over `Rc<Vec<T>>`, checked `i64` arithmetic that panics with a typed payload and a `&'static Loc` source location, a `ModConst<T>` lazy cell with a re-entry flag, and a test runner using `catch_unwind`;
  - per dat module, a Rust module (~370 lines) with 3 structs and their methods, a 3-member union enum with `From` impls, an interface trait with 3 impls and `Box<dyn>` values, a module const, 10 functions (loops, checked arithmetic, `match` on the union, dynamic dispatch, callbacks, cross-module calls) and 3 tests.
- Method:
  - "Body edit" changes one literal inside one function body of one module. "Signature edit" appends a new `pub fn` to that module.
  - Edit timings are medians of 3. Clean builds are single runs unless noted.
  - `run_tests_s` is the first launch of the freshly linked test binary.
- Scripts (temporary scratchpad, session-specific):
  - `/private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/scripts/gen.py`
  - `/private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/scripts/bench.py`
  - `/private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/scripts/gen_dispatch.py`
  - `/private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/scripts/bench_dispatch.sh`
- Raw results: `/private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/results.jsonl`
- Every Rust sketch in these notes compiles and runs on rustc 1.99, edition 2024: `/private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/sketches/sketch.rs`

Below, "[local bench]" refers to these measurements.

---

## Q1. rustc build-latency levers, realistic floors, and whether the 3 s budget holds

### Takeaway
With incremental compilation, rustc rebuilds a test binary after a one-module edit in 0.27 s for 7k generated lines, 0.9 s for 37k and 2.6 s for 110k on an M4 Pro. Clean builds take 1.0 s, 4.3 s and 12.9 s. Incremental rebuild time grows linearly with crate size, because parsing, macro expansion, name resolution and dep-graph load/save run over the whole crate on every edit. So a 3 s budget for "one module changed" holds up to about 100k generated lines in one crate on a fast laptop. Beyond that, or on slower machines, generated code must be split into several crates. The parallel front end, Cranelift, debug-info settings and linkers barely move the incremental number on macOS.

### Cited Findings

**Local measurements (single crate unless noted)** — [local bench]

| Variant | Generated Rust lines | Clean build | No-op cargo | Body edit → rebuilt | Signature edit → rebuilt | First launch + run of test binary |
|---|---|---|---|---|---|---|
| 20 modules, default dev profile | 7.4k | 1.01 s | 0.017 s | 0.27 s | 0.28 s | 0.14 s |
| 100 modules, default dev | 36.8k | 4.27 s | 0.019 s | 0.89 s | 0.95 s | 0.15 s |
| 300 modules, default dev | 110k | 12.88 s | 0.034 s | 2.62 s | 2.87 s | 0.24 s |
| 100 mod, `incremental = false` | 36.8k | 3.68 s | — | 3.54 s | 3.51 s | — |
| 100 mod, `codegen-units = 1` (incremental on) | 36.8k | 5.36 s | — | 2.48 s | 0.77 s (outlier?) | — |
| 100 mod, `debug = 0` | 36.8k | 4.05 s | — | 0.96 s | 0.98 s | — |
| 100 mod, `debug = "line-tables-only"` | 36.8k | 4.13 s | — | 0.90 s | 0.95 s | — |
| 100 mod, `opt-level = 1` | 36.8k | 6.10 s | — | 1.07 s | 0.94 s | — |
| 100 mod, `-Zthreads=8` (via `RUSTC_BOOTSTRAP=1`) | 36.8k | 2.24 s | — | 0.92 s | 0.92 s | — |
| 300 mod, `-Zthreads=8` | 110k | 6.71 s | — | 2.38 s | 2.32 s | — |
| 100 mod, `derive(Clone, PartialEq)` instead of hand-written impls | 36.8k | 4.50 / 4.53 s, vs 4.35 / 4.32 s hand-written (two reruns each, so +4%). The first derive run measured 7.55 s, an outlier from the project's first build. | — | 0.97 s | 1.04 s | — |
| 100 mod, 10 crates in a chain, edit in top crate | 36.8k | 4.66 s | 0.024 s | 0.27 s | 0.27 s | — |
| 100 mod, 10 crates in a chain, edit in bottom crate | 36.8k | 4.64 s | — | 1.13 s | 1.17 s | — |
| 300 mod, 10 crates in a chain, top / bottom edit | 110k | 12.87 s | — | 0.50 / 2.66 s | 0.50 / 2.71 s | — |
| 300 mod, 30 crates in a chain, top / bottom edit | 110k | 12.75 s | — | 0.33 / 3.11 s | 0.32 / 3.20 s | — |
| 300 mod, 10 crates as a star (all depend only on crate 0), leaf / base edit | 110k | 4.67 s | — | 0.51 / 1.21 s | 0.50 / 1.24 s | — |
| same star + `-Zthreads=8`, leaf edit | 110k | 4.03 s | — | 0.51 s | 0.52 s | — |
| 100 mod release, `opt-level = 3`, default thin-local LTO, `codegen-units = 16` | 36.8k | 5.94 s | — | — | — | — |
| 100 mod release, `lto = "fat"`, `codegen-units = 1` (the roadmap's M8 profile) | 36.8k | 18.07 s | — | — | — | — |

- Where an incremental rebuild of the 37k-line crate goes (`-Ztime-passes`, total 0.85 s) — [local bench]:
  - not incremental at all: `expand_crate` 0.126 s (macro expansion of `vec!`, `thread_local!`), `resolve_crate` 0.056 s;
  - codegen: `codegen_crate` 0.160 s, `LLVM_passes` 0.083 s;
  - link 0.069 s (Apple ld);
  - incremental bookkeeping: `serialize_dep_graph` plus `incr_comp_persist_result_cache` ≈0.12 s, `incr_comp_prepare_session_directory` 0.059 s;
  - checking: `misc_checking_3` 0.058 s, `type_check_crate` 0.039 s, `MIR_borrow_checking` 0.023 s.
- A clean build of the same crate (4.27 s) is front-end heavy: `MIR_borrow_checking` 1.34 s, `type_check_crate` 1.16 s, `codegen_crate` 0.99 s, `LLVM_passes` 0.50 s, monomorphization collector 0.36 s — [local bench].
- Other single numbers — [local bench]:
  - `rustc hello.rs` (debug) takes 0.18 s wall.
  - Cargo's no-op check is 17–34 ms, so Cargo adds little overhead compared with invoking rustc directly.
  - In rustc 1.99.0 (stable), `-Zthreads` and `-Zcodegen-backend=cranelift` are rejected with "the option `Z` is only accepted on the nightly compiler".
- macOS first-launch cost — [local bench]: the first run of a freshly linked binary took 0.15 s, and later runs about 0.00 s.
  - The cause is XProtect/Gatekeeper checking new binaries. Adding the terminal to "Developer Tools" (`sudo spctl developer-mode enable-terminal`) excludes processes it starts from XProtect — [nextest docs](https://nexte.st/docs/installation/macos/).
  - One measured first-run XProtect scan took over 6 s of an almost 8 s launch — [Eclectic Light, May 2025](https://eclecticlight.co/2025/05/04/last-week-on-my-mac-checking-code-can-take-longer-now/).

**Rust compiler performance survey 2025** (results published 10 Sep 2025) — [Rust Blog](https://blog.rust-lang.org/2025/09/10/rust-compiler-performance-survey-2025-results)
- Over 3,700 responses. Average satisfaction with build performance is 6/10, and about 45% of people who stopped using Rust cite compile times as at least one reason.
- The top complaint is incremental rebuilds after a small change. 55% wait more than 10 s for a rebuild of their most troublesome project.
- About 20% call clean builds a significant blocker. About 25% of the 1,495 CI users call CI performance a big blocker.
- Reducing debuginfo to `line-tables-only` gives 2–30% improvements in benchmarks, and disabling it gives more. Cargo is considering reducing debuginfo in the `dev` profile.
- Splitting large crates and reducing dependencies were reported as the most effective mechanisms. Almost 42% of respondents had tried none.

**Rust project goals on compile time (2025H2 and 2026)**
- 2026 "Fast Builds" roadmap — [goals.rust-lang.org](https://goals.rust-lang.org/2026/roadmap-fast-builds.html):
  - **Relink-Don't-Rebuild**: skip recompiling downstream crates when only function bodies change, targeting "5–10x for common changes". This is the bottom-crate-edit case in the table above.
  - **Promoting Parallel Front End** (continued from 2025H2).
  - **Improve cg_clif performance**.
  - **TPDE** compilation option: "compiles 10–20x faster than LLVM -O0 with similar code quality".
  - Incremental linking with **Wild**.
  - **Crate slicing** (future; experiments show 1.4x–30x).
  - **Polymorphic code generation experiment**.
- 2026 Cranelift goal — [goals.rust-lang.org](https://goals.rust-lang.org/2026/improve-cg_clif-performance.html):
  - Accepted, $65,000 (OpenAI and AWS), targeting a 2x speedup inside the backend.
  - February 2026 measurements on Zed incremental builds (16 cores): Cranelift is 5% faster than LLVM with `-Cdebug-info=none` and 12% faster with `line-tables-only`. On the rustc benchmark suite the single-crate gain ranges from negligible to ~30%, averaging 10–15%.
- The 2025H2 goal "Production-ready cranelift backend" was reported as "Will not complete" in the February 2026 goals update — [goals.rust-lang.org 2025H2](https://goals.rust-lang.org/2025h2/production-ready-cranelift.html), with status per the [Feb 2026 goals blog post](https://goals.rust-lang.org/blog-post-2026-02.html) as summarized by search. I did not read the update post itself.
- The 2025H2 parallel front end goal page claims it "can already reduce the overall compilation time by 20~30+ percent" — [goals.rust-lang.org](https://goals.rust-lang.org/2025h2/parallel-front-end.html).

**Cranelift backend status** — [rustc_codegen_cranelift README](https://github.com/rust-lang/rustc_codegen_cranelift)
- Nightly-only component: `rustup component add rustc-codegen-cranelift-preview --toolchain nightly`.
- Supported on Linux (x86_64, AArch64), macOS (x86_64, AArch64) and Windows x86_64.
- **Unwinding on panics is "not yet supported"**: experimental, and not supported on Windows or macOS. `-Cpanic=abort` is the default.
- This blocks the roadmap's catch_unwind-based runtime errors on the owner's macOS machine.

**Parallel front end** (older: Nov 2023) — [Rust Blog](https://blog.rust-lang.org/2023/11/09/parallel-rustc/)
- `-Z threads=8` cut compile times "up to 50%", with dev builds benefiting more than release.
- Memory use can rise up to 35%. Tiny programs can get slower.
- My measurement agrees on clean builds (1.9x faster at 37k and 110k lines) and shows no effect on incremental one-module edits — [local bench].

**Linkers**
- Since Rust 1.90.0 (18 Sep 2025), `x86_64-unknown-linux-gnu` links with LLD by default — [Rust Blog](https://blog.rust-lang.org/2025/09/01/rust-lld-on-1.90.0-stable/).
  - On ripgrep debug builds, an incremental rebuild links about 7x faster and finishes about 40% faster end to end. A from-scratch debug build is about 20% faster.
  - About half of ripgrep's debug build time had been spent in the linker.
- On macOS, Apple's ld-prime (the default since Xcode 15) is "faster than LLD for local macOS development" per Chromium's benchmarks. It is supported only for local arm64 macOS builds in Chromium's setup — [Chromium docs](https://chromium.googlesource.com/chromium/src.git/+/main/docs/apple_platform_linkers.md). On my machine, linking the 37k-line test binary took 0.07 s of a 0.85 s rebuild — [local bench].
- Wild aims at incremental linking, but has not shipped it as of 2026 — [wild repo](https://github.com/davidlattimore/wild).
  - Speed comparisons between Wild, mold and lld in 2026 are disputed: one Chrome-link benchmark shows lld ~5x slower than Wild and mold ~2x slower, while mold's own suite shows Wild slower — [MaskRay, Apr 2026](https://maskray.me/blog/2026-04-12-recent-lld-elf-performance-improvements); [pistack.xyz](https://www.pistack.xyz/posts/2026-10-04-mold-vs-lld-vs-gold-linker-benchmarks-2026/).
  - Treat those multiples as indicative only.

**Caching and generics**
- sccache cannot cache incrementally compiled crates, nor crates that invoke the linker (bin, dylib, cdylib, proc-macro). Absolute paths must match for a hit — [sccache README](https://github.com/mozilla/sccache). It does nothing for the edit-rebuild loop.
- `share-generics` lets downstream crates reuse monomorphizations instantiated upstream — [rust-lang/rust#48611](https://github.com/rust-lang/rust/pull/48611).
  - In 2025, instantiations containing local-only types stopped being shared, because loading shared generics slowed Zed's incremental builds — [rust-lang/rust#138522](https://github.com/rust-lang/rust/pull/138522).
- Polymorphization was removed from rustc in December 2024 (`-Zpolymorphize` dropped in PR #133883, compiler-team #810) — [compiler-team#810](https://github.com/rust-lang/compiler-team/issues/810). A 2026 goal proposes a new polymorphic codegen experiment — [goals.rust-lang.org](https://goals.rust-lang.org/2026/polymorphization.html).

**Generated code at scale: Feldera (SQL → Rust)** — [Feldera blog](https://www.feldera.com/blog/cutting-down-rust-compile-times-from-30-to-2-minutes-with-one-thousand-crates)
- Before: 8,562 lines of SQL became about 100k lines of Rust in one crate. The release-style build took 1,617.77 s on 64 cores, with one core busy.
  - `-Ztime-passes`: `LLVM_passes` 1,333 s, `type_check_crate` 44 s, `MIR_borrow_checking` 19 s.
  - Raising `codegen-units` from 16 to 256 "made no meaningful difference".
- After: one crate per dataflow operator, 1,106 crates named by a content hash of their code, so unchanged operators reuse cached artifacts. The build took 150.24 s, and mold linked in about 7 s.
- The ideal parallel estimate was about 12–24 s, so roughly 7x per-crate overhead remained unexplained.
- They had earlier moved to dynamic dispatch and "pretty much eliminated all monomorphization".

### Inferences
- **Realistic floor for "one dat module changed → test binary rebuilt and run"** on a recent laptop with one crate is about 0.3 s + 0.15 s macOS first-launch for small programs (~7k generated lines). It is about 1.0 s for ~37k lines and about 2.9 s for ~110k lines. The fixed per-crate incremental cost (~0.85 s at 37k lines) is the floor, and splitting into crates is the only lever that lowers it.
- **The 3 s budget is realistic** under four conditions:
  1. incremental compilation stays on (turning it off made the edit 4x slower);
  2. the default 256 codegen units for incremental builds are kept (`codegen-units = 1` made the edit 2.8x slower);
  3. the dat-rt runtime is prebuilt;
  4. projects above roughly 60–100k generated lines are split into several crates.

  On a 4-core CI runner I would expect 2–3x my numbers. That is an inference, not measured.
- **Clean build floor:** about 1 s for small programs, 4.3 s for 37k lines and 12.9 s for 110k (2.2 s and 6.7 s with the unstable parallel front end; 4.0–4.7 s with a star-shaped 10-crate split).
- **Crate splitting has a sharp trade-off.**
  - An edit in a crate near the top of the dependency graph rebuilds in 0.3–0.5 s at 110k lines, against 2.6 s for one crate.
  - An edit in a crate everything depends on is as slow or slower than one crate (1.1–3.1 s), because Cargo reruns rustc on every dependent and each pays its fixed incremental cost. Relink-Don't-Rebuild, if it lands, would fix exactly that case.
  - Clean builds speed up only when the crate graph is wide: the star split went from 12.9 s to 4.7 s, while the chain split stayed at 12.8 s.
- Debuginfo level, the linker (on macOS) and `-Zthreads` do not change the incremental edit time measurably at these sizes. The leftover levers for the edit loop are crate size, crate topology and how much non-incremental work (macros) the generated code causes.
- `-Zthreads` helps clean builds 1.9x but needs nightly or `RUSTC_BOOTSTRAP=1`. Depending on `RUSTC_BOOTSTRAP` in a shipped toolchain is fragile, and I would not rely on it.
- Cranelift is not usable for dat's compiled tests today, for two reasons. It is nightly-only, and it has no unwinding on macOS, while the roadmap's runtime errors rely on `catch_unwind`. Its measured end-to-end gain (5–12%) is also small next to crate splitting.
- **The release build profile matters for `dat build`.** Fat LTO with one codegen unit tripled the 37k-line release build: 18.1 s vs 5.9 s with thin-local LTO and 16 units. Its runtime benefit for dat programs is unmeasured and should be justified by the M8 benchmark suite before it becomes the default.
- On macOS, each new test binary costs about 150 ms before its first instruction runs. dat should link **one** test binary per project (not one per module) and can tell users about the Developer Tools setting in `dat doctor` or the docs.

### Gaps
- No measurements on Linux (lld/mold/wild) or on a 2–4 core CI runner. Linux numbers would need a separate run.
- Cranelift and TPDE could not be measured locally: no nightly toolchain was installed, and I did not install one.
- The ratio of dat source lines to generated Rust lines is unknown until a code generator exists. My modules (~370 Rust lines each) are synthetic.
- I found no primary number for the 2026 status of parallel front end stabilization, beyond it still being a goal and still being a `-Z` flag in 1.99.0.
- Prebuilt dat-rt as a dylib (`-C prefer-dynamic`) versus an rlib was not measured, because my runtime crate is tiny. A real dat-rt (formatting, collections, string methods) would need its own test.

---

## Q2. Lessons from compilers that emit Rust (or C) as their target

### Takeaway
Few production compilers target Rust, and the clearest lessons come from Feldera (SQL→Rust), Dafny's Rust backend and Fable's Rust target. Languages targeting C are more common.
- Generated code must be split into many crates with stable, content-addressed names to get parallel and incremental builds.
- Monomorphization is the first thing to cut when compile times explode.
- Rust has no `#line` directive, so mapping back to source must be done by the generator.
- The "fast dev mode plus slow optimized build" split that dat plans is the standard answer: Haxe (cppia/HashLink vs hxcpp/HL-C), V (tcc vs clang), Zig (self-hosted backend vs LLVM).

### Cited Findings
- **Feldera (SQL → Rust):**
  - one crate per operator with content-hash names took a ~100k-line release build from 1,618 s to 150 s;
  - `codegen-units` changes did not help;
  - they removed monomorphization in favour of dynamic dispatch;
  - a 7x gap from the ideal parallel time remained unexplained — [Feldera blog](https://www.feldera.com/blog/cutting-down-rust-compile-times-from-30-to-2-minutes-with-one-thousand-crates).
- **Dafny → Rust** (AWS, POPL 2025 Dafny workshop):
  - a translation architecture producing "efficient and largely readable Rust", tackling "type system disparities, memory management, and cross-language interoperability";
  - in 9 of 10 benchmarks the generated code was at most 22% slower than native Rust, and 2 reached parity;
  - the backend translates from Dafny's new intermediate representation rather than from the AST.
  - Sources: [POPL'25 workshop page](https://popl25.sigplan.org/details/dafny-2025-papers/14/Performant-Readable-and-Interoperable-Rust-from-Dafny), and the IR detail from [the CakeML backend paper](https://arxiv.org/html/2501.05111v1).
  - I could not retrieve the full paper text, so its specific representation choices (Rc vs raw pointers for classes) are unverified.
- **Fable (F# → Rust):** "Rust target is in alpha". It emits one `.rs` file per F# file plus a runtime crate `fable_library_rust`, built with Cargo — [Fable docs](https://fable.io/docs/getting-started/rust.html).
  - Maintainers note the hard part is "mapping the F# generated Rust types to native Rust types", and suggest a hand-written Rust wrapper module for glue — [Fable discussion #3304](https://github.com/fable-compiler/Fable/discussions/3304).
- **Rust has no `#line`.** Pre-RFCs for `#line`-style directives were posted in 2017, 2023 and 2024.
  - The 2024 `#[line]` attribute thread was auto-closed on 24 Sep 2024 without an RFC. Language team members said they "generally wanted it" but called it "endlessly bikesheddable", and also discussed source maps and DWARF post-processing as alternatives — [internals 2024](https://internals.rust-lang.org/t/pre-rfc-line-attribute/21069); [internals 2023](https://internals.rust-lang.org/t/pre-rfc-enable-setting-of-source-file-line-column/19156); [internals 2017](https://internals.rust-lang.org/t/pre-rfc-supporting-line-directives-either-directly-or-with-an-extension/4765).
- **Nim (→ C):** `--lineDir:on|off` emits `#line` directives into the generated C, and `--stackTrace:on|off` toggles Nim's own stack traces. `nimcache` holds generated files. Incremental compilation is an experimental `--incremental` switch — [Nim compiler guide 1.4.8 (older)](https://nim-lang.org/1.4.8/nimc.html); [nim(1) manpage](https://man.archlinux.org/man/nim.1.en). I found no published overhead numbers for `--stackTrace:on`.
- **V (→ C):** about 110k lines/s with a Clang backend versus about 500k lines/s with tcc or its native backend, on an Intel i5-7500 without optimization. tcc "does almost no optimizations" and is meant for development, with `-prod` using clang/gcc — [V README mirror](https://github.com/Vlang/v). These are older README figures, unverified for current releases.
- **Haxe:** the C++ target (hxcpp) is slow to compile. Haxe offers cppia, a scripted C++ subtarget with "near-zero compilation time" and full speed for host code, and HashLink bytecode ("very fast compilation time, so it is suitable for daily development") next to HL/C for native release builds — [Haxe Code Cookbook](https://code.haxe.org/category/other/working-with-cppia/); [Haxe manual HL/C](https://haxe.org/manual/target-hl-c-compilation.html); [hashlink.haxe.org](https://hashlink.haxe.org/).
- **Zig:** since 0.15, the self-hosted x86_64 backend is the default for Debug builds on x86_64 Linux, while LLVM is kept for release — [Ziggit](https://ziggit.dev/t/self-hosted-x86-backend-is-now-default-in-debug-mode/10447). Ghostty builds faster "in every single scenario" with 0.15 — [Mitchell Hashimoto](https://mitchellh.com/writing/zig-builds-getting-faster). One forum report measured 27x slower runtime for the self-hosted backend's debug code — [Utensil's blog](https://utensil.bearblog.dev/zig-self-hosted-backend/), a single project.
- **Large generated Rust in general:** a user reported 2–3 hour builds of heavy `syn`/`quote` output. The advice there:
  - measure with `cargo llvm-lines`, `--timings` and `-Ztime-passes`;
  - move non-generic code out of generic functions, remove `#[inline]`, and break up the longest functions;
  - use `opt-level = 2`;
  - split into several crates, because some passes scale worse than O(n log n).
  - Source: [users.rust-lang.org](https://users.rust-lang.org/t/unmanageable-compile-time-with-long-lots-of-generated-code/118976).
- **Panics as exceptions:** `catch_unwind` "is not recommended" as a general try/catch. It catches only unwinding panics (not aborts), requires `UnwindSafe` (`AssertUnwindSafe` works around it), and the panic hook runs before the panic is caught — [std docs](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html).

### Inferences
- Feldera is the closest precedent to dat (a compiler that controls all generated code), and its lesson matches my local data. Controlling the crate structure is the strongest lever, and content-hash naming makes the split cache-friendly.
- Dafny's ≤22% gap to hand-written Rust shows a high-level language can get close to native Rust through generated code, and supports the roadmap's ≤1.2× geomean target as plausible but ambitious. Dafny has no dat-style COW collections, so the comparison is loose.
- Because Rust has no `#line`, dat must keep its own map from generated `.rs` lines to `.dat` spans. It serves two uses: rewriting rustc diagnostics, which the roadmap already treats as compiler bugs, and producing dat-level stack traces.
- Since `catch_unwind` is officially discouraged as general try/catch but works, dat's runtime-error design (panic plus typed payload) is viable under these conditions:
  - generated code and dat-rt must never rely on panic=abort;
  - the silent panic hook is mandatory;
  - every `catch` region needs `AssertUnwindSafe`, and the spec's "changes made before the error stay" rule matches what unwinding does.

### Gaps
- I found no first-hand write-ups on borrow-checker friction from languages compiling to Rust beyond Fable's interop note. Dafny's full paper was unreachable. Koka, Lean 4 and Jakt were not researched within the budget.
- I found no source on code size or debug-info quality of generated Rust in these projects.

---

## Q3. Concrete mappings for the hardest features (options, sketches, recommendation)

All sketches below compile and run on rustc 1.99 (edition 2024) — [local sketch file](file:///private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/bench/sketches/sketch.rs).

### Takeaway
Every hard feature has a safe, direct Rust form.
- **Narrowing**, including field paths and narrowing after an early return, maps to let-else, if-let or a labelled block that breaks with a reference. The roadmap's "no direct form after early exit" is outdated, since let-else has been stable since Rust 1.65.
- **Unions** map to one canonical enum per flattened member set, with `null` as an outer `Option`.
- **Interfaces** map to a generated enum when the implementors are known and fit the crate layout, and to `Box<dyn>` with generated `__tag`/`__as_X`/`__clone_box` methods otherwise. No `Any` is needed.
- **Module consts** cannot use `static LazyLock`, because dat's Rc-based values are not `Sync`. They need a thread-local lazy cell with a re-entry flag, or compile-time evaluation.

### Cited Findings

**(1) Narrowing (spec 05)**
- let-else and `break` from labelled blocks are stable since Rust 1.65 (3 Nov 2022) — [Rust 1.65.0 blog](https://blog.rust-lang.org/2022/11/03/Rust-1.65.0.html).
- if-let chains (`if let Some(x) = a && cond(x)`) are stable since Rust 1.88.0 (26 Jun 2025), only in the 2024 edition — [Rust 1.88.0 blog](https://blog.rust-lang.org/2025/06/26/Rust-1.88.0).
- Verified sketches — [local sketch file]:
  ```rust
  // dat: if (user.address == null) { return "unknown"; } return user.address.city;
  let Some(user__address) = &user.address else { return s("unknown"); };
  user__address.city.clone()

  // a sibling-field assignment keeps the borrow valid; assigning the path ends narrowing
  let Some(user__address) = &user.address else { return s("none"); };
  user.name = s("renamed");               // disjoint field borrow: compiles
  let z = user__address.zip.clone();      // still narrowed
  user.address = None;                    // narrowing ends; codegen reverts to `user.address`

  // `||` plus early exit, with no unwrap and no duplicated branch:
  // dat: if (x == null || x.isEmpty()) { print("empty"); return 0; }
  let x__n: &Vec<i64> = 'narrow: {
      if let Some(v) = x { if !v.is_empty() { break 'narrow v; } }
      println!("empty");
      return 0;
  };

  // `&&`: if (x != null && x.isEmpty()) -> let-chain (edition 2024)
  if let Some(v) = x && v.is_empty() { return true; }

  // narrowed `let` local mutated through the narrowed name
  if let Some(x__n) = &mut x { x__n.push(1); }
  ```

**(2) Unions (spec 18)**
- Verified sketch — [local sketch file]: a canonical enum per sorted, flattened member set, with `null` lifted to `Option`.
  ```rust
  // dat: int | string | null  ==>  Option<U_int_string>
  pub enum U_int_string { int(i64), string(Str) }
  impl From<i64> for U_int_string { ... }          // one From per member
  // `value is int`  ==>  if let Some(U_int_string::int(v)) = value
  // after `if (value == null) return`: let Some(value__n) = value else { return ... };
  //   value__n: &U_int_string, exactly the type `int | string`, with no conversion

  // widening between unions that share members, generated only for pairs the program uses
  impl From<U_Circle_Square> for U_Circle_Square_Triangle { ... }
  // shared field (spec 18) read without narrowing: generated accessor
  impl U_Circle_Square_Triangle { pub fn label(&self) -> &Str { match self { ... } } }
  // sub-union narrowing (`if (shape is Triangle) return`) keeps the original enum;
  // later matches get an `unreachable!()` arm the dat checker has proven dead
  ```

**(3) Interfaces (spec 15) and the closed-world question (owner Q4)**
- Local dispatch micro-benchmark — [local bench, gen_dispatch.py]:
  - Setup: one interface with 4 methods; K implementors (8 or 64); 1,000 generated functions, each of which copies a `list<Shape>`, calls a `mut` method on every element, dispatches, runs an `is` check and an `as?` downcast. Release run over 2M elements.

  | Lowering | K | Debug clean build | Release clean build | Debug / release binary | Rebuild after adding one implementor | Copy+mutate+dispatch (×20) | Dispatch only (×50) |
  |---|---|---|---|---|---|---|---|
  | `Vec<Box<dyn Shape>>` + generated `clone_box`/`tag`/`as_s0` | 8 | 1.46 s | 5.59 s | 1.97 / 1.45 MB | 1.35 s | 1.61 s | 124 ms |
  | closed-world `enum ShapeV` + `match` | 8 | 1.17 s | 5.98 s | 1.84 / 1.41 MB | 1.14 s | 0.86 s | 102 ms |
  | `Vec<Box<dyn Shape>>` | 64 | 1.48 s | 5.67 s | 2.15 / 1.54 MB | 1.39 s | 1.72 s | 129 ms |
  | closed-world enum | 64 | 1.21 s | 5.56 s | 1.97 / 1.40 MB | 1.18 s | 0.91 s | 137 ms |

  - In the 100-module whole-program bench, every module's `Box<dyn>` interface values compiled fine. Copying uses a generated `clone_box` without the `dyn-clone` crate: `impl Clone for Box<dyn Shape> { fn clone(&self) -> Self { self.__clone_box() } }` — [local sketch file].
- enum_dispatch's own benchmarks (Ryzen 7 2700x) report the generated-enum approach "almost 5 times faster" than `Box<dyn>` in a blackbox test, and about 10x on a homogeneous Vec — [enum_dispatch docs](https://docs.rs/enum_dispatch/latest/enum_dispatch/).
- `is`/`as?` without `Any`, verified — [local sketch file]: a generated `fn __tag(&self) -> u32` (dense compile-time tag) and `fn __as_Circle(&self) -> Option<&Circle> { None }` with a default body. Only structs actually used as `is`/`as?` targets get the override.

**(4) Closures (spec 06)**
- Verified sketch — [local sketch file]:
  - stored function values: `Rc<dyn Fn(i64) -> i64>` (cheap to copy, as value semantics require);
  - parameters of function type: `&dyn Fn(i64) -> i64` (no allocation, no monomorphization; a named function passes as `&square`);
  - capture by copy: `move` closures.
  - The spec's rule that closures capture only constants keeps every closure `Fn`, so no `RefCell` is needed. A function type with `throws E` becomes `dyn Fn(A) -> Result<R, E>`.

**(5) Generics (spec 19)**
- The spec promises "The compiler generates specialized code for each set of type arguments, so generics cost nothing at runtime" (reference/19-generics.md), so dyn erasure would contradict the spec.
- Feldera reduced compile times by eliminating monomorphization at large scale — [Feldera blog](https://www.feldera.com/blog/cutting-down-rust-compile-times-from-30-to-2-minutes-with-one-thousand-crates).
- Polymorphization no longer exists in rustc (removed December 2024) — [compiler-team#810](https://github.com/rust-lang/compiler-team/issues/810).
- `cargo llvm-lines` counts LLVM IR per generic function and is the standard way to find runaway instantiations — [users.rust-lang.org](https://users.rust-lang.org/t/unmanageable-compile-time-with-long-lots-of-generated-code/118976).

**(6) Module consts (spec 17)**
- `LazyLock` (stable 1.80) is `Sync` only when `T: Sync + Send`. If its initializer panics, the lock is poisoned "unrecoverably" and every later access panics — [std::sync::LazyLock docs](https://doc.rust-lang.org/std/sync/struct.LazyLock.html).
- `std::cell::OnceCell` (stable 1.70): if the initializer panics, "the cell remains uninitialized" so a later call retries. Re-entrant initialization "results in a panic" — [std::cell::OnceCell docs](https://doc.rust-lang.org/std/cell/struct.OnceCell.html).
- Verified in the 100-module bench — [local bench, gen.py `ModConst`]: a `thread_local!` holding `ModConst<T> { cell: OnceCell<T>, busy: Cell<bool> }`. On re-entry it throws a dat `AssertionError` itself, so the outcome is deterministic in both modes rather than left to std.

**(7) Source locations, stack traces, rustc errors**
- Rust has no `#line` (see Q2).
- In the bench, every runtime-checked operation takes a `&'static Loc { file, line, col }`, and the slow path is a `#[cold] #[inline(never)]` function. This gives exact dat locations in runtime error messages without debuginfo — [local bench, gen.py `throw_range`].
- Nim's `--stackTrace:on` is the precedent for a language-maintained stack — [Nim compiler guide](https://nim-lang.org/1.4.8/nimc.html).

### Inferences

**(1) Narrowing.**
- Recommended lowering: every narrowing scope binds a fresh name (`<path>__n`) to a *reference* produced by `if let`, `let ... else`, a let-chain, or the general labelled-block form. The labelled block handles any `&&`/`||` mix plus a diverging branch, with one copy of the branch.
- Uses of the narrowed path are rewritten to the fresh name until narrowing ends. Because narrowing ends when the path is assigned or passed as `mut`, the reference's last use always comes before the conflicting write. So NLL accepts the code, and no `unwrap`, `unsafe` or clone is needed.
- A `.as_ref().unwrap()` fallback per use remains available for any shape this misses. It adds a panic path per use, which in dat semantics would be a `TypeError` that can never be thrown, so it stays a fallback.
- Owner question: the spec says narrowing ends when "any part of the path is assigned". Does assigning a sibling field such as `user.name` end narrowing of `user.address`? Rust accepts either answer (the disjoint-field sketch compiles), so this is purely a spec choice.

**(2) Unions.**
- Canonical identity: sort members by a stable key (fully qualified type name), drop `null`, name the enum from the sorted list. Long names get a hash, e.g. `U_3fa9c1`, with a doc comment listing the members. Every alias of the same set (`type Shape = Circle | Square` in two modules) maps to one Rust enum via `pub type Shape = U_Circle_Square;`.
- Placement: with one crate, all union enums live in a generated `__unions` module at the crate root. With several crates, a union must live in a crate that can see every member type, which constrains crate splitting (see the decisions section).
- `Option<U>` outside makes the most common narrowing (null checks) free.
- Nested unions cannot occur after flattening, except `Option<U>`.
- Matching across unions that share members is by member type, through generated `From` widening impls. Narrowing to a sub-union keeps the original enum with dead arms, which avoids converting and copying.
- Recursive members (`ListNode?` inside `ListNode`) need `Option<Box<T>>`, as the roadmap notes. A union containing a struct that contains the union needs `Box` too, and the generator must detect such cycles.

**(3) Interfaces: closed-world lowering, as the owner asked.**
- **Runtime:** the generated enum is 1.9x faster when values are copied (no heap allocation per element, contiguous storage) and roughly equal on pure dispatch (−18% to +6% wall time). The large enum_dispatch speedups (5–10x) are not reproduced for pure dispatch on an M4 Pro, so the main win comes from avoiding allocation and from cheaper `Clone`, `is` and `as?`. `is` on an enum is a discriminant compare instead of a virtual call.
- **Compile time and code size:** no worse. Debug clean builds were 18–20% faster, release builds were within noise, binaries were 2–9% smaller, and adding an implementor rebuilt as fast as with `Box<dyn>`.
- **Costs and limits:**
  - Enum size equals the largest implementor. A size threshold should box big variants (`Big(Box<BigStruct>)`).
  - Generic interfaces (`Source<T>`) need one enum per instantiation.
  - A struct that implements the interface but never flows into an interface-typed slot can be left out of the enum (whole-program knowledge).
  - The closed world breaks when packages or separately compiled libraries arrive (spec: "not yet specified"). The `Box<dyn>` lowering must therefore stay as the fallback, and the choice should be per interface.
  - **Crate-layout interaction:** the enum must be nameable wherever `Shape` values appear, and must depend on every implementor. If code is split into crates and an implementor lives in a crate above a user of `Shape`, the enum cannot exist without a dependency cycle. The closed-world lowering then only works if all implementors sit at or below every crate that uses the interface type, for example in a shared "types" crate.
- **Copy semantics:** spec 15 says "copying [an interface value] copies the struct inside it". The `Box<dyn>` fallback can use `Rc<dyn Shape>` with clone-on-mutate through a generated `__clone_rc`, which matches dat's copy-on-write model and makes copies a refcount bump. That is a pure codegen choice.
- **No `std::any::Any`:** generated `__tag`/`__as_X` keep "no runtime reflection" literally true. The owner's Q4 answer allows tags but not field listing or lookup by name, and these helpers do neither.

**(4) Closures.**
- Use `&dyn Fn` for function-typed parameters of user functions and `Rc<dyn Fn>` for function values stored in variables, fields or collections. This avoids monomorphizing every user function per closure type, the pattern Feldera removed for compile time.
- Inside dat-rt built-ins (`map`, `filter`, `reduce`, `sort` comparators), a generic `F: Fn` gives inlining in release builds. Each call site then becomes an instantiation in the user's crate, a compile-time cost to measure in M2.
- My bench used `&dyn Fn` in `List::map`/`filter`, and they compiled cheaply.

**(5) Generics.**
- Emit Rust generics with trait bounds (`T: dat_rt::Equatable` and so on) and let rustc monomorphize. Rustc type-checks and borrow-checks the generic body once, while self-monomorphizing in dat would multiply front-end work, which dominates clean builds (borrowck 1.34 s plus typeck 1.16 s of 4.27 s in my bench).
- Keep dat-rt's generic surface thin (non-generic inner functions) and track instantiation counts with `cargo llvm-lines` in CI.
- Switching user generics to dyn erasure would break the spec's "cost nothing at runtime" promise. That would be an owner decision.

**(6) Module consts.**
- dat values hold `Rc`, so a `static LazyLock<List<T>>` does not compile: `LazyLock` is `Sync` only when `T: Sync + Send`.
- Options:
  - (a) A `thread_local!` with `OnceCell` plus a re-entry flag. Verified, cheap, safe. But each thread computes its own copy. That is observable only if an initializer has side effects (`print`), and only under the parallel test runner.
  - (b) Switch dat-rt to `Arc` everywhere, which costs atomic refcounts on every copy, just to share consts.
  - (c) The dat compiler evaluates pure initializers at compile time into Rust constant data (e.g. `const DEFAULT_PORTS` → a static array wrapped on access), with (a) for the rest.
- Re-entry detection must be dat's own flag. `OnceCell` panics with a std message, and `OnceLock`/`LazyLock` deadlock or poison.
- Owner questions:
  - If an initializer throws a runtime error, does the next access retry (`OnceCell` behaviour) or rethrow (`LazyLock` poisoning)? Both modes must agree.
  - Is "computed once" per program or may it be per thread?

**(7) Source locations.**
- Three layers:
  - (i) Every runtime-checked operation carries a `&'static Loc`, so error messages always name the dat file, line and column, at near-zero cost.
  - (ii) The generator writes a line map (`.rs` line → `.dat` span) next to the generated code. It emits one dat statement per Rust line where possible, and adds a `// file.dat:12:3` comment at each function start for humans.
  - (iii) Stack traces have two options:
    - a shadow stack: a thread-local push/pop of (function id, call-site `Loc`) per dat call. It gives frames identical to the interpreter, at a small cost per call, and is Nim's approach;
    - native backtraces symbolized through debuginfo and mapped through the line map. These cost nothing until an error, but need line-tables debuginfo, and inlining in release drops frames, so traces would differ between modes.
- rustc diagnostics: run Cargo with `--message-format=json`, map each primary span through the line map, and report "internal compiler error: generated Rust failed to compile" with the dat location and the rustc message attached.
- Owner question: must uncaught-error stack traces be identical between the interpreter and compiled release builds? If yes, the shadow stack is required in all builds. If only in tests, it can be limited to `--compiled` test builds.

### Gaps
- The enum-vs-dyn benchmark uses one interface with small structs on one machine. The runtime result depends on struct sizes and on how predictable branches are.
- I did not measure shadow-stack overhead. It is cheap in principle, but a number is needed before choosing it for release builds.
- I did not measure generic `F: Fn` against `&dyn Fn` in dat-rt for compile time or runtime.
- Interaction of `catch_unwind` with the narrowing borrows inside `try` blocks was not tested (e.g. a narrowed `&mut` captured in an `AssertUnwindSafe` closure).

---

## Q4. Generated-code shape for fast compiles: what to emit and what to avoid

### Takeaway
rustc's cost scales with crate size, borrow-checking and type-checking, macro expansion and monomorphization. The generator should emit plain, macro-free, non-generic-where-possible Rust in many small functions, write only files that changed, and keep each crate below a size where the incremental floor stays under the budget.

### Cited Findings
- Clean build passes: front-end checks (borrowck 1.34 s, typeck 1.16 s) outweigh LLVM in debug builds of generated code (codegen 0.99 s, LLVM 0.50 s of 4.27 s). In incremental rebuilds, non-incremental passes (macro expansion 0.126 s, name resolution 0.056 s) and dep-graph bookkeeping (~0.18 s) are about half of the 0.85 s — [local bench].
- Built-in `derive(Clone, PartialEq)` cost about +4% on clean builds against hand-written impls in my bench (4.50/4.53 s vs 4.35/4.32 s). `derive(Debug)` failed when field types lacked `Debug` — [local bench]. Proc-macro derives (e.g. serde) are a known hot spot, and derive expansion caching is listed as in-progress work — [survey results](https://blog.rust-lang.org/2025/09/10/rust-compiler-performance-survey-2025-results).
- Feldera and the forum advice: huge functions, long branchy bodies and `#[inline]` on large functions slow LLVM, and some passes scale worse than O(n log n). rustc's unstable `-Z hint-mostly-unused` defers codegen of functions that may never be called — [Feldera](https://www.feldera.com/blog/cutting-down-rust-compile-times-from-30-to-2-minutes-with-one-thousand-crates); [users.rust-lang.org](https://users.rust-lang.org/t/unmanageable-compile-time-with-long-lots-of-generated-code/118976); [rust-lang/rust#142689 (hint-mostly-unused, per search summary)](https://github.com/rust-lang/rust/pull/142689).
- Cargo reruns rustc on every crate whose inputs' fingerprints changed. A body change in an upstream crate rebuilds all dependents today, and Relink-Don't-Rebuild aims to fix this — [Fast Builds roadmap](https://goals.rust-lang.org/2026/roadmap-fast-builds.html); [local bench, bottom-edit rows].

### Inferences
Rules for the generator, each tied to evidence above:
1. **No macros in generated code.** Emit `List::from_vec(Vec::from([..]))` rather than `vec![..]`, direct `std::fmt::Write` calls rather than `format!`/`println!`, and `std::thread::LocalKey` accessors from one dat-rt macro use rather than one `thread_local!` per const where possible. Macro expansion runs on every rebuild and is not incremental.
2. **Emit trait impls directly instead of derives** when dat semantics differ (custom `operator equals`, float rules) or a derive would need bounds on dat-rt types. Use derives where they are exactly right; the compile-time difference is small (+4%).
3. **One dat function → one Rust function.** Never inline dat bodies at the source level. Mark error slow paths `#[cold] #[inline(never)]` in dat-rt so checked arithmetic and indexing stay small at every use.
4. **Loops, not iterator chains, in generated code.** Generated loops are what a dat `for…of` means anyway, and they keep debug-build test runs fast. Iterator chains are left to hand-written dat-rt.
5. **`&dyn Fn` and `&dyn Trait` at user-function boundaries**, generics only where the spec promises specialization (user generics) or where dat-rt needs inlining.
6. **Write only changed files**, with deterministic output and stable names. Cargo fingerprints by mtime, and an unchanged file rewritten would trigger rebuilds of its crate and every dependent.
7. **Crate-level `#![allow(...)]`** (or `--cap-lints allow`) for naming and unused lints, so warnings never flood output and lint passes stay cheap. Generated names like `user__address` trigger `non_snake_case` otherwise, as seen when compiling the sketches.
8. **Pin the rustc version** used for generated code and the prebuilt dat-rt. Rlibs are tied to the exact compiler version, and an incompatible rustc fails with E0514 ("found crate compiled by an incompatible version of rustc"). The E0514 wording is from the rustc error index; I did not fetch it.
9. **One test binary per project**, not per module: one link, and one macOS first-launch scan (0.15 s each).
10. Keep each crate below roughly 40–60k generated lines if the edit budget is ~1 s on this class of machine. Split by import-graph strongly connected components above that.

### Gaps
- Not measured: emitting `vec![]` versus `Vec::from([..])`, and per-const `thread_local!` versus one table. The 0.126 s macro-expansion figure is for my generator's mix.
- Effect of very large single functions (e.g. a 5,000-line dat function) on rustc was not tested.

---

## Decisions for dat-lang

Each decision lists the options, evidence, a recommendation, its cost, the milestone that needs it, and whether it touches the spec. Numbers marked [local] come from the M4 Pro benchmark above.

### D1. Budget for compiled mode (`dat test --compiled`, `dat build`)
- **Options:**
  - (a) Keep one number: "< 3 s, warm cache, one module changed".
  - (b) Tiered budgets on a named reference machine.
  - (c) No budget.
- **Evidence:**
  - Rebuild after a one-module edit, plus first launch [local]: 0.27 + 0.14 s at 7k generated lines; 0.89 + 0.15 s at 37k; 2.62 + 0.24 s at 110k (one crate). With a star-shaped 10-crate split at 110k: 0.51 s for a leaf edit and 1.21 s for a base edit.
  - Clean builds [local]: 1.0 / 4.3 / 12.9 s at 7k / 37k / 110k lines.
  - Release build of 37k lines: 5.9 s thin-local, 18.1 s with fat LTO and one codegen unit.
  - Survey: 55% of Rust users wait more than 10 s per rebuild, so sub-3 s is well ahead of typical Rust projects.
  - (a) is too loose for small programs and fails for large ones without splitting.
- **Recommendation:** (b), measured on a reference machine (e.g. an M-series MacBook and a 4-core Linux CI runner, each with its own numbers):
  - warm one-module edit → test results: ≤ 1 s up to 40k generated Rust lines, ≤ 3 s up to 150k (requires D2's split);
  - clean `dat test --compiled` with a prebuilt runtime: ≤ 5 s at 40k lines;
  - `dat build --release`: ≤ 30 s at 40k lines;
  - track the generated-lines-per-dat-line ratio from M1, so the budget can be restated in dat lines.
- **Cost:** low (CI timing jobs already planned).
- **Milestone:** M0 (spike measures it); enforced from M6, as the roadmap proposes.
- **Spec change:** no.

### D2. Crate layout for generated code
- **Options:**
  - (a) Whole program → one crate (the roadmap's proposal).
  - (b) One crate per dat module.
  - (c) One crate per strongly connected component of the import graph, or per group of modules, with content-hash names (Feldera).
  - (d) (a) by default, switching to (c) above a size threshold.
- **Evidence:**
  - One crate's incremental cost grows linearly: 0.27 / 0.89 / 2.62 s [local].
  - Splitting helps edits near the top of the graph (0.27–0.5 s) but not edits in widely used crates (1.1–3.1 s), until Relink-Don't-Rebuild lands (targeting 5–10x).
  - Clean builds speed up only for wide graphs: star 4.7 s vs chain 12.9 s at 110k [local].
  - Feldera: 1,618 s → 150 s with 1,106 crates.
  - (b) is impossible in general: dat allows circular imports (spec 17), and Rust crates cannot depend on each other cyclically, so cycles must share a crate. That is (c).
  - Structural unions and closed-world interface enums need a crate that sees every member type.
  - Struct methods must stay in the struct's crate (inherent impls), and interface impls must sit in the trait's or the struct's crate (orphan rule).
- **Recommendation:** (d).
  - M0–M5 generate one crate, with each dat module as a Rust module addressed by fully qualified paths, so splitting later is mechanical.
  - In M6, implement SCC grouping with a "types" crate at the bottom: all structs, enums, unions, interfaces and their impls, plus the closed-world enums. Above it sit body crates per SCC group, named by content hash.
  - Switch to the split above about 40k generated lines, or make it always-on if M6 measurements favour it.
- **Cost:** (a) low; (c) medium to high (cross-crate visibility, `pub(crate)` vs dat `export`, placing generated unions and interface enums).
- **Milestone:** M0 decides that (a) comes first; M6 decides on (c).
- **Spec change:** no. Note that the types-crate design puts every struct method body in the bottom crate, so editing a method rebuilds everything. Measure in M6 before committing.

### D3. How dat drives rustc
- **Options:**
  - (a) Generate a Cargo workspace in `.dat/build/` and run `cargo build --message-format=json`.
  - (b) Invoke `rustc` directly with a hand-made dependency graph.
- **Evidence:** Cargo no-op is 17–34 ms [local], so Cargo's overhead is negligible next to rustc. Cargo already handles fingerprints, the jobserver, `--timings` and JSON diagnostics.
- **Recommendation:** (a). Revisit (b) only if Cargo's per-crate overhead shows up after the D2 split; Feldera observed an unexplained 7x per-crate overhead with 1,106 crates.
- **Cost:** low.
- **Milestone:** M0.
- **Spec change:** no.

### D4. Rust build profiles
- **Options:**
  - Dev/test: (a) Cargo defaults; (b) tuned.
  - Release: (c) fat LTO with `codegen-units = 1` (roadmap); (d) default opt-level 3 with thin-local LTO and 16 units; (e) thin LTO.
- **Evidence:**
  - `incremental = false` makes an edit 4x slower (3.5 s vs 0.9 s) [local].
  - `codegen-units = 1` makes it 2.8x slower (2.48 s) [local].
  - debuginfo has no measurable effect on macOS (0.90–0.96 s); the survey reports 2–30% from `line-tables-only` elsewhere.
  - `opt-level = 1` makes clean builds +43% slower (6.1 s).
  - Fat LTO plus one codegen unit takes 18.1 s vs 5.9 s.
  - `panic = "unwind"` is required by the roadmap's `catch_unwind` design.
- **Recommendation:**
  - dev/test: incremental on, default codegen units, `debug = "line-tables-only"` (needed for backtraces under D10 at little cost), `opt-level = 0`, `panic = "unwind"`;
  - release: start with (d) and adopt (c) only if the M8 benchmarks show a runtime gain worth roughly 3x longer builds, or offer it as `dat build --max`.
- **Cost:** low.
- **Milestone:** dev in M0; release in M8.
- **Spec change:** no.

### D5. The dat-rt runtime crate
- **Options:**
  - (a) Built from source on first use and cached per rustc version.
  - (b) Shipped prebuilt with the dat toolchain, like Rust's std.
  - (c) dylib (`prefer-dynamic`) for dev, rlib for release.
- **Evidence:**
  - Rlibs only load with the same rustc (E0514), so the rustc version must be pinned.
  - Generic dat-rt code (`List<T>`) is instantiated in the user's crate anyway, so prebuilding saves only non-generic code.
  - share-generics reuses upstream instantiations in debug builds, but no longer for local-only types (rust-lang/rust#138522).
  - sccache does not help incremental or bin crates.
  - No local dylib measurement was taken.
- **Recommendation:** (a) with a pinned toolchain in the generated workspace, moving to (b) once dat ships binaries. Keep dat-rt's generic APIs as thin wrappers over non-generic cores. Measure (c) in M6.
- **Cost:** low to medium.
- **Milestone:** M0 (pinning); M6 (dylib test).
- **Spec change:** no.

### D6. Codegen backend and front-end parallelism
- **Options:**
  - (a) LLVM only.
  - (b) Cranelift for dev/test.
  - (c) the parallel front end via `-Zthreads`.
- **Evidence:**
  - Cranelift is nightly-only, has no unwinding on macOS or Windows (panic=abort by default), and gains 5–12% end to end on Zed. The 2025H2 production-ready goal will not complete; a 2026 goal targets a 2x backend speedup.
  - `-Zthreads=8` makes clean builds 1.9x faster (4.27 → 2.24 s, 12.9 → 6.7 s) but leaves incremental edits unchanged [local]. It is still a nightly-only `-Z` flag in rustc 1.99.0.
  - TPDE (10–20x faster than LLVM at -O0) is only a 2026 goal.
- **Recommendation:** (a). Track Cranelift unwinding support and parallel front end stabilization, and revisit at M6 and M8. Do not ship `RUSTC_BOOTSTRAP`.
- **Cost:** none now.
- **Milestone:** revisit at M6.
- **Spec change:** no.

### D7. Linker
- **Options:**
  - (a) Platform defaults: ld-prime on macOS, lld on Linux x86_64 since Rust 1.90.
  - (b) mold or wild on Linux when installed.
- **Evidence:** linking took 0.07 s of a 0.85 s rebuild on macOS [local]. lld made ripgrep's incremental link 7x faster and its incremental build 40% faster. Wild has no incremental linking yet.
- **Recommendation:** (a). Optionally detect mold on Linux in `dat doctor`.
- **Cost:** none.
- **Milestone:** M6.
- **Spec change:** no.

### D8. Narrowing lowering (spec 05)
- **Options:**
  - (a) Rebind to a fresh reference name via let-else, if-let, let-chains, or the labelled-block-break form for arbitrary `&&`/`||` plus a diverging branch.
  - (b) `.as_ref().unwrap()` at each use (the roadmap's fallback).
  - (c) Restructure the rest of the block into the `if let` branch, duplicating code.
- **Evidence:**
  - All (a) forms compile on rustc 1.99, including narrowing that survives an early return, sibling-field assignment while narrowed, and mutation through a narrowed `let` [local sketch].
  - let-else and labelled-block breaks are stable since 1.65; let-chains since 1.88, edition 2024 only.
  - (b) adds a dead panic path per use. (c) grows code size.
- **Recommendation:** (a), with (b) as a rare fallback. Generate edition 2024.
- **Cost:** medium (the checker already tracks narrowing scopes; codegen needs the rename map).
- **Milestone:** M3. The roadmap rated this risk "high"; with let-else available it is medium.
- **Spec change:** none required. **Question for the owner:** does assigning a sibling field (`user.name = ...`) end narrowing of `user.address`? The spec's "any part of the path" is ambiguous, and Rust accepts both readings.

### D9. Union representation (spec 18)
- **Options:**
  - (a) One canonical enum per flattened, sorted member set, with `T?` / `| null` as an outer `Option<U>`.
  - (b) `null` as an ordinary enum variant.
  - (c) A separate enum per `type` alias, with conversions.
- **Evidence:**
  - With (a), null-narrowing yields exactly the canonical payload type at zero cost, and `is` maps to `if let` [local sketch].
  - (c) contradicts "flattened, deduplicated, order-free" (spec 18), which requires `type A = int | string` and `string | int` to be the same type.
  - Widening between unions with shared members needs generated `From` impls, emitted only for the pairs the program uses.
- **Recommendation:** (a).
  - Hash-suffixed names for long member lists, `pub type Alias = U_…;` for aliases.
  - Sub-union narrowing keeps the original enum with checker-proven `unreachable!()` arms.
  - Shared fields become generated accessor methods.
  - Every enum lives in a `__unions` module, or in the types crate under D2(c).
- **Cost:** medium.
- **Milestone:** M3.
- **Spec change:** no.

### D10. Interface values (spec 15): closed-world enum or `Box<dyn>`
- **Options:**
  - (a) Always `Box<dyn Trait>`, with generated `__clone_box`, `__tag` and `__as_X`.
  - (b) `Rc<dyn Trait>` with clone-on-mutate (copy-on-write, matching dat values).
  - (c) A closed-world generated enum per interface (per instantiation for generic interfaces), falling back to (a)/(b) when the world is not closed or the crate layout forbids it.
- **Evidence** [local dispatch bench, K = 8 and 64, 1,000 functions]:
  - (c) runs copy-heavy code 1.9x faster (0.86 s vs 1.61 s) and pure dispatch about the same (102 vs 124 ms at K=8; 137 vs 129 ms at K=64).
  - (c) builds no slower: debug clean 1.17 vs 1.46 s, release within ±7%, binaries 2–9% smaller, and adding an implementor rebuilds at the same speed (1.14 vs 1.35 s).
  - enum_dispatch reports 4.5–10x on its own benchmarks.
  - Limits: enum size equals the largest variant; under multi-crate splitting the enum must sit where it can see every implementor; packages and separate compilation (unspecified) would open the world.
  - Neither (a) nor (c) needs `Any` or TypeId. Tags are dense integers, which matches the owner's Q4 answer, and nothing lists fields or looks anything up by name.
- **Recommendation:** (c) as the default lowering when the whole program is visible, with (b) as the fallback. Prefer (b) over (a), because copies of interface values are then a refcount bump, consistent with dat's copy-on-write values.
  - Box any enum variant above a size threshold (e.g. 64 bytes).
  - Emit `__as_X` only for structs used as `is`/`as?`/`match` targets.
  - Run the M5 conformance suite under both lowerings, as a flag, to keep the fallback honest.
- **Cost:** medium. Two lowerings to maintain, though both are mechanical.
- **Milestone:** M5. Revisit when packages are specified.
- **Spec change:** none. **Question for the owner:** will packages, plugins or separately compiled libraries ever need to add implementors after the program is compiled? If yes, the fallback becomes the path for public interfaces of packages.

### D11. Function values and closures (spec 06)
- **Options:**
  - (a) `Rc<dyn Fn>` everywhere.
  - (b) `&dyn Fn` for parameters and `Rc<dyn Fn>` for stored values.
  - (c) Generic `F: Fn` for parameters (monomorphized per closure).
- **Evidence:**
  - (b) compiles and avoids allocation for the common case of callbacks passed to functions [local sketch].
  - Monomorphization was Feldera's main compile-time cost.
  - Capturing only constants (spec 06) keeps every closure `Fn`, so no `RefCell` is needed.
- **Recommendation:** (b) for user code. Inside dat-rt built-ins, decide between `&dyn Fn` and generic `F: Fn` by measurement in M2 (generic gives release-build inlining but one instantiation per call site).
- **Cost:** low.
- **Milestone:** M2.
- **Spec change:** no.

### D12. Generics (spec 19)
- **Options:**
  - (a) Emit Rust generics with trait bounds; rustc monomorphizes.
  - (b) The dat compiler monomorphizes and emits concrete code.
  - (c) Dyn erasure.
- **Evidence:**
  - (c) breaks spec 19's "specialized code … cost nothing at runtime".
  - (b) multiplies front-end work, and borrowck plus typeck are 59% of a clean debug build (2.5 of 4.27 s) [local].
  - Polymorphization is gone from rustc.
- **Recommendation:** (a). Add `cargo llvm-lines` reports to the M8 benchmark CI to catch runaway instantiation.
- **Cost:** low.
- **Milestone:** M5.
- **Spec change:** no; (c) would need one.

### D13. Module consts (spec 17)
- **Options:**
  - (a) `thread_local!` + `OnceCell` + dat's own re-entry flag (verified [local]).
  - (b) `static LazyLock`. Impossible with Rc-based values (needs `T: Sync + Send`) unless dat-rt switches to `Arc`. It also poisons permanently on panic.
  - (c) Compile-time evaluation of pure initializers into Rust constant data, plus (a) for the rest.
- **Evidence:** `OnceCell` retries after a panicking initializer; `LazyLock` poisons forever. `OnceCell` panics on re-entry; `OnceLock`'s behaviour on re-entry is unspecified (currently a deadlock). Under (a), parallel test threads each compute their own copy.
- **Recommendation:** (c) + (a). Have dat's guard throw `AssertionError` on re-entry with the dat location, so behaviour is deterministic in both modes.
- **Cost:** low for (a); medium for (c).
- **Milestone:** M6.
- **Spec change:** no, but there are two **questions for the owner**:
  1. After an initializer throws a runtime error, does the next access retry or rethrow the same error?
  2. Is "computed once" per program, or is once per thread acceptable? This is observable only when an initializer prints or otherwise has effects.

### D14. Source locations, stack traces and rustc errors
- **Options:**
  - Runtime error locations: (a) pass `&'static Loc` into every checked operation (verified [local]); (b) `#[track_caller]` (gives Rust locations, which are useless without `#line`).
  - Stack traces: (c) a shadow stack per dat call; (d) native backtrace plus a `.rs`→`.dat` line map; (e) (c) in test and dev builds, (d) in release.
  - rustc errors: (f) JSON diagnostics mapped through the line map.
- **Evidence:** Rust has no `#line` (pre-RFC closed in 2024). Nim offers both `#line` output and its own stack traces. Release inlining loses native frames.
- **Recommendation:** (a) + (f) from M1. For stack traces, choose (c) if traces must match the interpreter exactly, otherwise (e).
- **Cost:** low for (a)/(f); medium for (c); measure (c)'s per-call overhead in M4.
- **Milestone:** M1 for (a)/(f); M4 for traces.
- **Spec change:** none. **Question for the owner:** must the uncaught-error stack trace be identical between interpreted and compiled release runs, or only the error type, message and location? The conformance diff compares "stdout, exit, error type".

### D15. Generated-code style rules
- **Options:**
  - (a) Readable, idiomatic Rust (derives, macros, iterator chains).
  - (b) Compile-time-oriented Rust: no macros, hand-written impls where semantics differ, loops, `&dyn` at boundaries, one Rust function per dat function, `#[cold]` slow paths, write only changed files, crate-level lint allows.
- **Evidence:** macro expansion is ~15% of a 37k-line incremental rebuild (0.126 of 0.85 s) and is not incremental; derive costs +4% on clean builds; unchanged files rewritten trigger Cargo rebuilds; generated names trip `non_snake_case` [local].
- **Recommendation:** (b), with derives allowed where exactly right.
- **Cost:** low.
- **Milestone:** M1, and as each feature lands.
- **Spec change:** no.

### D16. macOS first-launch cost
- **Options:**
  - (a) Accept it.
  - (b) Document the Developer Tools setting.
  - (c) Link one test binary per project.
- **Evidence:** 0.15 s per freshly linked binary [local]. The Developer Tools exclusion is documented by nextest.
- **Recommendation:** (b) + (c).
- **Cost:** none.
- **Milestone:** M0/M6.
- **Spec change:** no.

### Questions for the owner from this area, none settled in code
1. **Narrowing:** does assigning a sibling field (`user.name`) end narrowing of `user.address` (spec 05, "any part of the path")?
2. **Module consts:** retry or rethrow after an initializer fails, and per-program or per-thread "once" (spec 17)?
3. **Stack traces:** must they be identical between interpreted and compiled release runs (spec 12, 20)?
4. **Closed world:** will packages or separately compiled code ever add interface implementors after compilation (spec README, "Packages … not yet specified")?
5. **Compiled budget:** confirm tiered budgets (D1) on named reference machines, now that compiled runs are exempt from the millisecond goals (owner's Q3 answer).
