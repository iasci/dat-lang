# Enums and Unions

## Enums

An enum is a type with a fixed list of named members.

```
enum Status { Active, Suspended, Deleted }

const status = Status.Active;
print(status);                       // Active
print(status == Status.Active);      // true
```

Members have no attached values. Enums support `==`, are [hashable](./01-builtin-types.md#equality-hashing-and-ordering), and can be matched exhaustively with [`match`](#match).

## Union Types

A union type holds a value of one of several types, and knows which one it holds.

```
type Id = int | string;

struct Circle {
  radius: float;
}

struct Square {
  side: float;
}

type Shape = Circle | Square;
```

A value of any member type can be used where the union is expected:

```
let shape: Shape = Circle { radius: 1.0 };
shape = Square { side: 2.0 };

const id: Id = 42;
```

A union of structs needs no tag field such as `kind`, because the struct's type is the tag. When the expected type is a union, a struct literal must name its struct: `Circle { radius: 1.0 }`, not `{ radius: 1.0 }`.

Unions are flattened and deduplicated, and member order doesn't matter: `type A = int | string; type B = A | null;` makes `B` the same type as `string | int | null`. `T?` is shorthand for `T | null`.

### Shared Fields

A field that every member of a union has, with the same type, can be read and assigned without narrowing:

```
struct Circle {
  label: string;
  radius: float;
}

struct Square {
  label: string;
  side: float;
}

type Shape = Circle | Square;

function caption(shape: Shape): string {
  return shape.label;   // ✅ every member has `label: string`
}
```

Other fields and methods need narrowing first, using `is` or `match`.

## `match`

`match` chooses a branch by comparing a value against a list of patterns.

```
import { PI } from "@dat/math";

function area(shape: Shape): float {
  return match (shape) {
    Circle => PI * shape.radius * shape.radius,
    Square => shape.side * shape.side,
  };
}
```

Each arm is `pattern => result`, and arms are separated by commas. Arms are tried from top to bottom, and only the first matching one runs; there's no fall-through.

### Patterns

| Pattern | Matches |
| --- | --- |
| A type: `Circle`, `int`, `null` | A union or interface value that holds that type |
| An enum member: `Status.Active` | That member |
| A literal: `0`, `"GET"`, `true` | An equal value |
| `A \| B` | Either pattern |
| `_` | Anything |

When the subject is a variable, a parameter or a field path, it's narrowed inside each arm, the same way as with [`is`](./05-type-narrowing.md#narrowing). In the example above, `shape` is a `Circle` in the first arm.

### Exhaustiveness

A `match` must cover every possible value:

- For unions, enums, `bool` and nullable types, every member must have an arm, or there must be a `_` arm.
- For `int`, `float` and `string`, a `_` arm is required.
- For interface values, a `_` arm is required, because other modules can add implementations.
- An arm that can never be reached, such as one after `_` or a repeated pattern, is an error.

Adding a member to a union or an enum makes every `match` that doesn't handle it a compile error, so the compiler lists every place that needs updating.

```
enum Status { Active, Suspended, Deleted, Archived }

const label = match (status) {
  Status.Active => "active",
  Status.Suspended | Status.Deleted => "inactive",
};   // ❌ E1801: match doesn't handle Status.Archived
     //    fix (review, new arm): Status.Archived => _,
```

### Expressions and Statements

`match` used as a value is an expression, and each arm must then be an expression. As a statement, its arms can also be blocks:

```
match (command) {
  "start" => {
    print("starting");
    start();
  },
  "stop" => stop(),
  _ => print(`unknown command ${command}`),
}
```

## Unions or Interfaces?

Both let one variable hold different struct types.

- Use a **union** when the set of variants is fixed and known in one place. Then `match` checks that every variant is handled, and adding a variant shows every place that needs updating.
- Use an **interface** when the set is open, for example when other modules add their own implementations, or when you care about what values can do rather than which variant they are.
