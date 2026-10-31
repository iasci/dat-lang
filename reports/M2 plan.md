# M2 plan: values and collections

M2 settles dat's value model: lists, sets, maps and tuples that copy on write, `mut` at the call site, arrow functions and closures, `for...of`, and one liveness pass that makes the copies disappear where the spec promises they do. In the [roadmap](https://claude.ai/artifact/JNuKMm6CfFmLtBKFjjF4hG) it delivers the collection parts of §01, §02 and §03, the closure and `mut` parts of §06, `for...of` in §07, `range` in §09, and §10, §11 and §16. Structs, nullable types, unions and `match` come in M3; declared errors in M4; user generics and interfaces in M5; files that import files and module constants in M6.

Every later milestone builds on what M2 decides, and M2 also changes the compiler's insides more than any milestone since M0: types stop being a flat list, and the IR gets places, moves and copies. So M2 starts with groundwork that changes no behaviour, and this plan says, for each decision, what it means for M3 to M8.

## Decisions this plan builds on

From the owner:

- **Order (Q25).** Sets and maps keep insertion order. `delete` keeps the remaining order, a re-added key goes last, and setting an existing key keeps its place (§11, §16).
- **Callbacks that throw (Q8).** The list, set or map a method was called on is left as it was (§12). An inconsistent comparator throws nothing and keeps every element; the order of its calls isn't specified but is the same in both modes (§10).
- **No-copy patterns (Q27).** A variable's last use hands over the value itself, and §02 lists the patterns that never copy.
- **Exported constants (Q18, Q19).** An exported `const` states its type unless its value does; an exported arrow declares its return type and `throws` (E1704, §17).
- **`split("")`** gives the string's characters, as `[...s]` does, with no empty strings at the edges (§04).
- **The agent pilot** uses whichever models give the best price for what they measure (step 12).

From the roadmap, already adopted:

- `list<T>` is `dat_rt::List<T>` over a copy-on-write `Rc<Vec<T>>`, checked for uniqueness once per statement or loop, not per element. Sets and maps are an ordered hash map in dat-rt, hashed with foldhash and a random seed per process. Insertion order means the seed never reaches output.
- Every reference count goes through one `Shared<T>`, so a later switch to `Arc` or a signed count stays inside dat-rt.
- A function parameter of function type is `&dyn Fn`; a stored function is a shared `dyn Fn`. The roadmap left dat-rt's built-in methods to a measurement at M2. That's now done: they take their callback as a generic parameter (step 4).
- One liveness pass over the typed IR decides moves, owned or borrowed parameters, drops and take-and-restore, and gives E0202. Both modes execute its marks, so both have the same complexity.
- Out of memory: one allocator wrapper in dat-rt prints the same message and aborts in both modes.

## What the spec can prove

Of the 175 spec cases that can't run today, 51 need nothing but M2: 8 in §16, 8 in §02, 7 in §06, 6 in §01, 6 in §22, 5 in §07, 3 in §11 and the rest spread out. Most examples in §10 also use a struct, such as `people.toSortedBy((p) => p.age)`, so they wait for M3. As in M1, M2 proves itself mostly with its own example programs and goldens in `examples/`.

## Groundwork, before any feature

None of these steps changes behaviour. Each is done when every M1 example and benchmark gives byte-identical IR and Rust (step 1 of the perf-check skill) and all conformance passes.

