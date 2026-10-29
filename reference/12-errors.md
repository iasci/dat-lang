# Errors

dat has two kinds of errors:

- **Declared errors** are failures the caller should expect: a file is missing, or input doesn't parse. A function that can produce one declares it with `throws`, and every call to that function is marked with `try`.
- **Runtime errors** come from operations that can fail anywhere: an index out of range, a missing map key, `x!` on `null`, a failed `assert`. They are [built-in types](#built-in-error-types), and they need no `throws` and no `try`.

Both kinds are caught the same way, with `try`/`catch`, so one bad input doesn't have to stop the program. An error that nothing catches stops the program, or fails the current test, with its message, its source location and a stack trace.

## Declaring Errors

Any struct or enum can be a declared error. A function lists the error types it can throw after `throws`:

```
struct ParseError {
  input: string;
  reason: string;
}

function parsePort(text: string): int throws ParseError {
  const n = try? text.toInt();
  if (n == null) {
    throw ParseError { input: text, reason: "not a number" };
  }
  if (n < 1 || n > 65535) {
    throw ParseError { input: text, reason: "out of range" };
  }
  return n;
}
```

A function that can throw several types lists them as a union: `throws ParseError | IoError`. A function can throw only the declared error types it lists. The one exception is the general [`Error`](#the-error-type) type, which every other error converts into.

Enums make good error types when the cases carry no data:

```
enum HttpError { NotFound, Unauthorized, Timeout }

function fetchUser(id: int): User throws HttpError { ... }
```

## Passing Errors On: `try`

Every call to a function that can throw a declared error must be marked with `try`. If the call fails, the error propagates: the current function stops and throws the same error, so it has to declare that error type too.

```
struct MissingSetting {
  name: string;
}

function readPort(env: map<string, string>): int throws ParseError | MissingSetting {
  if (!("PORT" in env)) {
    throw MissingSetting { name: "PORT" };
  }
  return try parsePort(env["PORT"]);
}
```

Calling a throwing function without `try` is a compile error. `try` covers the call right after it, including a chain of calls: `try client.get(url).text()`.

## Handling Errors: `try` / `catch`

```
try {
  const port = try readPort(env);
  print(`listening on ${port}`);
} catch (e: ParseError) {
  print(`bad port "${e.input}": ${e.reason}`);
} catch (e: MissingSetting) {
  print(`${e.name} is not set`);
}
```

- When a `try` call in the block fails, or a runtime error happens anywhere in the block, including inside the functions it calls, control jumps to the first `catch` clause whose type matches the error.
- `catch (e: A | B)` catches either type, and `e` has the type `A | B`.
- `catch (e: Error)` catches every remaining error, declared or runtime, converted to [`Error`](#the-error-type). It must be the last clause.
- `catch (e)` without a type also catches every remaining error, but keeps its type: `e` is the union of the remaining declared error types and the runtime error types. It must be the last clause.
- A `catch` clause for a declared error type that nothing in the block can throw is a compile error. This doesn't apply to `catch (e: Error)` or `catch (e)`: runtime errors can come from almost any line, so a catch-all is never dead code.
- Declared errors that no clause catches propagate out of the function, which must declare them. Runtime errors that no clause catches keep propagating.
- A `catch` clause can rethrow the error with `throw e;`.
- Changes made in the block before the error stay: a `let` variable or a `mut` parameter changed before the failure keeps the change.

A clause must name an error type, and a clause that catches every error comes last:

```
function check(port: int): void {
  if (port < 1) {
    throw RangeError { message: `port out of range: ${port}` };
  }
}

try { check(0); } catch (e: string) { }                            // ❌ E1251: `string` isn't an error type
try { check(0); } catch (e: Error) { } catch (e: RangeError) { }   // ❌ E1252: a clause that catches every error must be the last one
```

Catching runtime errors keeps one failure from taking down everything else, as in this request handler:

```
function handle(request: Request): Response {
  try {
    return try route(request);
  } catch (e: Error) {
    print(`request failed: ${e.message}`);
    return Response { status: 500 };
  }
}
```

## `finally`

A `finally` block runs when the `try` block and any `catch` clause finish, whichever way they finish: normally, with `return`, `break` or `continue`, or with an error, whether caught or not.

```
function importAll(paths: list<string>): int throws ImportError {
  print("import started");
  try {
    for (const path of paths) {
      try importFile(path);
    }
    return paths.length;
  } finally {
    print("import finished");   // runs after the return, and when an error leaves the block
  }
}
```

- A `try` block needs at least one `catch` clause or a `finally` block.
- A `finally` block can't leave early: `return`, `break`, `continue`, `throw` and `try` calls are compile errors inside it. Use `try?` or `try!` for calls that can fail.
- If a runtime error happens inside `finally`, it replaces any error that was passing through.

## Discarding or Asserting: `try?` and `try!`

```
const port = try? parsePort(text) ?? 8080;   // try? gives null on error
const port = try! parsePort("8080");          // try! throws an AssertionError on error
```

`try?` turns a `T` result into `T?` and drops the error. It works on two kinds of operations:

- A call to a function that throws declared errors: `try? parsePort(text)`.
- A built-in operation that throws a `RangeError`, `KeyError` or `TypeError`: `try? xs[i]`, `try? xs.pop()`, `try? m[key]`.

`try?` covers only the failures of the expression it marks. A runtime error raised inside a function that the expression calls, or inside a callback, passes through, so `try? parsePort(text)` doesn't hide a bug inside `parsePort`.

`try!` is for calls that can't fail in practice. If the call does fail, `try!` throws an `AssertionError` whose message includes the original error. It applies only to declared errors.

## Built-in Error Types

Every built-in error type is a struct with one field, `message: string`.

| Type | Kind | Thrown by |
| --- | --- | --- |
| `Error` | Declared | Code that doesn't need specific error types. See [The `Error` Type](#the-error-type). |
| `ValueError` | Declared | `string.toInt()` and `string.toFloat()` when the text isn't a number |
| `RangeError` | Runtime | `xs[i]`, `xs.at(i)` and `s[i]` with an index out of range; `pop()` and `shift()` on an empty list; `insert` and `removeAt` with an invalid index; a string position inside a multi-byte character; destructuring a list that's too short when no default is given; `int` overflow and `int % 0`; `float.toInt()`, `Math.floor`, `Math.ceil`, `Math.round` and `Math.trunc` when the value is `NaN` or out of range; `Math.min()` and `Math.max()` with no values; `range` with a `step` of `0` |
| `KeyError` | Runtime | `m[key]` when `key` is missing |
| `TypeError` | Runtime | `x!` when `x` is `null`; `x as T` when `x` isn't a `T` |
| `AssertionError` | Runtime | A failed `assert`; `fail(message)`; `try!` when the call fails; a module constant whose initializer depends on itself |
| `StackOverflowError` | Runtime | A call that nests deeper than the stack allows. See [Recursion Depth](#recursion-depth). |

### Throwing Runtime Errors

Code can throw a runtime error anywhere, without `throws`:

```
function percent(value: int): int {
  if (value < 0 || value > 100) {
    throw RangeError { message: `percent out of range: ${value}` };
  }
  return value;
}
```

Use a runtime error when a caller broke a rule it should have known, such as passing an argument out of range. Use a declared error when the caller is expected to handle the failure. Runtime error types can't appear after `throws`.

Only errors can be thrown, not strings or other values:

```
throw "percent out of range";   // ❌ E1250: only errors can be thrown, and this is a string
```

## The `Error` Type

`Error` is a built-in struct for code that doesn't need specific error types:

```
struct Error {
  message: string;
}
```

Any error, declared or runtime, converts to `Error` automatically in two places: when it propagates into a function declared `throws Error`, and when it's caught by `catch (e: Error)`. The message becomes the original error's `toString()`.

```
function main(): void throws Error {
  const port = try readPort(env);    // ParseError and MissingSetting become Error
  if (port == 80) {
    throw Error { message: "port 80 needs root" };
  }
}
```

These are the only implicit conversions between error types. After a conversion, the original type is gone, so code that needs to tell error types apart should catch them first.

## Error Messages

An error is shown using its `toString()`. Define one to control the message:

```
struct ParseError {
  input: string;
  reason: string;

  toString(): string {
    return `can't parse "${this.input}": ${this.reason}`;
  }
}
```

The built-in error types show their name and message: `RangeError: index 5 out of range for length 3`.

`int` overflow and `int % 0` name the operation and the values involved:

```
RangeError: int overflow: 9223372036854775807 + 1
RangeError: int overflow: -(-9223372036854775808)
RangeError: int remainder by zero: 7 % 0
```

`i++`, `i--` and compound assignments such as `i += 1` report the operation they perform, so an `i++` that overflows reads `9223372036854775807 + 1`.

## Recursion Depth

A call that nests deeper than the stack allows throws a `StackOverflowError`. It's a runtime error, so it needs no `throws`, and it can be caught like any other:

```
function forever(n: int): void {
  forever(n + 1);
}

try {
  forever(0);
} catch (e: StackOverflowError) {
  print("too deep");
}
```

- Every program can nest at least 10,000 calls, in both run modes. How much deeper it can go depends on the run mode and the platform, so code that needs more depth should use a loop.
- `try?` never turns a `StackOverflowError` into `null`. The overflow always happens inside a called function, and `try?` doesn't cover errors raised inside the functions it calls.

## Errors in Callbacks

If a callback passed to a built-in method can throw a declared error, the method call can throw the same errors and needs `try`:

```
function parseAll(texts: list<string>): list<int> throws ParseError {
  return try texts.map((t) => try parsePort(t));
}
```

A function type declares its errors the way a function does: `(text: string) => int throws ParseError`. An arrow function's errors are inferred from its body.

## Uncaught Errors

If an error of either kind escapes `main`, the program writes it to stderr and exits with status 1. The output shows the error, then where it was thrown and each call that led there:

```
RangeError: int overflow: 4660046610375530309 + 7540113804746346429
  at fibIter (overflow_uncaught.dat:9:18)
  at main (overflow_uncaught.dat:17:9)
```

- The trace lists dat functions only, never frames from the interpreter or the generated Rust.
- Output the program printed to stdout before the error is flushed first, so a terminal shows the two in the order they happened.

In a test, an uncaught error fails the test. See [Testing](./20-testing.md).
