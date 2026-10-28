# Agent-first diagnostics and tooling for dat-lang: structured fixes, evidence on LLM feedback, and measuring "fewest turns"

> **Status, 29 October 2026.** These notes predate the owner's answers to questions 32, 36 and 37 in [the research report](../../reports/Dat%20lang%20roadmap%20decisions.md#questions-for-the-owner). Those answers are now in `reference/`, and they win wherever this note calls the questions open or proposes something else. The fix levels are `safe`, `review` and `option`, every ❌ line in the spec now carries a code numbered by section (`if (user)` is `E0701`) and a fix, and `dat test` exits 2 when the tests couldn't run, not 1.

Scope note (October 28, 2026): these notes cover compiler diagnostics, fixes, output properties, in-context teaching of a new language, error recovery, and benchmark design. They are measured against the project rules in `AGENTS.md` ("an agent should finish a task in the fewest turns"; "compiler errors that state the exact fix"; checks and tests in milliseconds), the spec in `reference/`, and the draft roadmap (one diagnostic record with code, span, message and fix; `dat check --json`; `dat fix`; `dat explain`; `dat fmt`; LSP; error recovery; an agent benchmark scored by turns until `dat test` passes). Older sources (before 2025) are marked as such. "Measured" means a controlled experiment reported numbers; "claim" means a designer or vendor stated it without published measurements.

What the spec already says about errors (from reading `reference/`):
- `// ❌ Error:` marks a compile error in examples ([reference/README.md](/Users/ismailasci/Work/Personal/dat-lang/reference/README.md)). There are 37 such lines. Only 3 state a fix in the comment (`let items = [];` → "write `let items: list<int> = [];`", `if (user)` → "write `user != null`", `if (items.length)` → "write `items.length > 0` or `!items.isEmpty()`"). Two have no message at all (`admin.name = "Kai"; // ❌ Error`). A few fixes are given in prose instead, for example the "modified copy is never read" error in [02-variable-declaration.md](/Users/ismailasci/Work/Personal/dat-lang/reference/02-variable-declaration.md) line 82.
- The spec already fixes a text format for test failures: `FAIL math.dat:9  <test name>` followed by the failing `assert(...)` and `left:` / `right:` values ([reference/20-testing.md](/Users/ismailasci/Work/Personal/dat-lang/reference/20-testing.md)). Uncaught runtime errors print "its message, its source location and a stack trace" ([reference/12-errors.md](/Users/ismailasci/Work/Personal/dat-lang/reference/12-errors.md)), but the exact layout is not specified.
- The spec's design principle 3 says there is "No `==` next to `===`", but the spec has no ❌ example for TypeScript habits that dat rejects (`===`, `class`, `function` expressions, `any`, `undefined`) ([reference/README.md](/Users/ismailasci/Work/Personal/dat-lang/reference/README.md)).

---

## 1. Structured fix designs in existing compilers and tools

### Takeaway
Every mature tool separates "fix that is safe to apply mechanically" from "suggestion a human or agent must choose", and the details converge: a stable code per diagnostic, an applicability or safety level on each fix, multi-edit fixes that must not overlap, alternatives that are never auto-applied, and a rule that the compiler should keep checking as if the fix had been applied. Position encoding is the one place tools disagree: rustc gives byte offsets plus character columns, LSP defaults to UTF-16 columns but can negotiate UTF-8, and ESLint uses JavaScript string indices.

### Cited Findings