1. **Interned types in dat-ir.** Today the checker's `Ty` and the IR's `Type` are flat enums with no compound types (`dat-check/src/lib.rs:21`, `dat-ir/src/lib.rs:74`), and roadmap decision 8 ("interned types") isn't built. Replace both with `TypeId` handles into one table per compilation, with kinds for `list`, `set`, `map`, tuples and function types, and room for M3's nullable, union and struct types and M5's type parameters. Equal types get equal ids, so invariance needs no code. Whether a type is equatable, hashable or comparable is worked out once per type. Ids never reach output or generated names: unions are ordered by content (Q22). The checker's `Poison` stays a checker-only id.
2. **Checking against an expected type.** Today a type is worked out bottom-up and compared afterwards. M2 needs `check(expr, expected)`: `[]` and `{}` have no type of their own, a list literal is a tuple or a set only where one is expected, and a callback's parameter types come from the function type it's passed to.
3. **Split the four largest files along their seams**, as pure moves: `dat-check/src/body.rs` (2,330 lines) into statements, expressions, places, calls, the method table, built-in functions and name suggestions; the parser into items, types, statements and expressions; the interpreter into values, the machine, statements and its typed compilers; codegen into names, types, statements and expressions. M2 adds to every one of them.
4. **One visitor in dat-ir.** Four crates walk the IR with their own code today. Each new IR node should be added in one place.
5. **`Shared<T>` becomes a type of its own.** Today it's an alias of `Rc` (`dat-rt/src/lib.rs:56`) and the interpreter uses `Rc` directly. A type of its own is what the later count header needs, and moving to it is cheap before lists and maps use it. Rust can't turn a custom pointer into a `dyn Fn` on stable, so dat-rt gets one closure constructor per number of parameters. The interpreter's `Value` gets a check that it stays 16 bytes.
6. **Spans and a line table in the IR.** E0202 must say "a copy of `user.tags`" and point at it. M6 adds a file index to the same table.
7. **One general path in the interpreter for every kind of node.** Today the typed compilers for `int`, `float`, `bool`, strings and errors each handle locals, calls and conditions again. Let the general path handle every kind, and the typed compilers keep only their fast shapes and fall back to it; otherwise each new M2 node needs six arms. Identical IR doesn't prove the interpreter's speed, so this step also reads the interpreter's lines in `perf`.
8. **Spec cases keyed by content.** A case's id is its file, line and code today, so every spec edit above a block moves its cases and forces re-blessing, as the Q27 and Q18 edits just did. Key a case by its section, a hash of its block and its position inside the block. `--bless` refuses to run while a case regresses, so a real regression can't be blessed away.

## What M2 builds

In this order. A step is done when it parses, reproduces the spec's ❌ lines, gives the same output in both modes, and stays inside the speed budgets.

1. **Collections in dat-rt.**
   - `List<T>`, `Set<T>`, `Map<K, V>`; tuples are Rust tuples.
   - A list is a `Vec` plus a head offset. `shift` moves the first element out and advances the head; the buffer compacts when the dead front outgrows the live part. Measured on this machine:
     - A breadth-first search over 100k nodes takes 441 ms with `remove(0)`, against 0.5 ms for hand-written Rust. With the head offset it runs at hand-written speed.
     - Reads, writes, `push`, iteration and `sort` stay within noise of a plain `Vec`, and the live part is always one slice, which `sort`, `iter_mut` and `&[T]` arguments need.
     - A ring buffer (`VecDeque`) also fixes `shift`, but costs 27–99% on reads, iteration, writes and `push`, and has to be made contiguous before it can be passed as a slice.
   - Moving a value out of a slot in safe Rust leaves something behind. So every dat type has a cheap placeholder: `0`, `false`, or a shared empty string, list, set or map. Cloning instead of moving was 50× slower on a queue of lists, because the stale copy keeps each list shared. The same placeholder serves take-and-restore and moves out of a variable inside a `try` (step 9).
   - The ordered map is written in dat-rt on hashbrown's `HashTable`, not forked from indexmap. Its index holds a slot number and the hash; its slots hold the entries in insertion order, with holes left by `delete` and a cursor at the first live entry, so a map used as a queue stays linear.
     - Measured against indexmap: inserting into an empty map is 3.7× faster, a counter (`m[w] = m.get(w, 0) + 1`) 1.5× faster, and deleting 10k keys from 100k takes 0.4 ms instead of 4.5 s.
     - Against Rust's unordered `HashMap`, it costs 3–11% on counter code and about 2× on lookups that miss the cache in a 1M-entry map. That's the price of insertion order: two dependent loads per lookup.
     - indexmap is about 12k lines built around dense entries and positional access, which dat doesn't have. hashbrown is a rust-lang crate that std's own `HashMap` copies, and `HashTable` is fully safe to use. Pin its exact version.
   - Holes slow down iteration: with 49% holes, just under a trigger of holes > live entries, a loop runs 7× slower than over a dense map. Compaction of 100k entries costs about as much as two loops. So the trigger is set by a dat benchmark, starting at holes > a quarter of the live entries, and the decision register's row 40 gets the number.
   - One set of value traits in dat-rt for equality, hashing, ordering and printing, implemented by the primitive types, the collections and tuples, and by the interpreter's `Value`. `==`, `includes`, `unique`, `sort()`, `print` and `assert` all go through them, so the modes can't differ.
   - Printing as §04 says: `[1, 2]`, `{1, 2}`, `{"alice": 10}`, strings quoted inside a collection.
   - `KeyError`, and the `RangeError`s for indexes, `pop` and `shift` on an empty list, and a `range` step of `0`.
   - Sorting with a callback sorts a copy with Rust's stable sort and swaps it in only when it finishes. That costs 3% on 1M ints and 15% on many small lists. When the list is shared, copy-on-write copies it anyway, so the copy is free. Sorting without a callback can't throw, so it sorts in place.
     - Rust's sort leaves the list in an unspecified order when the comparator throws, which Q8 rules out; the copy fixes that.
     - Rust's sort panics with "does not correctly implement a total order" when it notices an inconsistent comparator (on this toolchain, only for 21 or more elements). Q8 says that's no error, so dat-rt catches exactly that panic and keeps the sorted copy, which still holds every element. The panic hook stays silent for exactly that message. A conformance case pins it, so a toolchain upgrade that changes it fails loudly.
     - A merge sort written in dat-rt would avoid the panic, but it measured 1.4–5× slower.
