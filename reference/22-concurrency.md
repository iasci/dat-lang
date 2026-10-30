# Concurrency

A task is a function that runs at the same time as others. Any function can run as a task, and waiting is an ordinary call: a network request, a file read or a call to `parallel` pauses only the task that makes it. There's no `async` and no `await`, so there's one kind of function.

Three calls start tasks: `parallel`, `parallelMap` and `taskGroup`. Each one waits for every task it started before it returns, so no task outlives the call that started it, and none is ever forgotten.

A call **waits** when it pauses until something outside its task happens: input or output, a timer, or other tasks finishing. Each built-in that waits says so where it's defined, and a function that calls one waits too.

`parallel` and `parallelMap` are built in. Everything else in this chapter comes from the standard library module `"@dat/tasks"`, listed [at the end](#the-dattasks-module).

## `parallel`

`parallel` runs each function it's given as a task, waits for all of them, and returns their results as a tuple, in the order the functions are written:

```
const [user, orders] = try parallel(() => try fetchUser(id), () => try fetchOrders(id));
// user: User    orders: list<Order>
```

- If a task can throw a declared error, `parallel` can throw it too, and the call needs `try`, as with [callbacks](./12-errors.md#errors-in-callbacks).
- When every task returns `void`, so does `parallel`. Otherwise a `void` task takes its place in the tuple, and destructuring skips it with `_`: `const [_, orders] = parallel(() => saveVisit(id), () => loadOrders(id));`.
- An optional last argument, `OnFailure.Stop`, decides what happens to the other tasks when one fails. See [When a Task Fails](#when-a-task-fails).

### Tasks Get Copies

A task's function is a [closure](./06-functions.md#closures), so it can only use variables that can't change, and it keeps its own copy of each. No task can change its caller's variables or another task's. Inside a task, `let` variables change freely, because they belong to that task. Results come back as copies too.

```
let total = 0;
parallel(() => {
  total += 1;     // ❌ E0603: closures can't use the `let` variable `total`
}, () => print("other"));
```

Tasks that must change the same value use a [`Shared<T>`](#sharedt).

## `parallelMap`

`xs.parallelMap(fn)` works like `xs.map(fn)`, except that each call to `fn` runs as its own task. The results keep the list's order, whatever order the tasks finish in. How many tasks run at once is up to the runtime.

| Method | Signature | Description |
| --- | --- | --- |
| `parallelMap(fn, onFailure?)` | `(fn: (item: T) => U, onFailure: OnFailure = OnFailure.Finish) => list<U>` | Apply `fn` to every element, each call in its own task |

```
const pages = try urls.parallelMap((url) => try fetchPage(url));
const sizes = images.parallelMap((image) => compress(image).length);
```

## Task Groups

`parallel` and `parallelMap` know their tasks up front. A server doesn't: it starts a task for each connection as the connection arrives. `taskGroup` covers that case. It calls its function with a group, and `group.start(fn)` starts `fn` as a task in that group. `taskGroup` returns once its function and every task in the group have ended.

```
import { taskGroup } from "@dat/tasks";

taskGroup((group) => {
  for (const order of orders) {
    if (order.paid) {
      group.start(() => sendReceipt(order));
    }
  }
});
```

| Function or method | Signature | Description |
| --- | --- | --- |
| `taskGroup(fn, onFailure?)` | `(fn: (group: TaskGroup) => void, onFailure: OnFailure = OnFailure.Finish) => void` | Call `fn` with a new group, then wait for every task in it |
| `group.start(fn)` | `(fn: () => void) => void` | Start `fn` as a task in the group |

- A task can start more tasks in its own group: `group` is a parameter, so its closures can use it.
- The function given to `taskGroup` counts as the group's first task. If it throws, that's a failure like any other task's.
- `start` on a group whose `taskGroup` call has returned throws a `StoppedError`.
- A server library built on a group catches each connection's errors itself and answers that connection with an error, so one connection's failure never stops the server.

## When a Task Fails

If a task throws, the call that started it throws the same error once every task has ended. The trace shows the task's frames, then the call that started the task, then the calls that led there.

```
function loadAccount(id: int): Account throws HttpError {
  const [user, orders] = try parallel(() => try fetchUser(id), () => try fetchOrders(id));
  return Account { user, orders };
}
```

If `fetchOrders` fails with `NotFound`, `parallel` throws `NotFound`, `loadAccount` passes it on, and a caller catches it with an ordinary `try`/`catch`.

When several tasks fail, the error thrown is the one from the earliest of them: in the order they're written for `parallel`, in list order for `parallelMap`, and in the order they were started for a group. It's never simply whichever failed a moment sooner.

A task that handles its own error doesn't fail, and the others carry on:

```
const [user, orders] = parallel(() => try? fetchUser(id), () => try? fetchOrders(id));
// user: User?    orders: list<Order>?
```

### What Happens to the Other Tasks

By default, nothing that talks to the outside world is cut off. A request cut off on its way would leave the program not knowing whether it took effect: whether a card was charged, or a row was written. So after a task fails:

- Tasks that are already running carry on to their end, so every request they send gets its answer.
- Tasks that haven't started don't start. The remaining items of a `parallelMap` are skipped, and `group.start` throws a `StoppedError` instead of starting a task.
- A wait that might never end on its own, such as waiting for a new connection, ends with a `StoppedError`. The standard library says which of its waits these are.

With `OnFailure.Stop`, or [`.Stop`](./18-enums-and-unions.md#leaving-out-the-enums-name) for short, as the last argument, the other tasks are stopped at once. Whatever they're waiting on, or wait on next, ends with a `StoppedError`, even a request on its way. Use it when the tasks only read, so cutting them off loses nothing:

```
const [user, orders] = try parallel(() => try fetchUser(id), () => try fetchOrders(id), .Stop);
const pages = try urls.parallelMap((url) => try fetchPage(url), .Stop);
```

Leaving out `OnFailure.Stop` is never a bug: a failure only takes longer to report.

In both cases:

- Code that doesn't wait runs to its end. Only waits check for a stop, which keeps loops and calls as fast as code without tasks.
- `StoppedError` is a runtime error, so it needs no `throws`, and `catch` and `finally` blocks run as they do for any other error. A task gets it only once, so its cleanup code can still make calls.
- A task that ends with a `StoppedError` doesn't count as failing. The error thrown is the one that caused the stop.

### Steps That Must All Happen

No rule about stopping can undo a step that already happened. If two tasks must both succeed or neither take effect, such as reserving stock and charging a card, one of them can succeed after the other failed. Run such steps one after another, so the second runs only if the first succeeded:

```
const stock = try reserveStock(order);
const receipt = try chargeCard(order);
```

Or let each task report its own outcome, and undo what needs undoing:

```
const [stock, receipt] = parallel(() => try? reserveStock(order), () => try? chargeCard(order));
if (receipt != null && stock == null) {
  try refund(receipt);
}
```

## `Shared<T>`

Tasks get copies, so they can't change each other's values. For state that several tasks change, such as a counter or a cache, there's `Shared<T>`. Every copy of a shared value is a handle to the same value, and each change is made under the value's own lock.

```
import { Shared } from "@dat/tasks";

const hits = Shared.of(0);
parallel(() => hits.update((n) => n + 1), () => hits.update((n) => n + 1));
print(hits.get());   // 2
```

| Function or method | Signature | Description |
| --- | --- | --- |
| `Shared.of(value)` | `(value: T) => Shared<T>` | A new shared value |
| `get()` | `() => T` | A copy of the current value |
| `set(value)` | `(value: T) => void` | Replace the value |
| `update(fn)` | `(fn: (value: T) => T) => T` | Call `fn` with the value while holding its lock, store what it returns and return it. If `fn` throws, the value stays as it was. |

- A shared value is the one dat value that copies don't separate, which is its purpose. `const` on a variable that holds one fixes which value it refers to, while the value inside can still change.
- Shared values can't be compared with `==` and aren't [hashable](./01-builtin-types.md#equality-hashing-and-ordering). `print` shows the value inside: `Shared(2)`.
- A `Shared<T>` compiles to Rust's `Arc<Mutex<T>>`.

### Rules for `update`

An `update` function can't wait and can't use any shared value. So a lock is never held across a wait or taken while another is held, and two tasks can never end up waiting for each other forever.

```
prices.update((p) => p * fetchRate());     // ❌ E2201: an `update` function can't wait, and `fetchRate` waits
                                           //    fix (review, line above): const rate = fetchRate();
                                           //    fix (review): prices.update((p) => p * rate);
total.update((t) => t + count.get());      // ❌ E2202: an `update` function can't use another shared value: `count`
                                           //    fix (review, line above): const counted = count.get();
                                           //    fix (review): total.update((t) => t + counted);
```

When two values must change together, put them in one shared value.

Reading a shared value and then setting it loses an update if another task changes the value in between. The compiler rejects a `set` whose new value uses a `get` of the same value:

```
hits.set(hits.get() + 1);   // ❌ E2203: another task can change `hits` between this `get` and `set`
                            //    fix: hits.update((value) => value + 1);
```

### No Shared Module Constants

A module constant can't hold a shared value. Every test and every task in the program would share it, so tests could no longer run in parallel without affecting each other:

```
import { Shared } from "@dat/tasks";

const visits = Shared.of(0);   // ❌ E2204: a module constant can't hold a shared value

function visitCount(): int {
  return visits.get();
}
```

Create the shared value where it's first needed, such as in `main`, and pass it to the functions that use it.

## Output, Stacks and Tests

- Each `print` line stays whole, but lines printed by different tasks can arrive in any order.
- Each task has its own stack, so every task can nest at least 10,000 calls. See [Recursion Depth](./12-errors.md#recursion-depth).
- Tasks see only copies, so their results don't depend on timing, unless they use a shared value or the outside world. Even then, which error a failing call throws follows the order of its tasks, not their timing.
- How tasks are spread over threads is up to the implementation. The interpreter and compiled programs give the same results.
- [Tests](./20-testing.md) already run in parallel, and a test can start tasks like any other code. Each test's output is captured on its own.

## No `async` or `await`

Any function can wait, so dat has neither keyword. People and agents used to TypeScript will write them anyway, so the compiler recognizes both and gives the fix:

```
const user = await loadUser(id);   // ❌ E2205: dat has no `await`: a call waits by itself
                                   //    fix: const user = loadUser(id);
async function main(): void { }    // ❌ E2206: dat has no `async`: any function can wait
                                   //    fix: function main(): void { }
```

Neither is a reserved word. `await` before a call, or `async` before `function`, is never valid dat, so the compiler can spot the mistake while both words stay free as names.

## The `@dat/tasks` Module

```
import { taskGroup, TaskGroup, Shared, OnFailure, StoppedError } from "@dat/tasks";
```

| Name | Description |
| --- | --- |
| `taskGroup` | Start tasks one at a time, and wait for all of them. See [Task Groups](#task-groups). |
| `TaskGroup` | The group that `taskGroup` passes to its function |
| `Shared<T>` | A value that tasks change together. See [`Shared<T>`](#sharedt). |
| `OnFailure` | An enum with two members. `OnFailure.Finish`, the default, lets running tasks finish and starts nothing new. `OnFailure.Stop` stops the running tasks at their waits. |
| `StoppedError` | A runtime error with a `message`, like the [built-in runtime errors](./12-errors.md#built-in-error-types), so it needs no `throws`. A wait in a task that's being stopped throws it, and so does `group.start` after a failure or after its group has ended. |

Using one of these without importing it is an error, and its fix adds the import:

```
const hits = Shared.of(0);   // ❌ E1401: Shared is not defined
                             //    fix (import): import { Shared } from "@dat/tasks";
```
