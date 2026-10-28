# Operators

## Arithmetic

dat supports `+`, `-`, `*`, `/`, `%` and unary `-`.

| Operands | `+` `-` `*` `%` | `/` |
| --- | --- | --- |
| `int` and `int` | `int` | `float` |
| any mix with a `float` | `float` | `float` |

```
10 + 3       // 13
10 % 3       // 1
10 + 2.5     // 12.5
10 / 4       // 2.5
10 / 5       // 2.0, a float
```

`/` always returns a `float`, as it does in TypeScript. For integer division, use `Math.trunc(a / b)` or `Math.floor(a / b)`. When these are applied directly to `a / b` with two `int` operands, the compiler computes the result exactly in integer arithmetic.

```
const middle = Math.floor((low + high) / 2);   // int
```

Edge cases:

- `int` overflow throws a `RangeError`: `9223372036854775807 + 1` doesn't wrap around.
- `int % 0` throws a `RangeError`.
- Float division by zero follows IEEE 754 and produces `Infinity`, `-Infinity` or `NaN`, so `1 / 0` is `Infinity`.
- `%` keeps the sign of the left operand: `-7 % 3` is `-1`.

## String Concatenation

`+` joins two strings. Any other value must be converted first, with a template string or `toString()`.

```
"Hello, " + name            // ✅ both are strings
"Total: " + 42              // ❌ Error: can't add string and int
`Total: ${42}`              // ✅
"Total: " + 42.toString()   // ✅
```

## Assignment

`=` assigns. The compound forms `+=`, `-=`, `*=`, `/=` and `%=` are short for `x = x op y`.

```
let total = 0;
total += 5;
total *= 2;      // 10
total /= 4;      // ❌ Error: total is an int, but `total / 4` is a float
```

Assignments are statements, not expressions, so `if (x = 1)` and `a = b = 0` are errors.

## Increment and Decrement

`++` and `--` add or subtract `1` from an `int` variable. They are postfix statements, not expressions.

```
let i = 0;
i++;           // i is now 1
i--;           // i is now 0

let y = i++;   // ❌ Error: ++ is a statement
++i;           // ❌ Error: no prefix form
```

## Comparison

`==` and `!=` work on every type that supports equality, and compare contents. See [Equality, Hashing and Ordering](./01-builtin-types.md#equality-hashing-and-ordering).

```
[1, 2] == [1, 2]                              // true
Point { x: 1, y: 2 } == Point { x: 1, y: 2 }  // true
```

`<`, `<=`, `>` and `>=` work on `int`, `float`, `string`, and structs that define [`operator compare`](./08-structs.md#operator-overloading). Comparing an `int` with a `float` compares their numeric values. Strings compare by Unicode code point order.

## Membership: `in`

`item in c` is `true` if `c` contains `item`:

| Right side | `true` when | Same as |
| --- | --- | --- |
| `list<T>` | An element equals `item` | `xs.includes(item)` |
| `set<T>` | `item` is in the set | `s.has(item)` |
| `map<K, V>` | `item` is a key | `m.has(item)` |
| `string` | `item`, a string, occurs in it | `s.includes(item)` |
| A struct with `operator contains` | The method returns `true` | |

```
3 in [1, 2, 3]            // true
"PORT" in env             // a map: checks the keys
"lo" in "hello"           // true
!("guest" in roles)       // parentheses needed: `!` binds tighter than `in`
```

TypeScript's `in` checks keys, so in TypeScript `2 in [10, 20, 30]` is `true` because index 2 exists. In dat, `in` checks elements, so it's `false`. For maps, both check keys. `for...in` isn't valid in dat; use [`for...of`](./07-control-flow.md#loops).

## Operators on Your Own Types

Structs can define what `+`, `-`, `*`, `/`, `%`, unary `-`, `==`, `<`, `[]` and `in` do with `operator` methods. See [Operator Overloading](./08-structs.md#operator-overloading).

## Logical and Nullish Operators

`&&`, `||` and `!` work on `bool` only. `??`, `?.` and postfix `!` work on nullable values. See [Control Flow](./07-control-flow.md).

## Precedence

From highest to lowest:

| Level | Operators |
| --- | --- |
| 1 | `.` `?.` `()` `[]` postfix `!` |
| 2 | prefix `-` `!` `try` `try?` `try!` |
| 3 | `*` `/` `%` |
| 4 | `+` `-` |
| 5 | `<` `<=` `>` `>=` `in` `is` `as` `as?` |
| 6 | `==` `!=` |
| 7 | `&&` |
| 8 | `\|\|` |
| 9 | `??`, which can't be mixed with `&&` or `\|\|` without parentheses |
| 10 | `? :` |
| 11 | `=>` |

Operators at the same level group from left to right, except `? :`, which groups from right to left.