2. **Collection types and literals in the checker.** `list<T>`, `set<T>`, `map<K, V>` and `[A, B]` in type positions; list, set, map and tuple literals; `xs.length` and `s.size`; indexing; `in` on a list, set or map; `==` on collections. `[]` without an expected type is E0206. A tuple index must be a constant (E0101). Set elements and map keys must be hashable.
3. **Places and changes in place.** `xs[i] = v`, `m[k] = v`, `xs[i] += 1` and the methods that change their receiver, through a place in the IR: a variable followed by indexes and keys, with M3's fields as one more step. `const` is deep, so a change through a `const` at any depth is E0201. Index and key operands are evaluated first and hoisted in the generated Rust, so a `&mut` borrow never overlaps them.
4. **Methods.** One table for every method of every built-in type, replacing today's two lists that must agree (`body.rs:2148` and `body.rs:2194`). An entry gives the signature in a small type language (`T`, `U`, `list<T>`, a callback with an optional index parameter), whether it changes the receiver, whether it throws, and what the element type must support. One routine fills in the type parameters from the receiver and the arguments; M5's generic functions reuse it. dat-rt has one generic implementation per method, whose callbacks may fail, so the interpreter and compiled code call the same algorithm in the same order.
   - The callback is a generic parameter, not `&dyn Fn`. Measured: through `&dyn Fn`, `map` over 1M ints runs at 1.8× hand-written Rust and a sort with a comparator at 1.4–1.6×. Generic, `map` runs at hand-written speed and the sort matches a hand-written `sort_by`. Generated code passes an arrow function straight in; the interpreter uses one instantiation with its own closure type.
   - The price is build time: about 6–7 KB of machine code per sort call site. A synthetic crate with 1,200 callbacks built in 0.85 s instead of 0.44 s for a dev build, and 24.3 s instead of 6.5 s for release. A program has far fewer callbacks than that, and exit criterion 13 checks the build budgets.
   - User functions that take a function keep `&dyn Fn`, so each is compiled once.
   - Changing a list in a loop takes the uniqueness check once per loop and works on a `&mut [T]`. Taking it as a `&mut Vec` measured 8–13% slower, because every write reloads the `Vec`'s header.
   - `max(...scores)` and the other `@dat/math` functions that take any number of values accept a spread list, and throw a `RangeError` for an empty one.
