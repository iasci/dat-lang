# Testing

Tests are part of the language. A `test` block declares a test, and `dat test` finds and runs every test.

```
// math.dat
export function clamp(value: int, low: int, high: int): int {
  return Math.max(low, Math.min(value, high));
}

test "clamp keeps values inside the range" {
  assert(clamp(5, 0, 10) == 5);
  assert(clamp(-3, 0, 10) == 0);
  assert(clamp(42, 0, 10) == 10);
}
```

## Rules

- `test` blocks appear at module level, in any file. Each test's name must be unique within its module.
- A test can use its module's non-exported declarations.
- A test body behaves like a function declared `throws Error`, so it can use `try` directly. An error that escapes fails the test.
- A runtime error that escapes, such as a failed `assert` or an index out of range, fails the test.
- Tests share no state, because dat has no global mutable state, so they run in parallel and in any order.
- Tests aren't included in production builds.

## Failure Output

When an `assert` condition is a comparison, a failure shows both sides, so finding the cause rarely needs an extra `print`. Suppose `clamp` forgot the lower bound and returned `Math.min(value, high)`. The failure shows:

```
FAIL math.dat:8  clamp keeps values inside the range
  assert(clamp(-3, 0, 10) == 0)
    left:  -3
    right: 0
```

## Testing Errors

```
test "rejects ports out of range" {
  assert(try? parsePort("70000") == null);
}

test "explains why a port is invalid" {
  try {
    try parsePort("abc");
    fail("expected a ParseError");
  } catch (e: ParseError) {
    assert(e.reason == "not a number");
  }
}
```

## Running Tests

```
dat test                      # every test, on the interpreter
dat test src/math.dat         # the tests in one file
dat test --filter clamp       # tests whose name contains "clamp"
dat test --compiled           # compile to Rust and run the same tests
dat test --verbose            # also list the tests that pass
```

CI runs every test both on the interpreter and compiled, and fails if the two results differ.

## Test Output

`dat test` prints nothing for a test that passes. It prints each failure in full, then one summary line. If the failing test above had also called `print("checking -3")`, `dat test math.dat` would show:

```
FAIL math.dat:8  clamp keeps values inside the range
  assert(clamp(-3, 0, 10) == 0)
    left:  -3
    right: 0
  output:
    checking -3

1 failed, 0 passed (3 ms)
```

- Output from `print` is captured for each test and shown under `output:` only when that test fails. Tests run in parallel, so output from different tests never interleaves.
- The report goes to stdout. Compile errors go to stderr.
- The exit status is `0` when every test passes, `1` when any test fails, and `2` when the tests couldn't run: a compile error, an unknown option, or a `--filter` that matches no test.

See [Command Line](./21-command-line.md) for the other commands.
