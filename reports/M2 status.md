# M2 status: values and collections

**M2's language is built: lists, sets, maps and tuples with value semantics, `mut` parameters, arguments and loop variables, arrow functions and closures, `for...of`, destructuring, spread, and a liveness pass that hands values over at their last use. Every M2 program gives the same output in both modes, and 12 of the 15 exit criteria pass. Three don't: compiled collection code runs at 0.86–1.21× hand-written Rust against the 4% aim (criterion 13), the agent pilot's compiler support is in but the owner has kept the pilot open for a later milestone (14), and CI runs with the push of this report (15).** A probe of 341 programs in both modes and 122 mistakes found 11 kinds of bugs, all fixed, and one question for the owner, now decided (see [The probe](#the-probe)). The spec ratchet goes from 76 passing cases to 120, with none lost. Everything is built from the [M2 plan](./M2%20plan.md), on top of M1. It adds about 11,700 lines of Rust, for about 25,100 in all, including the runtime's collections and their tests. Two outside crates come in, both pinned and both named in the plan: `hashbrown` (the table under the ordered map) and `foldhash` (its hasher).

Measurements come from the reference machine named in the [M0 report](./M0%20status.md). Times are the median of 20 runs after 3 warmups, wall time including process start, with the programs compared run interleaved.

## Exit criteria