5. **Arrow functions, function types and closures.** Arrow functions with an expression or a block body, function types such as `(value: int) => int`, and named functions passed as values. Parameter types come from the expected function type, or E0602. A closure copies the `const` variables it uses when it's created; using a `let` variable is E0603. A function value prints as `<function>` and can't be compared. A call through a callback is a trace site in both modes, and an arrow function's frame is `<arrow>`. Exported arrows follow E1704.
6. **Callback methods.** `map`, `filter`, `flatMap`, `reduce`, `forEach`, `find` with a fallback, `findIndex`, `some`, `every`, `count`, `sort`, `sortBy`, `toSorted`, `toSortedBy` and `groupBy`, with the index parameter where §10 allows it. A callback that throws leaves the receiver as it was.
7. **`for...of`, destructuring and spread.** `for...of` over a list, a set, a map's `[key, value]` entries, a string's characters and `range`, which builds no list in a loop. The loop goes through the collection as it was when the loop started; that costs a count increment, not a copy. Destructuring of lists and tuples with defaults, `...rest` and `_`, in declarations and loop headers, with a `RangeError` when a list is too short. Spread in list, set and map literals, and of a string into its characters. `split`, where an empty separator gives the characters.
8. **`mut`.** `mut` parameters and arguments, including an element (`bump(mut counts[0])`): a `mut` parameter is `&mut` in Rust. One call can't pass a value as `mut` twice, or a value and part of it; that gets a new code in §06. `for (mut x of xs)` changes each element in place through `iter_mut()`, and `xs` can't be used inside the body (E0703).
9. **The liveness pass.** A new crate, `dat-own`, that depends only on dat-ir and runs after lowering. Until it lands, every read copies, which is correct but slow.
   - It goes backward over the IR's structured regions; loops repeat until nothing changes. No control-flow graph is needed.
   - Error edges: inside a `try`, every operation that can throw also keeps alive what the `catch`, the `finally` and the code after them read. A `mut` parameter is alive at the function's error exit, since the caller can catch. This needs a may-throw bit on each IR node, with a summary per function.
   - Every read of a variable becomes a move, a borrow or a copy. A value is dropped right after its last use, in both modes; Rust drops at the end of a scope, which would leave a list shared longer in compiled code than in the interpreter.
   - Parameters start borrowed and become owned when the body stores, returns, or copies and then changes them, or passes them on to an owned parameter. Mutually recursive functions are solved together, and the result is part of the function's interface.
   - Take-and-restore for a value taken out of a map or a list element and stored back: the take swaps a placeholder into the entry, so the order is kept. A throw before the first change puts the value back. A throw after it, when the map can be read after the error, means the value is copied instead.
   - Generated Rust runs a `try` body as a closure, so a move inside it takes the value out of the variable and leaves the placeholder, instead of moving the variable into the closure, which wouldn't compile when the variable is used after the `try`.
   - E0202 when a `let` copy is changed and never read, with its fix.
   - A debug check in the conformance runs: no use after a move, exactly one drop per value.
   - `dat test --report-copies`: a count of copies per source location, in both modes. The conformance suite uses it to check that every pattern §02 promises makes no copy.
   - §02's text gets the exceptions this pass implies (see "Settled without asking").
10. **Out of memory.** The allocator wrapper prints one message and aborts the same way in both modes. It needs `unsafe`, so it gets the same narrow exception as `stack.rs`. Capacity overflow uses `try_reserve` and throws a `RangeError`, as `repeat` already does.
11. **Speed.**
    - New benchmarks, each in dat, hand-written Rust and Python: a word count with a map, grouping with take-and-restore, a list built with `acc = append(acc, x)`, a breadth-first search with `shift`, sorting with a comparator, and `map`/`filter`/`reduce` chains.
    - The aim from AGENTS.md: everyday collection code within about 4% of hand-written Rust, compiled. The hand-written version must print the same output, so where order shows, it uses an ordered map (indexmap), not `HashMap`.
    - Complexity cases at n and 4n in both modes, for each pattern §02 promises and for `shift` in a loop. Linear means about 4× the time, not 16×.
    - The interpreter against CPython on the same programs.
