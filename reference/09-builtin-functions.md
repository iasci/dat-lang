# Built-in Functions

These are available everywhere without an import.

| Function | Signature | Description |
| --- | --- | --- |
| `print` | `(...values) => void` | Print values separated by spaces, then a newline |
| `range` | `(end: int) => list<int>`<br>`(start: int, end: int, step: int = 1) => list<int>` | Integers from `start` (default `0`) up to but not including `end` |
| `assert` | `(condition: bool, message: string?) => void` | Throw an `AssertionError` if `condition` is `false` |
| `fail` | `(message: string) => never` | Throw an `AssertionError` with `message` |

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

## `Math`

| Member | Signature | Description |
| --- | --- | --- |
| `Math.PI` | `float` | π |
| `Math.E` | `float` | e |
| `Math.abs(x)` | `(x: int) => int`<br>`(x: float) => float` | Absolute value |
| `Math.min(...values)` | `(...values: int) => int`<br>`(...values: float) => float` | Smallest value. Throws a `RangeError` when given none. |
| `Math.max(...values)` | `(...values: int) => int`<br>`(...values: float) => float` | Largest value. Throws a `RangeError` when given none. |
| `Math.floor(x)` | `(x: float) => int` | Round down |
| `Math.ceil(x)` | `(x: float) => int` | Round up |
| `Math.round(x)` | `(x: float) => int` | Round to nearest; halves round up: `Math.round(-2.5)` is `-2` |
| `Math.trunc(x)` | `(x: float) => int` | Round toward zero |
| `Math.sqrt(x)` | `(x: float) => float` | Square root |
| `Math.pow(base, exponent)` | `(base: float, exponent: float) => float` | `base` to the power of `exponent` |

`Math` functions that take a `float` also accept an `int`. If any argument to `min` or `max` is a `float`, the result is a `float`. `floor`, `ceil`, `round` and `trunc` throw a `RangeError` when the result is `NaN` or doesn't fit in an `int`. When they're applied directly to an `int / int` division, the result is computed exactly. See [Arithmetic](./03-operators.md#arithmetic).

```
Math.max(3, 7, 5)              // 7
Math.max(...scores)            // largest score; throws RangeError if scores is empty
Math.sqrt(16)                  // 4.0
Math.floor(7 / 2)              // 3
```
