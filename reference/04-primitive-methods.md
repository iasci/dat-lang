# Primitive Methods

## `toString()`

Every value has a `toString()` method that returns its text form. `print()` and template strings use it.

| Type | Format |
| --- | --- |
| `int` | `42`, `-7` |
| `float` | The shortest form that reads back as the same number, always with a decimal point or an exponent: `2.0`, `0.1`, `3.14`. Exponent form is used when the exponent is 21 or more, or -7 or less: `1e21`, `1.5e-7`. Special values are `NaN`, `Infinity` and `-Infinity`. |
| `string` | The text itself. Inside a collection or struct, it's quoted: `"text"`. |
| `bool` | `true`, `false` |
| `null` | `null` |
| list, tuple | `[1, 2, 3]` |
| `set` | `{1, 2, 3}` |
| `map` | `{"alice": 10, "bob": 20}` |
| struct | `User { name: "Ada", age: 36 }` |
| enum | The member name: `Active` |
| function | `<function>` |

A struct can define its own `toString()`, which is then used everywhere, including inside collections:

```
struct Money {
  cents: int;

  toString(): string {
    return `$${(this.cents / 100).toFixed(2)}`;
  }
}

const price = Money { cents: 1999 };
print(price);              // $19.99
print([price, price]);     // [$19.99, $19.99]
```

## `int` Methods

| Method | Signature | Description |
| --- | --- | --- |
| `toFloat()` | `() => float` | Convert to a float |
| `toString()` | `() => string` | Convert to text |

## `float` Methods

| Method | Signature | Description |
| --- | --- | --- |
| `toInt()` | `() => int` | Truncate toward zero. Throws a `RangeError` if the value is `NaN` or outside the `int` range. |
| `toFixed(digits)` | `(digits: int) => string` | Text with exactly `digits` decimals, rounded to nearest with ties away from zero: `3.14159.toFixed(2)` is `"3.14"` |
| `isNaN()` | `() => bool` | `true` if the value is `NaN` |
| `toString()` | `() => string` | Convert to text |

For rounding to an `int`, see `Math.floor`, `Math.ceil`, `Math.round` and `Math.trunc` in [Built-in Functions](./09-builtin-functions.md#math).

## `string` Methods

Strings are UTF-8. Lengths and positions count **bytes**, which is the same as counting characters for ASCII text. Every position a method returns is a valid character boundary, so the results of `indexOf()` can be passed safely to `slice()`.

| Member | Signature | Description |
| --- | --- | --- |
| `length` | `int` | Length in bytes |
| `isEmpty()` | `() => bool` | `true` if the string has no characters |
| `includes(text)` | `(text: string) => bool` | `true` if `text` occurs in the string |
| `startsWith(prefix)` | `(prefix: string) => bool` | `true` if the string starts with `prefix` |
| `endsWith(suffix)` | `(suffix: string) => bool` | `true` if the string ends with `suffix` |
| `indexOf(text)` | `(text: string) => int` | Byte position of the first occurrence, or `-1` |
| `lastIndexOf(text)` | `(text: string) => int` | Byte position of the last occurrence, or `-1` |
| `slice(start, end?)` | `(start: int, end: int?) => string` | Text from `start` (inclusive) to `end` (exclusive, default the end). Negative positions count from the end, and out-of-range positions are clamped. Throws a `RangeError` if a position falls inside a multi-byte character. |
| `split(separator)` | `(separator: string) => list<string>` | Split on `separator`. Separators at the edges give empty strings: `"a,".split(",")` is `["a", ""]`. |
| `trim()` | `() => string` | Remove leading and trailing whitespace |
| `trimStart()` | `() => string` | Remove leading whitespace |
| `trimEnd()` | `() => string` | Remove trailing whitespace |
| `toUpperCase()` | `() => string` | Convert to uppercase |
| `toLowerCase()` | `() => string` | Convert to lowercase |
| `replace(from, to)` | `(from: string, to: string) => string` | Replace the **first** occurrence of `from` |
| `replaceAll(from, to)` | `(from: string, to: string) => string` | Replace every occurrence of `from` |
| `repeat(count)` | `(count: int) => string` | The string repeated `count` times |
| `padStart(length, fill?)` | `(length: int, fill: string = " ") => string` | Pad at the start up to `length` bytes |
| `padEnd(length, fill?)` | `(length: int, fill: string = " ") => string` | Pad at the end up to `length` bytes |
| `toInt()` | `() => int throws ValueError` | Parse as an integer |
| `toFloat()` | `() => float throws ValueError` | Parse as a float |
| `toString()` | `() => string` | The string itself |

```
const port = try "8080".toInt();             // 8080
const n = try "12x".toInt();                 // throws ValueError: invalid digit "x" in "12x"
const m = try? "abc".toInt() ?? 0;           // 0
"a-b-c".replace("-", "+")                    // "a+b-c"
"a-b-c".replaceAll("-", "+")                 // "a+b+c"
"report.pdf".slice(-3)                       // "pdf"
```

### Index Access

`s[i]` returns the character that starts at byte position `i`, as a string. It throws a `RangeError` if `i` is out of range or falls inside a multi-byte character. Strings are immutable, so `s[i] = ...` is a compile error.

```
const s = "hello";
s[0]         // "h"
s[4]         // "o"
s[5]         // throws RangeError: index 5 out of range for length 5
```

### Iterating Characters

`for...of` and spread go through a string one character (Unicode code point) at a time:

```
for (const c of "héllo") {
  print(c);                  // h, é, l, l, o
}
const chars = [..."héllo"];  // ["h", "é", "l", "l", "o"]
print(chars.length);         // 5, while "héllo".length is 6
```