12. **The agent benchmark pilot.** It measures whether fix lines and the compact text format save turns, the core bet in AGENTS.md that no study has tested.
    - Compiler support first: a hidden setting that strips fix lines from text and `fixes` from JSON and turns off `dat fix`; a log of the full JSON record of every `dat` run; `dat --version` with the commit.
    - 20 tasks: 6 repairs of seeded errors that have fixes (E0202, E0603, a missing `mut`, `===`, `int` and `float`, an unknown member), 6 tasks written from the spec, 4 failing tests, and 4 ports of small programs.
    - Each task has visible tests, and 8–20 hidden tests run in both modes after the agent stops.
    - Four conditions: fix lines on or off, text or JSON, with 5 runs per task and condition. Each run is a fresh container with headless Claude Code, capped at 40 turns and 10 minutes.
    - Models, chosen for price: Haiku 5.5 runs all four conditions (400 sessions, about $10). Sonnet 5.5 runs the two that decide the core bet, fix lines on and off in the text format (200 sessions, about $65). A weaker model leans on feedback more, so Haiku shows the effect most clearly, and Sonnet checks that it holds for the agents most likely to write dat. Opus is left out: about 1.8× Sonnet's cost for a question the other two answer.
    - Measured: success on the hidden tests, pass^5, turns to the first clean `dat check` and the first green `dat test`, tokens and cost, and how often the agent's next edit is the offered fix.
    - The thresholds are written down before the runs, for example: keep fix lines if turns to green drop by at least 15% and success falls by at most 5 points.

## What these decisions mean for later milestones

| M2 decision | Later effect |
|---|---|
| Interned types with room for more kinds | M3 adds nullable, union and struct kinds without touching the table. Unions are canonical by content (Q22). M5's type parameters are one more kind. |
| One method table and one routine that fills in type parameters | M5's generic functions and constraints (`T extends Comparable`) use the same routine. In M6, standard library modules written in dat get their entries from their declarations; only operations that need Rust stay built-ins. |
| Value traits in dat-rt | M3's generated structs implement them, with `operator equals`, `hash()` and `toString()` overriding the derived ones. M5's `Equatable`, `Hashable` and `Comparable` are these traits. |
| Places in the IR | M3 adds field steps: `user.tags.push(x)`, `mut order.cart`, take-and-restore on fields, and narrowing on field paths. |
| Error edges in the liveness pass | M4's declared errors and `finally` are more edges in the same pass. M8 removes uniqueness checks for values the same marks prove unique. |
| Callbacks that may fail, one algorithm per method | M4's callbacks with declared errors use the same implementations, returning a `Result`. The concurrency milestone's `parallelMap` follows the same rule for a failing callback. |
| `Shared<T>` as a type of its own; closures that capture only copies | Parallel tests in M6 and concurrency later switch the count without touching generated code. Stored closures then need `Send` and `Sync`, which captured copies satisfy. |
| Spans and a line table | M6 adds a file index; M7's LSP and `dat explain` read the same spans. |
| Built-in methods take callbacks as generic parameters; user functions take `&dyn Fn` | Build time grows with the number of callback call sites. M6's tiered build budget has to cover it, and M8 can move user functions to generics behind `dat build --max` if a benchmark shows the gain. |
| A placeholder for every type | Each type a later milestone adds needs one: M3's structs and enums get one derived, a nullable type uses `null`, a union its first member's, M5's interface values the first implementor's, and M5's type parameters require it. |
| The ordered map on hashbrown's `HashTable` | dat-rt owns the map's code, so M6's switch to a shared count and M8's tuning stay inside dat-rt. The hashbrown version is pinned and reviewed at each upgrade. |
| Uniqueness checked once per loop, on a slice | When the count becomes atomic for concurrency, a check per write would cost 13×, so the hoisting M2 builds becomes mandatory then. |
| Spec cases keyed by content | No later spec edit forces re-blessing. |
| Left as they are: the interpreter's compiled code isn't `Sync`, and each compile leaks its memory on purpose (`Box::leak`) | Parallel tests in M6 need the first fixed, and M7's resident checker the second. |

## Exit criteria

The roadmap's exit check, which must pass in both modes:

```
function addOne(mut xs: list<int>): void {
  xs.push(1);
}

test "assignment copies, mut shares" {
  let a = [1, 2, 3];
  let b = a;
  b.push(4);
  assert(a == [1, 2, 3]);
  assert(b == [1, 2, 3, 4]);
  addOne(mut a);
  assert(a == [1, 2, 3, 1]);
}
```

