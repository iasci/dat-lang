# Built-in Functions

These are available everywhere without an import.

| Function | Signature | Description |
| --- | --- | --- |
| `print` | `(...values) => void` | Print values separated by spaces, then a newline |
| `range` | `(end: int) => list<int>`<br>`(start: int, end: int, step: int = 1) => list<int>` | Integers from `start` (default `0`) up to but not including `end` |
| `assert` | `(condition: bool, message: string?) => void` | Throw an `AssertionError` if `condition` is `false` |
| `fail` | `(message: string) => never` | Throw an `AssertionError` with `message` |
| `parallel` | `(...tasks, onFailure?) => [...results]` | Run each function as a task, wait for all of them, and return their results as a tuple. See [Concurrency](./22-concurrency.md#parallel). |

Rounding, square roots and the other math operations are [methods on numbers](./04-primitive-methods.md), and also functions in the [`@dat/math`](./04-primitive-methods.md#the-datmath-module) module.

## `print`

`print` accepts any number of values of any type. It converts each one with `toString()`, joins them with spaces, ends the line, and writes the result to stdout. Strings are printed without quotes.

```
print("Hello, World!");          // Hello, World!
print("total:", 42, [1, 2]);     // total: 42 [1, 2]
print(User { name: "Ada" });     // User { name: "Ada", email: null, active: true, tags: [] }
```

## `range`

```
range(5)            // [0, 1, 2, 3, 4]
range(2, 5)         // [2, 3, 4]
range(10, 0, -3)    // [10, 7, 4, 1]
range(0)            // []
```

A `step` of `0` throws a `RangeError`. When `range` is used directly in a `for...of` loop, no list is created.

## `assert`

`assert` throws an `AssertionError` when its condition is `false`. When the condition is a comparison (`==`, `!=`, `<`, `<=`, `>` or `>=`), the message shows the value of both sides.

```
assert(items.length > 0, "items must not be empty");
assert(total == 100);
// AssertionError: assertion failed: total == 100
//   left:  95
//   right: 100
```

`assert` is the main tool in [tests](./20-testing.md).

## `fail`

`fail` throws an `AssertionError` with `message`, for code that should never be reached. Its return type is `never`, so it can be used wherever a value is expected:

```
const port = match (env) {
  "prod" => 443,
  "dev" => 8080,
  _ => fail(`unknown environment ${env}`),
};
```

Like every runtime error, it can be caught. See [Errors](./12-errors.md) for when to use a runtime error and when to declare one.
