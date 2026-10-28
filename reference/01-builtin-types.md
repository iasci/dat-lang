# Built-in Types

## Primitive Types

| Type | Description |
| --- | --- |
| `int` | 64-bit signed integer. Overflow throws a `RangeError`; see [Operators](./03-operators.md). |
| `float` | 64-bit IEEE 754 floating point number |
| `string` | Immutable UTF-8 text |
| `bool` | `true` or `false` |
| `null` | The absence of a value. Only nullable types can hold it. |
| `void` | The return type of a function that returns nothing. It isn't a value: no variable can have type `void`. |
| `never` | The type of an expression that never finishes, such as `fail("...")`. It can be used wherever any type is expected. |

## Collection Types

| Type | Description |
| --- | --- |
| `list<T>` | Ordered sequence of `T`. See [List Methods](./10-list-methods.md). |
| `set<T>` | Unique values of type `T`, kept in insertion order. See [Set Methods](./11-set-methods.md). |
| `map<K, V>` | Key-value pairs, kept in insertion order. See [Map Methods](./16-map-methods.md). |
| `[A, B, ...]` | Tuple: a fixed number of values, each with its own type |

```
const scores: list<int> = [90, 85, 77];
const tags: set<string> = ["new", "sale"];   // a list literal with a set type builds a set
const ages: map<string, int> = { alice: 30, bob: 25 };
const entry: [string, int] = ["alice", 30];
```

Collections are values like everything else: assigning a list gives an independent copy. See [Value Semantics](./02-variable-declaration.md#value-semantics).

### Tuples

A tuple has a fixed length, and each position has its own type. Read a position with a constant index, or destructure the tuple:

```
const entry: [string, int] = ["alice", 30];
print(entry[0]);              // alice
const [name, age] = entry;    // name: string, age: int

let i = 0;
print(entry[i]);              // ❌ E0101: tuple index must be a constant
                              //    option: print(entry[0]);
                              //    option: print(entry[1]);
```

A list literal becomes a tuple only where a tuple type is expected. Elsewhere it's a list: `["alice", 30]` on its own is a `list<int | string>`.

## Nullable Types

`T?` is shorthand for `T | null`. A value of a non-nullable type can never be `null`.

```
let email: string? = null;
let phone: string | null = "555-0100";   // same kind of type
let name: string = null;                 // ❌ E0102: string is not nullable
                                         //    fix: let name: string? = null;
```

## Union Types

`A | B` holds a value of either type, and knows which one it holds. Use `is` or `match` to find out. See [Enums and Unions](./18-enums-and-unions.md).

```
let id: int | string = 42;
id = "user-42";
```

## Function Types

A function type is written like an arrow function's signature, with named parameters. The names are documentation only.

```
let onSave: (name: string, size: int) => void = (name, size) => {
  print(`saved ${name}`);
};
const parse: (text: string) => int throws ParseError = parsePort;
```

## User-defined Types

- `struct`: see [Structs](./08-structs.md)
- `enum` and union types: see [Enums and Unions](./18-enums-and-unions.md)
- `interface`: see [Interfaces](./15-interfaces.md)
- `type Name = ...;` gives another name to an existing type. An alias is the same type, not a new one: `type UserId = int;`

## Literals

```
42          // int
-7          // int
1_000_000   // int; underscores are ignored
0xff        // int, hexadecimal
0b1010      // int, binary
3.14        // float
2.0         // float
1e9         // float
"text"      // string
true        // bool
null
```

An integer literal can be used where a `float` is expected: `const ratio: float = 1;`. An `int` variable can't; convert it with `.toFloat()`.

### String Literals

Strings use double quotes. The following escape sequences are supported:

| Sequence | Meaning |
| --- | --- |
| `\n` | Newline |
| `\t` | Tab |
| `\r` | Carriage return |
| `\"` | Double quote |
| `\\` | Backslash |
| `\0` | Null byte |
| `\u{1F600}` | Unicode code point |

### Template Strings

Backtick strings can embed expressions with `${...}`. Each embedded value is converted with `toString()`. Template strings can span multiple lines. Inside them, `` \` `` and `\${` are escapes.

```
const name = "Ada";
const count = 3;
print(`Hello, ${name}! You have ${count + 1} messages.`);
```

## Equality, Hashing and Ordering

`==` and `!=` compare contents:

- Primitives compare by value. Floats follow IEEE 754, so `NaN != NaN`.
- Lists and tuples compare element by element, in order.
- Sets and maps compare their contents regardless of insertion order.
- Structs compare field by field, unless they define [`operator equals`](./08-structs.md#equality-and-hashing). Enums compare by member.
- Union values are equal when they hold the same type and equal values.
- Function values and interface values can't be compared.

**Hashable** types can be set elements and map keys. They are `int`, `string`, `bool` and enums, plus lists, tuples, structs and unions built only from hashable types. `float` isn't hashable, because `NaN != NaN`, and neither is anything that contains a float, except a struct that defines its own `operator equals` and `hash()`. A struct that defines only `operator equals` isn't hashable. Sets, maps, function values and interface values aren't hashable.

**Comparable** types support `<`, `<=`, `>`, `>=`, and `sort()` without a comparator. They are `int`, `float`, `string`, and structs that define [`operator compare`](./08-structs.md#operator-overloading). An `int` can be compared with a `float`.

These three capabilities exist as the built-in interfaces `Equatable`, `Hashable` and `Comparable`, for use as generic constraints. See [Generics](./19-generics.md).

## Generic Type Variance

Generic types are invariant: a `list<int>` is not a `list<int | string>`. A conversion would have to change every element, so dat makes it explicit. Spreading into a literal of the target type converts each element:

```
const ints: list<int> = [1, 2, 3];
const mixed: list<int | string> = ints;       // ❌ E0103: list<int> is not list<int | string>
                                              //    fix: const mixed: list<int | string> = [...ints];
const mixed: list<int | string> = [...ints];  // ✅ converts each element
```

Single values convert implicitly: an `int` can be used where an `int | string` or an `int?` is expected.

## Comments

```
// This is a single-line comment

/*
  This is a
  multi-line comment
*/
```