| # | Area | Passes when |
|---|---|---|
| 1 | Groundwork | After each groundwork step, every M1 example and benchmark gives byte-identical IR and Rust. |
| 2 | Collections | Every method in §10, §11 and §16 that M2 delivers is covered in both modes, including the order rules after `delete`, printing, and each `RangeError` and `KeyError`. |
| 3 | Values | Assignment copies and `mut` shares; `const` is deep (E0201); a loop goes through the collection as it was when it started. |
| 4 | Closures | Arrow functions, function types and named functions as values work in both modes. E0602 and E0603 are reproduced with their fixes. |
| 5 | `mut` | `mut` parameters and arguments, including elements, and `mut` loop variables work in both modes. E0703 and the new code for passing a value as `mut` twice are reproduced. |
| 6 | Loops and destructuring | `for...of` over every collection, strings and `range`; destructuring with defaults, `...rest` and `_`; spread; `split`. |
| 7 | Throwing callbacks | A comparator that throws leaves the list unchanged, and an inconsistent one gives no error and keeps every element, with the same output in both modes. |
| 8 | Liveness | E0202 is reproduced with its fix. `=== ir` goldens pin the marks. Every pattern §02 promises makes no copy under `--report-copies`, and its complexity case is linear in both modes. |
| 9 | Traces | An error inside a callback gives the same trace in both modes, with `<arrow>` frames. |
| 10 | Out of memory | A program that allocates too much prints the same message and exits the same way in both modes. |
| 11 | Spec ratchet | No case regresses, and the cases that need only M2 pass. |
| 12 | Exit check | The test above passes in both modes. |
| 13 | Speed | The collection benchmarks run within about 4% of hand-written Rust, compiled; the interpreter runs the word count within 2× of CPython; a program with 100 callbacks stays inside the compiled-path budget; M0's and M1's gates and latency budgets still pass. |
| 14 | Agent pilot | The compiler support is in, and the pilot has run on both models and been reported against its thresholds. |
| 15 | CI | All of the above pass on Linux and macOS. |

## Settled without asking

These follow from the spec, earlier answers or what M1 built. Each can still be changed.

- **`[1, 2.5]` is a `list<float>`.** An integer literal joins a float the way M1's `ready ? 1 : 2.5` already does. A literal whose elements have different types otherwise needs union types and is E0002 until M3.
- **`sort()` on floats puts `NaN` after every number** and otherwise orders as `<` does, so `-0.0` and `0.0` keep their order. Ordering by the bits instead would make the result depend on the processor, since `0.0 / 0.0` gives a `NaN` with a different sign on x86 and on ARM.
- **The no-copy promise names its exceptions.** §02 promises that a last use doesn't copy. The pass can't keep that promise where a copy is required to keep the meaning, so §02 lists these cases:
  - a variable named twice in one call, as in `f(acc, acc)`;
  - a `mut` argument that overlaps another argument;
  - inside a `try`, a variable that the `catch`, the `finally` or the code after them can read, at an operation that can throw;
  - a variable a stored closure captured, until the closure's last use;
  - a value that another copy, such as a copy of the struct around it, still shares.
- **An arrow function's frame in a trace is `<arrow>`**, with its location, as a function value prints as `<function>`.
- **Take-and-restore never changes the order of a map.** The value is swapped for a placeholder and swapped back.
- **The new code for passing a value as `mut` twice** is a compiler code in §06's range, with a ❌ example added to §06.

## Left for later

- `find(fn)` without a fallback returns `T?`: M3, with nullable types. The fallback form lands in M2.
- Struct fields as places, struct destructuring, `this` in closures, and `mut` methods: M3.
- Literals with elements of different types, such as `["alice", 30]`, and E0103: M3, with unions.
- Callbacks that throw declared errors: M4.
- User-defined generic functions: M5.
- Module constants, including the checks for E1704: M6.
- `parallel` and `parallelMap`: with concurrency, after M8.
- Generic function parameters for user functions behind `dat build --max`, and removing uniqueness checks for values proven unique: M8.
