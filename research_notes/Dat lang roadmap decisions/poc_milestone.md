# The first milestone as an end-to-end fibonacci proof of concept

Scope: how to design dat-lang's first milestone as a walking skeleton. One fibonacci program goes through every component: lexer, parser, checker, typed IR, interpreter, Rust codegen with rustc, the shared `dat-rt` crate, the differential conformance harness, diagnostics, the CLI and benchmarks. Researched 28 October 2026. Spec facts come from `reference/` as it stands today. Benchmark numbers marked "local measurement" were measured during this research on one machine (method in section 4). They are not published figures.

Owner decisions received during the research, and applied throughout:

1. Millisecond goals apply only to the interpreter path (`dat check`, `dat run`, `dat test`). Anything that goes through rustc gets its own, separate budget.
2. Stack overflow should be caught with a new `StackOverflowError` type if that can be done cheaply. Otherwise a crash is acceptable. The type is a spec addition still awaiting the owner's approval.
3. Out of memory is accepted as the one failure that can't be caught.
4. Agents get the terminal diagnostic format (source line plus inline fix). JSON is for tools such as `dat fix` and the LSP.

---

## 1. Walking skeletons and tracer bullets: how other language projects built their first end-to-end milestone

### Takeaway
The strongest precedents (Ghuloum's incremental compiler, Cockburn's and Freeman & Pryce's walking skeleton, the multi-backend test corpora in Zig and Roc) agree on one point: get a tiny language subset through every stage on day one, run it under an automated test driver that compares real program output, and then grow the subset one small step at a time. Carbon is the counterexample. Its separate "executable spec" interpreter fell behind the design and was archived. That supports dat's plan of splitting the two run modes only after a shared typed IR.

### Cited Findings
- Ghuloum's paper (2006 Scheme and Functional Programming Workshop, University of Chicago TR-2006-06) breaks compiler development into 24 incremental steps. "Every step yields a fully working compiler for a progressively expanding subset of Scheme." The first step compiles only fixnum integers: the compiler emits assembly that returns 42, and a small C runtime prints the result. — [Ghuloum, An Incremental Approach to Compiler Construction](http://scheme2006.cs.uchicago.edu/11-ghuloum.pdf)
- Ghuloum's methodology: pick a small subset, write as many test cases as needed to cover it, write a compiler that passes all of them, refactor without breaking any, then "enlarge the subset of the language in a very small step and repeat the cycle." He states: "A fully working compiler for the given subset of the language is available at every step in the development cycle starting from the first day of development." — [Ghuloum 2006](http://scheme2006.cs.uchicago.edu/11-ghuloum.pdf)
- Ghuloum's test infrastructure is a set of test cases, each a sample program with its expected output, for example `(test-case '(+ 10 15) "25")`. A test driver compiles each case, links it with a minimal runtime, runs the executable and compares its output with the expected string. Every earlier test is rerun at every step. — [Ghuloum 2006](http://scheme2006.cs.uchicago.edu/11-ghuloum.pdf)
- Ghuloum on why this matters: "The result of completing every step is a fully working compiler... the risk of not 'completing' the compiler is minimized." He contrasts it with phase-by-phase textbooks, where "only the last of which gives the sense of accomplishment" (quoting Appel: a student who implements all phases of Part I "will have a working compiler"). — [Ghuloum 2006](http://scheme2006.cs.uchicago.edu/11-ghuloum.pdf)
- Cockburn's walking skeleton: "a tiny implementation of the system that performs a small end-to-end function. It need not use the final architecture, but it should link together the main architectural components. The architecture and the functionality can then evolve in parallel." — [quoted in Blodgett, Start with a Walking Skeleton (2020)](https://www.mattblodgett.com/2020/09/start-with-walking-skeleton.html)
- Freeman & Pryce (*Growing Object-Oriented Software, Guided by Tests*): a walking skeleton is "an implementation of the thinnest possible slice of real functionality that we can automatically build, deploy, and test end-to-end." — [quoted in Blodgett 2020](https://www.mattblodgett.com/2020/09/start-with-walking-skeleton.html)
- Secondary write-ups separate the two ideas: a spike or proof of concept is throwaway code, while a walking skeleton is meant to evolve. Gojko Adzic has a related variant, "put it on crutches". — [search-surfaced summaries; e.g. Gojko Adzic 2014](https://gojko.net/2014/06/09/forget-the-walking-skeleton-put-it-on-crutches/) (I didn't verify the original wording)
- "Tracer bullets" is the Pragmatic Programmer's name for the same idea: code that goes end to end and is kept. — [The Pragmatic Programmer](https://pragprog.com/titles/tpp20/the-pragmatic-programmer-20th-anniversary-edition/) (book not re-read for this note)
- Crafting Interpreters builds a complete tree-walking interpreter (jlox) for the whole language before writing the bytecode VM (clox), and uses recursive `fib` as its motivating benchmark. Nystrom reports jlox takes about 72 seconds for `fib(40)` on his laptop, while equivalent C finishes in about half a second. — [Crafting Interpreters, "Chunks of Bytecode"](https://craftinginterpreters.com/chunks-of-bytecode.html)
- Carbon: "explorer", the prototype interpreter whose stated purpose was to act as a clear, executable specification, implemented an older version of the language design. It is no longer developed and has been archived. The project now develops a separate "toolchain" (compiler plus linker). — [Carbon explorer docs](https://docs.carbon-lang.dev/explorer); [Carbon roadmap](https://github.com/carbon-language/carbon-lang/blob/trunk/docs/project/roadmap.md)
- Zig runs one shared behaviour-test corpus across several backends (LLVM, C, wasm, arm, x86_64). Each test opts out of a backend that can't run it yet by checking `builtin.zig_backend` and returning `error.SkipZigTest`, and commits move tests out of skip lists as backends mature. — [Zig commit mirror (2022)](https://shale-1.onrender.com/zig/commit/496eb69273d363f21ca3923f00714b229882e42a) (mirror of ziglang/zig; I didn't check the current pass rate per backend)
- Roc keeps its code-generation tests in one crate, `test_gen`, shared by its backends (LLVM, the "dev" backend and wasm). An older gen_wasm README planned to "share tests between wasm and x64, with some way of saying which tests work on which backends." — [Roc crates README (mirror)](https://git.joshthomas.dev/language-servers/roc/raw/branch/import-builtins/crates/README.md); [older gen_wasm README, c. 2021–22](https://git.joshthomas.dev/language-servers/roc/raw/commit/72194b87dfa27d83d49d1dfcb07830a45ab75f04/compiler/gen_wasm/README.md)
- The current draft roadmap's M0 is "foundations and spikes" with a Hello World exit check. The six-step "definition of done" says no feature may be interpreter-only for a while with codegen promised later. — draft roadmap artifact (local file named in the assignment)

### Inferences
- Fibonacci fits Ghuloum's "first step" better than Hello World. Hello World touches only `print` and string literals. Fibonacci forces calls, recursion, locals, loops, checked integer arithmetic, a catchable runtime error and `test` blocks, which is enough to prove that the typed IR, the shared runtime and the error model work in both modes. That's still small: about 15 grammar productions (section 5).
- Carbon's archived explorer suggests a second, independently written implementation of the semantics drifts once the design moves. dat's two modes are two implementations of execution. Keeping everything before the typed IR, plus every observable built-in, in shared code (`dat-rt`) is the mitigation, and the PoC should enforce it structurally (section 6).
- Zig and Roc both show that one test corpus run against every backend, with explicit and visible skip markers, is how projects keep backends honest while they grow. dat's equivalent: every conformance case runs in both modes, and "unsupported in this milestone" is a reported status, never a silent skip.
- The walking-skeleton literature says to keep the skeleton and grow it. So the fib PoC components should be real code with a tiny scope, not throwaway spikes. Spikes stay throwaway only for questions fib doesn't touch, such as copy-on-write lists.

### Gaps
- I found no first-hand post-mortems from Roc, Gleam or Hylo about what their first end-to-end milestone was, or what they wish they had proven earlier. The Roc material is from repo mirrors and dates from 2021–22. Gleam and Hylo weren't researched in depth.
- I didn't verify Zig's bootstrap stages (stage1 / stage2 / zig1.wasm). They aren't needed for the recommendation.
- I found no direct "we wish we had done X earlier" quotes from compiler authors. The lessons above are inferred from Ghuloum's rationale and Carbon's outcome.

---

## 2. Spec-as-tests: tools that run Markdown code blocks as tests, and an annotation format for dat's reference

### Takeaway
The closest precedent is Astral's mdtest, used by ty. Markdown sections are tests, fenced blocks are files, inline `# error: [code] "text"` comments are assertions, and HTML comments carry invisible directives. rustc's UI tests add two ideas worth copying: `.stderr` snapshots blessed with `--bless`, and `run-rustfix`, which proves that suggested fixes really apply. dat's reference already has a usable inline convention (`// ❌ Error: <message>`, `// throws <Type>[: <message>]`, `print(x); // <output>`). But its 158 blocks are illustrative fragments, not runnable files. The runner needs classification, hidden setup code and per-line splitting. For the PoC, the extractor can infer all of this without editing the spec.

