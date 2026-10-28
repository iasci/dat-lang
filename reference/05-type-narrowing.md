# Type Narrowing and Casts

dat has no `any` and no runtime type inspection. Every value's type is known at compile time. Only union types (including nullable types) and interface values can hold different types at runtime, and `is`, `match` and the casts below work on those.

## Type Guard (`is`)

`value is T` is `true` when the value currently holds a `T`. `T` must be a member of the value's union type or, for an interface value, a struct that implements the interface.

```
function describe(value: int | string | null): string {
  if (value is int) {
    return `number ${value + 1}`;            // value: int
  }
  if (value is string) {
    return `text ${value.toUpperCase()}`;    // value: string
  }
  return "nothing";                          // value: null
}
```

A check that can never be true is a compile error:

```
let id: int | string = 42;
if (id is float) { }     // ❌ E0501: int | string can never be a float
                         //    option: if (id is int) { }
                         //    option: if (id is string) { }
```

`is` can also be used as an ordinary `bool` expression: `const isText = value is string;`.

## Narrowing

The compiler tracks control flow and narrows a variable's type wherever a check guarantees it. Narrowing works with:

- `if (x is T)`, `if (x != null)` and `if (x == null)`, in both the `if` branch and the `else` branch
- `&&` and `||`: in `x != null && x.isEmpty()`, `x` is non-null on the right side
- Early exits: after a branch that ends in `return`, `throw`, `break`, `continue` or a call to a `never` function such as `fail()`, the rest of the block keeps the narrowed type
- `match` arms; see [Enums and Unions](./18-enums-and-unions.md#match)

```
function city(user: User): string {
  if (user.address == null) {
    return "unknown";
  }
  return user.address.city;   // user.address is narrowed to Address
}
```

Narrowing applies to local variables, parameters and field paths such as `user.address`. It ends when the variable, or any part of the path, is assigned or passed as `mut`.

## Non-null Assertion (`!`)

`value!` turns a `T?` into a `T`. If the value is `null`, it throws a `TypeError` whose message shows the source location.

```
const first = users.find((u) => u.isAdmin)!;   // throws TypeError if there is no admin
```

Use it only when other code guarantees the value isn't `null`. Otherwise, use `??`, `?.` or a `!= null` check.

## Casts (`as` and `as?`)

`value as? T` returns the value as a `T`, or `null` if it holds something else. `value as T` returns a `T` and throws a `TypeError` if the value holds something else. Both accept the same targets as `is`.

```
const shape: Shape = loadShape();
const circle = shape as? Circle;   // Circle?
const square = shape as Square;    // Square; throws TypeError if shape isn't a Square
```

Prefer `is` with narrowing, or `match`. Use `as` only when other code guarantees the type.

Casts don't convert between number types or parse text. Use `.toFloat()`, `.toInt()` and the string methods for that.
