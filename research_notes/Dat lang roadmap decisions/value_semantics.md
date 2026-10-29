# Value semantics in dat: memory model, representations and costs

Research notes, 28 October 2026. Scope: how to implement dat's value model (assignment copies, `mut` parameters share, values have no identity) at close to hand-written-Rust speed with no GC and no `RefCell`. Sources are primary where possible. Older items carry their year.

What the spec already fixes (read before the findings; the decisions at the end depend on it):

- "Copies are cheap because they share memory until one of them changes (copy-on-write)." "There is no garbage collector, no aliasing and no shared mutable state." — [README principle 1](../../reference/README.md)
- "Lists, sets, maps and strings share memory between copies, and copy their contents only when one copy is modified while another still exists (copy-on-write). Assigning or passing a collection never copies its elements by itself." This is a stated performance guarantee, not just semantics. — [02-variable-declaration.md](../../reference/02-variable-declaration.md)
- Parameters are constants. `mut` parameters give the callee the caller's value. One call can't pass the same value, or a value and a part of it, as `mut` twice. "Returning a new value usually reads better. Use `mut` for in-place updates of large values." — [06-functions.md](../../reference/06-functions.md)
- Closures capture only values that can't change, and "the closure keeps its own copy of each". — [06-functions.md](../../reference/06-functions.md)
- `for…of` "goes through a copy of the collection taken when the loop starts". `for (mut x of xs)` locks `xs` for the loop. — [07-control-flow.md](../../reference/07-control-flow.md)
- Strings: immutable UTF-8. `length` and positions count bytes. `slice` and `s[i]` throw `RangeError` inside a multi-byte character. `for…of` yields code points. Comparison is by code point order. — [04-primitive-methods.md](../../reference/04-primitive-methods.md), [03-operators.md](../../reference/03-operators.md)
- Sets and maps keep insertion order and compare equal regardless of order. The spec says nothing about the order after `delete`, or after re-adding a deleted key. Its delete examples (`scores.delete("bob")` → keys `["alice", "charlie"]`; `tags.delete("python")`) give the same result under swap-remove and under order-preserving remove, so they don't settle it. — [11-set-methods.md](../../reference/11-set-methods.md), [16-map-methods.md](../../reference/16-map-methods.md), [01-builtin-types.md](../../reference/01-builtin-types.md)
- Structs can contain themselves through a list, a map or a nullable field. `operator get` / `mut operator set` implement `a[i]` and `a[i] = v`, and `words["dat"] += 1` "calls get, then set". — [08-structs.md](../../reference/08-structs.md)
- A module-level `const` "is computed once, the first time it's used". Tests "run in parallel". — [17-imports.md](../../reference/17-imports.md), [20-testing.md](../../reference/20-testing.md)
- The spec's own grouping example is get, copy, push, store back: `let members = byTeam.get(person.team, []); members.push(person.name); byTeam[person.team] = members;` — [16-map-methods.md](../../reference/16-map-methods.md)

Local measurements (cited as [bench]): a micro-benchmark crate written for this note.

- Machine and toolchain: Apple M4 Pro (14 cores), macOS 15 (Darwin 24.6), rustc 1.99.0 aarch64. Release build with opt-level 3, codegen-units 1, thin LTO.
- Harness: each case is batched until a batch takes at least 20 ms, then the median of 7 batches is reported (3 batches for cases slower than 200 ms per call).
- Crates: indexmap 2.14.2, hashlink 0.12.2, imbl 7.0.2 (Arc-based by default), rpds 1.2.1 (Rc-based by default), ecow 0.3.1 (atomic refcount), compact_str 0.10.0, smol_str 0.3.7 (Arc), foldhash 0.2.0, ahash 0.8.12, rustc-hash 2.1.3.
- Source: `/private/tmp/claude-501/-Users-ismailasci-Work-Personal-dat-lang/86c21c44-25b1-4b39-b14c-f0dc9cd65a23/scratchpad/vsbench/src/main.rs`. The results are in `results.txt` beside it.
- These are single-machine micro-benchmarks. Treat the ratios as indicative, not as absolute numbers. One case was measured twice with different results (per-write `make_mut`: 518 µs and 1.23 ms), so expect up to about 2× run-to-run or layout variance on the tightest loops.

---

## 1. Mutable value semantics in research and production languages

### Takeaway
Every production-grade design that pairs value semantics with reference counting gets its speed from three things: last-use ownership transfer (moves), in-place mutation when the refcount is 1, and non-atomic refcounts for objects that never leave a thread. Borrowing helps less than expected. Swift, Lean, Koka and Roc all use copy-on-write or uniqueness-checked updates. Mojo has made collection copies explicit, and a Hylo maintainer says Hylo will do the same, because implicit copies caused accidental cost.

### Cited Findings

