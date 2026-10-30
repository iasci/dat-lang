# Concurrency design

dat has no concurrency yet; `reference/README.md` lists "Concurrency and async" as not yet specified. This note sets out how it will work, so that runtime choices made before it don't have to be undone. The owner has made all three decisions it asked for. Nothing here is built yet.

## What programs need it for

- Waiting on several slow things at once: network calls, files, other programs.
- Using every core for heavy work over a list.
- Servers that handle many connections at once.

## What dat already guarantees

Three rules in the spec do most of the work:

- Every value is a copy. Assigning or passing a list gives the receiver its own list ([§02](../reference/02-variable-declaration.md#value-semantics)).
- There's no global mutable state: a module's top level has no `let` ([§14](../reference/14-scope.md)).
- A closure can only use variables that can't change, and keeps its own copy of them ([§06](../reference/06-functions.md#closures)).

Together they mean two tasks can never change the same data. A task that is given values gets copies, so there are no locks and no data races, without any new rule.

## Decision 1: how code waits

Decided by the owner: B.

**A. `async` and `await`, as in TypeScript.** Agents know it, but it splits functions into two kinds. A function that waits must be marked `async`, and every caller must `await` it and be marked in turn. Forgetting an `await` is a well-known TypeScript mistake; linters carry a rule just for it (`no-floating-promises`). In Rust it maps to `async fn`, where recursion needs extra boxing and stack traces are harder to read. The interpreter would have to pause a function halfway and resume it later, which needs a different design from the one it has.

**B. Plain calls, and tasks started explicitly (recommended).** Any function can run as a task, and waiting is an ordinary call that blocks only its own task, so there's one kind of function. Two built-ins cover most needs, with no new syntax:

```
// Runs both at once and waits for both. Each closure is a task.
const [user, orders] = parallel(() => fetchUser(id), () => fetchOrders(id));

// Runs the closure on every item, several at a time, keeping the order.
const pages = urls.parallelMap((url) => fetchPage(url));
```

A task can't outlive the call that started it, so no task is ever forgotten. This maps directly onto Rust's `std::thread::scope` in both modes. Each task has its own stack, so exact traces, the stack-overflow check and catchable errors work as they do now. Agents will still write `await` from habit; the compiler answers with the exact fix, as it does for `===`.

**How it scales (proposed).** In compiled code, a task isn't an operating-system thread, which takes 0.1–0.2 ms to start on the reference machine and needs a whole stack. The compiler already walks the call graph to find recursive functions. The same walk tells it which functions can reach a wait, such as a network call, a file read or `parallel`, and it compiles exactly those to Rust's `async` form. There a waiting task is a small object of a few kilobytes, and all tasks share one thread per core, so a dat server holds as many connections as async Rust does. Functions that never wait stay plain Rust and cost nothing extra. None of it shows in dat code. The interpreter runs each task on its own thread, which is plenty for development and gives the same results.

Zig worked this way until 2023: it inferred which functions wait. Its weak spot was calls through function values. dat compiles the whole program at once and knows every closure, so that spot is smaller here. A prototype has to show three things before the spec promises any of this:

- traces, `try`/`catch` and the stack-overflow check work in Rust's `async` form;
- recursion inside a waiting function stays cheap (Rust needs one allocation per such call);
- a long computation inside one task doesn't hold up the tasks that share its thread.

Servers need a third piece besides `parallel` and `parallelMap`: a group that tasks join as connections arrive, and that waits for all of them before it ends. The spec calls it `taskGroup`.

## Decision 2: what tasks share

Decided by the owner: copies, channels and `Shared<T>`, as recommended below. `Shared<T>`, `taskGroup`, `OnFailure` and `StoppedError` come from the standard library module `"@dat/tasks"`; `parallel` and `parallelMap` are built in.

A task gets copies of what it uses, as every function call already does, and hands back its result as a copy. Inside a task, `let` variables change freely; they belong to it. A closure can't use an outer `let` variable (E0603), so no task can change its caller's variables. Tasks that must talk while they run send values over a channel, also as copies.

For state that several tasks change, such as a cache or a hit counter, there are three options:

- **Never share.** One task owns the cache, and the others ask it over a channel. It's safe, but every cache becomes a small server, which is a lot of code for agents to write.
- **Plain locks** with `lock()` and `unlock()`, as in most languages. They're flexible, but bring back the bugs that copies rule out: forgetting to lock, and two tasks waiting on each other forever.
- **A `Shared<T>` value whose changes run inside its own lock (recommended).**

```
const hits: Shared<int> = Shared.of(0);
parallel(() => hits.update((n) => n + 1), () => hits.update((n) => n + 1));
print(hits.get());   // 2
```

`get()` returns a copy, `set(v)` replaces the value, and `update(f)` runs `f` on the value while holding the lock and stores what it returns. The compiler rejects an `update` function that waits or touches another shared value, with the fix, so a lock is never held across a wait or taken inside another, and two tasks can never wait on each other forever. `hits.set(hits.get() + 1)` can lose an update when another task runs in between; the compiler flags it and offers `hits.update((n) => n + 1)`. Two values that must change together go in one shared value.

A `Shared<T>` is the one dat value with identity: a copy is another handle to the same value, which is the point. It maps onto Rust's `Arc<Mutex<T>>`. dat-rt's internal `Shared<T>` gets another name to avoid the clash. Graphs, the other half of the spec's open "shared references" topic, stay a separate question, including whether they reuse `shared<T>`.

Copies are cheap because dat copies lazily: a copy shares the original's memory until one side changes it, and a count records how many values share it. That count is the one runtime cost of concurrency, because a count that is safe across threads costs more on every copy. Switching dat's strings to one today made the `rows` benchmark (building text rows) 9% slower compiled and 7% slower interpreted; `fib`, which uses no strings, didn't change. That's over the 4% allowed for everyday code. The better plan, used by Koka and Lean, is a count that stays cheap until a value is actually handed to another task. It gets built and measured with concurrency. Until then the count stays behind one type in dat-rt (`Shared<T>`), so the switch touches one line.

## Decision 3: what happens when a task fails

Decided by the owner: the error is rethrown where the tasks were started. By default the other tasks finish first; `OnFailure.Stop` stops them at once.

**The error comes out of `parallel`.** If `fetchOrders` throws, `parallel(...)` throws the same error, with a trace that shows the task's frames and then the `parallel` call. The `try` before `parallel` works as it does before any call that can fail: `loadAccount` passes the error on, and a caller catches it with an ordinary `try`/`catch`.

```
function loadAccount(id: int): Account throws HttpError {
  const [user, orders] = try parallel(() => try fetchUser(id), () => try fetchOrders(id));
  return Account { user, orders };
}
```

When several tasks fail, the error thrown is the one from the earliest task in the order they were written (for a server's group, the order they joined it), not whichever failed a moment sooner. A task that was stopped doesn't count as failing. A task that handles its own error doesn't fail at all:

```
// A failure becomes null, and the other task carries on.
const [user, orders] = parallel(() => try? fetchUser(id), () => try? fetchOrders(id));
// user: User?    orders: list<Order>?
```

**By default, the other tasks finish.** Stopping a task while a request it sent is on its way would leave the program not knowing whether a card was charged or a row was written. The runtime can't tell such a request from any other read either: at that level a database query is bytes sent, then bytes read back. So by default nothing that talks to the outside world is cut off:

- Tasks that are already running carry on to their end, so every request they send gets its answer.
- `parallelMap` doesn't start items that haven't started yet.
- A task waiting on a channel, or for a new connection, is woken with a stop error, since otherwise it could wait forever, for example for a message from a task that already failed. These waits are the runtime's own built-ins, so nothing needs marking.

Once everything has ended, `parallel` throws.

**`OnFailure.Stop` stops the other tasks at once**, at whatever they're waiting on, a request on its way included. Each gets a stop error, so its `finally` blocks run; the stop error is thrown only once, so cleanup code can still make calls. The caller picks it when the tasks only read, so cutting them off loses nothing:

```
const [user, orders] = try parallel(() => try fetchUser(id), () => try fetchOrders(id), OnFailure.Stop);
const pages = try urls.parallelMap((url) => try fetchPage(url), OnFailure.Stop);
```

Forgetting the option never causes a bug; a failure only takes longer to report, and correct code can be written without knowing the option exists. It goes last, as parameters with defaults do ([§06](../reference/06-functions.md)). The names can change in the spec section. In both modes, plain computation runs to its end: checking for a stop only where code already waits keeps loops and calls as fast as they are now.

**What no rule can fix.** Two tasks that must both succeed or both not happen, such as reserving stock and charging a card: if one fails after the other's write went through, that write stays. Waiting for every task doesn't undo it. The program runs such steps one after another, or lets each task report its own outcome and undoes what happened:

```
const [stock, receipt] = parallel(() => try? reserveStock(order), () => try? chargeCard(order));
if (receipt != null && stock == null) {
  try refund(receipt);
}
```

The same rule holds for a server's group, so one connection's uncaught error would end the server once the other connections finish. The server library therefore catches each connection's errors itself and answers that connection with an error, as web servers do, and the group's rule stays the same as everywhere else.

**Not chosen:**

- Stopping the other tasks at once by default, with a marker that library authors put before each request so a stop never cuts one off. It's one more thing to remember, the same flaw that ruled out `await`.
- Handing back every result as a value or an error, as `Promise.allSettled` does (`User | HttpError`). Every result would need a check before use, and passing an error on would always take code. The `try?` form above gives that behavior wherever it's wanted.

Output from tasks that print at the same time can arrive in any order, and each `print` line stays whole. Tests are unaffected: each test's output is already captured on its own.

## What the code needs once these are decided

- The spec section, which the roadmap requires first: written as [§22](../reference/22-concurrency.md). Channels aren't in it yet.
- A stop error, and channel and connection waits that wake on it.
- One stack limit per task. Today the stack check compares against one limit for the whole program.
- An interpreter whose compiled program can be shared between threads. Today its closures hold strings with the cheap count.
- Tests running in parallel, as [§20](../reference/20-testing.md) already says. They run one after another today.

## When

The roadmap puts concurrency after M8, but says it matters earliest because of the count. With the count behind one type, deciding can happen now and building later. With decisions 1–3 made, the spec section is written. Building waits until lists exist, since `parallelMap` needs them.