### Cited Findings: tools
- **mdtest (ty, in the ruff repo).** Any Markdown file can be a test suite. Markdown headers separate tests, each fenced block tagged `py`/`pyi`/`toml` is an embedded file, and an untagged path defaults to `/src/mdtest_snippet.py`. Several unnamed blocks in one test are merged in document order. — [ty_test README](https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md)
- mdtest assertion syntax: `# error: [invalid-assignment]` (rule code in square brackets), `# error: "Some text"` (message substring in double quotes), `# error: 8 [invalid-assignment] "Some text"` (1-indexed column first), and `reveal_type(x)  # revealed: Literal[1]`. A trailing comment applies to its own line, and a comment on its own line applies to the next non-assertion line. A test passes only if every diagnostic and every assertion is matched. — [ty_test README](https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md)
- mdtest directives in invisible HTML comments: `<!-- expect-panic: <substring> -->` and `<!-- snapshot-diagnostics -->`, plus inline `# snapshot` that writes rendered diagnostics into a `snapshot` block under `MDTEST_UPDATE_SNAPSHOTS=1`. Filtering uses `MDTEST_TEST_FILTER`, and TOML blocks configure a section and its subsections. — [ty_test README](https://github.com/astral-sh/ruff/blob/main/crates/ty_test/README.md)
- **rustc UI tests (compiletest).** `//~ ERROR <message>` attaches to its own line, `//~^` to the line above (one `^` per line), `//~v` to the line below, `//~|` to the same line as the previous annotation, and `//~?` to diagnostics without a line. By default ERROR and WARN diagnostics must be fully annotated. Expected compiler output lives in adjacent `.stderr`/`.stdout` files generated with `--bless` and reviewed by hand. — [rustc-dev-guide, UI tests](https://rustc-dev-guide.rust-lang.org/tests/ui.html)
- compiletest modes: `check-fail` (the default), `check-pass`, `build-pass`, `run-pass` (exit code 0), `run-fail` (exit code 1..=127) and `run-crash`. Run output is compared only with `check-run-results` (`.run.stdout` / `.run.stderr`). Output normalization replaces the test directory with `$DIR`, turns CRLF into LF, and supports custom regex rules (`normalize-stderr`). `revisions` runs one file under several configurations, each with its own `.stderr`. — [rustc-dev-guide, UI tests](https://rustc-dev-guide.rust-lang.org/tests/ui.html)
- `//@ run-rustfix` applies the compiler's suggestions, compares the result with a `.fixed` file and requires the fixed source to compile. By default it applies all suggestions; `rustfix-only-machine-applicable` restricts it to machine-applicable ones. — [rustc-dev-guide, UI tests](https://rustc-dev-guide.rust-lang.org/tests/ui.html)
- **Rust doctests.** Code-fence attributes classify blocks: `ignore`, `should_panic`, `no_run`, `compile_fail`, `edition20xx`, `standalone_crate`, `test_harness`. Lines starting with `# ` are hidden from the rendered docs but compiled, and an example without `fn main` is wrapped in `fn main() { ... }`. From the 2024 edition, compatible doctests are merged into one crate "for performance reasons: the slowest part of doctests is to compile them." — [rustdoc book, documentation tests](https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html) (the fetched page doesn't document the `compile_fail,E0XXX` error-code form)
- **trybuild.** `TestCases::new().compile_fail("tests/ui/*.rs")` requires each file to fail compilation and compares the output with an adjacent `*.stderr`. A missing `.stderr` file is written to `wip/`, and `TRYBUILD=overwrite` writes in place. — [trybuild README](https://github.com/dtolnay/trybuild)
- **insta.** A snapshot-testing tool for Rust with globbing, which can create one snapshot per input file, and a `cargo insta` review tool. — [insta docs](https://insta.rs/docs/) (the macro details are on sub-pages I didn't fetch)
- **LLVM lit.** It finds suites through `lit.cfg`, substitutes `%s` (the test's source path) and `%t` (a per-test temp path), and reports PASS, FAIL, XFAIL (failed as expected), XPASS (passed unexpectedly, which counts as a failure), UNSUPPORTED, UNRESOLVED and TIMEOUT. `XFAIL`, `REQUIRES` and `UNSUPPORTED` directives gate tests by feature. — [lit command guide](https://llvm.org/docs/CommandGuide/lit.html). FileCheck matches ordered `CHECK:` patterns against tool output. — [FileCheck docs](https://llvm.org/docs/CommandGuide/FileCheck.html)
- **Python doctest** runs `>>>` examples embedded in docstrings and compares printed output; expected exceptions are written as a traceback header followed by the exception line. — [Python doctest docs](https://docs.python.org/3/library/doctest.html). **Elixir doctests** (`iex>` prompts) can show an expected exception as `** (RuntimeError) message`. — [ExUnit.DocTest](https://hexdocs.pm/ex_unit/ExUnit.DocTest.html)
- **test262** (the ECMAScript conformance suite) puts expected-error metadata in YAML front matter: `negative: { phase: parse | resolution | runtime, type: SyntaxError }`. This separates compile-time from run-time expectations in machine-readable form. — [test262 INTERPRETING.md](https://github.com/tc39/test262/blob/main/INTERPRETING.md)

### Cited Findings: dat's reference conventions (counted by script over `reference/*.md`)
- README: "Code examples are fragments of a function body, unless they declare top-level items such as functions, structs or tests. `// ❌ Error:` marks a compile error, and `// throws RangeError` marks an error thrown at runtime." — [reference/README.md](../../reference/README.md)
- There are 158 fenced blocks, all with an empty info string (no language tag). 88 are function-body fragments and 70 contain top-level items. — local script over [reference/](../../reference/)
- There are 37 `// ❌ Error` lines in 26 blocks. Two have no message: `admin.name = "Kai"; // ❌ Error` in [02-variable-declaration.md](../../reference/02-variable-declaration.md). One message continues on a second comment line (`tags.push("admin");`, [02-variable-declaration.md](../../reference/02-variable-declaration.md) lines 78–79). Several messages already include the fix after a semicolon: "condition must be bool; write `user != null`" ([07-control-flow.md](../../reference/07-control-flow.md)), and "can't infer the element type; write `let items: list<int> = [];`" ([02-variable-declaration.md](../../reference/02-variable-declaration.md)).
- There are 7 blocks with `// throws` comments. Two forms appear: with a message (`print(colors[5]); // throws RangeError: index 5 out of range for length 3`, [10-list-methods.md](../../reference/10-list-methods.md)) and conditional prose (`// throws TypeError if there is no admin`, [05-type-narrowing.md](../../reference/05-type-narrowing.md)).
- Around 51 blocks carry output or value comments (`print(entry[0]); // alice`), and 9 contain bare expression lines whose comment is the value in literal form (`10 / 4 // 2.5`, `"a-b-c".replace("-", "+") // "a+b-c"`). Some comments add prose: `10 / 5 // 2.0, a float` ([03-operators.md](../../reference/03-operators.md)).
- 8 blocks declare the same name twice to show alternatives side by side (for example `let email` three times in [02-variable-declaration.md](../../reference/02-variable-declaration.md), and `const name = user.name; // ❌` followed by `const name = user?.name;` in [07-control-flow.md](../../reference/07-control-flow.md)). Compiled as one unit, each would also trip "x is already declared" ([14-scope.md](../../reference/14-scope.md)).
- 8 blocks use `{ ... }` placeholder bodies (for example `function fetchUser(id: int): User throws HttpError { ... }`). Many fragments use names defined nowhere in the block (`user`, `items`, `env`, `isValid()`). 9 blocks start with a file-name comment (`// app/main.dat`), mostly in [17-imports.md](../../reference/17-imports.md). One block is shell commands, and two are not code (the keyword list and the `FAIL` output sample).
- Spec inconsistency found: in [20-testing.md](../../reference/20-testing.md), the failure sample reads `FAIL math.dat:9 ... assert(clamp(-3, 0, 10) == 0) left: -3`. In the example file, line 9 is the `clamp(42, ...)` assertion, the `-3` assertion is on line 8, and the `clamp` shown is correct, so that assertion wouldn't fail.

### Inferences: proposed annotation format the runner can parse (no spec edits needed for the PoC)
The runner should treat the reference's existing comments as the annotation language, add a classification step, and put anything the prose can't express in a sidecar file:

| Spec text today | Runner meaning | Check |
| --- | --- | --- |
| `// ❌ Error: <msg>` (plus `//    <more>` continuation lines) | Expected compile error on that line | The diagnostic's one-line form (`message`, or `message; fix.title`) equals `<msg>`. A bare `// ❌ Error` matches any error on that line. |
| `// ✅ ...` | Line must produce no diagnostic | No diagnostics on that line |
| `// throws <Type>` / `// throws <Type>: <msg>` | Running this statement throws that runtime error | The runner wraps the line in `try { … } catch (e) { … }` and compares the type and, if given, `e.message`. Each throwing line runs as its own case. |
| `// throws <Type> if …` | Prose, conditional | Documentation only; reported as unchecked |
| `print(...); // <text>` | Expected stdout line(s) | Exact stdout comparison in both modes |
| `<expr> // <dat literal>[, prose]` | Expected value | Becomes `assert((<expr>) == (<literal>))`; prose after a comma is ignored |
| Block with `// file.dat` first line | Multi-file case | Files are grouped in document order (like mdtest's named files) |
| `{ ... }` bodies, shell blocks, output samples, keyword lists | Not code | Classified `doc-only` |

Two mechanisms cover what the prose can't express:
- **Per-line splitting** for blocks that redeclare names or hold several ❌ lines. Each ❌ line is checked in a variant of the block with the other ❌ lines removed. Redeclared ✅ alternatives are split into separate variants. compiletest keeps one error-annotated file per test. dat's spec, though, is written for readers, so splitting must happen in the runner.
- **Hidden setup ("preludes")** for fragments that use undefined names. For the PoC, keep them in a sidecar (for example `conformance/preludes.toml`) keyed by spec file plus a hash of the block text, so an edit to the spec block invalidates the prelude loudly. Later, with the owner's approval, move them into the spec as invisible `<!-- dat: prelude=… -->` comments (the mdtest precedent), or tag fences ```` ```dat ```` / ```` ```dat ignore ```` (the rustdoc precedent).

Every extracted case gets one status per mode: `pass`, `fail`, `unsupported` (uses a feature outside the current milestone), `doc-only`, or `xfail` (a known bug, listed in a checked-in expectations file the way lit and test262 consumers do). CI fails on `fail` and on `xpass`. The `pass` count is a ratchet that can only grow.

### Gaps
- I didn't verify insta's exact macro and CLI names (`assert_snapshot!`, `cargo insta review`) on the fetched page; they are on insta.rs sub-pages.
- The rustdoc page I fetched doesn't document `compile_fail,E0XXX` (the error-code check).
- How many of the 158 blocks are runnable after preludes isn't known until someone writes the preludes. My heuristic scan found about a dozen blocks fully inside the fib subset (section 5).

---

## 3. Differential testing between two backends: harness design, generators and test-case reduction

### Takeaway
Differential testing finds wrong-code bugs only when the program has a single defined behaviour. Csmith's key design rule was to generate programs free of undefined and unspecified behaviour, so for dat every observable behaviour must be fully specified (evaluation order, error messages, number formatting). For the PoC, a hand-written corpus plus the extracted spec blocks is enough. The harness's interface should still take generated programs later: a wasm-smith-style generator driven by `arbitrary`, with test-case reduction by treereduce once dat has a tree-sitter grammar.

### Cited Findings
- Csmith (Yang, Chen, Eide, Regehr, PLDI 2011) "generates programs that cover a large subset of C while avoiding the undefined and unspecified behaviors that would destroy its ability to automatically find wrong-code bugs." It found "more than 325 previously unknown bugs", later 344, and "every compiler we tested was found to crash and also to silently generate wrong code when presented with valid input." — [Regehr blog](https://blog.regehr.org/?p=492); [PLDI'11 preprint](https://www-old.cs.utah.edu/~regehr/papers/pldi11-preprint.pdf) (2011, older but foundational)
- YARPGen (Livinskii, Babokin, Regehr, OOPSLA 2020, Distinguished Paper) found 83 new bugs in GCC, 62 in LLVM and 76 in the Intel C++ compiler. The project README now claims more than 260 bugs across gcc, clang, ispc, dpc++ and others. — [YARPGen paper](https://users.cs.utah.edu/~regehr/yarpgen-oopsla20.pdf); [intel/yarpgen](https://github.com/intel/yarpgen)
- Wasmtime's fuzzing crate has differential "oracles": one evaluates an exported function with the WebAssembly spec reference interpreter (`diff_spec`), others with wasmi (`diff_wasmi`) and Wasmtime itself (`diff_wasmtime`). A helper runs the same function in two engine instances (lhs and rhs) and compares the results. The wasmi wrapper switches off proposals that not every engine supports (threads, exceptions, GC), so only the shared feature set is compared. — [wasmtime_fuzzing oracles](https://docs.wasmtime.dev/api/wasmtime_fuzzing/oracles/index.html); [diff_spec.rs](https://docs.wasmtime.dev/api/src/wasmtime_fuzzing/oracles/diff_spec.rs.html); [diff_wasmi.rs](https://docs.wasmtime.dev/api/src/wasmtime_fuzzing/oracles/diff_wasmi.rs.html)
- wasm-smith generates valid WebAssembly modules from fuzzer bytes through the `arbitrary` crate. It is the generator behind Wasmtime's fuzz targets. — [wasm-smith docs](https://docs.rs/wasm-smith)
- C-Reduce (PLDI 2012) is the standard reducer for compiler-bug test cases. C-Vise is a parallel Python reimplementation, and a 2023 Rust internals thread discusses using it on Rust code. — [C-Reduce paper](https://users.cs.utah.edu/~regehr/papers/pldi12-preprint.pdf); [C-Vise on Rust internals](https://internals.rust-lang.org/t/rust-support-in-parallel-reduction-tool-called-c-vise/19668)
- treereduce is "a fast, parallel, syntax-aware test case reducer based on tree-sitter grammars". It works for any language that has a tree-sitter grammar, and its author reports dozens of bugs found in Clang, Deno and rustc. — [treereduce on docs.rs](https://docs.rs/treereduce); [author page](https://oosmetrics.com/u/langston-barrett)
- Rust evaluates the operands of binary operators left to right, so generated Rust preserves source order. — [Rust reference, evaluation order of operands](https://doc.rust-lang.org/reference/expressions.html#evaluation-order-of-operands)
- dat's spec already demands the differential property for tests: "CI runs every test both on the interpreter and compiled, and fails if the two results differ." — [20-testing.md](../../reference/20-testing.md)

### Inferences: harness design for the PoC
- **What one run produces:** `Outcome { stdout: bytes, stderr: normalized text, exit: i32, uncaught: Option<(type, message, dat_trace)>, tests: [(name, Pass | Fail(rendered))] }`. Compare the interpreter's Outcome with the compiled Outcome field by field, then compare both with the golden file. Matching one golden in both modes implies the modes match each other, and the golden also catches the case where both modes are wrong in the same way.
- **Normalization:** drop absolute paths (compiletest's `$DIR` idea) and any Rust frames. The spec wants errors shown "with its message, its source location and a stack trace" ([12-errors.md](../../reference/12-errors.md)), so the dat trace itself must be identical in both modes.
- **Harness self-test (mutation check):** a hidden `DAT_FAULT=interp-plus-one` switch makes the interpreter compute `+` wrong. CI checks that the harness then reports a difference. A differential harness that never fails proves nothing.
- **Things the spec must define for differential testing to be meaningful (the Csmith lesson):** operand evaluation order (not stated in `reference/`, which only states associativity, [03-operators.md](../../reference/03-operators.md)), the text of the overflow `RangeError` message, the uncaught-error output format and stream, and the stack-overflow behaviour. These are listed as owner questions under the Decisions heading.
- **Generators and reducers belong in later milestones,** once collections and structs exist. A type-directed dat generator driven by `arbitrary` (wasm-smith style) generates only well-typed programs, the interpreter is the oracle, and the compiled binary is the subject. treereduce needs a tree-sitter grammar for dat, which editors want anyway.

### Gaps
- I didn't open the C-Reduce paper, so the notes contain no reduction statistics.
- I didn't look up how Cranelift's own `differential` fuzz target is scheduled in CI (OSS-Fuzz versus local).

---

## 4. Benchmarking tiny programs fairly, and what speed ratios the PoC should demand

### Takeaway
Measure process start-up separately from run time, and use `fib(35)` (about 30 M calls), not `fib(30)`, for the compiled ratio. A compiled `fib(30)` finishes in about 1.5 ms of compute, close to the 1.7 ms process start. Compare generated Rust with hand-written Rust that also has `overflow-checks` on. Without checks, LLVM turns one of fib's two recursive calls into a loop, so the unchecked baseline is unfairly fast. In local measurements, generated-style Rust with unwinding runtime errors came out at 1.0× checked hand-written Rust. A Result-based design came out at 1.6×, and the stack-pointer depth guard added 0–4%. A simple slot-resolved tree-walking interpreter ran `fib(30)` in about 100 ms, about 1.7× CPython 3.15 and 3× Lua 5.5 on the same machine.

### Cited Findings: tools
- hyperfine: `--warmup N`, `--runs N` (at least 10 runs by default), `--prepare` (a command before each run), and `-N` / `--shell=none`. The current README describes running commands directly as the default, which "avoids shell startup overhead and the noise from correcting for it, especially for fast commands (< 5 ms)". With a shell enabled, hyperfine measures the empty-shell start-up and subtracts it. It also offers outlier detection, a relative comparison against the first command, `--export-json`/`-markdown`/`-csv`, and `time_wall_clock` and `memory_peak_resident` as default metrics. — [hyperfine README](https://github.com/sharkdp/hyperfine) (my recollection is that older versions defaulted to spawning a shell; check the installed version's `--help`)
- Gungraun, the renamed successor of iai-callgrind (itself a fork of Brook Heisler's iai), measures instruction counts and cache behaviour under Valgrind (Callgrind, Cachegrind, DHAT) "to provide extremely accurate and consistent measurements". Bencher's guide notes that such metrics "stay nearly identical between runs, allowing single-shot benchmarking". The Rust `compiler-builtins` crate adopted it for CI. — [gungraun docs.rs](https://docs.rs/crate/gungraun/); [Bencher iai guide](https://bencher.dev/learn/benchmarking/rust/iai); [compiler-builtins bench-icount.sh](https://rust.googlesource.com/rust/+/HEAD/library/compiler-builtins/ci/bench-icount.sh)
- criterion.rs (statistics-driven in-process micro-benchmarks with saved baselines) and divan (`#[divan::bench]` attribute benchmarks) measure inside one process, so they exclude process start-up. — [criterion.rs](https://github.com/bheisler/criterion.rs); [divan](https://github.com/nvzqz/divan)
- Overflow-check cost: I found no rigorous published benchmark. Anecdotes range from "barely measurable" to more than 10× when the checks block auto-vectorisation of array loops (LWN discussion). Enabling `overflow-checks` also raises compile time and memory (rust-lang issue). Overflow checks are off by default in Rust release builds. — [LWN thread](https://lwn.net/Articles/960822/); [rust#117295](https://github.com/rust-lang/rust/issues/117295); [arXiv 1909.06344](https://arxiv.org/pdf/1909.06344)
- Published cross-language fib numbers are scattered and old. The GopherLua wiki reports `fib(35)` at 1.71 s on Lua 5.1.4 and 5.84 s on Python 3.4, on an old machine and old versions. — [GopherLua benchmarks](https://github.com/yuin/gopher-lua/wiki/Benchmarks). Crafting Interpreters: jlox `fib(40)` about 72 s against C at about 0.5 s. — [craftinginterpreters.com](https://craftinginterpreters.com/chunks-of-bytecode.html)
- `catch_unwind` "only catches unwinding panics, not those that abort the process", and the docs say it is not recommended "for a general try/catch mechanism" (they point to `Result` for functions that fail regularly). — [std::panic::catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)
- In JavaScript, V8 reports stack exhaustion as a catchable `RangeError: Maximum call stack size exceeded`, and Firefox as `InternalError: too much recursion`. — [MDN, Too much recursion](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Errors/Too_much_recursion)

### Cited Findings: local measurements (this research)
Method: Apple M4 Pro (14 cores), macOS Darwin 24.6. Toolchains: rustc 1.99.0 (2026-09-28) with `-C opt-level=3 -C codegen-units=1`, Python 3.15.1, Node 26.0.0, Bun 1.3.14, Lua 5.5.1. Each command ran as a separate process, timed by a Python `subprocess` + `perf_counter` harness (hyperfine wasn't installed). The first run is discarded and the minimum of the remaining runs is reported (3–9 runs for `fib(35)`, 7 for `fib(30)`, 20 for start-up). All numbers are wall time including process start. Every program uses the same recursive `if n < 2 return n; return fib(n-1)+fib(n-2)`. The "tree-walker" programs are a roughly 100-line Rust AST interpreter written for this note. It has no parser and a hand-built AST, with checked i64 arithmetic and `Result`-based errors. In the "naive" variant, names are strings, each call builds a new `HashMap` environment, and functions are looked up by name. In the "resolved" variant, locals live in slots on a reused `Vec` stack and functions are indexed. These are single-machine, small-sample numbers, good for orders of magnitude only.

| Program | start-up only | `fib(30)` | `fib(35)` | `fib(35)` vs checked hand Rust |
| --- | --- | --- | --- | --- |
| Rust, empty `main` | 1.7 ms | | | |
| Rust hand-written, wrapping `+` (release default) | | 3.2 ms | 16.9–19.5 ms | 0.75–0.85× |
| Rust hand-written, `-C overflow-checks=on` | | 3.5 ms | 22.3–22.9 ms | **1.0× (baseline)** |
| Generated style: `checked_add` + `panic_any(RangeError)` / `catch_unwind` in `main` | | 3.3 ms | 21.8–24.5 ms | 0.98–1.07× |
| Same + stack-pointer depth guard (compare SP with a global limit) | | — | 22.7–23.0 ms | 1.0–1.03× |
| Generated style: `checked_add` → `Result<i64, RangeError>` + `?` everywhere | | 4.9 ms | 35.0–36.8 ms | ~1.6× |
| Same as unwind + thread-local call-depth counter (inc/dec through a `Drop` guard) | | 5.5 ms | 45.3–47.0 ms | ~2.0× |
| Same as unwind + global atomic call-depth counter | | — | 73.9 ms | ~3.3× |
| Tree-walker, resolved slots (Rust) | | 100.6 ms | 1124 ms | ~50× |
| Tree-walker, naive HashMap env (Rust) | | 321.9 ms | 3567 ms | ~160× |
| Lua 5.5.1 | 2.1 ms | 31.9 ms | 321.6 ms | |
| CPython 3.15.1 | 11.6 ms | 59.7 ms | 532.9 ms | |
| Node 26.0.0 | 38.8 ms | 49.1 ms | 94.7 ms | |
| Bun 1.3.14 | 2.9 ms | 15.8 ms | 46.0 ms | |

Further local observations:
- Looking at the emitted assembly, unchecked hand-written fib keeps 1 recursive call site (LLVM rewrites the other as an accumulator loop). The `overflow-checks=on` build keeps 2. This is why the baseline must be checked hand-written Rust.
- Deep recursion with no guard (`sum(n) = n + sum(n-1)`, n = 10⁸): Rust prints "thread 'main' has overflowed its stack / fatal runtime error: stack overflow, aborting", and the process exits with status 134 (SIGABRT), which nothing can catch. The stack-pointer-guarded version throws a typed panic payload that `catch_unwind` catches, and the program exits normally. At n = 10⁵ the guard cost was within noise (2.5 ms vs 2.4 ms).
- Compile times for the compiled path (single-file program, tiny runtime crate): `rustc` on the single file took 78 ms at opt-level 0 and 103 ms at opt-level 3; an empty file takes 59 ms. With a separate `dat_rt` crate: `cargo build --release` cold (runtime and program) 248 ms, no-op 16 ms, rebuild after editing the program 122–135 ms (92 ms in debug, warm). Direct `rustc --extern dat_rt=<prebuilt rlib>` took 74 ms at opt-level 3 and 68 ms at opt-level 0.

### Inferences
- **Compiled ratio:** demand ≤ 1.2× checked hand-written Rust on `fib(35)` at the PoC, measured with the same profile (`opt-level=3`, `codegen-units=1`, `overflow-checks=on`, `panic=unwind`). The unwind design measured about 1.0×, so 1.2× leaves room for real-world overheads such as a dat-rt call boundary and the depth guard, while still catching a Result-everywhere design (1.6×) or a TLS counter guard (2×). Also report the ratio to unchecked Rust, for information only.
- **Interpreter ratio:** a gate in absolute milliseconds is meaningless across CI runners. Gate on a ratio measured in the same job: interpreted `fib(30)` ≤ 2× `python3 fib.py 30` at the PoC. The resolved toy walker was at about 1.7×; the naive one at about 5.4× would fail, and should. As an M1 stretch target, aim for ≤ 1× CPython (Lua-class speed, roughly a typed-IR walker with closure compilation or a register bytecode). A typed IR allows a faster interpreter than any of these dynamically tagged toys, because the static types remove tag checks.
- **Start-up:** a Rust binary starts in about 1.7 ms here. Interpreter budgets of "≤ 5 ms to the first statement" (the roadmap's M1 number) and "`dat check fib.dat` ≤ 10 ms" are therefore plausible. Compute-heavy benchmarks should subtract start-up or use `fib(35)`.
- **CI noise:** use hyperfine wall-clock numbers (`-N --warmup 3 --runs 20`) for budgets on shared runners, with hard-fail thresholds set at about 2× the budget, and track the trend. Use Gungraun instruction counts on the Linux runner for precise regression detection. Gungraun is Valgrind-based, so in my understanding it is Linux-only for this purpose (not verified for macOS on arm64).
- **Stack guard cost (owner decision 2):** the stack-pointer check is cheap (0–4% on the worst-case call-heavy benchmark), so a catchable stack overflow is affordable in compiled code. Counter-based guards measured 2–3.3× on macOS. Counters do give a depth that is identical across modes, which a stack-byte limit doesn't.

### Gaps
- No Linux measurements. Thread-local access is much cheaper on Linux (initial-exec TLS) than on macOS, so the TLS-counter cost may be far lower there. That needs a measurement on the CI runner.
- No measurement of a real dat interpreter. The toy walkers bound the range but aren't dat.
- hyperfine wasn't available, so these numbers come from a simple harness without outlier analysis.
- No rigorous published study of Rust overflow-check overhead was found.

---

## 5. The fibonacci PoC, defined concretely for dat

### Takeaway
Every construct in the program below is in the spec today. It needs about a dozen statement and expression forms, and it exercises a declared-style `throw` of a runtime error, a dat-rt-raised overflow `RangeError` caught in `main`, four `test` blocks with `assert` and `fail`, `print` with several values, and template strings. `fibIter(92)` = 7,540,113,804,746,346,429 is the largest Fibonacci number that fits in dat's 64-bit `int`. `fibIter(93)` = 12,200,160,415,121,876,738 overflows. The loop must not compute one step ahead, or `fibIter(92)` itself would overflow. Recursive `fib(93)` is infeasible (≈ 3.9 × 10¹⁹ calls), so the overflow comes from the iterative version. About eight behaviours the PoC needs are not specified, and are listed below as owner questions.

### Cited Findings: spec facts the PoC relies on
- `int` is a "64-bit signed integer. Overflow throws a `RangeError`". `9223372036854775807 + 1` doesn't wrap. — [01-builtin-types.md](../../reference/01-builtin-types.md); [03-operators.md](../../reference/03-operators.md)
- Functions are declared at top level with required parameter types. A return type may be inferred, except on exports. `main` is the entry point. "If an error escapes it, declared or runtime, the program prints the error and exits with status 1." — [06-functions.md](../../reference/06-functions.md)
- `for (let i = 0; i < 10; i++)`, `if`/`else` with mandatory braces and a `bool` condition, `++` as a postfix statement only, and assignments as statements. — [07-control-flow.md](../../reference/07-control-flow.md); [03-operators.md](../../reference/03-operators.md)
- `print(...values)` joins `toString()` results with spaces. `assert(cond, message?)` shows both sides of a failed comparison. `fail(message): never` throws `AssertionError`. — [09-builtin-functions.md](../../reference/09-builtin-functions.md)
- Template strings call `toString()` on embedded values. — [01-builtin-types.md](../../reference/01-builtin-types.md)
- Runtime errors need no `throws` or `try`. They are caught by `try`/`catch` "anywhere in the block, including inside the functions it calls". User code may `throw RangeError { message: … }` without `throws`. Built-in errors are structs with a `message: string` field, and they print as `RangeError: <message>`. `catch (e)` with no type catches every remaining error. — [12-errors.md](../../reference/12-errors.md)
- `test "name" { … }` at module level behaves like a `throws Error` function. A runtime error that escapes fails the test. `dat test file.dat` and `dat test --compiled` both exist, and the failure format is specified (`FAIL <file>:<line>  <name>`, the condition, `left:`/`right:`). — [20-testing.md](../../reference/20-testing.md)
- `dat run` is mentioned once: "The file passed to `dat run` must define a `main` function." `dat check` and `dat build` don't appear in the spec. — [17-imports.md](../../reference/17-imports.md)
- A string plus an int is a compile error with a specified message: `"Total: " + 42 // ❌ Error: can't add string and int`. The spec's fixes are `` `Total: ${42}` `` or `42.toString()`. — [03-operators.md](../../reference/03-operators.md)

### Minimal spec subset (the PoC grammar)
- Types: `int`, `bool`, `string`, `void`, `never`, and the built-in error structs `RangeError`, `AssertionError` and `Error`, with field access `e.message`.
- Literals: decimal `int` (with `_` separators), negation through unary `-`, `"…"` strings with the escapes table, template strings with `${expr}`, `true`/`false`.
- Expressions: `+ -` (checked), `< <= == !=` on `int`; `==`/`!=` on `string`; unary `-`; calls; field access; the struct literal `RangeError { message: … }` (only for built-in error structs at this milestone); precedence levels 1–6 of the table.
- Statements: `let`, `const`, `=`, `i++`, `if`/`else`, C-style `for`, `return`, expression statements, `throw`, `try { } catch (e: T) { }` and `catch (e)`.
- Items: `function` (with typed parameters and an explicit return type), `main`, `test`.
- Built-ins: `print`, `assert` (with left/right reporting for comparisons), `fail`, and `toString` for `int`/`bool`/`string`.
- Checker rules: name resolution with block scope and shadowing, the duplicate-declaration error, a `bool` condition, argument and return types, missing return (per path, in its simple form), and "can't add string and int" with a fix. Anything else gets a clean "not supported yet in this milestone" diagnostic, never a crash.
- A heuristic scan over `reference/` finds about a dozen of the 158 blocks entirely inside this subset. Examples: template strings in [01](../../reference/01-builtin-types.md), `++`/`--` with two ❌ lines in [03](../../reference/03-operators.md), `add`/`greet`, missing return and `main` in [06](../../reference/06-functions.md), `if`/`for` in [07](../../reference/07-control-flow.md), and all three blocks in [14-scope.md](../../reference/14-scope.md).

### Draft PoC program: `examples/fib/fib.dat`
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
Expected `dat run examples/fib/fib.dat` stdout, identical in both modes, exit status 0:
```
fib(25) = 75025
fibIter(92) = 7540113804746346429
fibIter(93) overflows int: caught RangeError
```
Notes on spec accuracy:
- `catch (e: RangeError)` is legal because the rule "a `catch` clause for a declared error type that nothing in the block can throw is a compile error" covers only declared types, and `RangeError` is a runtime type ([12-errors.md](../../reference/12-errors.md)).
- The test bodies use `const f = …; fail(…)` instead of a bare `fibIter(93);` statement, because the spec doesn't say whether discarding a non-`void` result is allowed.
- The overflow test checks only that the message is non-empty, because the spec doesn't define the overflow message.

### Companion programs
`examples/fib/bad_fib.dat`: the compile error with an exact fix (line 10, columns 9–30):
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
Terminal rendering. This is the form agents read (owner decision 4); the error code is a placeholder:
```
error[E0101]: can't add string and int
  --> bad_fib.dat:10:9
   |
10 |   print("fib(10) = " + fib(10));
   |         ^^^^^^^^^^^^^^^^^^^^^^ `fib(10)` is an int
   |
fix: use a template string
10 |   print(`fib(10) = ${fib(10)}`);
```
`--json` rendering, for `dat fix`, the LSP and the harness:
```json
{ "code": "E0101", "severity": "error", "file": "bad_fib.dat",
  "span": { "line": 10, "col": 9, "endLine": 10, "endCol": 31 },
  "message": "can't add string and int",
  "fix": { "title": "use a template string", "applicability": "always",
           "edits": [{ "span": { "line": 10, "col": 9, "endLine": 10, "endCol": 31 },
                       "text": "`fib(10) = ${fib(10)}`" }] } }
```
The harness applies the edits, rustfix-style. It compares the result with `bad_fib.dat.fixed`, checks that the fixed file has no diagnostics, and checks that it prints `fib(10) = 55` in both modes.

`examples/fib/overflow_uncaught.dat`: an error escapes `main`, so the exit status is 1 in both modes:
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
Proposed stderr. The format is unspecified (owner question). The operands at the overflow are F(91) and F(92).
```
RangeError: int overflow: 4660046610375530309 + 7540113804746346429
  at fibIter (overflow_uncaught.dat:9:18)
  at main (overflow_uncaught.dat:17:9)
```
`examples/fib/fib_fail.dat`: a deliberately failing test. The format is the one in [20-testing.md](../../reference/20-testing.md):
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
Expected in both modes:
```
FAIL fib_fail.dat:9  deliberately wrong
  assert(fib(10) == 56)
    left:  55
    right: 56
```
`examples/fib/deep.dat`: the stack-guard measurement case for owner decision 2. It uses `catch (e)`, which is legal today whatever the new type ends up being called:
```
function depth(n: int): int {
  if (n == 0) {
    return 0;
  }
  return 1 + depth(n - 1);
}

function main(): void {
  print(depth(10_000));
  try {
    print(depth(100_000_000));
    print("not reached");
  } catch (e) {
    print("deep recursion caught");
  }
}
```
With a guard: `10000`, then `deep recursion caught`, exit status 0 in both modes. Without one: the compiled binary aborts with status 134 on macOS (local measurement). A tree-walking interpreter overflows the host stack at a far smaller dat depth than compiled code, so the interpreter needs its own depth check and a large dedicated thread stack.

`bench/fib35.dat` (`function main(): void { print(fib(35)); }` with the same `fib`), plus `bench/fib35.rs`, the hand-written baseline: the same algorithm, built with `overflow-checks=on`.

### CLI commands the PoC exercises
- `dat check examples/fib/fib.dat` exits 0 with no output. `dat check examples/fib/bad_fib.dat` exits 1 and prints the terminal diagnostic. `dat check --json …` prints the record.
- `dat run examples/fib/fib.dat` (interpreter) and `dat run --compiled examples/fib/fib.dat` (generate Rust, run rustc, run the binary). `dat build examples/fib/fib.dat -o fib` produces the binary on its own.
- `dat test examples/fib/fib.dat` and `dat test --compiled examples/fib/fib.dat`, both from the spec, with `--filter`.
- `dat-conformance` (or `cargo xtask conformance`) runs all examples and every extracted spec block in both modes.

### Exit-criteria table
"Both modes" means the interpreter and the compiled binary. The reference machine is the CI runner for ratios; the absolute budgets are for the interpreter path only (owner decision 1).

| # | Component | Criterion | How measured | Pass threshold |
| --- | --- | --- | --- | --- |
| 1 | Lexer + parser (`dat-syntax`) | Parses all PoC programs and every spec block in the subset. Byte spans on every node. A syntax error reports `file:line:col` and doesn't panic. | Unit tests; parse of all 158 blocks never panics (spec blocks outside the subset report "unsupported") | 0 panics; all subset blocks parse |
| 2 | Checker (`dat-check`) | `fib.dat` clean. `bad_fib.dat` reports exactly one error with the spec message "can't add string and int" at 10:9–10:31. The subset's ❌ lines in `03-operators` (`++` forms) and `14-scope` are reproduced with their spec text. The `06-functions` "missing return when x == 0" line needs path-condition reporting, so it may be `xfail` at the PoC. | Golden diagnostics; spec-line matcher | 100% of the in-subset ❌ lines matched, except listed `xfail`s; 0 extra diagnostics |
| 3 | Diagnostics | Terminal form (primary) and `--json` form golden-tested. The JSON fix applies, rustfix-style: the fixed file checks clean and runs. | `.diag`, `.diag.json` and `.fixed` goldens | Byte-identical goldens; the fixed program prints `fib(10) = 55` in both modes |
| 4 | Typed IR (`dat-ir`) | Both backends consume only the IR. A `dat ir fib.dat` text dump is snapshot-tested. | The crate graph forbids `dat-interp`/`dat-codegen` → `dat-syntax`/`dat-check` dependencies (checked in CI with `cargo metadata`); insta snapshot | No forbidden edges; snapshot stable |
| 5 | Interpreter (`dat-interp`) | Runs every PoC program with the expected stdout, stderr and exit status | Conformance runner | All goldens match |
| 6 | Codegen + rustc (`dat-codegen`) | Generated Rust compiles with no warnings, contains no `unsafe`/`RefCell` (`#![forbid(unsafe_code)]` in the generated crate root), and runs with the same goldens | Conformance runner `--compiled` | 0 rustc warnings; all goldens match |
| 7 | Runtime (`dat-rt`) | The overflow check, the `RangeError` message, `toString`, `print`, `assert`'s left/right rendering and uncaught-error printing exist once, in dat-rt, and both modes call them | Code review rule plus a test that the overflow message is byte-identical in both modes | Identical bytes |
| 8 | Error model | Overflow and the user-thrown `RangeError` are catchable in both modes. An uncaught error exits with status 1 and a dat-only trace (no Rust frames). | `fib.dat` tests; `overflow_uncaught.dat` golden | Exit statuses and stderr identical |
| 9 | Stack guard (owner decision 2) | `deep.dat` catches deep recursion in both modes, or the guard's cost is recorded and a crash is accepted | `bench/fib35` with and without the guard | Adopt if guard cost ≤ 5% on `fib(35)` compiled (measured 0–4% locally for the stack-pointer check) |
| 10 | `dat test` | `fib.dat`: 4 passed, 0 failed, with identical per-test results in both modes. `fib_fail.dat` prints the spec `FAIL` block in both modes and exits non-zero. | Conformance runner | Identical result lists |
| 11 | Differential harness (`dat-conformance`) | Compares stdout, normalized stderr, exit status, uncaught error type and message, and the per-test result list. Detects an injected fault. | `DAT_FAULT=interp-plus-one` self-test | Harness reports the difference (otherwise CI fails) |
| 12 | Spec extraction | Extracts all 158 blocks and classifies every annotation as checked, unsupported or doc-only, with counts. Subset cases pass in both modes. | Runner summary | 158/158 classified; ≥ 10 subset cases passing; `pass` count recorded as the ratchet baseline |
| 13 | Interpreter-path latency (ms budgets) | `dat check fib.dat` ≤ 10 ms; `dat test fib.dat` (interpreted, 4 tests) ≤ 20 ms; `dat run hello.dat` ≤ 5 ms; all wall time including process start | hyperfine `-N --warmup 3 --runs 20` in CI | Median under budget; CI hard-fails at 2× (noise margin) |
| 14 | Compiled-path budget (separate) | `dat run --compiled fib.dat` warm (dat-rt prebuilt) ≤ 1 s; `dat test --compiled fib.dat` warm ≤ 2 s; cold recorded only | hyperfine | Under budget (local evidence: rustc on the program 70–100 ms, cargo incremental 120–135 ms) |
| 15 | Compiled speed | `bench/fib35` generated vs hand-written Rust, same profile, `overflow-checks=on` | hyperfine `-N`, 20 runs, in the same CI job | ≤ 1.2× (unwind design measured ~1.0×) |
| 16 | Interpreter speed | Interpreted `fib(30)` vs `python3 fib.py 30` on the same runner | hyperfine, same job | ≤ 2× CPython at the PoC (toy slot-resolved walker measured 1.7×); M1 stretch ≤ 1× |
| 17 | CI | All of the above on Linux and macOS for every push | GitHub Actions matrix | Green on both OSes |

### Unspecified behaviour the PoC runs into (owner questions, not settled in code)
1. The text of the `int` overflow `RangeError` message. The spec gives a format only for index errors.
2. Where `print` writes (stdout, presumably) and where uncaught errors go (stderr?). The stack-trace format.
3. Operand evaluation order. Only associativity is specified. Left to right matches Rust and TypeScript.
4. Stack overflow: the type (owner leans to a new `StackOverflowError`; V8 uses `RangeError`), and whether the depth is deterministic across modes.
5. Whether `try?` applies to arithmetic (`try? a + b`) or to a call of a non-throwing function. The spec lists `xs[i]`, `pop()` and `m[key]` as built-in operations.
6. Whether a non-`void` expression statement may discard its result, and whether `_` may name a catch variable.
7. Whether `catch (e: Error)` around a block that can throw only runtime errors trips the "declared error type that nothing in the block can throw" rule, since `Error` is listed as "Declared" in the table.
8. `dat test` output for passing tests, the summary line and the exit status; `dat check` exit statuses.
9. Out-of-range integer literals (`9223372036854775808`) and `-9223372036854775808`.
10. The `FAIL math.dat:9` example in [20-testing.md](../../reference/20-testing.md) has the wrong line number and an assertion that can't fail.

### Inferences
- The overflow message should carry the operands (`int overflow: a + b`). That matches the spec's agent-first goal (the index error already names the index and the length) and costs nothing, because it is built only on the cold path (`#[cold]` in the local measurements).
- The draft deliberately avoids every unspecified construct, so the program itself needs no spec change. Only the golden outputs for the overflow message, the stderr format and the stack guard depend on owner answers.

### Gaps
- Exact column conventions (1-based, byte or character) aren't specified. The draft uses 1-based byte columns, which equal character columns for ASCII.

---

## 6. What must be real versus stubbed, and the CI that enforces it

### Takeaway
The walking-skeleton principle says to make every box real but tiny. The lexer and parser, the checker, the typed IR, both backends, dat-rt, the diagnostics record, the conformance runner and the benchmark harness should all be production code scoped to the fib subset. Stubs are fine for breadth (other types, modules, error recovery inside the parser, LSP, caching) but not for interfaces, because interfaces are what later milestones extend. CI on Linux and macOS with Swatinem/rust-cache runs everything in both modes on every push.

### Cited Findings
- Walking skeleton versus spike: the skeleton is kept and evolved, and it should be automatically built and tested end to end. — [Blodgett 2020, quoting Cockburn and Freeman & Pryce](https://www.mattblodgett.com/2020/09/start-with-walking-skeleton.html)
- Swatinem/rust-cache caches `~/.cargo` (registry, git dependencies) and the dependencies' build artifacts in `./target`. "The workspace crates themselves are not cached since doing so is generally not effective." It removes incremental artifacts and sets `CARGO_INCREMENTAL=0`. The cache key includes the rustc release, host and hash, plus Cargo.lock and Cargo.toml. Recommended usage: install the toolchain first, then `- uses: Swatinem/rust-cache@v2`, optionally with `save-if` restricted to the default branch. — [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache)
- rustc applies fixes as span-based text edits (rustfix and `run-rustfix`), which shows that machine-applicable fixes don't need a lossless syntax tree. — [rustc-dev-guide, UI tests](https://rustc-dev-guide.rust-lang.org/tests/ui.html)
- Rust doctests merge compatible tests into one crate in the 2024 edition because "the slowest part of doctests is to compile them". The same pressure applies to `dat test --compiled`: generate all tests into one crate and one binary, not one per test. — [rustdoc book](https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html)

### Inferences: real versus stub
| Component | Real in the PoC (later milestones extend it) | Stubbed or deferred |
| --- | --- | --- |
| `dat-syntax` | Hand-written lexer (keeps comments as trivia) and Pratt parser for the subset; byte spans on every node | Parser error recovery (stop at the first syntax error); lossless CST (only needed for `dat fmt` in M7) |
| `dat-check` | Name resolution, block scope, types for the subset, multiple type errors per run, the fix-carrying diagnostic | Inference beyond literals and locals, narrowing, generics, ownership pass |
| `dat-ir` | Typed IR: functions, slot-indexed locals, checked int ops as distinct opcodes, structured control flow, try/catch regions, calls, source locations for traces; text dump | Optimizations |
| `dat-interp` | Walks the IR (not the AST); calls dat-rt for every observable built-in; own call-depth limit; runs on a large-stack thread | Closure compilation or bytecode (M1+) |
| `dat-codegen` | Emits Rust from the same IR; `#![forbid(unsafe_code)]`; drives cargo or rustc; one crate for all tests | Release profile tuning, LTO |
| `dat-rt` | Checked arithmetic, error structs, panic-payload throw and catch helpers, `print`/`toString` for int/bool/string, `assert` rendering, uncaught-error reporter, test harness, stack guard | Collections, float formatting, strings beyond concatenation and template |
| Diagnostics | One record (code, severity, span, message, fix{title, applicability, edits}); terminal renderer (primary, owner decision 4); `--json` | `dat fix` command, LSP, `dat explain` |
| `dat-cli` | `check`, `run`, `run --compiled`, `build`, `test`, `test --compiled`, `--json`, `--filter` | Watch mode, project files |
| `dat-conformance` | Extractor for all 158 blocks, classifier, sidecar preludes, example goldens, mode diff, fault-injection self-test, ratchet file | Program generator, reducer |
| Benchmarks | hyperfine scripts with start-up measured separately, the hand-written Rust baseline, CPython and Lua reference runs, ratio gates | Gungraun instruction-count job (can follow quickly) |

CI sketch (GitHub Actions):
- Matrix `os: [ubuntu-latest, macos-latest]`. Toolchain pinned with `rust-toolchain.toml`, then `Swatinem/rust-cache@v2`.
- Steps: `cargo build`, `cargo test` (unit and snapshot tests), `cargo run -p dat-conformance -- --modes both`. On failure, upload the generated `.rs` files and the diffs as artifacts.
- Cache the generated-program build directory (`target/dat-gen`) keyed on the hash of dat-rt plus the rustc version.
- A separate `perf` job on ubuntu runs hyperfine budgets and ratios (and Gungraun when added). It hard-fails only beyond 2× the budget and posts the numbers as a job summary.

### Gaps
- I didn't check current GitHub-hosted runner images (macOS architecture and version) for October 2026.
- I didn't evaluate sccache against rust-cache for the generated-code builds.

---

## Decisions for dat-lang

Each decision lists the options, the evidence for and against each, a recommendation, the cost, and whether it needs a spec change. Nothing here changes the spec. Spec-affecting items are questions for the owner.

### D1. What the first milestone is
- **Options:** (a) the current M0: harness, spikes and a Hello World exit check; (b) a fib walking skeleton through every component, replacing the Hello World check; (c) (b) plus the existing throwaway spikes for copy-on-write lists and compiled-test turnaround.
- **Evidence:** Ghuloum's working compiler from day one with an automated output-comparing test driver; Cockburn and Freeman & Pryce's thinnest real end-to-end slice, kept and evolved; Zig and Roc's single test corpus across backends. Hello World doesn't exercise calls, locals, control flow, checked arithmetic or catchable errors, so it can't validate the typed IR or the error model. Against (b): it front-loads more work before any harness metric exists. But the harness is part of (b).
- **Recommendation:** (c). Fib replaces Hello World as the M0 exit check. The unwinding and stack-guard questions get answered by the PoC itself (they become real code in dat-rt). The copy-on-write spike stays a throwaway spike, because fib has no collections.
- **Cost:** my rough estimate is the subset-scoped crates plus the runner, on the order of 5–8k lines of Rust. That's well above Hello World, but almost none of it is thrown away.
- **Spec change:** none.

### D2. How runtime errors work in generated Rust
- **Options:** (a) unwinding (`panic_any(RangeError)` plus `catch_unwind` at each `try`, `panic = "unwind"`); (b) `Result` for every function that can raise any runtime error.
- **Evidence:** measured `fib(35)` at ~1.0× checked hand-written Rust for (a) and ~1.6× for (b). The `catch_unwind` docs say it is not recommended as a general try/catch, and it doesn't catch aborts. Runtime errors are exceptional, so the throw-side cost of unwinding matters less than hot-path cost. Declared errors stay `Result` (roadmap).
- **Recommendation:** (a) for runtime errors and `Result` for declared errors. Silence the default panic hook in generated binaries and pin `panic = "unwind"`.
- **Cost:** low in the PoC. `finally` and `return` inside `try` come later (M4).
- **Spec change:** none.

### D3. Stack overflow (owner decision 2)
- **Options:** (a) no guard: the process aborts (exit 134, uncatchable); (b) stack-pointer check against a per-thread limit in compiled code, plus an explicit depth limit in the interpreter running on a big-stack thread, with the exact depth unspecified; (c) a call-depth counter with a fixed limit in both modes (deterministic).
- **Evidence:** measured on macOS: (b) costs 0–4% on `fib(35)`; (c) costs ~2× with a thread-local counter and ~3.3× with a global atomic; (a) aborts. V8 throws a catchable `RangeError` for stack exhaustion ([MDN](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Errors/Too_much_recursion)), while Python has its own `RecursionError` ([Python docs](https://docs.python.org/3/library/exceptions.html#RecursionError)). Against (b): the overflow depth differs between modes, so a program that recurses to depth 50,000 could succeed in one mode and fail in the other. Against (c): its cost, though thread-local storage is cheaper on Linux (not measured).
- **Recommendation:** (b), with a guaranteed minimum depth stated in the spec (for example "at least 10,000 nested calls") so programs below it behave the same in both modes. Measure (c) on Linux before rejecting it for good.
- **Cost:** small: a prologue check in generated functions, and one interpreter field.
- **Spec change:** yes, owner question. A new runtime error type (`StackOverflowError`, the owner's lean) or an extra row for `RangeError` (the JS precedent), plus the minimum-depth guarantee.

### D4. Overflow message, uncaught-error output and evaluation order
- **Options:** leave them implementation-defined, or specify them.
- **Evidence:** the Csmith lesson: differential testing finds bugs only when behaviour is fully defined. Two modes that share dat-rt would print the same text anyway, but the spec is the golden source.
- **Recommendation:** specify `RangeError: int overflow: <a> <op> <b>`, uncaught errors on stderr in the format shown in section 5, and left-to-right operand evaluation (matching Rust and TypeScript).
- **Cost:** trivial.
- **Spec change:** yes, owner question (additions, not changes to designed features).

### D5. Annotation format for spec-as-tests
- **Options:** (a) leave the spec untouched: the runner infers from `// ❌ Error:`, `// throws`, output comments and value comments, with a sidecar file for preludes and splits; (b) invisible `<!-- dat: … -->` directives in the spec (mdtest precedent); (c) ```` ```dat ```` / ```` ```dat ignore ```` fence tags (rustdoc precedent, and syntax highlighting later); (d) separate test files, rustc UI style.
- **Evidence:** the current conventions already encode most expectations. Problems: 8 blocks redeclare names, 8 use `{ ... }` placeholders, many use undefined names, and some comments mix values with prose. Option (d) loses the "spec is the test suite" property.
- **Recommendation:** (a) for the PoC; propose (c) plus (b) to the owner once the extractor shows how many blocks need them.
- **Cost:** (a) is a few hundred lines of extractor plus the sidecar.
- **Spec change:** none for (a); (b) and (c) edit spec files (presentation only), owner question.

### D6. Matching diagnostics against the spec
- **Options:** match the exact message text; match error codes added to the spec; match a substring (mdtest `"text"`).
- **Evidence:** the spec has message text and no codes. Several ❌ comments include the fix after a semicolon. The owner wants the terminal format with an inline fix as the agent interface.
- **Recommendation:** exact match on the one-line form (`message`, or `message; fix.title`). Codes live in the compiler and the conformance mapping, not in the spec. The terminal golden is primary and the JSON golden secondary (owner decision 4).
- **Cost:** low.
- **Spec change:** none. Optional owner question: should the spec show error codes?

### D7. Interpreter design for the PoC
- **Options:** AST walker with name lookup; IR walker with slot-resolved locals; closure compilation; bytecode VM.
- **Evidence:** measured `fib(30)`: naive walker 322 ms, slot-resolved walker 101 ms, CPython 60 ms, Lua 32 ms. AGENTS.md wants a fast interpreter, and the roadmap has both modes split after the typed IR.
- **Recommendation:** an IR walker with slots now (gate: ≤ 2× CPython); closure compilation in M1 if the ≤ 1× stretch is missed.
- **Cost:** low.
- **Spec change:** none.

### D8. How the compiled path builds
- **Options:** (a) a cached cargo workspace with a path dependency on dat-rt; (b) direct `rustc --extern dat_rt=<prebuilt rlib>`.
- **Evidence:** measured for a tiny program: cargo incremental rebuild 122–135 ms, cold 248 ms; direct rustc 68–74 ms. Rlibs are tied to the exact rustc version, so (b) needs dat-rt built per toolchain.
- **Recommendation:** (a) in the PoC. Measure against the separate compiled-path budget, and switch to (b) only if the budget requires it.
- **Cost:** low.
- **Spec change:** none.

### D9. Benchmark gates
- **Options:** absolute times; ratios within the same CI job; instruction counts.
- **Evidence:** shared runners are noisy. Instruction counts are near-deterministic (Gungraun, Bencher). hyperfine `-N` avoids shell noise for commands under 5 ms. The measured ratios support ≤ 1.2× compiled and ≤ 2× CPython for the interpreter.
- **Recommendation:** ratio gates (rows 15–16 of the table), interpreter-path ms budgets at 2× hard-fail (row 13), a separate compiled-path budget (row 14), and Gungraun added on Linux soon after.
- **Cost:** low.
- **Spec change:** none.

### D10. Parser output: AST with spans or lossless CST
- **Options:** AST with byte spans (comments kept in the token stream); a lossless CST from the start (rowan/Roslyn style).
- **Evidence:** fixes are span-based text edits (rustfix), so a lossless tree isn't needed for fixes. The roadmap needs a lossless tree for `dat fmt` in M7.
- **Recommendation:** AST plus spans in the PoC, with trivia kept in the lexer output. Decide on the CST at M7 with `dat fmt`, accepting a contained parser refactor then.
- **Cost:** it saves work now, with a moderate retrofit later.
- **Spec change:** none.

### D11. CLI names outside the spec
- **Options:** keep `check`, `build` and `run --compiled` as tooling, outside `reference/`; or document the CLI in the spec.
- **Evidence:** the spec names only `dat run` and `dat test` (with `--compiled` and `--filter`).
- **Recommendation:** keep the CLI out of the language reference, but document the exit statuses and output streams in it (see D4).
- **Cost:** none.
- **Spec change:** none. Owner question: where should the CLI reference live?

### D12. Spec inconsistencies found
- `FAIL math.dat:9` in [20-testing.md](../../reference/20-testing.md) points to the wrong line, and the shown `clamp` can't fail that assertion.
- `catch (e: Error)` versus the "declared type nothing can throw" rule ([12-errors.md](../../reference/12-errors.md)).
- **Recommendation:** ask the owner. The PoC avoids both (it uses `catch (e)` and generates its own FAIL goldens).
- **Spec change:** yes (corrections), owner question.