**rustc suggestions, Applicability and JSON**
- rustc's four Applicability levels: `MachineApplicable` "Can be applied mechanically"; `HasPlaceholders` "Cannot be applied mechanically because it has placeholder text in the suggestions"; `MaybeIncorrect` "Cannot be applied mechanically because the suggestion may or may not be a good one"; `Unspecified` "we don't know which of the above cases it falls into." Contributors are told to be conservative when choosing a level. — [rustc dev guide: Errors and lints](https://rustc-dev-guide.rust-lang.org/diagnostics.html)
- rustc's JSON docs say a `MachineApplicable` replacement is "definitely what the user intended" and "should be applied automatically", while a `MaybeIncorrect` one "should still produce valid Rust if applied". — [rustc book: JSON output](https://doc.rust-lang.org/rustc/json.html)
- Suggestion display: suggestions may be shown inline, but "when the suggestion spans multiple lines or when there are multiple suggestions, the suggestions are displayed on their own." Tool-only (hidden) suggestions exist for "_very_ obvious cases" that tools should still be able to apply. — [rustc dev guide](https://rustc-dev-guide.rust-lang.org/diagnostics.html)
- Wording rules: "Suggestions should not be a question"; avoid "did you mean"; messages are "matter of fact", without capitalization or trailing periods; keep the span as small as possible; "Try not to emit multiple error messages for the same error." — [rustc dev guide](https://rustc-dev-guide.rust-lang.org/diagnostics.html)
- Error codes and `--explain`: most errors have a code with a long explanation that includes "an example of how to trigger the error and in-depth details". The rule for adding one is: "give an error a code (with an associated explanation) if the explanation would give more information than the error itself." Lint diagnostics have names instead of codes. — [rustc dev guide](https://rustc-dev-guide.rust-lang.org/diagnostics.html)
- rustc's JSON format is one JSON object per line (the stream as a whole "is not valid JSON"), and each diagnostic carries a `rendered` field with the human-readable text. — [rustc dev guide](https://rustc-dev-guide.rust-lang.org/diagnostics.html); [rustc book: JSON output](https://doc.rust-lang.org/rustc/json.html)
- rustc span positions: `byte_start` is "The byte offset where the span starts (0-based, inclusive)" and `byte_end` is 0-based and exclusive. `line_start` is 1-based. `column_start` is "The first character offset of the line_start (1-based, inclusive)", and "Character offsets are offsets of Unicode Scalar Values." Each span also carries the full source `text` of each covered line with highlight ranges, plus `suggested_replacement` and `suggestion_applicability`. — [rustc book: JSON output](https://doc.rust-lang.org/rustc/json.html)
- Fix suggestions sit in `children` sub-diagnostics, for example a `help` child "if this is intentional, prefix it with an underscore" with `suggested_replacement: "_x"` and `MachineApplicable`. Consumers are told to be forward-compatible because new fields and new enum values may be added. — [rustc book: JSON output](https://doc.rust-lang.org/rustc/json.html)

**Clang fix-it hints (long-standing rules, older but still current)**
- Fix-its on errors and warnings are applied automatically with `-fixit`, so they "should only be used when it's very likely they match the user's intent". Clang "must recover from errors as if the fix-it had been applied". A fix-it on a warning must not change the code's meaning. Fixes that fail these rules go on a note, and fix-its on notes are never applied automatically. — [Clang Internals Manual: Fix-It Hints](https://clang.llvm.org/docs/InternalsManual.html)
- Clang's wording rules: no leading capital or trailing punctuation; "The wording should be actionable"; it "should clearly explain what is wrong with the code rather than restating what the code does." — [Clang Internals Manual](https://clang.llvm.org/docs/InternalsManual.html)

**Ruff (safe vs unsafe fixes)**
- "The meaning and intent of your code will be retained when applying safe fixes." Unsafe fixes may change runtime behavior or remove comments. Example: RUF015 is unsafe because it changes the exception raised on an empty collection from IndexError to StopIteration. — [Ruff docs: The Ruff Linter](https://docs.astral.sh/ruff/linter/)
- "Ruff only applies safe fixes by default"; `--unsafe-fixes` opts in. By default Ruff prints a hint when unsafe fixes exist but weren't applied. Safety can be overridden per rule with `lint.extend-safe-fixes` and `lint.extend-unsafe-fixes`. — [Ruff docs](https://docs.astral.sh/ruff/linter/)
- The JSON output shows every fix, and "Each fix's safety appears in the `applicability` field." The default `full` text output also shows all suggested fixes, including ones that can't be auto-applied. — [Ruff docs](https://docs.astral.sh/ruff/linter/)
- Ruff's exit codes: 0 for no violations, 1 when violations were found, 2 for abnormal termination (bad configuration, bad CLI options, internal error). — [Ruff docs: Exit codes](https://docs.astral.sh/ruff/linter/#exit-codes)

**ESLint (fixes vs suggestions)**
- A rule with `meta.fixable` provides a `fix` function that `--fix` applies. A rule with `meta.hasSuggestions` provides a `suggest` array that editors show for manual application, "applied as stand-alone changes, without triggering multipass fixes." — [ESLint: Custom Rules](https://eslint.org/docs/latest/extend/custom-rules)
- Fixes must avoid "any fixes that could change the runtime behavior of code and cause it to stop working". When several valid repairs exist, or a change may alter behavior, the rule should offer a suggestion instead. — [ESLint: Custom Rules](https://eslint.org/docs/latest/extend/custom-rules)
- A fix may return several fixing objects, but "those fixing objects must not overlap". When fixes from different reports conflict, only one is applied in a pass. Ranges are two-element arrays of character indices, start inclusive and end exclusive. — [ESLint: Custom Rules](https://eslint.org/docs/latest/extend/custom-rules)

**TypeScript and Roslyn code fixes**
- TypeScript code fix actions can carry a `fixId`. When it is present, `getCombinedCodeFix` returns the edits for that fix applied to the whole file ("fix all"). A missing `fixId` means the fix "can't be applied in a group". Fix IDs are short strings such as `fixMissingMember` or `inferFromUsage`. — [ts-morph LanguageService docs](https://jsr.io/@ts-morph/ts-morph/doc/~/LanguageService.prototype.getCombinedCodeFix); [DeepWiki: TypeScript code fixes](https://deepwiki.com/microsoft/TypeScript/4.2-code-fixes-and-quick-fixes) (both secondary; the official TypeScript source was not checked)
- Roslyn's syntax trees stay complete and round-trippable to the source text even when the source has errors. Missing tokens are parser-generated placeholders "to represent constructs that should have been present in the source code for the source code to compile". — [Microsoft Learn: Work with syntax](https://learn.microsoft.com/dotnet/csharp/roslyn-sdk/work-with-syntax); [SyntaxToken API](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.syntaxtoken)

**Go `go fix` (2026, current)**
- Go 1.26 rebuilt `go fix` on the analysis framework used by `go vet`. It ships "a couple dozen 'modernizers', analyzers that suggest safe fixes". A fix is a set of concrete text edits computed with full type information. — [Go 1.26 release notes source](https://go.googlesource.com/website/+show/cd9f0a846a136f7b452395772638018114795f37/_content/doc/go1.26.md); [Go blog: Using go fix](https://golang.google.cn/blog/gofix)
- `go fix ./...` silently rewrites files. Running it again finds follow-on fixes. When two fixes touch the same lines, one is applied and the user is told to rerun. The project's docs admit that fixes can leave unused imports or variables that break the build, and they treat "any modernizer whose fix changes program behavior" as a serious bug. — [Go blog: Using go fix](https://golang.google.cn/blog/gofix); [x/tools modernize doc.go](https://go.googlesource.com/tools/+/bf54bcd2f14a330f0dcffa4cf631235de771bde2/go/analysis/passes/modernize/doc.go)

**LSP**
- LSP positions are zero-based line and zero-based character offset. The client advertises encodings in `general.positionEncodings` and the server picks one in `capabilities.positionEncoding`. "To stay backwards compatible the only mandatory encoding is UTF-16", and if the server omits the field the encoding "defaults to the string value `utf-16`". `utf-8` counts bytes, and `utf-32` counts code points. — [LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
- A Diagnostic has `code`, an optional `codeDescription` with an `href` to documentation, `source`, `relatedInformation` (other locations), `tags` (Unnecessary or Deprecated), and a `data` field "preserved between a publish-diagnostics notification and a code action request". — [LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
- A WorkspaceEdit holds either `changes` or versioned `documentChanges`, which are preferred when supported. `changeAnnotations` group edits, and `needsConfirmation` flags that "user confirmation is needed before applying the change". — [LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
- Not re-checked in this pass because the fetched page was truncated: CodeAction fields (`kind` such as `quickfix` and `source.fixAll`, `isPreferred`, `diagnostics`, `edit`, `command`) and pull diagnostics. They are in the same specification. — [LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)

**SARIF**
- A SARIF 2.1.0 `fix` object has a description and `artifactChanges`. Each artifact change names a file and lists `replacements`, and each replacement pairs a `deletedRegion` with `insertedContent`. Multi-file fixes are therefore native. — [OASIS SARIF 2.1.0 errata01](https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/sarif-v2.1.0-errata01.html); [Go SARIF model (honnef.co)](https://pkg.go.dev/honnef.co/go/tools@v0.2.0/sarif)

**New agent-oriented toolchains (claims, not measurements)**
- Vercel Labs' Zero (v0.1.x, released May 15, 2026): `zero check --json` emits a `diagnostics` array. Each entry has `code`, `severity`, `message`, `span` (file, line, column) and `repair` (`id` plus `candidates`). Repair IDs come from a "finite, documented vocabulary". `zero fix --plan --json` returns a plan without applying it, `zero explain CODE` explains a code, and `zero skills get zero --full` serves version-matched usage guides from the binary. Codes are promised stable: "The wording of the message is allowed to change. The code is not." The article states there are no benchmarks and the round-trip reduction is untested. — [HackerNoon on Vercel Zero, May 19, 2026](https://hackernoon.com/vercels-zero-wants-compilers-to-talk-to-ai-agents)
- A directory of "programming languages designed for AI agents to write" lists several similar early-stage projects (for example Codong, with JSON errors carrying fix and retry fields). — [agentlanguages.dev](https://agentlanguages.dev/)

### Inferences
- The roadmap's JSON example (`line`, `col`, `endCol`, one `fix`) lacks the fields every mature format has: a confidence level, a file per edit (for multi-file fixes), a byte span, a column unit, room for alternatives, and a `rendered` text. These are cheap to add at M0 and expensive to change after tools depend on the schema.
- Clang's "recover as if the fix-it had been applied" is the most useful rule for dat. It makes the checker suppress follow-on errors that the fix would remove. It also gives a testable property: apply every safe fix, re-check, and the error count must drop with no new errors. The roadmap already proposes this check for the spec's ❌ cases at M7.
- dat strings are UTF-8 and `.length` counts bytes (roadmap M1), so byte offsets are the natural internal span. A human or agent reading `file:14:7` counts columns as characters, which is the convention rustc uses for its line/column output.

### Gaps
- I could not verify the exact SARIF `columnKind` values and default, the Roslyn `FixAllProvider` contract, or TypeScript's official `fixId` documentation. The TypeScript description comes from third-party wrappers.
- I could not fetch Elm's "Compiler Errors for Humans" post (elm-lang.org returned no content), so Elm's design principles (first-person voice, showing the user's code, hints) are not cited here. I did not research Gleam's error design. Both are known for friendly errors, but this pass gathered no evidence on them.

---

## 2. Empirical evidence on how compiler and test feedback affects LLMs

### Takeaway
Measured results agree that feedback helps, that most of the gain comes in the first one to three repair rounds, and that the quality of the feedback limits the gain more than the model's willingness to retry. Noisy static-analysis feedback can be worse than none: in FeedbackEval, pylint feedback scored below a generic "please fix" message. Test feedback with concrete failures is the most reliably useful signal. Static types help by turning wrong programs into compile errors, and about 94% of LLM compile errors in TypeScript are type errors. No study isolates the effect of adding an exact-fix line to compiler errors, so that claim is unmeasured.

### Cited Findings

**Repair with compiler and test feedback**
- RustAssistant (Microsoft Research, ICSE 2025; arXiv 2023) loops an LLM over rustc errors to produce patches. It reaches "a peak accuracy of roughly 74% on real-world compilation errors in popular open-source Rust repositories". Its micro-benchmarks cover 270 of 506 official Rust error codes. On Clippy-reported errors in the top-10 crates it reports 75% accuracy, "almost 2.4 times better" than Clippy's own auto-fix. — [Microsoft Research: RustAssistant](https://www.microsoft.com/en-us/research/publication/rustassistant-using-llms-to-fix-compilation-errors-in-rust-code/); [arXiv 2308.05177](https://arxiv.org/abs/2308.05177v1) (the 74% is a 2023–2024 model generation, so it is older)
- Olausson et al., "Is Self-Repair a Silver Bullet for Code Generation?" (ICLR 2024, older): once the cost of repair is counted, gains "are often modest, vary a lot between subsets of the data, and are sometimes not present at all." The bottleneck is the feedback. Replacing GPT-4's self-generated feedback with an experienced programmer's raised the number of repaired programs passing all tests by 1.58×. — [arXiv 2306.09896](https://arxiv.org/abs/2306.09896)
- Self-Debugging (Chen et al., ICLR 2024, older): with unit-test feedback, self-debugging "improves the baseline accuracy by up to 12%" on TransCoder and MBPP. On Spider, which has no unit tests, code explanation gives 2–3%. — [arXiv 2304.05128](https://arxiv.org/abs/2304.05128)
- FeedbackEval (2025; Python; GPT-4o, Claude-3.5, DeepSeek-R1, GLM-4, Qwen2.5). Average single-round repair success by feedback type: mixed 63.6%, simulated expert 62.9%, test feedback 57.9%, minimal generic request 53.1%, compiler/linter feedback (pylint) 49.2%, simulated "skilled" 48.8%. Gains "typically stabiliz[e] after two to three iterations of repair." Removing the docstring (the description of intended behavior) caused the steepest prompt-ablation drop, from 49.5% to 47.4%. — [FeedbackEval, arXiv 2504.06939v2](https://arxiv.org/html/2504.06939v2)
- A 2026 replication (Polytechnique Montréal, August 31, 2026) confirms that test feedback is strongest in Python. In Java, simple feedback and JUnit test feedback did not differ significantly. The authors conclude that earlier results "may be sensitive to benchmark construction, feedback representation, and tooling ecosystem." Lighter prompts cut cost without hurting repair. — [arXiv 2609.00362](https://arxiv.org/abs/2609.00362)

**Static types and language choice**
- Type-constrained decoding (Mündler et al., PLDI 2025) enforces well-typedness during generation for TypeScript. It reduces compilation errors by more than half (75.3% on HumanEval, 52.1% on MBPP in the arXiv version) and improves functional correctness. The authors found "on average 94% of compilation errors result from failing type checks." — [arXiv 2504.09246](https://arxiv.org/abs/2504.09246v1)
- MultiPL-E (2022, older; Codex, CodeGen, InCoder): "static type-checking neither helps nor hinders code generation model performance", and "some niche languages perform as well as more popular languages." — [arXiv 2208.08227](https://arxiv.org/abs/2208.08227)
- "The Best Programming Language for Tokenmaxxing" (Wu, Anderson, Guha; July 2026, revised October 2026). It ran 2,000 agent sessions on 100 LiveCodeBench tasks in Python, Java, Rust and OCaml, with 5 models (including GPT-5.5 and Claude Sonnet 4.6), capped at 40 turns and 10 minutes. Accuracy is "not affected very much by language". Most models score 92–100% everywhere, the exception being Gemma on OCaml at 62%. Token cost relative to Python on the same problem: Sonnet 4.6 Java 1.18×, Rust 1.16×, OCaml 1.28×; GPT-5.5 1.30×, 1.36×, 1.30×; Qwen OCaml 1.69×. The cost is not from code size: median final file is 272 tokens in OCaml versus 190 in Python, but total output is 6,706 versus 2,548. In one Gemma OCaml case study, "92% of failed snapshots are compile-time errors". 22 of 100 Gemma OCaml runs hit the 40-turn cap. Agents "sidestep unfamiliar target languages by prototyping in Python"; GPT-5.5 embedded Python on 27% of OCaml problems. The authors recommend by-language token efficiency as a benchmark metric. — [arXiv 2607.22807](https://arxiv.org/html/2607.22807)
- SWE-bench Multilingual (2025; SWE-agent plus Claude 3.7 Sonnet, so older): Rust had the highest resolution rate of 9 languages at 58.14%, versus 34.88% for JavaScript/TypeScript and 28.57% for C/C++. The authors note that only one model was evaluated. — [SWE-bench Multilingual](https://www.swebench.com/multilingual.html)

**Agent-computer interface (ACI) findings**
- SWE-agent (NeurIPS 2024, GPT-4 Turbo, older) ablations on SWE-bench Lite (full system 18.0% resolved):
  - Editing: an edit command without its linting guardrail scored 15.0%, and no edit command 10.3%.
  - Search: iterative search 12.0%, no search 15.7%, against 18.0% for summarized search.
  - File viewer: 30 lines 14.3%, full file 12.7%, against 18.0% for 100 lines.
  - History: full history 15.0%, against 18.0% for the last 5 observations.
  The stated principles: "Environment feedback should be informative but concise"; "Guardrails mitigate error propagation and hasten recovery"; empty output is replaced by "Your command ran successfully and did not produce any output." — [SWE-agent, arXiv 2405.15793v3](https://arxiv.org/html/2405.15793v3)
- "The Devil Is in the Interface" (August 2026; 11,700 trajectories; issue fixing). Structured low-level interfaces improved consistency across repeated attempts "by up to 4.7×" over bash-only. CodeAct-style Python interfaces matched task performance with "41.6% fewer steps and 56.3% lower token usage". Text "cognitive scaffolding" tools had "limited effect". — [arXiv 2608.11386](https://arxiv.org/abs/2608.11386)

**Language designers' claims about LLM-friendly design (claims, not measurements)**
- Armin Ronacher, "A Language For Agents" (February 9, 2026), argues from experience, with no measurements. A good agent language works without an LSP; uses braces rather than significant whitespace; favors explicit types over heavy inference ("you now need an LSP or the resulting compiler error messages"); avoids macros, barrel files and re-exports; supports local reasoning and greppability; has deterministic tests; gives "one clear pass/fail signal" ("Ideally it either runs or doesn't"). He also says agents handle exceptions poorly ("they are afraid of them"). He proposes measuring languages by counting agent edits and iterations. — [lucumr.pocoo.org](https://lucumr.pocoo.org/2026/2/9/a-language-for-agents/)
- MoonBit presented "AI-friendly" design at LLM4Code 2024. Its team describes flat syntax for linear generation, a sampler that uses type information to avoid type errors during generation, and a `declare` keyword that lets AI write and test code incrementally. Vendor claims such as "thousands of lines of code running perfectly the first time" come from internal tests with no independent verification. — [LLM4Code 2024 paper page](https://conf.researchr.org/details/icse-2024/llm4code-2024-papers/9/MoonBit-Explore-the-Design-of-an-AI-Friendly-Programming-Language); [MoonBit blog: AI coding](https://www.moonbitlang.com/blog/ai-coding); [MoonBit about](https://www.moonbitlang.com/about-us)
- Vercel Zero claims stable codes and repair plans reduce agent round trips, with no benchmark published (see section 1). — [HackerNoon](https://hackernoon.com/vercels-zero-wants-compilers-to-talk-to-ai-agents)

### Inferences
- FeedbackEval's result that pylint feedback scored below a generic request is the strongest warning for dat. Diagnostics that are not precise, or that mix style noise with real errors, can cost turns. dat should keep warnings and style out of the error stream an agent sees when it runs `dat check` or `dat test`.
- The tokenmaxxing study suggests that in unfamiliar languages the extra cost is compile-error iterations, not code size. That is the cost dat's exact-fix diagnostics target. It also suggests a metric: tokens per solved task in dat relative to the same task in TypeScript.
- 94% of TypeScript compile errors from LLMs are type errors, so dat's type-error diagnostics (null, union, `int`/`float`, `mut`, `try` marking) will carry most of the agent-facing load, not syntax errors.
- The evidence says the first one to three feedback rounds matter most. "Fewest turns" therefore depends on whether the first `dat check` reports every problem with a usable fix, not on how many retries an agent may take.

### Gaps
- No controlled study measures adding an exact-fix suggestion to compiler errors versus the error message alone for LLM agents. RustAssistant feeds rustc output, which already includes many suggestions, but I found no ablation that removes them.
- I found no measured experience reports from the TypeScript, Gleam or Mojo teams on LLM-friendly design.

---

## 3. Text vs JSON: what an agent should read, and what JSON is for (owner's question)

### Takeaway
I found no study that directly compares compiler diagnostics as text versus JSON for coding agents. The indirect evidence favors compact text for the agent's eyes. JSON costs roughly twice the tokens for the same diagnostic. Frontier models still make mistakes extracting facts from JSON tool output. Agents apply edits through string-replacement tools, so they need the literal before and after code rather than offsets. The owner's reading ("the terminal version is more for agents") matches this evidence. The JSON form is for programs: `dat fix`, the LSP server, CI annotations, and benchmark or agent harnesses that apply edits mechanically.

### Cited Findings
- Prompt format changes LLM accuracy, and the size and direction of the effect depend on the model. The study tested plain text, Markdown, YAML and JSON on OpenAI models (2024, older). GPT-3.5 varied by up to about 40% on some tasks, while GPT-4 was more robust. Reported HumanEval values swing in both directions (GPT-3.5: JSON 59.8 vs plain text 40.2; a GPT-4 variant: plain text 76.2 vs JSON 21.95), but the table is hard to read in secondary copies. — [He et al., arXiv 2411.10541](https://arxiv.org/abs/2411.10541)
- Extracting information from JSON tool responses "remains a difficult task even for frontier models across multiple prompting strategies." The processing strategy changed accuracy by 3% to 50% across 15 models. — [Kate et al., arXiv 2510.15955 (October 2025, revised January 2026)](https://arxiv.org/abs/2510.15955)
- Long inputs reduce reliability. Across 18 LLMs, "performance grows increasingly unreliable as input length grows" (Chroma, July 2025). — [Chroma: Context Rot](https://research.trychroma.com/context-rot)
- SWE-agent's ablations favor "informative but concise" feedback. A 100-line file window beat both 30 lines and the whole file, and a 5-observation history beat full history. — [SWE-agent](https://arxiv.org/html/2405.15793v3)
- Agents edit through exact string replacement or search/replace blocks, not offsets. Anthropic's text editor tool uses `str_replace` (old string to new string), and aider's main edit formats are search/replace blocks and diffs. — [Anthropic text editor tool docs](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/text-editor-tool); [aider edit formats](https://aider.chat/docs/more/edit-formats.html)
- rustc's JSON embeds the human rendering (`rendered`) in every diagnostic, so a harness can show the model the text while acting on the structure. — [rustc book: JSON output](https://doc.rust-lang.org/rustc/json.html)
- Ruff's JSON is aimed at tools: it shows all fixes with an `applicability` field, while people read the `full` text format. — [Ruff docs](https://docs.astral.sh/ruff/linter/)
- Zero bets the other way, making JSON the agent interface. It has published no measurements. — [HackerNoon](https://hackernoon.com/vercels-zero-wants-compilers-to-talk-to-ai-agents)

### Inferences
- **Token cost (my own proxy measurement, not a real tokenizer).** I counted word and punctuation pieces for the roadmap's own `E0204` example. The rustc-style text block came to 59 pieces (174 characters, 8 lines). The JSON came to 110 pieces (213 characters compact; 329 characters and 21 lines pretty-printed). A two-line compact text form came to about 50 pieces. JSON is therefore roughly 1.9× the text for the same content, and the gap grows when `rendered` and source text are both included. Script: `/private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/sizes.py`.
- **Accuracy and familiarity.** rustc- and tsc-style text output is extremely common in public code, issues and CI logs, so models have seen it far more often than any new JSON schema. This is the diagnostics version of AGENTS.md's "familiar syntax that agents already know". No source counts this, so it is an inference.
- **What saves a turn.** The text form's most valuable parts are the offending source line (the agent doesn't need to open the file) and the corrected line (the agent can paste it straight into a `str_replace` edit). Line/column offsets in JSON help only a program.
- **What JSON is for.**
  - `dat fix` and `dat fix --plan`.
  - The LSP server, which converts to LSP positions.
  - CI annotations, including a possible SARIF export.
  - The dat agent benchmark, to score fix adoption and classify failures.
  - Third-party agent harnesses that want to apply safe fixes without a model turn.
- **Relabeling the roadmap.** Its labels "Terminal" and "--json, for agents" should become "Default output (people and agents)" and "--json (tools: dat fix, LSP, CI, harnesses)".

### Gaps
- No direct experiment exists, so this should be the first A/B test in the dat benchmark (decision 9): same tasks, text versus JSON diagnostics, comparing turns, tokens and success. The cost is small once the harness exists.
- I did not have a real tokenizer offline, so the 1.9× ratio is a proxy.

---

## 4. Output properties that reduce turns and tokens

### Takeaway
Report every independent error in one run, but suppress cascades so the agent sees only root causes. Keep output short and deterministic. Put the fix next to the error as literal code. End with a one-line summary. Make one command give the whole pass/fail verdict, with exit codes for harnesses. Most of this is supported by design rules from mature compilers and by SWE-agent-style ACI evidence, not by turn-count experiments on compilers.

### Cited Findings
- All errors vs first error: Go's compiler stops at 10 errors by default; `-e` removes the limit. — [cmd/compile docs](https://pkg.go.dev/cmd/compile)
- One check, one signal: `go test` runs a subset of `go vet` checks while building the test binary, so a single command reports both vet problems and test results. — [cmd/go: Test packages](https://pkg.go.dev/cmd/go#hdr-Test_packages)
- "Ideally it either runs or doesn't" (Ronacher's experience-based argument for a single pass/fail signal, with as much linting as possible auto-fixed). — [A Language For Agents](https://lucumr.pocoo.org/2026/2/9/a-language-for-agents/)
- Exit codes: Ruff uses 0 for clean, 1 for violations, and 2 for tool failure. — [Ruff docs: Exit codes](https://docs.astral.sh/ruff/linter/#exit-codes)
- Concise feedback and guardrails at edit time improved agent results: linting on edit gave +3.0 points (18.0% vs 15.0%), and summarized search beat iterative search (18.0% vs 12.0%). — [SWE-agent](https://arxiv.org/html/2405.15793v3)
- Don't emit several messages for one error. — [rustc dev guide](https://rustc-dev-guide.rust-lang.org/diagnostics.html)
- Clang recovers as if the fix-it had been applied, which suppresses follow-on errors. — [Clang Internals Manual](https://clang.llvm.org/docs/InternalsManual.html)
- "Did you mean": TypeScript emits spelling suggestions, for example TS2552 "Cannot find name '{0}'. Did you mean '{1}'?". rustc's guide instead says suggestions should not be phrased as questions. — [TypeScript diagnosticMessages.json](https://github.com/microsoft/TypeScript/blob/main/src/compiler/diagnosticMessages.json); [rustc dev guide](https://rustc-dev-guide.rust-lang.org/diagnostics.html)
- Fix inline vs separate: rustc shows a suggestion inline only when it is short and single. Multi-line or multiple suggestions are shown on their own. — [rustc dev guide](https://rustc-dev-guide.rust-lang.org/diagnostics.html)
- dat's spec already designs its test-failure output to save a turn: "a failure shows both sides, so finding the cause rarely needs an extra `print`". — [reference/20-testing.md](/Users/ismailasci/Work/Personal/dat-lang/reference/20-testing.md)

### Inferences
- **Ordering.** Sort diagnostics by file path, then position, then code, so two runs can be compared line by line. An agent can then tell whether its edit removed an error or merely moved it.
- **Size.** Use one diagnostic block of at most about 6 lines: header, location, source line, caret with a short label, and a fix line with the corrected code. Then add a summary such as `2 errors (1 fixable with dat fix)`. Ruff and Go both print summary or rerun hints. Cap printed diagnostics (for example at 50) with "and N more", so a broken import can't flood the context window.
- **Placement of the fix.** Show the fix directly under its error as a whole corrected line, not as a separate section after all errors. The agent then reads location, cause and replacement together and can copy the corrected line into a `str_replace` edit.
- **"Did you mean".** Present the candidate as a fix (`fix: rename to \`clamp\``) rather than a question. Mark it as a `review` fix unless there is exactly one in-scope candidate of the right type.

### Gaps
- I found no study that counts agent turns under "first error only" versus "all errors". The case for all errors with cascade suppression rests on the turn arithmetic (one run shows everything) and on the SWE-agent and FeedbackEval findings that noisy feedback hurts.
- I found no study measuring how tool latency affects agent success or turn count. Millisecond checks mostly reduce wall time and keep a human supervising the loop, and they make re-checking after every edit cheap.

---

## 5. How agents learn a language that isn't in their training data

### Takeaway
More documentation in context is not automatically better. Repository context files raised agent cost by more than 20% with no gain in success. In unfamiliar languages, strong agents route around the language, for example by writing Python generators. Agents learn more from examples and working helpers than from distilled prose rules. For dat this points to a small "differences from TypeScript" sheet built from ❌/✅ examples, diagnostics that teach at the moment of error, and the full spec only on demand.

### Cited Findings
- "Evaluating AGENTS.md" (ETH Zurich, February 2026, ICLR 2026). Context files "result in no improvement in task success rates, while also increasing inference cost by over 20%". LLM-generated files added 3.92 steps on average (+23% cost). Developer-written files gained about 4 points on AgentBench while raising cost by up to 19%. The cause is broader exploration, because agents follow the instructions. Agents tested: Claude Code with Sonnet 4.5, Codex with GPT-5.2 and GPT-5.1 mini, and Qwen Code. Benchmarks were Python-only. — [arXiv 2602.11988](https://arxiv.org/pdf/2602.11988); [heise summary](https://heise.de/-11245317)
- "Frontier Coding Agents Use Metaprogramming to Adapt to Unfamiliar Programming Languages" (June 2026). Six agents (including Claude Opus 4.6, GPT-5.4 xhigh, Sonnet 4.6 and Haiku 4.5) worked on four esoteric languages, with file editing, local execution and hidden tests. The strongest agents write Python programs that generate target code, and "Forbidding this metaprogramming strategy causes large performance drops." Text guidance distilled from this strategy "does not materially improve weaker agents", while Opus-written Python helper code "sharply improves Sonnet 4.6 and GPT-5.4 mini". More interpreter calls and tokens help strong agents only. — [arXiv 2606.10933](https://arxiv.org/abs/2606.10933)
- Tokenmaxxing study: agents "sidestep unfamiliar target languages by prototyping in Python" and repeatedly produce non-compiling solutions in unfamiliar languages. — [arXiv 2607.22807](https://arxiv.org/html/2607.22807)
- In natural-language translation (an analogy, not code), LLMs given a grammar book showed no evidence of using the grammatical explanations and relied mainly on its parallel example sentences. — [Aycock et al., arXiv 2409.19151](https://arxiv.org/abs/2409.19151) A 2026 study of Tulu found grammar documentation helped by 8–22 points depending on the model, and swapping in incorrect grammar caused a 47-point drop, so models do rely on supplied rules. — [arXiv 2602.15378](https://arxiv.org/pdf/2602.15378)
- For very low-resource programming and formal languages with few public examples, SPEAC (NeurIPS 2024, older) combines LLM generation with iterative compiler-feedback repair. — [arXiv 2406.03636](https://arxiv.org/pdf/2406.03636)
- llms.txt (proposed by Jeremy Howard, September 3, 2024; v2 August 10, 2026). It is a small Markdown index with an H1, a summary blockquote, H2 link lists and an "Optional" section, plus `.md` versions of pages, intended for use at inference time. The page reports adoption but no effectiveness data. — [llmstxt.org](https://llmstxt.org/)
- Zero ships version-matched usage guides inside the compiler binary (`zero skills get`). — [HackerNoon](https://hackernoon.com/vercels-zero-wants-compilers-to-talk-to-ai-agents)
- MoonBit's `declare` keyword produces warnings instead of errors for missing implementations, so AI can write and test code incrementally (vendor claim). — [MoonBit blog](https://moonbitlang.com/blog/moonbit-ai)

### Inferences
- **Spec size budget.** `reference/` is about 95 KB, roughly 20–25k tokens (estimated from byte count). Loading it into every session adds that cost to every turn, and the AGENTS.md study suggests more context makes agents explore more without succeeding more. A compact sheet of 1–2k tokens that lists only where dat differs from TypeScript fits the "describe only non-standard practices" pattern. Those differences are value semantics with `mut` at the call site, `try` marking, no truthiness, `int / int` returning `float`, no `===`, structs instead of classes, `list<T>`/`map<K, V>` syntax, and `throws`.
- **Examples over rules.** Write the sheet as ❌/✅ pairs. The grammar-book, esolang and FeedbackEval evidence all favor concrete examples, and the spec already uses this style.
- **Teaching at the moment of error.** Each TypeScript habit should hit a dedicated diagnostic with the dat fix. The agent then learns the rule from the compiler on its first mistake and needs no docs up front. Examples are `===` → `==`, `class` → `struct`, `function` expression → arrow function, `any` → union or generic, `undefined` → `null`, and `xs.length` as a condition → `xs.length > 0`.
- **Benchmark escape behavior.** Strong agents may write Python generators that emit dat, or prototype in Python. The benchmark should record that behavior and not just penalize it. A fast dat interpreter with a REPL or `dat run` on a single file makes prototyping in dat itself cheap.

### Gaps
- I found no study that measures LLM agents writing a new statically typed, mainstream-style language (as opposed to esoteric or formal languages) with documentation supplied in context. dat's TypeScript resemblance is a bet that transfer from TypeScript dominates. That bet is untested, and the benchmark should test it against a TypeScript baseline.
- I found no measurements of whether llms.txt improves agent outcomes.

---

## 6. Measuring "fewest turns": existing benchmarks and an internal benchmark for dat

### Takeaway
Public benchmarks don't measure what dat needs. They test known languages, have largely saturated (aggregator sites show frontier models above 90% on SWE-bench Multilingual), and report success rather than turns or tokens. The most reusable methodology is the 2026 tokenmaxxing study: the same problems across languages, turn and time caps, re-execution of every intermediate solution, and mixed-effects models for token cost. dat should build a small private benchmark early and use it to settle the diagnostics decisions by A/B test.

### Cited Findings
- Aider polyglot (December 2024, older) is the 225 hardest of 697 Exercism exercises in C++, Go, Java, JavaScript, Python and Rust. Exercises were kept if 3 or fewer of 7 calibration models solved them. Metrics are "Percent completed correctly" and "Percent using correct edit format". — [aider polyglot](https://aider.chat/2024/12/21/polyglot.html)
- SWE-bench Multilingual has 300 tasks from 42 repositories in 9 languages, each validated manually (tests fail before the fix and pass after). The gold patch's median size is 10 lines. — [SWE-bench Multilingual](https://www.swebench.com/multilingual.html) Aggregators list Claude Opus 5.5 at 93.9% as of October 2026. These figures are unverified against the official leaderboard. — [benchlm.ai](https://benchlm.ai/benchmarks/swe-bench-multilingual)
- Terminal-Bench 2.0 (January 2026) has 89 terminal tasks, each with its own environment, a human-written solution and verification tests. "Frontier models and agents score less than 65%" at publication. — [arXiv 2601.11868](https://arxiv.org/abs/2601.11868)
- The tokenmaxxing study's method: MiniLCB (100 LiveCodeBench tasks), a 40-turn and 10-minute cap, each trajectory abstracted to a sequence of test-outcome vectors by re-executing intermediate solutions, and mixed-effects token-cost ratios against Python. It also counts "turn to first passing solution" and runs that hit the turn cap. — [arXiv 2607.22807](https://arxiv.org/html/2607.22807)
- The esolang study uses a "sequential setup with file editing, local execution, and hidden-test grading". It argues that SWE-Bench Verified and Terminal-Bench 2.0 "compress [capability differences] into much narrower bands". — [arXiv 2606.10933](https://arxiv.org/abs/2606.10933)
- "The Devil Is in the Interface" measures consistency across repeated attempts and steps and tokens per task, over 11,700 trajectories. — [arXiv 2608.11386](https://arxiv.org/abs/2608.11386)
- τ-bench introduced pass^k, the probability that all k independent trials succeed, as a reliability metric for nondeterministic agents (2024). — [arXiv 2406.12045](https://arxiv.org/abs/2406.12045)
- Ronacher proposes measuring languages by counting agent edits and iterations. — [A Language For Agents](https://lucumr.pocoo.org/2026/2/9/a-language-for-agents/)

### Inferences (proposed design for the dat benchmark)
- **Task families.** Every task has hidden tests that run with `dat test` on the interpreter and `--compiled`, a reference solution, and where possible a TypeScript twin as the baseline.
  - A. **Spec-feature tasks.** About two per spec section: implement a function where dat departs from TypeScript, such as copy semantics, `mut`, `try`, narrowing, `int`/`float`, or exhaustive `match`.
  - B. **Repair tasks.** Code seeded with errors drawn from the 37 ❌ cases and from TypeScript habits. These measure diagnostics most directly.
  - C. **Failing-test tasks.** Runtime errors and assert failures, which measure runtime error and test output.
  - D. **Multi-file tasks** (from M6): add a feature across modules.
  - E. **Port tasks:** port a 50–150 line TypeScript program to dat.
- **Size.** A pilot of about 20 tasks at M1–M2, growing to 40–60 by M7. Keep a private held-out split, because public dat code will eventually reach training data.
- **Harness.**
  - Use 2–3 real agent harnesses: one headless commercial CLI agent, one other vendor's CLI agent, and one minimal open harness so results don't depend on one vendor.
  - Each harness runs in a container with the `dat` binary and a fixed prompt. Vary the context condition: no docs, the compact difference sheet, or the full spec.
  - Use caps similar to the tokenmaxxing study (40 turns, 10–15 minutes).
  - Pin model snapshot IDs and harness versions.
  - The Terminal-Bench task format (environment, instruction, tests, reference solution) can be reused.
- **Metrics.**
  - Success: hidden tests pass in both modes.
  - pass^k reliability.
  - Turns to first green `dat test` (count tool calls).
  - Input and output tokens per solved task, as a dat/TypeScript ratio.
  - Wall time and `dat check` iterations.
  - **Fix adoption rate**: the share of diagnostics whose next edit matches the suggested fix. This needs the JSON record.
  - Errors per run, and how many first-run errors were cascades.
- **Failure taxonomy.** Use these labels:
  - never compiled (compile-error loop)
  - compiled but wrong
  - misapplied or ignored fix
  - TypeScript habit not caught by a diagnostic
  - semantic misunderstanding (for example expected reference semantics)
  - tool misuse
  - escaped to another language
  - edited tests or cheated
  - hit the turn or time cap
  - harness error
- **Controls for nondeterminism.**
  - Use k ≥ 5 runs per task, condition and model, and k ≥ 10 for A/B decisions.
  - Use paired designs: the same tasks under both variants.
  - Randomize run order.
  - Report means with bootstrap confidence intervals, or a mixed-effects model with task as a random effect (as tokenmaxxing does).
  - Pre-register each ablation and its decision threshold before running it.
- **First ablations.** These map one-to-one to the decisions below:
  - fix line versus no fix line
  - all errors with cascade suppression versus first error only
  - text versus JSON
  - compact sheet versus full spec versus no docs
  - `dat test` that checks first versus separate `dat check` and `dat test`
- **Cost estimate.** 40 tasks × 5 runs × 3 conditions is 600 sessions. At tens of thousands of tokens each, that is on the order of 10^7 tokens per sweep. This is my estimate from session token counts in the cited studies, not a quoted figure.

### Gaps
- I found no public benchmark that measures turns-to-green across languages directly. The tokenmaxxing study is the closest.
- I could not verify the current official leaderboards for Aider polyglot, Multi-SWE-bench or Terminal-Bench 2.x. Aggregator figures conflict.

---

## 7. Error recovery so one run reports everything, without noisy cascades

### Takeaway
Use the standard recipe. A resilient parser never stops at the first error and wraps bad tokens in error nodes inside a complete tree. A checker gives broken expressions an error type that absorbs further checks. Compiler state is "tainted" after an error so follow-on errors are suppressed. The Clang rule of recovering as if the fix were applied keeps reported errors to root causes. The risk to manage is cascades, because noisy feedback measurably hurts LLM repair.

### Cited Findings
- Resilient LL parsing (matklad, 2023, the approach behind rust-analyzer's parser): "just not crash on the first error, and everything else more or less just works". Unexpected tokens are wrapped in `ErrorTree` nodes. Recovery sets decide whether to skip a token or break to an outer loop. Every loop iteration must consume a token, and a "fuel" counter catches infinite loops. "item parsing must consume at least one token (if only to report an error)." — [matklad: Resilient LL Parsing Tutorial](https://matklad.github.io/2023/05/21/resilient-ll-parsing-tutorial.html)
- Roslyn produces a full, round-trippable syntax tree even with syntax errors, using synthesized "missing" tokens and skipped-token trivia. — [Microsoft Learn: Work with syntax](https://learn.microsoft.com/dotnet/csharp/roslyn-sdk/work-with-syntax); [SyntaxToken API](https://learn.microsoft.com/en-us/dotnet/api/microsoft.codeanalysis.syntaxtoken)
- rustc's `ErrorGuaranteed` is a zero-sized proof that an error was reported, and it is used to build `ty::Error` types. The dev guide warns not to use it to decide whether to emit an error. — [rustc dev guide: ErrorGuaranteed](https://rustc-dev-guide.rust-lang.org/diagnostics/error-guaranteed.html) rustc code commonly returns an error type once inference is "tainted by errors" (`if let Some(e) = self.tainted_by_errors() { return self.tcx.ty_error_with_guaranteed(e); }`). — [rust-lang/rust source mirror](https://cdn05042023.gitlink.org.cn/dnrops/rust/commit/7f35493e9964f8d2e62ac127d2d948b60846109b) (mirror, not primary)
- Clang recovers as if the fix-it had been applied. — [Clang Internals Manual](https://clang.llvm.org/docs/InternalsManual.html)
- Noisy static feedback (pylint) gave lower LLM repair success than a generic request (49.2% vs 53.1%). — [FeedbackEval](https://arxiv.org/html/2504.06939v2)
- Go caps compiler errors at 10 by default. — [cmd/compile](https://pkg.go.dev/cmd/compile)

### Inferences
- **Recovery that fits dat.** Report one error per root cause.
  - An undefined name gets the error type, and every use of it is then silent.
  - An unknown import makes the imported names error-typed, so there is one error at the import, not one per use.
  - A struct literal with an unknown field still checks its other fields.
  - A failed narrowing reports once per variable, not once per use.
- **Module boundaries.** These are natural firewalls. The roadmap's per-module checking against imports' signatures means an error inside one module's body cannot cascade into other modules, provided exported functions declare return types, which the roadmap already proposes.
- **A testable rule** for the conformance suite: for each spec ❌ case, the checker reports exactly one diagnostic. Apply its safe fix and the checker reports zero.

### Gaps
- I did not verify tree-sitter's ERROR and MISSING node behavior or TypeScript's internal error type from primary sources in this pass.

---

## Decisions for dat-lang

Format for each decision: options, evidence for and against, recommendation, cost, the milestone that needs it settled, and whether it touches the spec. Spec changes are never assumed. Each one is listed as a question for the owner.

### D1. Default output format and labels (owner asked: "the terminal version is more for agents")
- **Options**
  - (a) Rich rustc-style text by default, with `--json` for tools. This is the roadmap today, though it labels JSON "for agents".
  - (b) JSON by default.
  - (c) A compact text default (header, location, source line, fix line, summary), with `--json` for tools and colors only on a TTY.
  - (d) Content that changes with TTY detection (rich on a terminal, JSON when piped).
- **Evidence**
  - For text, (a) and (c): JSON is about 1.9× the tokens in my proxy count. Frontier models still make mistakes reading JSON tool output ([Kate et al.](https://arxiv.org/abs/2510.15955)). Concise, informative feedback beats verbose feedback ([SWE-agent](https://arxiv.org/html/2405.15793v3)). Long inputs degrade reliability ([Chroma](https://research.trychroma.com/context-rot)). Agents edit by string replacement, so a corrected source line is directly usable ([Anthropic text editor tool](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/text-editor-tool)). rustc and tsc text is the error format models know best (inference).
  - Against text: no direct experiment exists. Format effects are model-dependent and were sometimes in JSON's favor for older models ([He et al.](https://arxiv.org/abs/2411.10541)).
  - For JSON-first, (b): Zero makes that bet, but only as a claim ([HackerNoon](https://hackernoon.com/vercels-zero-wants-compilers-to-talk-to-ai-agents)).
  - Against (d): the agent would see different output from the human and from the docs, and runs would be harder to reproduce.
- **Recommendation: (c).**
  - One text format for both people and agents.
  - Each diagnostic is at most about 6 lines: `error[CODE]: message`, `--> path:line:col`, the source line with a caret and short label, then `fix: <title>` followed by the full corrected line. Use `fix (review):` when the fix may change meaning, and `options:` with numbered alternatives when there are several.
  - End with a summary line such as `2 errors, 1 fixable with \`dat fix\``.
  - Drop the decorative blank `|` gutter lines. Use colors only on a TTY, with no other changes in content.
  - `--json` is labeled "for tools (dat fix, LSP, CI, harnesses)" and includes the `rendered` text.
  - Change the roadmap's labels to "Default output (people and agents)" and "--json (tools)".
  - Confirm with the D9 benchmark's text-vs-JSON A/B test.
- **Cost:** Small. It is one renderer, and the roadmap already plans two.
- **Milestone:** M0, where the record and renderer are defined. The A/B result should arrive by M2.
- **Spec change?**
  - Compile-error output: no, the spec doesn't define its format.
  - Test-failure output (`FAIL file:line name` / `left:` / `right:`) is defined in [20-testing.md](/Users/ismailasci/Work/Personal/dat-lang/reference/20-testing.md). **Question for owner:** should test failures adopt the same header style, for example adding an error code or a rerun command? That would change 20-testing.md.

### D2. Shape of the diagnostic record and JSON schema
- **Options**
  - (a) The roadmap's `line`/`col`/`endCol` with one `fix`.
  - (b) rustc-like: byte span plus 1-based line and column in Unicode scalar values, with fixes as child suggestions.
  - (c) LSP-native: 0-based line and character in UTF-16.
  - (d) A SARIF-shaped fix with `artifactChanges`/`replacements`.
- **Evidence**
  - rustc ships both byte offsets and character columns ([rustc JSON](https://doc.rust-lang.org/rustc/json.html)).
  - LSP defaults to UTF-16 but can negotiate UTF-8 ([LSP 3.17](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)).
  - SARIF supports multi-file fixes natively ([SARIF](https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/sarif-v2.1.0-errata01.html)).
  - ESLint and go fix require edits within a fix not to overlap ([ESLint](https://eslint.org/docs/latest/extend/custom-rules); [go fix](https://golang.google.cn/blog/gofix)).
  - dat strings are UTF-8 and lengths are in bytes (roadmap).
- **Recommendation: (b), extended.**
  - **Record fields:** `code`, `severity`, `message`, `file`, `span {byteStart, byteEnd, line, col, endLine, endCol}` (columns 1-based in Unicode scalar values, as rustc does; the unit documented in the schema), `labels[]` for secondary spans, `fixes[]`, `rendered`.
  - **Each fix:** `title`, `applicability` (D3), `isPreferred`, and `edits[]`.
  - **Each edit:** `file`, `byteStart`, `byteEnd`, `line`/`col`, `before` (the exact replaced text) and `after`. The `before` text lets tools detect stale offsets and lets agents use `str_replace`.
  - **Document shape:** emit one JSON document per run with `schemaVersion`, `diagnostics[]` and `summary`, rather than rustc's stream of objects that together aren't valid JSON. Checks take milliseconds, so streaming adds little.
  - **LSP:** the server negotiates `utf-8` and converts to UTF-16 otherwise.
  - **SARIF:** add a `--format sarif` export later for CI.
- **Cost:** Small at M0. Changing it later would break `dat fix`, the LSP and the benchmark.
- **Milestone:** M0.
- **Spec change?** No. It is tooling only.

### D3. Fix confidence levels and what `dat fix` applies
- **Options**
  - (a) Binary safe/unsafe (Ruff).
  - (b) rustc's 4 levels.
  - (c) ESLint's fix vs suggestion.
  - (d) Three levels: `safe`, `review`, and `options` (several alternatives or placeholders).
- **Evidence**
  - All mature tools separate the mechanical from the uncertain: [rustc](https://rustc-dev-guide.rust-lang.org/diagnostics.html), [Ruff](https://docs.astral.sh/ruff/linter/), [ESLint](https://eslint.org/docs/latest/extend/custom-rules), and [Clang](https://clang.llvm.org/docs/InternalsManual.html), whose uncertain fixes go on notes.
  - rustc's `Unspecified` level exists mainly because of its legacy suggestions. A new compiler doesn't need it.
  - Go treats a behavior-changing fix as a bug ([go fix](https://go.googlesource.com/tools/+/bf54bcd2f14a330f0dcffa4cf631235de771bde2/go/analysis/passes/modernize/doc.go)).
- **Recommendation: (d).**
  - `dat fix` applies `safe` fixes only. `dat fix --review` also applies `review` fixes. `options` fixes are never applied automatically.
  - Every `safe` fix must satisfy Clang's rule: the checker recovers as if it were applied.
  - Add a conformance property: apply every safe fix, re-check, and expect zero errors from that root cause and no new errors.
  - The roadmap's sentence "When there are two reasonable fixes, both are listed and neither is applied automatically" already fits `options`.
- **Cost:** Small to medium. Classifying each fix is per-diagnostic work, spread over M1–M5.
- **Milestone:** The field goes into the M0 record. `dat fix` ships at M7, but it could ship at M1 because it is cheap and helps the benchmark.
- **Spec change?** No.

### D4. Alternatives, placeholders and "did you mean", given the rule "errors state the exact fix"
- **Options**
  - (a) Always exactly one fix, even when it's a guess.
  - (b) An ordered list of alternatives with at most one preferred, none applied automatically.
  - (c) No fix when uncertain; explanation only.
- **Evidence**
  - The spec itself lists two fixes for `if (items.length)` ("`items.length > 0` or `!items.isEmpty()`").
  - ESLint uses suggestions when "several valid repairs exist" ([ESLint](https://eslint.org/docs/latest/extend/custom-rules)).
  - Placeholders are not mechanically applicable ([rustc](https://rustc-dev-guide.rust-lang.org/diagnostics.html)).
  - Wrong confident feedback is costly: noisy feedback was worse than a generic request ([FeedbackEval](https://arxiv.org/html/2504.06939v2)).
  - "Did you mean" questions are discouraged by rustc ([rustc](https://rustc-dev-guide.rust-lang.org/diagnostics.html)) but used by TypeScript ([TypeScript](https://github.com/microsoft/TypeScript/blob/main/src/compiler/diagnosticMessages.json)).
- **Recommendation: (b).** Read "exact fix" as "exact code for each reasonable fix".
  - Spelling candidates are stated as fixes (`fix (review): rename to \`clamp\``), not questions.
  - A fix with placeholders shows the placeholder as code (for example `let items: list<T> = [];` with `T` marked) and is never `safe`.
- **Cost:** Small.
- **Milestone:** M0 for the record shape. Policy applies per feature in M1–M5.
- **Spec change? Question for owner:** does "compiler errors that state the exact fix" in AGENTS.md allow several alternatives? The spec's own `items.length` example suggests it does.

### D5. Error codes, `dat explain`, and message stability
- **Options**
  - (a) No codes; messages only.
  - (b) Numeric codes (`E0204`, the roadmap's placeholder).
  - (c) Category-prefixed codes (`NUL001`, `MUT003`), in the style of Zero and Ruff.
  - (d) Readable slugs (`condition-not-bool`).
- **Evidence**
  - rustc gives a code when a longer explanation adds information, and serves it through `--explain` ([rustc](https://rustc-dev-guide.rust-lang.org/diagnostics.html)).
  - LSP `codeDescription.href` can link straight to documentation ([LSP](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)).
  - Zero claims stable codes let agents match without parsing English, with no measurement ([HackerNoon](https://hackernoon.com/vercels-zero-wants-compilers-to-talk-to-ai-agents)).
  - TypeScript's fix-all works by a stable `fixId` ([ts-morph docs](https://jsr.io/@ts-morph/ts-morph/doc/~/LanguageService.prototype.getCombinedCodeFix)).
- **Recommendation: (c) or (d).**
  - Codes are stable, never reused, and grouped by spec section, so `dat explain` maps back to `reference/`.
  - `dat explain CODE` prints the spec rule, one ❌/✅ example and the fix, generated from the `reference/` text so there is one source of truth. It also gives the `codeDescription` URL.
  - Messages may change wording. Codes may not.
  - The 37 spec ❌ lines become golden tests keyed by code.
- **Cost:** Small to medium. `dat explain` content comes from the spec.
- **Milestone:** The scheme is chosen at M0. Codes are frozen before any public release (M7).
- **Spec change? Questions for owner:**
  1. Should spec ❌ lines carry codes and fix text (for example `// ❌ NUL001: condition must be bool — fix: user != null`)? Today only 3 of 37 state a fix and 2 have no message.
  2. The roadmap says "A message changes only together with its spec line". Does the owner want the spec's ❌ wording to be normative, or only illustrative, with the compiler free to reword?

### D6. Error recovery, cascade suppression, caps and ordering
- **Options**
  - (a) Stop at the first error.
  - (b) Report all errors without suppression.
  - (c) Report all independent errors, using error types and tainting to suppress cascades, recovering as if safe fixes were applied, with a printed cap and deterministic order.
- **Evidence**
  - Resilient parsing is well understood ([matklad](https://matklad.github.io/2023/05/21/resilient-ll-parsing-tutorial.html); [Roslyn](https://learn.microsoft.com/dotnet/csharp/roslyn-sdk/work-with-syntax)).
  - Error types and tainting appear in rustc ([dev guide](https://rustc-dev-guide.rust-lang.org/diagnostics/error-guaranteed.html)), and recover-as-if-fixed in [Clang](https://clang.llvm.org/docs/InternalsManual.html).
  - Go caps at 10 ([cmd/compile](https://pkg.go.dev/cmd/compile)).
  - Noisy feedback hurts repair ([FeedbackEval](https://arxiv.org/html/2504.06939v2)), and concise feedback helps agents ([SWE-agent](https://arxiv.org/html/2405.15793v3)).
  - No turn-count study compares (a) with (c).
- **Recommendation: (c).**
  - Order diagnostics by file, then position, then code.
  - Print up to 50, then "and N more (dat check --all)".
  - Report one diagnostic per root cause, and check this in conformance tests: each spec ❌ case yields exactly one diagnostic.
  - Benchmark ablation: all errors versus first error only.
- **Cost:** Medium to large. Recovery is cheapest if designed into the parser and checker from the start (roadmap DoD step 1) and expensive to retrofit.
- **Milestone:** Parser recovery at M1. Checker poisoning grows through M1–M5. Module firewalls at M6.
- **Spec change?** No.

### D7. One command for checking and testing, and exit codes
- **Options**
  - (a) Separate `dat check` and `dat test`, where `dat test` stops on compile errors.
  - (b) `dat test` always checks the whole project first and prints compile errors plus test results in one run.
  - (c) Like (b), but tests in modules unaffected by errors still run.
- **Evidence**
  - `go test` runs vet checks as part of testing ([cmd/go](https://pkg.go.dev/cmd/go#hdr-Test_packages)).
  - Ronacher's experience is that "Ideally it either runs or doesn't" ([lucumr](https://lucumr.pocoo.org/2026/2/9/a-language-for-agents/)).
  - Ruff's 0/1/2 exit codes are a clear precedent ([Ruff](https://docs.astral.sh/ruff/linter/#exit-codes)).
  - The roadmap scores the benchmark by "turns until dat test passes", so `dat test` is the command agents will run most.
- **Recommendation: (b) now; (c) as an open question.**
  - `dat test` prints all compile diagnostics, in the D1 format, before test results.
  - Exit codes: 0 when everything passes; 1 for compile errors or failing tests (the summary line says which); 2 for tool or usage errors or an internal compiler error.
  - Keep `dat check` as the faster subset.
  - Running tests in a project with errors in other modules (c) is possible on the interpreter, but it changes what "the program" means.
- **Cost:** Small for (b). Medium for (c).
- **Milestone:** M1, when `dat test` first runs.
- **Spec change? Question for owner:** [20-testing.md](/Users/ismailasci/Work/Personal/dat-lang/reference/20-testing.md) says "`dat test` finds and runs every test". Should tests run when some module fails to type-check, and if so which ones? This is a semantics question and would change the spec.

### D8. Teaching dat to agents (in-context docs) and diagnostics for TypeScript habits
- **Options**
  - (a) Point agents at the full spec (about 95 KB, about 20–25k tokens).
  - (b) A compact "dat for TypeScript developers" sheet of 1–2k tokens, made of ❌/✅ pairs, shipped in the binary (`dat guide`) and referenced from a generated AGENTS.md snippet and an llms.txt.
  - (c) No docs; rely on diagnostics.
  - (d) (b) plus a dedicated diagnostic with a fix for every common TypeScript habit.
- **Evidence**
  - Context files raised cost by more than 20% with no success gain, and minimal "non-standard only" files are what helps ([AGENTS.md study](https://arxiv.org/pdf/2602.11988)).
  - Distilled text guidance didn't help weaker agents, while working helper code did ([esolang study](https://arxiv.org/abs/2606.10933)).
  - In translation, examples were used and grammar explanations weren't ([Aycock et al.](https://arxiv.org/abs/2409.19151)), though wrong rules hurt ([Tulu](https://arxiv.org/pdf/2602.15378)).
  - Zero ships guides inside the binary ([HackerNoon](https://hackernoon.com/vercels-zero-wants-compilers-to-talk-to-ai-agents)).
  - llms.txt has no effectiveness data ([llmstxt.org](https://llmstxt.org/)).
- **Recommendation: (d).**
  - Build a TypeScript-habit diagnostic list covering at least `===`/`!==`, `class`, `function` expressions, `any`, `undefined`, `var`, truthiness, `x++` as an expression, and `.length` on maps, each with a safe or review fix.
  - Generate the sheet from the spec and version it with the compiler.
  - Benchmark-test three conditions: no docs, the sheet, and the full spec.
- **Cost:** Small for the sheet. Medium for the habit diagnostics, which come from parser and checker work.
- **Milestone:** Sheet draft at M1, so the benchmark pilot can use it. Habit diagnostics at M1–M3.
- **Spec change? Questions for owner:**
  1. Should the spec gain ❌ examples for TypeScript habits (`===`, `class`, `any`, `undefined`)? The README already says there is "no `==` next to `===`" but shows no error.
  2. Should the compact sheet live in `reference/` or ship only with the tool?

### D9. The agent benchmark: when, what, and how to score it
- **Options**
  - (a) Build it at M7 as the roadmap says.
  - (b) A pilot of about 20 tasks at M1–M2 that grows to 40–60, used for A/B tests of D1, D3, D6, D7 and D8.
  - (c) Rely on public benchmarks.
- **Evidence**
  - Public benchmarks don't include dat, and they have saturated or compressed capability bands ([esolang study](https://arxiv.org/abs/2606.10933); [aggregator](https://benchlm.ai/benchmarks/swe-bench-multilingual)).
  - The tokenmaxxing methodology carries over directly ([arXiv 2607.22807](https://arxiv.org/html/2607.22807)).
  - pass^k handles nondeterminism ([τ-bench](https://arxiv.org/abs/2406.12045)).
  - The format and recovery decisions have no direct literature answer (sections 3 and 4), so dat must measure them itself.
- **Recommendation: (b).** Use the design in section 6:
  - Task families A–E with TypeScript twins.
  - Metrics: success in both modes, pass^k, turns to first green `dat test`, tokens per solved task as a dat/TypeScript ratio, fix adoption rate, and cascade count.
  - The failure taxonomy from section 6.
  - k ≥ 5 runs (≥ 10 for decisions), paired runs, pinned models and harnesses, two or three different harnesses, a private held-out split, and pre-registered ablations.
- **Cost:** Medium engineering (harness and about 20 tasks) plus roughly 10^7 tokens per full sweep (estimate).
- **Milestone:** Pilot at M2, when there are enough features for real tasks. The full suite becomes the M7 exit metric.
- **Spec change?** No.

### D10. Priority of the LSP versus the CLI for agents
- **Options**
  - (a) LSP early (M1–M3).
  - (b) CLI-first; LSP at M7 as planned, with diagnostics identical to the CLI.
- **Evidence**
  - Agents mostly loop through shell commands, and SWE-agent-style guardrails work through the CLI ([SWE-agent](https://arxiv.org/html/2405.15793v3)).
  - Ronacher argues a language should give agents "one unified way of working" without an LSP, and that heavy inference makes agents depend on one ([lucumr](https://lucumr.pocoo.org/2026/2/9/a-language-for-agents/)).
  - The LSP is valuable for humans and editor-embedded agents, and its positions and fix edits map from the D2 record.
- **Recommendation: (b).** Keep the CLI as the source of truth, and build the LSP as a thin adapter over the same record (UTF-8 negotiated).
- **Cost:** Large for the LSP, which is already in M7.
- **Milestone:** M7.
- **Spec change?** No.

### Summary of owner questions raised above (possible spec changes)
1. Should test-failure output adopt codes or rerun hints (changes 20-testing.md)? (D1)
2. Does "the exact fix" allow listing several alternatives? (D4)
3. Should spec ❌ lines carry stable codes and fix text, and is their wording normative? (D5)
4. Should `dat test` run tests when some modules fail to type-check? (D7)
5. Should the spec add ❌ cases for TypeScript habits, and where should the compact agent sheet live? (D8)