| # | Area | Result | Evidence |
|---|---|---|---|
| 1 | Groundwork | **pass** | After the groundwork and every feature, the IR and Rust of every M1 benchmark are byte-identical to M1's. Three M1 examples now differ, and only by moves: `return l0_text;` where M1 wrote `return l0_text.clone();` (see [Speed](#speed)). Of the eight groundwork steps, six are done and two are partly done; see [Groundwork](#groundwork). |
| 2 | Collections | **pass** | `examples/collections/lists.dat` and `sets_maps.dat` cover every method of §10, §11 and §16 that M2 delivers, in both modes: the order after `delete` and a re-add, printing (`[1, 2]`, `{"go"}`, `{"a": 1}`, strings quoted, `-0.0` and `1.0` inside lists), and each `RangeError` and `KeyError` message. |
| 3 | Values | **pass** | `examples/collections/exit_check.dat` and `closures.dat`: assignment copies, `mut` shares, a closure keeps the copy it made. `const` is deep: `fixed.push(3)` and `fixed[0] = 10` are E0201 with the spec's fix (`examples/errors/collections.dat`, and the §02 spec cases). `loops.dat`: a loop goes through the list as it was when it started. |
| 4 | Closures | **pass** | `closures.dat`: arrow functions with expression and block bodies, function types, named functions as values, functions returned from functions, function values in lists. E0602 with its `int` and `float` options and E0603 with its `for...of` fix are reproduced (`examples/errors/closures.dat`, `foreach_let.dat` with its `.fixed`, and the §06 spec cases). |
| 5 | `mut` | **pass** | `examples/collections/mut.dat`: `mut` parameters of lists, ints and maps, elements (`bump(mut counts[0])`), a `mut` value changed and then an error, two visibly different parts of one variable (list elements, map entries, nested lists, a tuple's positions, and an error halfway), and `mut` loop variables with `break`. E0703 and the three new codes, E0657 (one value passed `mut` twice, or two parts that may be one), E0658 (a missing `mut`, with a safe fix) and E0659 (a `mut` the parameter doesn't take, with a safe fix), are in `examples/errors/mut_args.dat` and have ❌ examples in §06. |
| 6 | Loops and destructuring | **pass** | `examples/collections/loops.dat`: `for...of` over lists, sets, map entries, strings and `range`, with `break` and `continue`; destructuring with defaults, `...rest`, `_`, nested patterns and tuples, in declarations and loop headers, and its `RangeError`; spread of lists, sets and strings into lists and sets; `split`, where `""` gives the characters. |
| 7 | Throwing callbacks | **pass** | `examples/collections/callbacks.dat`: a comparator or key that throws leaves the list as it was; a comparator that always says "greater" over 200 elements gives no error and keeps every element; the same output in both modes. |
| 8 | Liveness | **pass** | E0202 is reproduced with its fix (`examples/errors/copies.dat`, §02). `examples/collections/no_copies.dat` runs every pattern §02 promises under `--report-copies` and gets `copies: none` in both modes; its `=== ir` section pins the moves and takes that make it so. `copies.dat` shows each copy §02 says a last use still makes, reported at its line, the same in both modes. Each pattern runs in linear time in both modes (`perf`, at n and 4n). Every conformance run sets the debug checks: no use after a move (a static check of the pass's marks) and every value dropped (a count of the blocks a run leaves allocated). `moves.dat` pins the cases the probe found. |
| 9 | Traces | **pass** | `examples/collections/callbacks.dat` ends with an uncaught error inside a callback: the same trace in both modes, `at check`, `at <arrow>`, `at main`. |
| 10 | Out of memory | **pass** | `dat-conformance oom` (part of `all`, Linux only): a program that runs out of memory under `ulimit -v` prints `error: out of memory` and aborts with status 134 in both modes. |
| 11 | Spec ratchet | **pass** | 120 of 289 cases pass in both modes, up from 76, with no regression. The cases still blocked need structs (26), `try` expressions (11), nullable types, unions, `match`, interfaces, imports of files or `@dat/tasks`. |
| 12 | Exit check | **pass** | `examples/collections/exit_check.dat` is the roadmap's test, and passes in both modes. |
| 13 | Speed | **fail** | See [Speed](#speed). Sorting with a comparator runs at 0.86× hand-written Rust and the word count at 0.99×; the breadth-first search and map/filter/reduce at 1.12×, grouping at 1.14×, the `append`-built list and the search with a callback at 1.21×. What's left is mostly checked arithmetic, which dat guarantees, and the shared buffer behind every list. The interpreter runs the word count in 0.51× CPython's time, and the M0 and M1 gates still pass. A program with 100 callback call sites runs warm in 0.05 s, inside the 1 s compiled-path budget; its first build takes 3.45 s, against 0.19 s for one without callbacks, about 32 ms per call site. |
| 14 | Agent pilot | **open** | The compiler support is in: `DAT_NO_FIXES=1` strips fixes from text and JSON and leaves `dat fix` nothing to apply, `DAT_LOG=<file>` appends one JSON line per run with its arguments, status and full diagnostic record, and `dat --version` prints the commit. The pilot itself needs containers, the 20 tasks and about $75 of model spend, which is the owner's call. The owner keeps it open and won't run it in this milestone. |
| 15 | CI | **runs on push** | This report is pushed to `main` with the work, which starts CI on Linux and macOS. Locally, fmt, clippy with `-D warnings`, `cargo test` and `dat-conformance all` pass on Linux. |

## How to check it

```
cargo build --release
target/release/dat-conformance all     # examples, self-test, spec blocks, structure, concurrency, out of memory; both modes
target/release/dat-conformance perf    # latency budgets, speed gates, the collection benchmarks and the complexity checks
cargo test --release
```

## Speed

These are from the last `perf` run, after a second round of speed work (below), with the load near 1 from other sessions on the machine; ratios come from interleaved runs. Every gate passes.

| Measurement | Result | Limit |
|---|---|---|
| `dat check fib.dat` | 2.3 ms | 10 ms |
| `dat test fib.dat` | 2.8 ms | 20 ms |
| `dat run hello.dat` | 2.4 ms | 5 ms |
| `dat run --compiled fib.dat`, warm | 47.2 ms | 1 s |
| `dat test --compiled fib.dat`, warm | 46.8 ms | 2 s |
| Compiled `fib(35)` against hand-written Rust | 1.10× | 1.2× |
| Compiled float code against hand-written Rust | 1.01× | 1.04× |
| Interpreted `fib(30)` against CPython | 0.73× | 2× |

The new benchmarks, each in dat, hand-written Rust and Python with the same output (`bench/`):

| Benchmark | Compiled against hand-written Rust | After the code review | Before it | Interpreted against CPython |
|---|---|---|---|---|
| `wordcount`: counting words with a map | 0.99× | 1.01× | 0.99× | 0.51× |
| `groups`: grouping into a map of lists, taking each list out and back | 1.14× | 1.28× | 1.75× | 0.56× |
| `append`: a list built by `items = append(items, x)` | 1.21× | 1.20× | 1.25× | 0.47× |
| `bfs`: a breadth-first search with `shift` | 1.12× | 1.28× | 1.83× | 0.72× |
| `sortcmp`: sorting with a comparator, and by a key | 0.86× | 1.41× | 1.73× | 0.42× |
| `chains`: `map`, `filter` and `reduce` with arrow functions | 1.12× | 1.34× | 1.33× | 1.04× |
| `search`: a binary search that takes its test as a callback, called with a new arrow function each time | 1.21× | 1.22× | 1.87× before function parameters were lent | 0.91× |

The hand-written programs use what a person would: `HashMap` with its entry API, `VecDeque`, `sort_by` with `cmp`, iterator chains. Where the time went was measured by editing the generated Rust by hand, one change at a time, and timing each against the original by its least CPU time over interleaved runs (perf events can't be recorded on this VM). The code review's round made three changes:

- **A take-and-restore of a map entry no longer looks the key up twice.** The take remembers the slot it left a placeholder in, and the `insert` that puts the value back writes to that slot after checking that it still holds the key, without hashing it again.
- **A list a loop changes is checked for a shared buffer once, before the loop.** If the loop only reads and writes its elements, asks its length, pushes, pops and shifts, `List::edit` makes the buffer the list's own and the loop works through the `ListMut` it returns, which skips the check and the pointer to the buffer at every change. Any other use of the list in the loop, such as passing it on or printing it, keeps every check; `examples/collections/loop_edits.dat` covers the cases, throws included.
- **`sortBy` and `toSortedBy` sort each element together with its key**, instead of sorting keys with positions and then moving the elements into place, a pass that jumped around memory.

The second round made six more. None changes what a program does; `examples/collections/fast_paths.dat` runs each against its edge cases in both modes:

- **Any lookup remembers the slot it found**, not only a take, so `counts[w] = counts.get(w, 0) + 1` hashes `w` once: `wordcount` from 1.11× to 1.00× in the comparison.
- **A take-and-restore borrows the entry** in generated code (see [How it's built](#how-its-built)), and a constant fallback such as `[]` is made only when the key is missing, since making it can't throw or print: `groups` from 1.33× to 1.13×.
- **A loop that never changes a list's length works on a slice** (`ListSlice`), whose length stays in a register; `ListMut` reloads it from the buffer at every access: `bfs` from 1.31× to 1.20×.
- **`(a, b) => a - b` and `b - a` sort in place.** One pass finds the smallest and largest element; if their difference can't overflow, no difference can, so the comparator can't throw and its sign is `a.cmp(b)`, and the list is sorted in place, unstably, since equal integers can't be told apart. Otherwise the comparator runs as written, and its error leaves the list as it was.
- **A comparator or sort key that can't throw or print and costs little** (locals, constants, comparisons, logic, float arithmetic, lengths, `%` by a constant that can't fail) sorts in place, and the key is worked out in each comparison, as Rust's `sort_by_key` does. Nothing can tell how often it ran. With the previous change, `sortcmp` from 1.46× to 0.88×.
- **`filter` on a list just made, such as `map`'s, keeps what stays in that list's own buffer**, as Rust's `into_iter().filter().collect()` does: `chains` from 1.48× to 1.21× in the comparison.

Under `--report-copies` generated code leaves out the changes that move a copy to another place, so its report matches the interpreter's. Today's runtime additions cost nothing measurable: the copy counter's check in `make_mut` and the debug checks' allocation counter were each taken out of `wordcount` and `groups` and timed against them, within the 2–3% noise.

What's left:

- `search` is checked arithmetic and nothing else: with dat's overflow checks swapped for wrapping arithmetic in its generated code, it ran at hand-written speed. Each `x * x` in the callback and `a + b` in the loop is checked, as dat always does, and none of it can be vectorized.
- `append` pays for the shared buffer: each `push` reads the buffer's two counts and its length and capacity through the list's pointer, where the hand-written `Vec` keeps them in registers. Neither a fast path that tests uniqueness first nor leaving out the barrier that keeps a return from becoming a tail call made a difference.
- `bfs` and `groups` pay for the same indirection on the queue and on each group's list; a `VecDeque` in place of the queue gained 4%, and reserving the distances before they're pushed gained nothing.
- `chains` checks `x * 3 + round` and the sum for overflow; without the checks it ran at 1.12× where it had run at 1.17×.

The complexity checks (`perf`, criterion 8) run each of §02's patterns at n and 4n in both modes. Linear is 4×, a copy per step would be 16×; all ten measured 2.3–3.7×: `items = append(items, x)`, `text += "x"`, taking a list out of a map and back, taking a list element out and back, and `shift` in a loop.

Generated Rust grows from 3.2 to 3.7 lines per line of dat over the benchmarks; callbacks cost the most, at 6 lines per line in `sortcmp`. Every M0 and M1 benchmark's generated Rust is byte-identical to M1's.

## Groundwork

| Step | State |
|---|---|
| 1. Interned types | Done. A compound type (list, set, map, tuple, function) is interned once in a table that the IR program carries, and `Type::Of(TypeId)` refers to it, so type equality is structural and invariance needs no code. Primitive types stay variants of their own rather than ids: every existing match on `Type::Int` kept working, and they need no table to name. What each type supports (`==`, hashing) is worked out when it's interned. The checker's `Ty` is the IR type plus `Poison`, and a compound type never holds poison. |
| 2. Checking against an expected type | Done: `expr_want` gives `[]`, `{}`, a list literal that's a set or a tuple, an arrow function's parameters and a `?:` the type the context expects. |
| 3. Splitting the largest files | Partly. The new work went into new modules instead of the old files: `body/{collections, methods, closures, loops, copies}.rs` in the checker, `collections.rs` and `value.rs` in the interpreter, `collections.rs` in codegen. The old files weren't split by pure moves; `body.rs` is still 2,700 lines. |
| 4. One visitor | Done: `dat_ir::each_child` and `each_part`. The checker's call graph, codegen's call and change scans and the liveness pass use them. |
| 5. `Shared<T>` | Done in dat-rt: `Str`, `List`, `Set` and `Map` go through it, and function values have one constructor per number of parameters. The interpreter still uses `Rc` for its own closures, tuples and errors. `Value` stays 16 bytes, checked at compile time. |
| 6. Spans in the IR | Not done. E0202 needed them in the plan because the liveness pass would report it; it's reported by the checker instead (see [Decisions](#decisions-made-while-building)). |
| 7. One general path in the interpreter | Done: every new node is compiled once, by `Compiler::general`, and the typed compilers fall back to it, so a list element read as an `int` needs no arm of its own. |
| 8. Spec cases keyed by content | Done. A case is its file, a hash of its block's text and its line within the block, as `02-variable-declaration.md~3f9c1a20#E0202@+3`; the ratchet's second column says where it was when blessed. An edit above a block leaves its cases alone (tried: a paragraph added at the top of §02 changed nothing). `--bless` refuses to run while a case regresses. Preludes are keyed the same way; `dat-conformance spec --keys` lists the keys. |

## How it's built

- **Runtime collections.** `dat_rt::List<T>` is a vector and a head offset behind one shared pointer: `shift` moves the first element out and advances the head, and the dead front is dropped once it outgrows the live part, so a queue stays linear. `Set` and `Map` are one ordered table on hashbrown's `HashTable`, hashed with foldhash and a random seed per process; the index holds a slot number and the hash, the slots keep insertion order, `delete` leaves a hole, and the table compacts when holes exceed a quarter of the live entries. An empty collection holds no buffer, so `[]`, `{}` and the placeholder a move leaves cost no allocation. One set of value traits (`Show`, `Equal`, `DatHash`, `Order`, `Placeholder`) is implemented by the primitives, the collections, tuples and the interpreter's `Value`, so `==`, `includes`, `unique`, `sort()`, printing and `assert` can't differ between the modes.
- **Sorting.** A sort with a comparator sorts a copy and swaps it in when it's done, so a comparator that throws leaves the list as it was. Rust's sort panics when it notices an inconsistent comparator; dat-rt catches exactly that panic and keeps the sorted copy, which still holds every element, and the panic hook stays silent for that message. A unit test triggers the real panic, so a toolchain whose message changes fails loudly. `sortBy` and `toSortedBy` call the key once per element.
- **The method table.** `body/methods.rs` lists every collection method once, with its signature in a small type language (`T`, `K`, `V`, a free `U`, lists, sets, maps, pairs, and callbacks that may also take the index) and what the element type must support. One routine binds the type variables from the receiver and then the arguments: plain arguments first, so `reduce`'s initial value fixes the accumulator before its callback is checked, while the arguments still run left to right. Missing methods, similar names, `length()` for `length`, and "changes its receiver" all come from the same table. The primitive types' methods are still the M1 table.
- **Callbacks.** dat-rt's methods take their callback as a generic parameter returning `Result`, one implementation each. Generated code passes a Rust closure that calls the arrow function and returns `Ok`, since a dat error unwinds; `dat_rt::done` takes the result. The interpreter passes closures that call back into the machine; the collection is owned by the node, never borrowed from the machine's stack, and a list being sorted with a comparator leaves its place while the comparator runs.
- **Arrow functions** are lowered to functions of their own, named `<arrow>` in traces. A use of an outer `const` captures it: the closure copies it when it's created, through every arrow function in between. A stored function value is `dat_rt::FuncN`, a shared `dyn Fn`. A parameter that the function only calls or passes on is a `dat_rt::FnRefN` instead, which borrows the caller's function, so an arrow function passed to it isn't allocated (see [Decisions](#decisions-made-while-building)). A function that calls a function value, or a method with a callback, is linked in the call graph to every function used as a value, so cycles through callbacks get the stack check.
- **Places.** `xs[i] = v`, `m[k] += 1`, `xs.push(x)` and `mut` arguments go through an IR place: a variable and its index, key and tuple steps. Codegen works the step operands out into locals first and then takes one `&mut` path, so the borrow never overlaps them; a last `Key` step inserts. `xs[f()] += 1` works out `f()` once.
- **`mut`.** A `mut` parameter is `&mut T` in Rust. The interpreter moves the value out of the caller's place into the callee's frame and back when the call ends, even by an error, so changes are visible and nothing is copied; nothing else can reach the place during the call. `for (mut x of xs)` is `iter_mut()` in Rust; the interpreter moves each element through the loop variable.
- **The liveness pass** is the crate `dat-own`, which depends only on dat-ir and runs on every program after lowering. It walks each function backward over the structured IR, repeating loops until their live sets settle, and turns a read that is a variable's last use into a move, where the value is consumed: an assignment, an argument, a return, a list element. A `try` keeps alive, through its whole body, what its `catch` and the code after it read. Generated Rust moves the variable, or inside a `try`'s closure takes its value and leaves a placeholder; the interpreter takes the value out of its slot. A use after a move would read a placeholder: the interpreter fails loudly on one, and Rust's borrow checker rejects one in compiled code.
- **Take-and-restore.** When `let v = m.get(k, fallback);` (or `m[k]`, or `xs[i]`) is followed, with nothing in between touching `m` or what `k` reads or leaving the block, by `m[k] = v;` with `v`'s last use, the read becomes a take that leaves a placeholder in the entry, so changing `v` doesn't copy it and the store puts it back in the same place. The key can be any expression that gives the same value when worked out again: locals, constants, and arithmetic, joins and tuple positions of them, such as `grid[i % 2]`; first it had to be a local or a constant, which `--report-copies` showed copying `grid[i % 2]` on every pass. The map can't be seen after an error in that function, or the take isn't made. Generated code goes further: `v` borrows the entry (`Map::get_or_insert_with`, or the element) and the put-back disappears, the way hand-written Rust uses the entry API (`codegen/src/take.rs`).
- **Out of memory.** dat-rt's global allocator wraps the system's and, when it fails, writes `error: out of memory` and aborts. It's the second narrow `unsafe` exception in dat-rt, next to reading the stack pointer.
- **The copy report.** Every copy of a shared buffer goes through `Shared::make_mut`, which calls `copies::copied` on its cold path; counting is off unless `--report-copies` started it, and the check costs nothing measurable otherwise. Both backends tell the counter where they are around the same IR nodes: a change such as `push`, a call with `mut` arguments, a store through an index or key, `s += t` and a `for (mut ...)` loop. `Set` and `ForMut` gained a location for it. Under the report, generated code leaves out the faster paths that move a copy to another place (loop edits, borrowed takes, sorting in place), and the interpreter makes each string literal afresh, as generated code does, so both modes count the same copies at the same lines. The report goes to stderr after the run; §21 describes it.
- **The debug checks** (`DAT_DEBUG_CHECKS=1`, set by every conformance run). Before running, `dat` walks each function's IR forward (`dat_own::verify`), following both arms of a branch and every path an error can take, and stops with an internal error if a variable is used after it was moved or a map is used while a value taken out of it isn't back. While the program runs, the allocator counts, for its thread, the blocks allocated and not freed; a run that ends normally with blocks left over stops with an internal error. A unit test leaks a block on purpose to show the check fires. A run that ends with an uncaught error or a failed test isn't counted: printing a trace fills caches that live until the process ends.

## The probe

An agent wrote 341 programs against the M2 features and ran each in both modes, and 122 mistakes through `dat check` and `dat fix`. 302 programs gave the same output in both modes and the output the spec gives. What it found is fixed, and each fix has a case in `examples/collections/moves.dat`, `closures.dat`, `lists.dat` or `examples/errors/callbacks.dat`:

| Bug | Fix |
|---|---|
| `xs.concat(id(xs))`, `xs[first(xs)]`: generated Rust didn't compile, since the receiver was still borrowed when the argument moved `xs` | What generated code borrows while the other operands run stays alive until the node is done |
| Take-and-restore when the key changes in place between the take and the store (`change(mut k)`, `k.push(9)`): a placeholder was left in the map, `void` in one mode and `[]` in the other | The key counts as changed by any change, not only by assignment |
| `f(mut a, a)` with `a`'s last use as the value: the interpreter stopped on a placeholder, and rustc rejected the moved value | A `mut` argument's variable stays alive through the other arguments |
| `xs.map((x) => { return; })` passed the checker and built a list of void | E0252: a callback whose result is used must return one |
| A `mut` parameter with a default: generated Rust didn't compile | E0651, a new code with a ❌ example in §06 |
| An arrow function's return type with a comma in it, as `(x: int): [int, string] => ...`, was a syntax error | The parser skips brackets when it looks for `=>` |
| E0703's safe fix (`mut` to `const`) when the body changes the loop variable: the fixed code had E0201 | No fix then |
| An untyped arrow function in a call that's already wrong got E0602 too, once per parameter | It isn't checked on its own: nothing could give its parameters types |
| `(x) => { x > 1; }` as a predicate: E0305 and E0601 | One E0305, with `return x > 1;` as a review fix, and checking goes on as if applied |
| `xs.map(x => x * 2)`: a syntax error with no fix | The parentheses as a safe fix, parsing on as if applied |
| `xs.sorted()` got `sort` as its fix, which changes the list and returns nothing | `toSorted`; likewise `toReversed` |
| `m.length` on a map got `m["length"]`; `xs.size` got nothing | `size` and `length`, as safe fixes |
| `xs.length = 0` was "not supported yet: changing fields of structs" | `xs.length` can't be changed (E0350) |
| `ok ? xs : [0]` was "not supported yet" although both branches are `list<int>` | Branches of one collection type; `ok ? xs : []` takes `xs`'s type |
| `[[1], []]`, and `reduce(f, [])` where the result's type is declared, were E0206 | An empty literal takes the type of the elements around it, and a method whose result is its free type variable takes the expected type |
| `(x: int) => { a: x }` was read as a block | A body that starts like a map literal is one: a block can't start with `name:`, since dat has no labels |

One finding was a question, not a bug: `swap(mut xs[0], mut xs[2])` was E0657, "already passed as `mut`", although §06 only forbids passing "the same value twice, including a value and a part of it". The owner decided: two parts of one variable can both be passed `mut` when they're visibly different, that is, where their paths first part, both steps are literals and the literals differ (`xs[0]` and `xs[2]`, `m["a"]` and `m["b"]`, `grid[r][0]` and `grid[r][1]`, a tuple's positions). Parts that may be the same at run time, as `xs[i]` and `xs[j]`, stay E0657, now with a review fix that copies each into a `let` before the call, passes the copies, and stores them back after it. §06 has the rule, in its own section, with a ✅ and a ❌ case. Compiled code reaches each place once, in order, so a missing element throws where the interpreter throws it, then lends them all at once through dat-rt's `disjoint_mut`; the interpreter already moved each value out and back.

## The code review

A review of the whole M2 diff, after it was built, found seven bugs, each fixed now and kept as a case in `examples/` that runs in both modes, and three places where compiled code lost time (see [Speed](#speed)).

| Found | Now |
|---|---|
| A parameter whose last use is inside a `try` made rustc reject the generated code: the move takes the value out of the variable, which wasn't `mut` | Such a parameter is `mut` in the generated function (`examples/collections/edges.dat`) |
| Tuples of one value, of none, and of nine or more didn't compile: dat-rt's value traits covered 2 to 8 values, and function values 8 parameters | dat-rt covers tuples of 0 to 12 values and function values of up to 12 parameters, as std does for tuples; more is E0002 in both modes (`examples/errors/unsupported.dat`) |
| `xs.find(callback, fallback)` evaluated the fallback first in compiled code | The callback first, as the interpreter does |
| `let xs: list<int>= [];` didn't parse, since `>=` is one token | A `>=` right after a type's arguments is its `>` and an `=` |
| The out-of-memory allocator made `range()` and `repeat()` end the program where M1 threw a `RangeError` | A list or string longer than 2^48 elements or bytes is a `RangeError`, decided before anything is allocated, so it's the same on every machine and in both modes; a shorter one that can't be allocated is out of memory. §04 and §09 state the limit. Letting a failed reservation throw instead was tried and dropped: under a memory limit the compiled program then threw where the interpreter ran out of memory |
| `DAT_LOG` wrote arguments with Rust's escaping, which isn't JSON's | JSON escaping, with a unit test |
| `dat --version` could name a stale commit: the build script watched `.git` paths that don't exist in a worktree, and missed edits that aren't staged | It asks git where its directories are, and watches `crates/` too |

Two findings about the interpreter's speed are left: `range()`, `split` and `max`/`min` build a typed list and then copy it into one of values, and a call with `mut` arguments allocates a few small vectors each time.

## Decisions made while building

These are implementation choices; the ones that state behaviour the spec didn't pin down are in the spec now.

- **A function parameter is borrowed when it can be, and shared when it can't.** The plan made every parameter of function type a `&dyn Fn`, but §06 lets a parameter be stored, captured or returned, which a borrow can't outlive. The owner chose to let the compiler decide per parameter, in compiled code only:
  - A parameter the function only calls, passes to a built-in method as its callback, or passes on to another borrowed parameter is a `dat_rt::FnRefN`, a borrow of the caller's function. An arrow function passed to it is a plain Rust closure on the caller's stack, so nothing is allocated, and a stored function value is lent with `lend()`.
  - A parameter that's stored, returned, printed, or captured by an arrow function that's stored or returned is a shared `dat_rt::FuncN`, as every function value was before.
  - An arrow function passed straight to a built-in method or to a borrowed parameter borrows the function values it captures by the same rule. An arrow function's own parameters, and a parameter with a default, stay shared.
  - The rule is a fixed point over the program, so a parameter passed on recursively, or between functions that call each other, stays borrowed.

  Nothing changes in the language, the interpreter or traces. `bench/search.dat` (a binary search that takes its test as a callback, called with a new arrow function a million times) went from 1.87× hand-written Rust to 1.23×. Built-in methods still get the arrow function inlined, as before.
- **A last use moves only where the value is consumed.** A method's receiver, an index's base or a string passed to a method is only looked at, so moving it saves nothing and would only force a placeholder swap inside a `try`.
- **E0202 is reported by the checker, not the liveness pass.** A `let` that starts as a copy of a variable or an element, is modified, and is never read or passed on, gets the error, with the fix that changes the original (review). `for (let x of xs)` gets `mut` as its fix (review) when `xs` is a list that can change, and no fix otherwise. The checker has the source spans the message and fix need; the IR has only line and column. It's narrower than the spec's wording: a copy read before it's modified, and never after, isn't reported.
- **New codes**, each with a ❌ example in its section: E0151 (a set element, map key, or `unique`/`groupBy` key that isn't hashable) and E0152 (sorting or `includes` on values that can't be ordered or compared) in §01; E0651 (a `mut` parameter with a default), E0657 (one call passes a value as `mut` twice, or two parts that may be one), E0658 (a missing `mut`, safe fix) and E0659 (a `mut` the parameter doesn't take, safe fix) in §06.
- **Two `mut` arguments from one variable** are fine when they're visibly different, as `swap(mut xs[0], mut xs[1])`, and E0657 when they may be the same part, as `swap(mut xs[i], mut xs[j])`, with a review fix; the owner's decision, see [The probe](#the-probe).
- **E0703's fix** (`mut` to `const` in the loop header) is safe, as §07 shows it, but only offered when the body doesn't change the loop variable.
- **E0701's fix compares a `length` or `size` with `> 0`**, as the spec shows, and any other int with `!= 0`.
- **`m.alice` on a map with string keys** gets `m["alice"]` as a review fix.
- **A fix that deletes part of a line now shows the line it leaves.** A deletion of whole lines still shows nothing.
- **`[1, 2.5]` is a `list<float>`**; a list whose elements have different types otherwise is E0002 until unions come in M3. `[]` and `{}` with nothing to say their type are E0206, with `: list<_>` or `: map<_, _>` as a review fix in a `let`.
- **A `for...of` over a list literal of values** goes through the values without building a list, in generated code. It halved the breadth-first search.
- **The spec runner** now reads `false: already removed` as the value `false`, treats `print(x); // throws KeyError: ...` as a throwing statement, doesn't take a comment on a `print` inside a loop as its whole output, and drops a `✅` line that declares the ❌ line's name again from the ❌ case, since it's the corrected alternative. New preludes give the M2 blocks the names they use.
- **`dat --version`, `DAT_NO_FIXES` and `DAT_LOG`** are the agent pilot's compiler support. The two settings are environment variables, not options, since nobody else needs them.

## Left for later

- From M2's plan, not built: parameters that are borrowed until the body needs to own them, and dropping a value right after its last use in compiled code. The uniqueness check comes out of loops only for lists; a map or set a loop changes is still checked at each change.
- `--report-copies` prints text only; the JSON diagnostics don't carry it yet.
- The agent benchmark pilot itself, which the owner keeps open for a later milestone.

## What M3 builds on

- **Places** take one more step for struct fields: `user.tags.push(x)`, `mut order.cart`, and take-and-restore of a field, which the liveness pass already does for map entries and list elements.
- **The type table** takes nullable, union and struct kinds as more `Kind` variants; equal types already get equal ids.
- **The value traits** are what generated structs implement, and what `operator equals`, `hash()` and `toString()` replace.
- **`find` without a fallback**, `?.` and `??` need nullable types, and literals with elements of different types need unions.
- **Visibly different `mut` places** take struct fields: `mut order.cart` and `mut order.saved` in one call are fine, as different literal indexes are now.
