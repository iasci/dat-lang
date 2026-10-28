# Control Flow

## Conditional Statements

`if`, `else if` and `else`. The condition must be a `bool`; there is no truthiness. Braces are required.

```
if (x > 0) {
  print("Positive");
} else if (x < 0) {
  print("Negative");
} else {
  print("Zero");
}

if (user) { }           // ❌ Error: condition must be bool; write `user != null`
if (items.length) { }   // ❌ Error: write `items.length > 0` or `!items.isEmpty()`
```

## Ternary Operator

```
const label = x > 0 ? "positive" : "non-positive";
```

The result type is the union of the two branch types.

## Logical Operators

`&&` (and), `||` (or) and `!` (not) take `bool` operands and short-circuit: the right side runs only when it can change the result.

```
const safe = isValid() && process();       // process() runs only if isValid() is true
const ready = isReady() || initialize();   // initialize() runs only if isReady() is false
```

## Nullish Coalescing (`??`)

`a ?? b` is `a` if it isn't `null`, and `b` otherwise. The left side must be nullable, and the result has the non-null type.

```
let user: string? = null;
const display = user ?? "Anonymous";            // string
const result = first ?? second ?? "fallback";
```

## Optional Chaining (`?.`)

`a?.b` reads a field or calls a method only when `a` isn't `null`; otherwise the whole expression is `null`. Reading a field or calling a method directly on a nullable value is a compile error.

```
let user: User? = findUser(id);
const name = user.name;                  // ❌ Error: user may be null
const name = user?.name;                 // string?
const zip = user?.address?.zip;          // string?
const len = user?.name.length ?? 0;      // int
```

## Non-null Assertion (`!`)

`value!` is the value with `null` ruled out. It throws a `TypeError` if the value is `null`. See [Type Narrowing and Casts](./05-type-narrowing.md#non-null-assertion-).

## `match`

`match` picks a branch by value or by type. It's an expression, it must cover every case, and there's no fall-through.

```
const label = match (status) {
  Status.Active => "active",
  Status.Suspended | Status.Deleted => "inactive",
};

const area = match (shape) {
  Circle => Math.PI * shape.radius * shape.radius,
  Square => shape.side * shape.side,
};
```

See [Enums and Unions](./18-enums-and-unions.md#match) for the full rules.

## Loops

### `for`

```
for (let i = 0; i < 10; i++) {
  if (i == 3) {
    break;
  }
  if (i % 2 == 0) {
    continue;
  }
  print(i);
}
```

### `for...of`

`for...of` goes through the elements of a list, a set, a map or a string:

```
for (const name of ["Ada", "Lin"]) { print(name); }
for (const tag of tags) { print(tag); }               // set, in insertion order
for (const [key, value] of scores) { print(`${key}: ${value}`); }   // map entries
for (const c of "héllo") { print(c); }               // characters
for (const i of range(5)) { print(i); }              // 0, 1, 2, 3, 4
for (const [i, name] of names.entries()) { print(`${i}. ${name}`); }
```

The loop goes through a copy of the collection taken when the loop starts, so the body can change the collection without affecting the iteration:

```
let queue = [1, 2, 3];
for (const n of queue) {
  queue.push(n * 10);   // fine: the loop still runs three times
}
print(queue);           // [1, 2, 3, 10, 20, 30]
```

Loop variables can be destructured with the same patterns as declarations:

```
for (const { name, age } of users) {
  print(`${name} is ${age}`);
}
```

### `mut` Loop Variables

`for (mut x of xs)` gives the body direct access to each element of the list, so changes are made to the list itself. `xs` must be something that can be modified, such as a `let` variable or a `mut` parameter. Inside the body, the list can only be reached through the loop variable.

```
for (mut user of users) {
  user.active = true;      // changes the element in `users`
}

for (mut user of users) {
  users.push(user);        // ❌ Error: `users` is in use by the loop
}
```

### `while`

```
let attempts = 0;
while (attempts < 3 && !connected()) {
  attempts++;
}
```

`break` and `continue` work in every kind of loop.