**Hylo / Val (Racordon et al.)**
- The JOT 2022 paper studies "Swiftlet", a core language for Swift's value model, compiled to LLVM (2022). "Fixed-size values are allocated on the stack … while dynamically sized containers use copy-on-write to mitigate copying costs." — [JOT 21(2) 2022](https://www.jot.fm/issues/issue_2022_02/article2.pdf)
- Layout (2022): structs are laid out inline ("passive data structure … each field is laid out contiguously"). An array is a pointer to heap storage holding ⟨refcount, count, capacity, elements⟩, and "the counter r serves to implement copy-on-write". — [JOT 2022](https://www.jot.fm/issues/issue_2022_02/article2.pdf)
- `inout` (2022): "inout arguments are passed as (possibly interior) pointers". For an array element, the pointer is "a pointer to the array's storage, offset by the element's index". Pointers can't escape, so they "are known to be unique". Closures have "capture-by-copy semantics, and each capture is immutable". dat's `mut` and closure rules are the same design. — [JOT 2022](https://www.jot.fm/issues/issue_2022_02/article2.pdf)
- When it copies (2022):
  - Temporaries are moved, and "the same optimization could be applied on the last use of a value".
  - Pass-by-value arguments and `let` bindings of immutable paths become aliases ("akin to Rust-like immutable borrows").
  - Stated limitation: "the callee has no way to determine whether or not that pointer refers to a value inside of a shared buffer … the caller is compelled to copy non-unique storage defensively."
  - [JOT 2022](https://www.jot.fm/issues/issue_2022_02/article2.pdf)
- Results (2022):
  - "Swift is the fastest language in the overwhelming majority of the benchmarks", compared with C++, Scala Native and Swiftlet over 1344 generated programs plus the Marr et al. micro-benchmarks.
  - Swiftlet "lags behind Swift on … heavy mutation of arrays (Bounce, NBody, Permute)". "Unnecessary reference counter checks account for most of the performance loss."
  - Proposed fix: prove uniqueness once per straight-line sequence.
  - [JOT 2022](https://www.jot.fm/issues/issue_2022_02/article2.pdf)
- Hylo now makes copies explicit: "Copies are explicit by default", with a scoped `@implicitcopy` directive, and "assigning into a `var` binding consumes the source". — [Hylo language tour](https://docs.hylo-lang.org/language-tour/bindings)
- A Hylo maintainer (23 June 2026): Hylo "will get rid of the copy-on-write types in favor of explicit copies and better control over performance". This is a discussion comment, not a spec. — [hylo-lang discussion #1910](https://github.com/orgs/hylo-lang/discussions/1910)
- Status:
  - The old compiler repo is headed "Outdated Implementation of Hylo", and work has moved to `hylo-new`. — [hylo-lang/hylo](https://github.com/hylo-lang/hylo)
  - A July 2025 talk calls the language "still in development". — [PLSS @ ECOOP 2025](https://2025.ecoop.org/details/plss-2025-papers/12/Designing-Hylo-a-programming-language-for-safe-systems-programming)

**Swift**
- "All standard library containers in Swift are value types that use COW … only copying the underlying container if the reference count of the container is greater than 1 and the container is mutated." The custom-COW example uses `isKnownUniquelyReferenced`. "Sometimes COW can introduce additional unexpected copies … This can be avoided through the usage of inout parameters." — [swift/docs/OptimizationTips.rst](https://github.com/swiftlang/swift/blob/main/docs/OptimizationTips.rst)
- The dictionary get-modify-set trap (SE-0154, 2017): "The mutating key-based subscript wraps values in an Optional. This prevents types with copy-on-write optimizations from recognizing they are singly referenced." Both `dict["one"] = (dict["one"] ?? []) + [1]` and `dict["one"]?.append(1)` "introduce an unnecessary copy". — [SE-0154](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0154-dictionary-key-and-value-collections.md)
- SE-0474 Yielding accessors (Accepted):
  - With plain get/set, "every mutating operation on x made through property is actually causing a full copy of x's backing buffer. If we appended to this property in a loop, that loop would end up being quadratic". The proposal calls this "a frequent performance pitfall".
  - `yielding mutate` replaces the unofficial `_modify`, which has existed "since Swift 5.0".
  - [SE-0474](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0474-yielding-accessors.md)
- Ownership controls added for performance:
  - SE-0366 `consume` (2023) ends a binding's lifetime, to "control the uniqueness of COW data structures and reduce retain/release calls".
  - SE-0377 `borrowing`/`consuming` (2023): the default conventions "aren't always optimal".
  - SE-0390 (2023) and SE-0427 (2024) add noncopyable types.
  - [SE-0366](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0366-move-function.md), [SE-0377](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0377-parameter-ownership-modifiers.md), [SE-0390](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0390-noncopyable-structs-and-enums.md), [SE-0427](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0427-noncopyable-generics.md)
- Exclusivity (SE-0176, 2017): "two accesses to the same variable are not allowed to overlap unless both accesses are reads". Locals, inout parameters and struct properties are checked statically. Class properties and globals are checked dynamically. — [SE-0176](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0176-enforce-exclusive-access-to-memory.md)
  - Forum report (2021): an image decoder writing through a class property ran about 10× slower than C, and 2× with `-enforce-exclusivity=none`; the time went to `swift_beginAccess`/`swift_endAccess`. — [Swift forums](https://forums.swift.org/t/huge-performance-hit-from-exclusive-memory-access-checks/54374)
  - dat has no class properties and no globals, so its `mut` rule can be checked entirely at compile time.
- `indirect enum`: "it wraps the case payload in a heap-allocated reference counted box. The reference counting part is important, since it allows sharing", and "indirect cases are immutable" (Pestov, 2024). — [Swift forums](https://forums.swift.org/t/clarification-about-semantics-of-indirect-enum/74366)
- ARC cost in Swift, from the Biased Reference Counting paper (2018), measured with Swift 4-era programs:
  - RC operations take "on average 42% of the execution time in client programs, and 15% in server programs".
  - "Swift removes up to 97% of RC operations" at compile time.
  - [Choi, Shull, Torrellas, PACT 2018](https://iacoma.cs.uiuc.edu/iacoma-papers/pact18.pdf)

**Mojo**
- Argument conventions: the default is an immutable reference. `mut` changes "are visible outside the function". `var` takes ownership. `^` transfers ownership, "leaves the original variable uninitialized". — [Mojo manual: ownership](https://mojolang.org/docs/manual/values/ownership)
- "Mojo destroys values as soon as they're no longer used … At compile time, Mojo determines the last use of each value." — [Mojo manual: death of a value](https://mojolang.org/docs/manual/lifecycle/death/)
- v25.6 (September 2025):
  - `Copyable` now means "explicitly copied (using .copy())". `ImplicitlyCopyable` is an opt-in marker. "This swaps the default behavior from being implicitly copyable to being only explicitly copyable."
  - Rationale: "fewer performance and correctness issues caused by accidental implicit copies".
  - List, Dict, Set and others are "no longer implicitly copyable". The change was staged as a warning that "will become a hard error in the next release".
  - [Mojo v25.6 changelog](https://mojolang.org/releases/v0.25.6/)
- Mojo's List deep-copies: "assignment creates a deep copy of all elements". It is not copy-on-write. — [Mojo List docs](https://mojolang.org/docs/std/collections/list/List/)
- Mojo's String has "small string optimization … up to 23 bytes inline", and "Its copy constructor is now O(1), performing string data copy lazily on mutation" (v25.3, May 2025). — [Mojo v25.3](https://mojolang.org/releases/v0.25.3/)

**Koka / Perceus** (tech report MSR-TR-2020-42 v4, the extended version of the PLDI 2021 paper)
- Precise RC insertion passes ownership into callees, so values are dropped at their last use, not at scope end. Drop specialization and fusing then remove "almost all reference count operations in the fast path". With reuse analysis, "in the fast path, where xs is uniquely owned, there are no more reference counting operations at all!" — [Perceus TR v4](https://www.microsoft.com/en-us/research/uploads/prod/2020/11/perceus-tr-v4.pdf)
- Results (2020; AMD 3600XT; times relative to Koka = 1.00; values read from the Fig. 9 chart by a research subagent):

  | Benchmark | Koka without the optimizations | Swift 5.3 | C++ |
  |---|---|---|---|
  | rbtree | 2.45 | 7.85 | 0.92 |

  - On rbtree, Koka is "within 10% of the C++ performance".
  - Java uses "almost 10× the memory of Koka".
  - [Perceus TR v4](https://www.microsoft.com/en-us/research/uploads/prod/2020/11/perceus-tr-v4.pdf)
- Atomic-only refcounts (verified in the PDF text): "we also ran our benchmarks using just atomic operations for our reference counts … We observed a slowdown from 5% (rbtree) up to 59% (nqueens)". — [Perceus TR v4](https://www.microsoft.com/en-us/research/uploads/prod/2020/11/perceus-tr-v4.pdf)
- Frame-limited reuse (ICFP 2022):
  - It is "always as fast (and sometimes up to 1.6× faster)" than the old reuse algorithm.
  - On borrowing: "borrowing can increase the memory usage of a program by an arbitrary amount … Therefore, Koka currently has no automatic borrow inference and generally only uses borrowing for built-in primitives."
  - [Lorenzen & Leijen, ICFP 2022](https://www.microsoft.com/en-us/research/uploads/prod/2023/07/flreuse.pdf)
- FP² (ICFP 2023): a `fip` keyword makes the compiler check that a function runs with no allocation and constant stack when its arguments are unshared. — [Lorenzen, Leijen, Swierstra, ICFP 2023](https://www.microsoft.com/en-us/research/uploads/prod/2023/05/fip-tr-v2.pdf)

**Lean 4 ("Counting Immutable Beans", IFL 2019)**
- Borrow-inference heuristic: a parameter "should be owned if x or one of its projections is used in a reset, or is passed to a function that takes an owned reference". Marking a parameter borrowed saves RC operations "but we also prevent reset and reuse". `Array.write a i v` "will destructively update and return the array a if it is not shared". — [arXiv 1908.05647](https://arxiv.org/pdf/1908.05647)
- Ablation (2019, Fig. 6, read from the chart by a research subagent; i7-3770). Geometric means:

  | Configuration | Geomean |
  |---|---|
  | All optimizations (base) | 1.24 |
  | Without reuse | 1.74 |
  | Without borrow inference | 1.27 |
  | All objects multi-threaded (atomic) | 1.89 |

  - The base figure is above 1.00 because two benchmarks are normalized to a third.
  - Per benchmark, without reuse: rbmap 3.23. All atomic: unionfind 2.31.
  - [arXiv 1908.05647](https://arxiv.org/pdf/1908.05647)
- Current docs:
  - "Updating a shared array rules out mutation, and requires time linear in the size of the array." — [Lean reference: Arrays](https://lean-lang.org/doc/reference/latest/Basic-Types/Arrays/)
  - "Ensuring that arrays and strings are uniquely referenced is key to writing fast code in Lean." — [Lean reference: Reference Counting](https://lean-lang.org/doc/reference/4.34.0-rc1/Run-Time-Code/Reference-Counting/)
  - In compiler IR, "reference counting, allocation, and reuse are explicit"; `dbgTraceIfShared` diagnoses sharing. — [same page](https://lean-lang.org/doc/reference/4.34.0-rc1/Run-Time-Code/Reference-Counting/)
- Lean master has `Array.markLinear`, a header bit that makes copying the array panic, described as "a gadget to spot `Array` non-linearities in profiles". This was reported by a research subagent from `src/runtime/object.cpp`; I did not re-read it. — [lean4 object.cpp](https://github.com/leanprover/lean4/blob/master/src/runtime/object.cpp)

**Roc**
- "Builtin operations on reference-counted values will update them in place when their reference counts are 1. When their reference counts are greater than 1, they will be shallowly cloned first." List literals "are never unique". — [Roc langref: expressions](https://github.com/roc-lang/roc/blob/main/docs/langref/expressions.md), [lists](https://github.com/roc-lang/roc/blob/main/docs/langref/lists.md)
- The new compiler's LIR carries explicit ARC plans with owned and borrowed parameter modes. — [roc src/lir/LIR.zig](https://github.com/roc-lang/roc/blob/main/src/lir/LIR.zig)
- The Zig rewrite reached "feature parity with the original compiler" (July 2026); 0.1.0 is targeted "later this year". — [rtfeldman.com/rust-to-zig](https://rtfeldman.com/rust-to-zig)

**dat-specific measurement: return-a-new-value style ([bench] G1)**
- dat code: `function append(xs: list<int>, x: int): list<int> { let r = xs; r.push(x); return r; }`, called as `acc = append(acc, i)` in a loop.
- With the parameter as `&T` (the roadmap's mapping), `let r = xs` is a refcount bump, and the push copies the whole list:
  - 10k calls: 9.12 ms.
  - 40k calls: 210 ms. That is 23× for 4× more calls, so quadratic.
- With an owned parameter and the caller moving `acc` at its last use:
  - 10k calls: 25.2 µs.
  - 40k calls: 94.9 µs, which is linear and about 2,200× faster.

### Inferences
- The analyses that matter most, ranked by measured effect:
  1. Last-use moves with owned parameters where a value is consumed. G1: 2,200×. Perceus without its optimizations: 2.45× on rbtree. Lean without reuse: 1.74 vs 1.24 geomean.
  2. Non-atomic refcounts for thread-private objects. Lean all-atomic: 1.89 vs 1.24. Koka all-atomic: 5–59% slower.
  3. Hoisting uniqueness checks out of loops. The JOT paper names this as Swiftlet's main loss; see §2 for local numbers.
  4. Borrow inference comes last. Lean: 1.27 vs 1.24. Koka avoids automatic borrow inference because of unbounded memory growth.
- dat's spec recommends "returning a new value" over `mut`. Without last-use moves and owned-parameter inference, that recommended style is asymptotically slower (O(n) per call). These analyses therefore decide complexity, not just constant factors, and so they can't wait for a late speed milestone.
- The industry trend (Mojo's v25.6 change, Hylo's stated direction) is toward explicit copies. Both of those languages copy collections deeply, though, so their implicit copies cost O(n). With copy-on-write, dat's implicit copies cost O(1), and the hazard moves to the *first mutation after a copy* (§6). The spec's choice stays coherent, but it needs the analyses above and diagnostics for the get-copy-modify-store pattern.
- Swift and Lean both found it necessary to document which patterns stay in place. Lean goes further and ships tools to detect accidental sharing. dat's agent-first goal points the same way: an agent can't see an accidental O(n²) without a tool.

### Gaps
- No published fraction of RC operations removed by Lean, Koka or Roc. The only figure is Swift's "up to 97%", from 2018.
- No systematic Swift exclusivity-check cost data, only forum anecdotes.
- Roc has no published benchmark numbers, and Feldman's 2021 quicksort talk figures were not found in a primary written form.
- The exact PLDI 2021 camera-ready numbers were not checked; the tech report v4 was used.
- No 2023–2026 Hylo paper on projections or subscripts was found.

---

## 2. Collection representations for list (and the shape of generated code)

### Takeaway
`Rc<Vec<T>>` with `make_mut` (the roadmap's choice) is the only option that meets both the spec's "a copy never copies elements" guarantee and the "close to Rust" goal:

- A copy costs 2.6 ns, and iteration runs at plain `Vec` speed.
- Copying a nested list costs O(outer length), not O(total elements).
- A uniqueness check costs 1.1–1.8× per push, and 4.5–10× per element write when it isn't hoisted out of the loop. Codegen must hoist it.

Persistent vectors are 7–250× slower on everyday operations. A plain `Vec` deep-copied on every copy would contradict the spec text.

### Cited Findings
All numbers in this section are from [bench] unless a link says otherwise.

**Copying a list (A1)**

| Representation | n = 1,000 | n = 100,000 |
|---|---|---|
| `Vec<i64>` clone (deep copy) | 96 ns | 9.6 µs |
| `Rc<Vec>` clone | 2.6 ns | 2.6 ns |
| `Arc<Vec>` clone | 3.5 ns | 3.5 ns |
| `ecow::EcoVec` clone | 3.6 ns | 3.6 ns |
| `imbl::Vector` clone | 7.2 ns | 7.2 ns |
| `rpds::Vector` clone | 2.4 ns | 2.4 ns |

**Push 1M integers into an empty list (A2)**

| Representation | Time | vs `Vec` |
|---|---|---|
| `Vec::push` | 0.88 ms | 1.0× |
| `Rc<Vec>` + `make_mut` per push | 1.61 ms | 1.84× |
| `Arc<Vec>` + `make_mut` per push | 2.49 ms | 2.84× |
| `EcoVec` | 3.47 ms | 3.96× |
| `imbl` | 6.34 ms | 7.2× |
| `rpds` | 36.8 ms | 42× |

**Read 1M integers (A3)**

| Representation | Sum by iterator | Sum by `xs[i]` loop |
|---|---|---|
| `Vec` | 75.9 µs | 76.0 µs |
| `Rc<Vec>` | 74.6 µs | — |
| `EcoVec` | 74.1 µs | — |
| `imbl` | 759 µs (10×) | 6.60 ms (87×) |
| `rpds` | 2.04 ms (27×) | 2.16 ms (28×) |

**`xs[i] += 1` over a unique 1M list (A4, X1)**

| Code shape | Time |
|---|---|
| `Vec`, index loop or `iter_mut` | 114–117 µs |
| `Rc<Vec>`, `make_mut` on every write | 518 µs and 1.23 ms (two runs) |
| `Rc<Vec>`, `get_mut().unwrap()` on every write | 538 µs |
| `Rc<Vec>`, one `make_mut` before the loop, then `iter_mut` | 116 µs (equal to `Vec`) |
| `Rc<Vec>`, one `make_mut` before the loop, then an index loop over `&mut Vec` | 349–363 µs |
| `Arc<Vec>`, `make_mut` on every write | 1.96 ms |
| `EcoVec`, `make_mut` on every write | 516 µs |
| `imbl` `get_mut` | 18.9 ms (161×) |
| `rpds` `set_mut` | 29.5 ms (252×) |

**`list<list<int>>` of 1000×1000: copy, then change one element (A5)**

| Representation | Time |
|---|---|
| `Vec<Vec>` deep clone | 190.8 µs |
| `Rc<Vec<Rc<Vec>>>` (outer copy is 1,000 refcount bumps, plus one inner list copied) | 0.76 µs (251× faster) |
| `imbl` | 0.94 µs |
| `EcoVec<EcoVec>` | 35.8 µs |

**A list held in a struct field, pushed 1M times through `addItem(mut cart, x)` (A6)**
- `Vec` field: 2.63 ms. `Rc<Vec>` field with `make_mut`: 2.87 ms (+9%).

**Copying a map (B6)**
- `Rc<IndexMap>` clone: 2.7 ns. A deep `IndexMap` clone of 100k entries: 318 µs (B4 baseline).

**Implementation notes**
- `Rc::make_mut`: "If there are other `Rc` pointers to the same allocation, then `make_mut` will clone the inner value to a new allocation … This is also referred to as clone-on-write." If only `Weak` pointers remain, they are disassociated. — [std::rc::Rc](https://doc.rust-lang.org/std/rc/struct.Rc.html)
- `Rc::unwrap_or_clone` "will avoid cloning the inner value where possible", which is a direct lowering for moving a value out of a list. — [std::rc::Rc](https://doc.rust-lang.org/std/rc/struct.Rc.html)
- ecow's own comparison: `Arc<Vec<T>>` requires "two allocations instead of one". `EcoVec` "takes up two words" and stores refcount, capacity and elements in one allocation. — ecow 0.3.1 crate docs ([docs.rs/ecow](https://docs.rs/ecow))
- In ecow 0.3.1 the refcount is an `AtomicUsize` (seen in `src/vec.rs`). That explains its Arc-like clone cost. — [ecow source](https://github.com/typst/ecow)
- imbl's default pointer is `ArcK`, so it is atomic. rpds defaults to `RcK` and needs `new_sync()` to be shared across threads: "there is a performance cost to make data structures thread safe". — imbl 7.0.2 `src/shared_ptr.rs`; [rpds README](https://github.com/orium/rpds)

### Inferences
- `Rc<Vec<T>>` has a double indirection: pointer → RcBox{strong, weak, Vec header} → buffer. It showed no cost on reads (A3). The cost sits in the per-write uniqueness check, and only when codegen issues it per element.
- Codegen rules that recover `Vec` speed:
  1. Call `make_mut` once per statement, or once per loop when the loop body touches the list only through that borrow.
  2. Lower `for (mut x of xs)` to `make_mut(..).iter_mut()`.
  3. Prefer slices or iterators over indexing through `&mut Vec`. The index loop over `&mut Vec` obtained from `make_mut` ran about 3× slower than `iter_mut`; this is probably lost bounds-check elimination or vectorization (not investigated).
- A custom single-allocation COW vector (header {rc, len, cap} plus elements, non-atomic) would save one pointer hop and the unused weak count. ecow shows this layout works, but ecow's measured push path was slower than `Rc<Vec>`. Keep the representation behind an opaque `dat_rt::List<T>` so it can change later.
- Copying nested collections is cheap only because every level is its own refcounted buffer. A deep-copy representation makes `list<list<T>>` copies O(total elements): 251× slower here.

### Gaps
- Not measured: `VecDeque`-backed lists for `shift`/`unshift`. The spec has both, and `shift` on a `Vec` is O(n), so a queue loop with `shift()` is O(n²). This needs its own decision.
- Not measured: a custom non-atomic EcoVec-style vector.
- Not measured: interpreter-mode costs.

---

## 3. Insertion-ordered set and map, and hashing

### Takeaway
`indexmap` is the right base. Its lookups match `HashMap` (with foldhash, 7.2 vs 7.6 ms per 1M hits), it iterates 10× faster, and inserts are about 1.76× slower. Its removal is the problem:

- `swap_remove` is O(1) but moves the last entry into the hole, so the order is no longer insertion order.
- `shift_remove` keeps the order but is O(n): 1.39 s for 10k removals from a 100k map, against 62 µs for `swap_remove`.

Iterating in insertion order suggests "delete keeps the remaining order; re-adding goes to the end". The spec doesn't say this. Hashing with foldhash is about 2× faster than SipHash, and because iteration follows insertion order, the hasher and its random seed can't change program output in either run mode.

### Cited Findings
**What indexmap documents**
- indexmap "preserves insertion order (except after removals)". Order is kept "as long as you don't call `.remove()`, `.swap_remove()`" … "The alternate `.shift_remove()` does preserve relative order." — [indexmap README](https://github.com/indexmap-rs/indexmap)
- `remove` is deprecated: "`remove` disrupts the map order -- use `swap_remove` or `shift_remove` for explicit behavior." — indexmap 2.14.2 `src/map.rs` ([docs.rs](https://docs.rs/indexmap))
- The `ordermap` crate is "a wrapper over `indexmap` with stronger ordering properties". — [indexmap README](https://github.com/indexmap-rs/indexmap)

**Reference semantics in other languages**
- JavaScript: "Iteration happens in insertion order, which corresponds to the order in which each key-value pair was first inserted into the map by the set() method". The spec requires "access times that are sublinear". — [MDN Map](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Map)
- Python: "Dictionaries preserve insertion order. Note that updating a key does not affect the order. Keys added after deletion are inserted at the end." Changed in 3.7: order is guaranteed. — [Python stdtypes](https://docs.python.org/3/library/stdtypes.html#mapping-types-dict)

**Measured: 1M random u64 keys ([bench] B1–B3)**

| Hasher | Insert: HashMap / IndexMap / LinkedHashMap | Lookup hits: HashMap / IndexMap / LinkedHashMap | Iterate: HashMap / IndexMap / LinkedHashMap |
|---|---|---|---|
| SipHash-1-3 (std) | 32.9 / 37.2 / 89.3 ms | 19.3 / 15.7 / 10.5 ms | 2.08 ms / 218 µs / 1.47 ms |
| foldhash::fast | 18.8 / 33.1 / 45.3 ms | 7.6 / 7.2 / 4.5 ms | 2.11 ms / 213 µs / 1.52 ms |
| foldhash::quality | 19.7 / 33.1 / 49.5 ms | 8.5 / 8.0 / 5.6 ms | 2.08 ms / 216 µs / 1.34 ms |
| ahash | 19.5 / 33.1 / 47.9 ms | 9.0 / 7.0 / 6.2 ms | 2.11 ms / 232 µs / 1.54 ms |
| FxHash (rustc-hash 2) | 18.0 / 33.8 / 44.7 ms | 5.9 / 7.9 / 4.5 ms | 2.11 ms / 220 µs / 1.67 ms |

- The lookups were done in insertion order, which favors `LinkedHashMap`'s sequentially allocated nodes. Treat its lookup advantage as an artifact.

**Measured: string keys ([bench] B5, 1M lookups over 100k keys)**

| Hasher | HashMap | IndexMap |
|---|---|---|
| SipHash-1-3 | 20.8 ms | 27.9 ms |
| foldhash::fast | 12.1 ms | 15.1 ms |
| ahash | 12.7 ms | 16.8 ms |
| FxHash | 11.3 ms | 13.7 ms |

**Measured: removal ([bench] B4, 100k entries, removing every 10th key)**

| Operation | Time |
|---|---|
| `IndexMap` clone alone (baseline) | 318 µs |
| clone + 10k `swap_remove` | 380 µs (about 62 µs for the removals) |
| clone + 10k `shift_remove` | 1.39 s |
| clone + one `retain` pass | 680 µs |
| `hashlink::LinkedHashMap` clone alone | 2.74 ms (8.6× IndexMap) |
| `LinkedHashMap` clone + 10k removals (O(1), keeps order) | 3.00 ms |

**foldhash's own claims**
- foldhash is "minimally DoS-resistant". "foldhash does not claim to provide HashDoS resistance against interactive attackers." Do not use it when "you expect foldhash to have a consistent output across versions or platforms". — foldhash 0.2.0 README ([github.com/orlp/foldhash](https://github.com/orlp/foldhash))
- On Apple M2, u64 lookup hits take 1.78 ns with foldhash-f vs 8.53 ns with SipHash. The geometric mean over all its benchmarks is 4.41 vs 21.94.
- fxhash "has structural weaknesses": on `u64hibits`, lookup hits take 67.72 ns vs 1.77 ns for foldhash, "due to everything colliding".
- hashbrown's `default-hasher` feature uses foldhash (hashbrown 0.17.1 `Cargo.toml`).
- Sizes: `std HashMap<u32,u32>` = 48 bytes, foldhash map = 40, ahash = 64.
- [foldhash README](https://github.com/orlp/foldhash)

**Equality**
- dat requires that sets and maps "compare their contents regardless of insertion order". — [01-builtin-types.md](../../reference/01-builtin-types.md)
- indexmap's `PartialEq` checks that the lengths match and then looks each key up in the other map (indexmap 2.14.2 `src/map.rs`, `impl PartialEq for IndexMap`). It compares by lookup, not by position. The `ordermap` wrapper is the order-sensitive variant ([indexmap README](https://github.com/indexmap-rs/indexmap)). So indexmap already matches dat's equality semantics.

### Inferences
- Order-preserving O(1) deletion needs tombstones. The design CPython's dict uses:
  - an entries vector with holes;
  - a hash index of positions;
  - compaction when holes exceed about half the entries, or on resize.

  dat-rt can build this on `hashbrown::HashTable<usize>` plus `Vec<Option<(K, V)>>`. It keeps indexmap's dense iteration (holes are skipped) and its cheap clone for copy-on-write. `LinkedHashMap` also has O(1) ordered deletion, but it clones 8.6× slower and iterates 7× slower, and both matter under copy-on-write.
- Determinism across run modes: iteration order is insertion order, independent of hash values. So a per-process random seed is invisible in output, and the interpreter and compiled code print identical results even with different seeds. Only performance varies from run to run.
- Custom `hash(): int` methods feed an `i64` into the hasher. Results are not observable either.
- HashDoS: SipHash costs about 1.7–2.5× on lookups. foldhash with a random seed defeats non-adaptive collision attacks only. Today dat has no I/O standard library, so hostile keys are a later concern. Revisit when HTTP or JSON is specified.

### Gaps
- Not built or measured: a tombstone map. Its cost is inferred from CPython's design and indexmap's iteration numbers.
- No measurement of `set` operations (`union` and friends) or order-insensitive equality cost.
- No data on hash quality for dat's derived struct and list hashes.

---

## 4. Strings

### Takeaway
Every string operation in the spec maps directly onto Rust `str`: byte length, byte positions with a `RangeError` off a character boundary, iteration by code point, and comparison by code point.

The roadmap's `Rc<str>` has one serious problem. `s += t` in a loop is O(n²): 298 ms for 40k appends, against 49–160 µs for representations that append in place when the string is unique.

Small-string-optimized types clone in about 0.3–0.4 ns when inline, against 2.6 ns for `Rc<str>` and about 11 ns for `String`.

### Cited Findings
**Spec fit**
- Spec: length and positions count bytes; `slice`/`s[i]` throw inside a multi-byte character; strings are immutable; `for…of` yields code points; `<` compares by code point. — [04-primitive-methods.md](../../reference/04-primitive-methods.md), [03-operators.md](../../reference/03-operators.md)
- "The byte-value lexicographic sorting order of UTF-8 strings is the same as if ordered by character numbers." So Rust's byte-wise `Ord for str` gives dat's code-point order with no extra work. — [RFC 3629](https://www.rfc-editor.org/rfc/rfc3629)

**Sizes in bytes ([bench] C0)**

| Type | Size |
|---|---|
| `String` | 24 |
| `Box<str>` | 16 |
| `Rc<str>`, `Arc<str>` | 16 |
| `Option<Rc<str>>` | 16 |
| `Rc<String>` | 8 |
| `CompactString` | 24 |
| `SmolStr` | 24 |
| `EcoString` | 16 |
| `Option<EcoString>` | 24 (no niche, so `string?` would grow) |

**Copy cost, clone + drop ([bench] C1)**

| Type | Inline | Heap |
|---|---|---|
| `String` | — | 10.7–12.4 ns at every length |
| `Rc<str>` | — | 2.55–2.63 ns |
| `Arc<str>` | — | 3.57 ns |
| `CompactString` | 0.40 ns (≤ 24 bytes) | 11.5–12.0 ns (deep copy) |
| `SmolStr` | 0.37 ns (≤ 23 bytes) | 3.74 ns |
| `EcoString` | 0.31 ns (≤ 15 bytes) | 3.6 ns |

**Append `"ab"` in a loop ([bench] C2)**

| Type | 10k appends | 40k appends |
|---|---|---|
| `String::push_str` | 3.2 µs | 49.0 µs |
| `Rc<str>`, a new string on every `+=` | 2.34 ms | 298 ms (127× for 4× more work) |
| `Rc<String>` + `make_mut` | 7.8 µs | 53.8 µs |
| `EcoString::push_str` | 36.6 µs | 160 µs |
| `CompactString::push_str` | 16.3 µs | 92.8 µs |

- Building a 1000-word line while every intermediate value stays alive (forcing a copy each step) costs 79 µs with EcoString (C3).

**Crate documentation**
- `CompactString`: "Stores up to 24 bytes on the stack", "`Clone` is `O(n)`". — compact_str 0.10.0 README ([github.com/ParkMyCar/compact_str](https://github.com/ParkMyCar/compact_str))
- `SmolStr`: "`Clone` is `O(1)`", inline "Up to 23 bytes long", "Unlike `String`, however, `SmolStr` is immutable". It is Arc-based. — [smol_str README](https://github.com/rust-analyzer/smol_str)
- `EcoString`: "16 bytes … 15 bytes of inline storage and starting from 16 bytes it becomes an EcoVec<u8>", "is efficient to clone even when spilled, and at the same time mutable". — ecow 0.3.1 docs ([docs.rs/ecow](https://docs.rs/ecow))

**Prior art for the same choice**
- Mojo's String uses inline storage up to 23 bytes and copies lazily on mutation. — [Mojo v25.3](https://mojolang.org/releases/v0.25.3/)
- Lean's docs say strings, like arrays, must be uniquely referenced to be fast. — [Lean reference](https://lean-lang.org/doc/reference/4.34.0-rc1/Run-Time-Code/Reference-Counting/)

### Inferences
- In dat, "strings are immutable" means no `s[i] = …`. It doesn't prevent `let s = ""; s += x;`, which rebinds `s`. So building a string with `+=` in a loop (and template strings in loops) is common code. With `Rc<str>` it is O(n²); with an appendable copy-on-write buffer plus a last-use move it is O(1) amortized. The fix is a representation plus last-use analysis, not a spec change.
- A string type that fits dat:
  - an opaque `dat_rt::Str`;
  - small strings stored inline (15 bytes in a 16-byte value, or 23 in 24);
  - otherwise a refcounted heap buffer with capacity;
  - `+=` and `s = s + t` appending in place when the buffer is unique;
  - a non-atomic count to match the list decision.

  EcoString works as a stand-in today, but its count is atomic and its push is about 3× slower than `String`. `Rc<String>` gives in-place appends now, at the price of a second pointer hop on reads.
- A 16-byte inline type without a niche makes `string?` 24 bytes. A custom type can reserve a niche.

### Gaps
- No measurement of `split`, `slice` or substring sharing. Slices could share the parent buffer, but that keeps large strings alive, so it is a separate decision.
- No measurement of hashing strings in each representation.

---

## 5. Structs and recursive types

### Takeaway
Unboxed structs (deriving `Clone`, and `Copy` when every field is a scalar) are the right default.

- Copying a 5-reference-field struct costs 4.0 ns with `Rc` fields, 7.9 ns with `Arc` fields, 2.7 ns as an `Rc<Struct>`, and 84 ns with owned `String` fields.
- Copy-then-modify on a large list of structs is 2.3× cheaper with boxed structs, but a mutating loop is 2× slower.

Recursive nullable fields mapped to `Option<Box<T>>` (the roadmap's choice) make every copy a deep copy. Worse, the generated recursive `Drop` and `Clone` overflow the stack and abort the process at 1M and 200k nodes. That breaks the "every runtime error can be caught" rule.

### Cited Findings
All from [bench] unless a link says otherwise.

**Copying one `User` (5 string/list fields, 3 scalars) (E1)**

| Layout | Time |
|---|---|
| Unboxed, `Rc` fields | 4.02 ns |
| Unboxed, `Arc` fields | 7.94 ns |
| `Rc<User>` | 2.67 ns |
| Owned `String`/`Vec` fields | 83.6 ns |

**`list<User>` of 10k: copy, then set `users[5].age` (E2)**

| Layout | Time |
|---|---|
| `Rc<Vec<User>>` (10k struct clones) | 42.3 µs |
| `Rc<Vec<Rc<User>>>` | 18.4 µs |
| `Vec<User>` with owned fields | 948 µs |

**`for (mut u of users) u.age += 1` over 10k (E3)**
- Unboxed: 6.5 µs. Boxed with `make_mut` on each element: 12.9 µs.

**10k-node linked list (F1)**

| Operation | `Option<Box<Node>>` | `Option<Rc<Node>>` |
|---|---|---|
| Copy | 226 µs (deep, recursive) | 2.9 ns |
| Copy, then set `head.value` | 231 µs | 11.5 ns (copies one node) |

**Stack overflow ([bench] overflow mode, 8 MB main-thread stack)**
- Dropping an `Option<Box<Node>>` list: fine at 200k nodes; aborts at 1M with "thread 'main' has overflowed its stack … fatal runtime error: stack overflow, aborting", exit code 134.
- Cloning it with the derived `Clone`: aborts at 200k nodes.
- Dropping an `Option<Rc<Node>>` chain: also aborts at 1M nodes.

**Why**
- "We can't drop the contents of the Box after deallocating, so there's no way to drop in a tail-recursive manner!" The fix is "manually write an iterative drop". — [Learning Rust With Entirely Too Many Linked Lists](https://rust-unofficial.github.io/too-many-lists/first-drop.html)

**Prior art**
- Swift's `indirect` payloads use a refcounted shared box, which makes copies O(1). — [Swift forums, Pestov 2024](https://forums.swift.org/t/clarification-about-semantics-of-indirect-enum/74366)
- Hylo lays structs out inline, with copy-on-write only in arrays (2022). — [JOT 2022](https://www.jot.fm/issues/issue_2022_02/article2.pdf)

### Inferences
- Copy-on-write boxes (`Option<Rc<T>>` with `make_mut`) give O(1) copies and path copying on mutation. The cost is one refcount check per mutated node. This matches Swift `indirect` and keeps the value semantics.
- Every recursive type needs a generated iterative `Drop`. That includes recursion through `list<T>` and `map<K, T>`: a degenerate 1M-deep `TreeNode` overflows the same way. A Box representation also needs an iterative `Clone`. This connects to roadmap Q1 (stack overflow): drop and clone depth must be bounded by the runtime, not by the user's recursion.
- Struct size decides whether boxing pays off. An automatic "box structs above N reference fields" rule could be an M8 optimization that stays invisible to semantics.
- Scalar-only structs can derive `Copy`, which makes their copies free, as in hand-written Rust.

### Gaps
- No measurement of deep trees through `list<TreeNode>` with iterative drop.
- No measurement of the size threshold where boxing structs wins.
- Interface values (`Box<dyn I>` with a generated clone) were not measured.

---

## 6. Rc vs Arc, and how concurrency changes the choice

### Takeaway
Uncontended `Arc` costs about 1.25× `Rc` for a single clone+drop on Apple M4 Pro. The gap grows to 1.8–5× in batches, in `make_mut`, and in struct copies. A shared count hammered from 8 threads is 125× slower.

The literature reports whole-program slowdowns of 5–59% (Koka), a geomean of 1.24 → 1.89 (Lean) and about 25% of execution time (Swift) when every count is atomic. Koka and Lean both keep counts non-atomic by default and mark objects that become thread-shared; Roc chooses statically.

The spec already creates sharing across threads today, through module constants that are "computed once" combined with tests that "run in parallel". Rust can't put an `Rc` in a `static`, so this has to be decided before M6, not "after concurrency is specified".

### Cited Findings
**Measured on Apple M4 Pro ([bench] D)**

| Operation | `Rc` | `Arc` | Ratio |
|---|---|---|---|
| clone + drop | 2.79 ns | 3.49 ns | 1.25× |
| 1000 clones into a Vec, then drop all | 0.71 µs | 3.54 µs | 5× |
| `make_mut` on a unique value + write | 1.08 ns | 1.94 ns | 1.8× |
| `make_mut` per element over 1M (X1) | 518 µs | 1.96 ms | 3.8× |
| struct copy, 5 reference fields (E1) | 4.0 ns | 7.9 ns | 2× |

**Contention on one `Arc` (D4), clone+drop, wall-clock per operation per thread**

| Threads | Same `Arc` | One `Arc` per thread |
|---|---|---|
| 1 | 3.50 ns | 3.59 ns |
| 2 | 17.6 ns | 3.64 ns |
| 4 | 95.6 ns | 3.63 ns |
| 8 | 438.7 ns | 3.91 ns |

**Literature**
- BRC (2018):
  - Atomic vs non-atomic increment: 13.84 vs 5.77 ns (2.40×) on Haswell.
  - Removing atomics (unsafely) "reduces the average execution time … by 25%" in Swift.
  - Over 99% of client-program objects are thread-private, yet "Swift is forced to use atomic operations always".
  - Biased RC cut client execution time by 22.5%.
  - [PACT 2018](https://iacoma.cs.uiuc.edu/iacoma-papers/pact18.pdf)
- Koka:
  - "we encode the reference count for thread-shared objects as a negative value. This enables us to use a single inlined test". Objects are marked recursively (`tshare`) when passed to a new thread (2020).
  - Current kklib: "Negative reference counts use atomic increment/decrement (for thread shared objects)".
  - [Perceus TR v4](https://www.microsoft.com/en-us/research/uploads/prod/2020/11/perceus-tr-v4.pdf), [kklib.h](https://github.com/koka-lang/koka/blob/master/kklib/include/kklib.h)
- Lean:
  - "`m_rc` field also encodes whether the object is single threaded (> 0), multi threaded (< 0), or reference counting is not needed (== 0)".
  - `markMT` marks reachable objects at `Task.mk`/`Task.bind`.
  - `lean_is_exclusive` is true only for single-threaded objects with rc == 1, so arrays reachable from other threads always copy on update.
  - [lean.h](https://github.com/leanprover/lean4/blob/master/src/include/lean/lean.h), [arXiv 1908.05647](https://arxiv.org/pdf/1908.05647)
- Roc: "reference counts on values the host can see are updated atomically … when the compiler can tell that a value never reaches the host … it updates that value's reference count without atomic instructions." — [Roc langref](https://github.com/roc-lang/roc/blob/main/docs/langref/expressions.md), [LIR.zig](https://github.com/roc-lang/roc/blob/main/src/lir/LIR.zig)
- Swift (SE-0302, 2021): value types may be `Sendable` "if public mutators are implemented with COW", e.g. `extension Array: Sendable where Element: Sendable {}`. Swift's refcounts are always atomic. — [SE-0302](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0302-concurrent-value-and-concurrent-closures.md), [RefCount.h](https://github.com/swiftlang/swift/blob/main/stdlib/public/SwiftShims/swift/shims/RefCount.h)
- PEP 703 (free-threaded CPython) uses biased refcounting. Overall overhead on pyperformance: 6% single-threaded and 8% multi-threaded on Intel Skylake; 5% and 7% on AMD Zen 3. "The largest contribution to execution overhead is biased reference counting followed by per-object locking." — [PEP 703](https://peps.python.org/pep-0703/)
- Rust: `Rc` is `!Send` and `!Sync`. — [std::rc::Rc](https://doc.rust-lang.org/std/rc/struct.Rc.html)

### Inferences
- Contention is the real danger, not the atomic instruction itself. Values that many threads copy at once, such as a module constant read in parallel tests, would bounce one cache line. Lean's "rc == 0, no counting" (persistent objects) and Koka's sticky range avoid this for long-lived shared values.
- Rust's standard library has no `Rc`/`Arc` hybrid. Copying the Koka/Lean scheme (a signed count; negative means atomic; a sentinel means immortal) needs a custom header in dat-rt and `unsafe` code inside dat-rt only. Generated code stays safe.
- The roadmap's "swap `Rc` to `Arc` in one place" works only if dat-rt owns the pointer type from the start: a single `dat_rt::Shared<T>` used by List, Map, Set, Str and boxes.
- Module constants are a concrete case now. In compiled mode, a module `const` of type `set<int>` held in a `static` can't use `Rc` (`!Sync`). Options:
  1. Use `Arc` everywhere.
  2. Initialize per thread with `thread_local!`. This changes "computed once" into "once per thread", which is observable if an initializer prints.
  3. Freeze constants as immortal, shared objects that are never counted and always copied on mutation. This needs a custom header.
  4. Run compiled tests in separate processes.

  The interpreter faces the same problem if it runs tests on threads.

### Gaps
- No measurement on x86 for this note. The BRC Haswell figure (2.40× per increment) is the x86 reference point.
- No whole-program measurement of Rc vs Arc for dat-like code. The M8 benchmark suite should run both.
- Not checked: whether Swift's `incrementNonAtomic` runtime entry points are used by any shipping compiler mode.

---

## 7. Pitfalls, and how to detect them

### Takeaway
The main hazards under copy-on-write are first mutations that copy while another reference is alive:

- get-copy-modify-store on a map value: 21× slower at 50k items, quadratic. The spec's own §16 example has this shape.
- return-a-new-value with borrowed parameters: 2,200× slower at 40k, quadratic.
- `operator get`/`set` on structs, which by design can't mutate in place (Swift's SE-0474 problem).
- `+=` on strings: quadratic with `Rc<str>`.
- `for…of` when the body mutates the list: one extra O(n) copy, +38% in the measured case.

All of these can be detected with one liveness/ownership analysis in the shared IR. That analysis also yields the spec's "modified copy is never read" error. The interpreter must use the same runtime types and the same move decisions, or its complexity will differ from compiled code.

### Cited Findings
**The spec's §16 grouping example ([bench] A7)**

| Version | 10k items, 10 keys | 50k items, 10 keys | Growth for 5× more items |
|---|---|---|---|
| As written, naive copy-on-write | 1.39 ms | 19.9 ms | 14.3× (superlinear) |
| Same code, compiler moves the value out of the map and back (take and restore) | 194 µs | 943 µs | 4.9× (linear) |
| Rust `entry` API, in place | 94 µs | 464 µs | 4.9× |

**`for…of` with `queue.push` in the body ([bench] A8, 100k list)**

| Version | Time |
|---|---|
| `Vec`, index loop, no snapshot | 172.7 µs |
| `Rc` snapshot (one O(n) copy on the first push) | 238.4 µs |
| `Vec` with an eager snapshot clone | 238.2 µs |

- A loop that doesn't mutate the list pays only a refcount bump.

**Prior art**
- Swift: get/set accessors make appends "quadratic", "a frequent performance pitfall" (SE-0474). The dictionary-subscript copy trap was fixed with in-place accessors (SE-0154, 2017). — [SE-0474](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0474-yielding-accessors.md), [SE-0154](https://github.com/swiftlang/swift-evolution/blob/main/proposals/0154-dictionary-key-and-value-collections.md)
- Lean ships `dbgTraceIfShared`, which reports when a value is shared at a point the programmer expects to be unique, and advises: "knowing how to diagnose unique reference issues can be very important when writing performance-critical code". — [Lean reference](https://lean-lang.org/doc/reference/4.34.0-rc1/Run-Time-Code/Reference-Counting/)
- Lean master's `Array.markLinear` makes copying a marked array panic (reported by a subagent from `src/runtime/object.cpp`). — [lean4 object.cpp](https://github.com/leanprover/lean4/blob/master/src/runtime/object.cpp)
- Hylo's direction is that the compiler "will offer to insert these copies for you", making copies visible. — [Hylo tour](https://docs.hylo-lang.org/language-tour/bindings)
- Lean's compiler IR and Roc's LIR make RC operations, ownership and reuse explicit IR instructions. — [Lean reference](https://lean-lang.org/doc/reference/4.34.0-rc1/Run-Time-Code/Reference-Counting/), [Roc LIR.zig](https://github.com/roc-lang/roc/blob/main/src/lir/LIR.zig)

### Inferences
- **One analysis, four uses.** A backward liveness pass over the typed IR finds, for each use of a `let`, parameter or temporary, whether it is the last use. From that the compiler gets:
  - (a) the spec's "modified copy is never read" error, for a copy that is mutated and not live afterwards;
  - (b) move vs clone at every copy site;
  - (c) owned vs borrowed parameters;
  - (d) drop placement.

  The roadmap places (a) in M2 and (b) in M8. They should be the same M2 pass.
- **Interpreter parity.** If the moves are recorded in dat-ir and both modes execute them, both modes have the same complexity. If only codegen moves, the interpreter runs G1 and A7 quadratically while compiled code runs them linearly. The interpreter also needs:
  - place-based evaluation, so `xs.push(1)` and `m[k].push(x)` mutate the environment slot through `&mut` instead of evaluating `xs` to a temporary copy;
  - dat-rt's `List<Value>`, `Map<Value, Value>` and `Str` as its runtime representation.
- **Hidden copies at field access.** Reading `user.tags.length` or `users[i].name` borrows and copies nothing. `let t = user.tags` copies (a refcount bump), unless `user` is dead afterwards, in which case it is a partial move. `addItem(mut order.cart, order.cart.items[0])` passes a `&mut` and a `&` into the same value, which Rust rejects (E0502). The spec allows it, so codegen must clone the non-`mut` argument first. That is correct, since dat parameters are copies.
- **The get-copy-modify-store rewrite.** When `let x = m.get(k, d)`, `m[k]`, or a field is followed only by mutations of `x` and then `m[k] = x` (with no reads of `m[k]` in between), lower it to take-and-restore. This is the A7 "moved out" row. The rewrite is semantics-preserving because values have no identity.
- **`operator get`/`set` can't be fixed by the compiler.** `get` is user code returning a copy. For list-valued or map-valued results, `h[k].push(x)` is O(n) per call. Swift needed a language feature (yielding `mutate`) for exactly this.
- **Detection tooling that fits agent-first goals:**
  - dat-rt counts copy-on-write copies, and the bytes copied, by source location, in both modes. `dat test --report-copies` and the JSON diagnostics then report "this `push` copied 2,000 elements 5,000 times".
  - The conformance suite runs complexity checks: run a case at n and 4n and fail when the time ratio exceeds about 8. In A7 the ratios were 14.3× (quadratic) vs 4.9× (linear) for 5× n, which is easy to tell apart.
- **Lists used as queues.** `shift()` and `unshift()` on a `Vec` are O(n), so a BFS-style loop over a list is O(n²). Either use a ring buffer in dat-rt or document the cost.

### Gaps
- No interpreter prototype was measured.
- No data on how often agent-written code hits these patterns.
- The complexity-ratio threshold is a heuristic from two points, not validated.

---

## Decisions for dat-lang

Each decision lists the options, evidence with numbers, a recommendation, its cost, the milestone that needs it settled, and whether it needs a spec change. Spec changes are framed as questions for the owner, not assumptions.

### D1. Representation of list, set and map copies

- **Option A: `Rc<Vec<T>>` / `Rc<OrderedMap>` with `make_mut`** (roadmap).
  - For: copy 2.6 ns at any size. Iteration equal to `Vec`. Nested copy 0.76 µs vs 191 µs for a deep copy. A struct field updated via `mut` costs +9%. It satisfies the spec sentence "Assigning or passing a collection never copies its elements". Swift, Lean and Roc all ship this model.
  - Against: push 1.84× `Vec` when checked per push. Per-element writes 4.5–10× slower unless the check is hoisted. One extra pointer hop.
- **Option B: owned `Vec`, deep copy on every copy, move on last use** (Mojo's model).
  - For: no checks, and the fastest mutation.
  - Against: copies are O(n): 9.6 µs per 100k ints, 191 µs for a nested 1000×1000. It contradicts [02-variable-declaration.md](../../reference/02-variable-declaration.md) ("never copies its elements by itself") and README principle 1. Mojo now forces explicit `.copy()` for this reason.
- **Option C: persistent vectors and maps (imbl, rpds).**
  - For: O(1) copy plus O(log n) update, with no quadratic first-mutation copies.
  - Against: push 7–42×, iteration 10–27×, indexed read 28–87×, write 161–252× slower than `Vec`. Fails "close to hand-written Rust".
- **Recommendation: A**, behind opaque `dat_rt::List<T>`, `dat_rt::Map<K, V>` and `dat_rt::Set<T>` types so the internals (for example a single-allocation non-atomic header) can change later.
- **Cost:** low; the roadmap already plans it.
- **Milestone:** M0 spike, landing in M2.
- **Spec change:** none for A. B would need one; C needs none but fails the speed goal. Question for the owner only if B is wanted: should copies become explicit, as Mojo and (per a maintainer) Hylo are doing? The evidence says not needed if D2 lands.

### D2. Where last-use and ownership analysis lives, and when

- **Option A: codegen-only optimization in M8** (roadmap: "move on last use instead of cloning").
  - For: keeps M2 smaller.
  - Against:
    - The spec's recommended style, `acc = f(acc, x)` with a local copy that is pushed and returned, is O(n) per call without it: 210 ms vs 95 µs at 40k (2,200×).
    - The §16 grouping example is 21× slower at 50k and quadratic.
    - The interpreter and compiled code would differ in complexity, which breaks the two-modes rule in spirit.
    - The literature: Perceus without these optimizations, 2.45× on rbtree; Lean without reuse, 1.74 vs 1.24 geomean; Swiftlet lost most of its gap to "unnecessary reference counter checks".
- **Option B: one liveness pass on the shared typed IR in M2.** It marks every use as move, borrow or copy, and both modes execute those marks. The same pass emits the "modified copy is never read" error that M2 already needs.
  - For: the same complexity in both modes from day one. One analysis serves four uses (§7).
  - Against: more M2 work. Parameter-convention inference adds interprocedural facts, but exported signatures already exist and modules check separately, so this is the same per-signature caching.
- **Recommendation: B.** In M2:
  - last-use moves for locals and temporaries;
  - the "modified copy never read" error;
  - take-and-restore for get-copy-modify-store on map entries and fields.

  In M2 or M3, owned-parameter inference using the Lean heuristic: a parameter is owned when the body stores it, returns it, or copies it into a `let` that is mutated. Hoisting uniqueness checks out of loops stays in M8.
- **Cost:** medium, about one dataflow pass plus IR annotations.
- **Milestone:** M2.
- **Spec change:** none. Question for the owner: should the reference document which patterns are guaranteed in place (for example "`acc = f(acc, x)` doesn't copy `acc` when `acc` isn't used afterwards")? Lean documents this. It is a documentation addition, not a semantic change.

### D3. Parameter passing convention in generated Rust

- **Option A: always `&T`** (roadmap).
  - For: simple; no refcount traffic for read-only parameters.
  - Against: a parameter that is stored or returned needs a clone (bump), and a copy that is then mutated copies O(n) (G1). A call like `f(mut order.cart, order.cart.items[0])` is legal dat but a Rust E0502 error, so codegen must clone the non-`mut` argument.
- **Option B: always owned.** The caller moves at last use, otherwise clones.
  - For: no defensive copies in callees.
  - Against: a refcount bump at most call sites, for parameters that are only read.
- **Option C: `&T` by default, owned when inferred consumed** (Lean's heuristic; Swift's owned convention for initializers and setters).
  - For: combines the strengths of A and B.
  - Against: the convention is part of a function's compiled signature, so it is inferred per function and cached with exported signatures.
- **Recommendation: C**, plus the clone rule for overlapping `mut` and non-`mut` arguments in one call. Closures passed directly to `map` and friends compile to generic `impl Fn`. Stored closures compile to `Rc<dyn Fn>`, with captures moved at last use or else cloned.
- **Cost:** medium (shares D2's pass).
- **Milestone:** M2.
- **Spec change:** none.

### D4. Code shape for element writes and `mut` loops

- **Options:** `make_mut` per element write (naive) vs one uniqueness check per statement or loop, then `&mut [T]` or `iter_mut`.
- **Evidence:**
  - Per write: 518 µs–1.23 ms (`Rc`), 1.96 ms (`Arc`).
  - Hoisted with `iter_mut`: 116 µs, equal to `Vec` at 114 µs.
  - Hoisted with an index loop over `&mut Vec`: 349 µs.
- **Recommendation:**
  - Lower `for (mut x of xs)` to `make_mut(xs).iter_mut()` in M2. The spec already locks `xs` for the loop, so a single borrow is legal.
  - Hoist checks out of `for (let i…)` loops that write `xs[i]` in M8, when the loop has no other path to `xs`.
  - Prefer slices and iterators in generated code.
- **Cost:** low for M2, medium for M8.
- **Milestone:** M2 / M8.
- **Spec change:** none.

### D5. Order of set and map after `delete`, and the data structure

- **Option A: indexmap with `swap_remove`.**
  - For: O(1) removal (about 6 ns per removal measured); simplest.
  - Against: the last entry jumps into the hole, so the order is no longer insertion order. This differs from JavaScript Map and Python dict, which dat users and agents will expect. It is surprising for "insertion order".
- **Option B: indexmap with `shift_remove`.**
  - For: keeps JS/Python order; simple.
  - Against: O(n) removal: 1.39 s for 10k removals from a 100k map. Loops that delete from a map or set go quadratic.
- **Option C: a tombstone ordered map in dat-rt** (CPython-dict design: `hashbrown::HashTable<usize>` plus an entries `Vec` with holes, compacted on growth or when holes exceed half).
  - For: O(1) amortized removal, JS/Python order, indexmap-class iteration and clone cost.
  - Against: about 300–600 lines of custom runtime code plus tests. Not measured here.
- **Option D: `hashlink::LinkedHashMap`.**
  - For: O(1) ordered removal.
  - Against: insert 1.4× indexmap, iteration 7× slower, clone 8.6× slower. Clone cost matters for copy-on-write.
- **Recommendation:** first settle the semantics with the owner. Then C, or B as a stopgap in M2 behind the same API (same semantics as C, so swapping later changes only speed). Use indexmap's order-insensitive equality semantics. Re-adding a deleted key goes to the end; assigning to an existing key keeps its position.
- **Cost:** B is low; C is medium.
- **Milestone:** semantics before M2; C by M8 at the latest.
- **Spec change: yes, a clarification. Question for the owner:** "After `delete`, do the remaining entries keep their relative order (as in JavaScript and Python), and does re-adding a deleted key put it at the end?" The current examples don't decide it.

### D6. Hash function and seeding

- **Option A: SipHash-1-3** (std default).
  - For: HashDoS-resistant.
  - Against: 2.5× slower u64 lookups (19.3 vs 7.6 ms per 1M) and 1.7× slower string lookups (20.8 vs 12.1 ms).
- **Option B: foldhash::fast with a random seed per process** (hashbrown's default hasher).
  - For: on par with FxHash without its collision cliffs (fxhash takes 67.7 ns on high-bit keys vs 1.77 ns for foldhash).
  - Against: "does not claim to provide HashDoS resistance against interactive attackers".
- **Option C: a fixed-seed hasher.**
  - For: bit-for-bit reproducible internal layout.
  - Against: weakest against collision attacks. Output is already deterministic with insertion order, so it buys nothing visible.
- **Recommendation: B** in both modes, through dat-rt. Iteration is insertion-ordered, so the seed never reaches program output and the conformance diff stays exact. Record HashDoS as a known limitation, and revisit when an HTTP or JSON standard library is specified.
- **Cost:** none.
- **Milestone:** M2.
- **Spec change:** none. Question for the owner, later: should a server-facing standard library require a DoS-resistant hasher, as a global setting or per map?

### D7. String representation

- **Option A: `Rc<str>` wrapper** (roadmap).
  - For: 16 bytes; 2.6 ns copy; simple.
  - Against: `s += t` in a loop is O(n²): 298 ms vs 49 µs for 40k appends. Every small string allocates.
- **Option B: inline small strings plus an appendable refcounted heap buffer** (EcoString-like; ideally custom and non-atomic).
  - For: 0.3 ns copy inline; 3.6 ns copy on the heap; in-place append when unique (EcoString: 160 µs for 40k). Mojo and Swift both use small strings with lazy copies.
  - Against: custom code if non-atomic. EcoString's push is 3× slower than `String` (atomic, no tuning). `Option<EcoString>` is 24 bytes.
- **Option C: plain `String`, deep copy on every copy.**
  - For: fastest appends.
  - Against: about 11 ns and an allocation per copy. It violates the spec's statement that strings share memory between copies.
- **Recommendation: B**, as an opaque `dat_rt::Str`. In M1, wrap `ecow::EcoString` (or `Rc<String>`) to get in-place append immediately. Replace it with a non-atomic custom buffer that has a niche for `string?` when D10 settles the header. Lower `s += t` and `s = s + t` to in-place append when `s` is unique and not used afterwards (D2). Every spec string operation maps to `str` (RFC 3629 guarantees byte order equals code point order).
- **Cost:** low for the M1 wrapper; medium for the custom type.
- **Milestone:** M1, since strings land there.
- **Spec change:** none.

### D8. Struct layout

- **Option A: unboxed struct, `derive(Clone)`, plus `Copy` when all fields are scalars.**
  - For: 4.0 ns copy with 5 reference fields; a `mut` loop over 10k in 6.5 µs; no allocation per struct.
  - Against: copying a list of structs clones every struct (42 µs per 10k).
- **Option B: `Rc<Struct>` with `make_mut`.**
  - For: 2.7 ns copy; list copy-and-modify 18 µs per 10k.
  - Against: an allocation per struct and a pointer hop; the mutating loop is 2× slower (12.9 µs).
- **Recommendation: A** (roadmap), plus derived `Copy`. Optionally box very large structs automatically as an M8 optimization; this is invisible to semantics.
- **Cost:** low.
- **Milestone:** M3.
- **Spec change:** none.

### D9. Recursive types and deep drop and clone

- **Option A: `Option<Box<T>>`** (roadmap).
  - For: no refcount.
  - Against: a copy is a deep copy (226 µs per 10k nodes vs 2.9 ns). The derived `Clone` aborts at 200k nodes and `Drop` at 1M, uncatchable, which breaks the error-tolerance rule.
- **Option B: a copy-on-write box `Option<Rc<T>>` with `make_mut`** (Swift `indirect` semantics).
  - For: O(1) copy, O(depth) path copy on mutation (11.5 ns to copy and set the head).
  - Against: a refcount check on each mutation through the box. Drop is still recursive.
- **Recommendation: B, plus a generated iterative `Drop`** (worklist) for every type that can contain itself, including through `list` and `map`. Tie the remaining deep-recursion cases to roadmap Q1's depth guard.
- **Cost:** medium.
- **Milestone:** M3. The iterative-drop requirement should also be in the M4 torture suite.
- **Spec change:** none. It relates to the owner's open Q1 (stack overflow).

### D10. Refcount header: Rc, Arc, or a custom one, and module constants under parallel tests

- **Option A: std `Rc` now, switch to `Arc` later** (roadmap).
  - For: simplest; fastest single-threaded (`Rc` clone+drop 2.79 ns).
  - Against: `Rc` can't live in a `static`, and the spec's lazily "computed once" module constants plus parallel tests need cross-thread sharing in M6.
- **Option B: `Arc` everywhere now.**
  - For: solves module constants and future threads.
  - Against: 1.25× (single clone) to 5× (batched clones) slower refcount operations; 1.8× `make_mut`; 3.8× per-element writes when unhoisted; 2× struct copies. The literature reports 5–59% (Koka), geomean 1.24 → 1.89 (Lean) and 25% (Swift) whole-program cost. Contention on hot shared values reaches 439 ns per operation at 8 threads.
- **Option C: a custom dat-rt header with a signed count** (positive = thread-local, non-atomic; negative = thread-shared, atomic; sentinel = immortal, never counted), as in Koka and Lean.
  - For: single-thread speed, plus a path to concurrency. Module constants become immortal objects shared by all test threads with no counting and no contention.
  - Against: custom `unsafe` code inside dat-rt (generated code stays safe); a recursive "mark shared" walk when values cross threads.
- **Option D: A, with module constants initialized per thread (`thread_local!`)** or compiled tests run in separate processes.
  - For: no custom header.
  - Against: "computed once" becomes "once per thread", which is observable if an initializer prints, and wastes work.
- **Recommendation:** route every refcount in dat-rt through one `dat_rt::Shared<T>` from M0. Start with `Rc` (A). Choose between C and D for module constants before M6, with C as the long-term target, because it also prepares the concurrency spec. Measure A vs B vs C on the M8 suite.
- **Cost:** A is low; C is medium-high.
- **Milestone:** the abstraction in M0/M2; the module-constant answer in M6; the concurrency model after M8.
- **Spec change: possibly. Questions for the owner:**
  1. May a module constant's initializer have observable effects such as `print`? If not, per-thread initialization is indistinguishable from "computed once".
  2. When concurrency is specified, should values passed to another task be shared (marked atomic) or deep-copied?

### D11. `for…of` snapshot cost

- **Option A: snapshot as a refcount bump** (roadmap).
  - For: free when the body doesn't mutate the list. One O(n) copy when it does: +38% on a 100k push-in-body loop, the same as an eager clone.
  - Against: the hidden copy when the body mutates.
- **Option B: borrow the collection when the compiler proves the body doesn't mutate it.**
  - For: no refcount traffic.
  - Against: it is an optimization of A, not an alternative.
- **Recommendation:** A, plus B in M2. Optionally add a hint when a `for…of` body mutates the iterated list ("this loop copies `queue` once").
- **Cost:** low.
- **Milestone:** M2.
- **Spec change:** none.

### D12. Pitfall detection and the `operator get`/`set` gap

- **Options:**
  - (a) the liveness-based "modified copy is never read" error (spec, M2);
  - (b) automatic take-and-restore for get-copy-modify-store (21× at 50k; M2, from D2);
  - (c) a COW copy counter in dat-rt reported by `dat test --report-copies` and in `--json` diagnostics, in both modes (precedent: Lean's `dbgTraceIfShared` and `Array.markLinear`);
  - (d) complexity-ratio conformance cases (n vs 4n);
  - (e) a language feature for in-place projection through `operator get` (Swift needed `_modify`/`yielding mutate` for this; SE-0474).
- **Recommendation:** (a), (b) and (c) in M2; (d) in M8. Raise (e) as a question.
- **Cost:** (c) is low; (d) is low; (e) is a spec design.
- **Spec change:** (e) only. **Question for the owner:** `h[k].push(x)` on a struct with `operator get`/`mut operator set` copies the whole value each time (get returns a copy, set stores it back). Is that acceptable, documented as a cost, or should operator methods gain an in-place form?

### D13. Interpreter representation

- **Option A: the interpreter has its own value model** (for example `Rc<RefCell<…>>` or cloned values).
  - For: quick to write.
  - Against: different complexity from compiled code (A7, G1), and it violates the `RefCell` rule in spirit.
- **Option B: the interpreter uses dat-rt types** (`List<Value>`, `Map<Value, Value>`, `Str`, `Shared<T>`), evaluates assignable expressions as places (`&mut` into environment slots), and executes the move, borrow and copy marks from D2.
  - For: the same asymptotics in both modes; one runtime to test. Lean's IR and Roc's LIR likewise make RC operations explicit in a shared IR.
  - Against: the interpreter must model places and moves carefully.
- **Recommendation: B.**
- **Cost:** medium; it is the M1–M2 interpreter design.
- **Milestone:** M1 for places; M2 for collections and moves.
- **Spec change:** none.

### D14 (raised by the measurements, not in the brief). `shift` and `unshift` on lists

- **Options:**
  - `Vec`: `shift` is O(n), so a queue loop is O(n²).
  - a ring buffer (`VecDeque`) inside `dat_rt::List`: O(1) at both ends; `iter_mut` and slices become two slices.
  - document the cost.
- **Recommendation:** decide in M2 with a benchmark. Agents write BFS loops with `shift()`.
- **Spec change:** none.

---

Questions for the owner, collected (none are assumed answered):

1. **D5:** Does `delete` keep the relative order of the remaining set or map entries, and does re-adding a deleted key put it at the end, as in JavaScript and Python? Needed before M2.
2. **D10:** May module-constant initializers have observable side effects? This decides whether per-thread initialization is allowed under parallel tests. Needed before M6.
3. **D10:** When concurrency is specified, are values sent to another task shared (atomic or marked) or deep-copied?
4. **D12:** Should `operator get`/`set` gain an in-place form, or is copying through it a documented cost?
5. **D2:** Should the reference document which patterns are guaranteed to stay in place (`acc = f(acc, x)`, `s += t`, map-entry update)? This is documentation only.
6. **D6, later:** Is a DoS-resistant hasher required once networking enters the standard library?
7. **D1, only if wanted:** Mojo and (per one Hylo maintainer) Hylo moved to explicit copies. dat's spec makes copies implicit and cheap through copy-on-write. Keeping the spec as written is consistent with the evidence, provided D2 lands in M2.
