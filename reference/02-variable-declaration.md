# Variable Declaration

## `let` and `const`

A `let` variable can change: it can be reassigned, and its contents can be modified. A `const` variable can't change at all: no reassignment and no modification, at any depth.

```
let items: list<int> = [1, 2];
items.push(3);     // ✅
items[0] = 10;     // ✅
items = [4, 5];    // ✅

const fixed = [1, 2];
fixed.push(3);     // ❌ E0201: `fixed` is const and can't be modified
                   //    fix (review, declaration): let fixed = [1, 2];
fixed[0] = 10;     // ❌ E0201: `fixed` is const and can't be modified
                   //    fix (review, declaration): let fixed = [1, 2];
fixed = [4, 5];    // ❌ E0201: `fixed` is const and can't be reassigned
                   //    fix (review, declaration): let fixed = [1, 2];

let user = User { name: "Ada", tags: [] };
user.tags.push("admin");   // ✅ any depth can change through a `let`

const admin = User { name: "Lin", tags: [] };
admin.name = "Kai";        // ❌ E0201: `admin` is const and can't be modified
                           //    fix (review, declaration): let admin = User { name: "Lin", tags: [] };
admin.tags.push("ops");    // ❌ E0201: `admin` is const and can't be modified
                           //    fix (review, declaration): let admin = User { name: "Lin", tags: [] };
```

`const` prevents modification as well as reassignment. Mutability belongs to the variable, not to the type, so there are no separate read-only collection types.

Use `const` by default, and `let` when the value has to change.

## Value Semantics

Every dat value is a value, not a reference. Assigning a value to a variable, passing it to a function, returning it, or storing it in a collection or struct gives the receiver an independent copy.

```
let a = [1, 2, 3];
let b = a;
b.push(4);
print(a);   // [1, 2, 3]
print(b);   // [1, 2, 3, 4]

let first = User { name: "Ada", tags: [] };
let second = first;
second.name = "Lin";
print(first.name);    // Ada
print(second.name);   // Lin
```

Copies are cheap. Lists, sets, maps and strings share memory between copies, and copy their contents only when one copy is modified while another still exists (copy-on-write). Assigning or passing a collection never copies its elements by itself.

### Changing a Value in Place

To let a function change the caller's value, mark the parameter `mut` and pass the argument with `mut`. See [`mut` parameters](./06-functions.md#mut-parameters).

```
function addTag(mut user: User, tag: string): void {
  user.tags.push(tag);
}

let ada = User { name: "Ada", tags: [] };
addTag(mut ada, "admin");
print(ada.tags);   // ["admin"]
```

To change list elements while looping, use a `mut` loop variable. See [`mut` loop variables](./07-control-flow.md#mut-loop-variables).

```
for (mut user of users) {
  user.active = true;
}
```

### Changing a Copy by Mistake

Code written with references in mind tends to change a copy and expect the original to change:

```
let tags = user.tags;
tags.push("admin");   // ❌ E0202: `tags` is modified but never used afterwards.
                      //    It is a copy of `user.tags`; modify `user.tags` directly.
                      //    fix (review): user.tags.push("admin");
```

If a `let` variable starts as a copy of another variable, a field or an element, is modified, and is never read afterwards, the compiler reports an error, so this mistake can't pass silently. Write `user.tags.push("admin");` instead, or assign the copy back with `user.tags = tags;`. The same check catches `for (let user of users) { user.active = true; }`, where the fix is a [`mut` loop variable](./07-control-flow.md#mut-loop-variables).

## Nullable Variables

Nullable types are written `T?` or `T | null`. A nullable `let` declared without a value starts as `null`.

```
let email: string? = null;
let email: string | null = "ada@example.com";
let email: string?;              // starts as null

let email: string = null;        // ❌ E0102: string is not nullable
                                 //    fix: let email: string? = null;
```

## Type Inference

When a variable has an initial value, its type annotation can be left out.

```
let x = 42;                    // int
let name = "Alice";            // string
let pi = 3.14;                 // float
let active = true;             // bool
let items = [1, 2, 3];         // list<int>
let scores = { alice: 10 };    // map<string, int>
let mixed = [1, "two", 3];     // list<int | string>
let pair = ["a", 1];           // list<int | string>, not a tuple
let total = add(1, 2);         // int, from add's return type
```

Where a type is expected, a literal takes that type:

```
const tags: set<string> = ["a", "b"];          // set
const entry: [string, int] = ["a", 1];         // tuple
const user: User = { name: "Ada", age: 36 };   // struct, same as User { ... }
const ratio: float = 1;                        // float
```

Any other value must already have the declared type. `void` isn't a value, so no variable can hold one:

```
const count = 3;
const label: string = count;          // ❌ E0250: expected a string, found an int
                                      //    fix: const label: string = `${count}`;
const saved: void = print("saved");   // ❌ E0252: a variable can't hold void
```

Some declarations can't be inferred and are invalid:

```
let x: int;           // ❌ E0203: a non-nullable variable needs a value
                      //    option: let x: int = _;
                      //    option: let x: int?;
let x;                // ❌ E0204: no type and no value
                      //    fix (review): let x = _;
const x: string;      // ❌ E0205: const must be initialized
                      //    fix (review): const x: string = _;
let items = [];       // ❌ E0206: can't infer the element type
                      //    fix (review): let items: list<_> = [];
```

## Destructuring

### Lists and Tuples

```
const numbers = [1, 2, 3, 4, 5];

const [first, second] = numbers;       // first = 1, second = 2
const [head, ...rest] = numbers;       // head = 1, rest = [2, 3, 4, 5]
const [_, _, third] = numbers;         // third = 3
const [a, b = 0] = [1];                // b = 0, the default for a missing element

const entry: [string, int] = ["alice", 30];
const [name, age] = entry;             // name: string, age: int
```

If a list has fewer elements than the pattern needs and no default is given, destructuring throws a `RangeError`.

### Structs

```
const user = User { name: "Alice", age: 30, email: null };

const { name, age } = user;               // name = "Alice", age = 30
const { name: firstName } = user;         // firstName = "Alice"
const { email = "none" } = user;          // "none": the default replaces null
```

Struct destructuring has no `...rest`, because the rest wouldn't be a named type. Maps can't be destructured, since their keys may be missing; read them with `m["key"]`.

### Nested Destructuring

```
const order = Order { customer: User { name: "Alice", age: 30 }, items: [10, 20, 30] };

const { customer: { name }, items: [firstItem, ...otherItems] } = order;
// name = "Alice", firstItem = 10, otherItems = [20, 30]
```

### In Parameters and Loops

```
function greet({ name, age }: User): string {
  return `${name} is ${age}`;
}

for (const [key, value] of scores) {
  print(`${key}: ${value}`);
}
```

## Spread

`...` copies the elements of a collection into a new literal.

```
const a = [1, 2];
const b = [3, 4];
const merged = [...a, ...b];             // [1, 2, 3, 4]
const extended = [0, ...a, 9];           // [0, 1, 2, 9]

const unique: set<int> = [...a, ...b];   // a list literal with a set type builds a set

const defaults = { theme: "dark", lang: "en" };
const overrides = { lang: "fr" };
const config = { ...defaults, ...overrides };   // { theme: "dark", lang: "fr" }

const letters = [..."héllo"];            // ["h", "é", "l", "l", "o"]
```

Spreading a struct copies it with some fields changed. The result has the struct's type:

```
const older = { ...user, age: 31 };      // User
```

## Compound Assignment

`+=`, `-=`, `*=`, `/=`, `%=`, `++` and `--` work on `let` variables. See [Operators](./03-operators.md#assignment).

## Module-level Variables

Only `const` is allowed at module level; there's no global mutable state. See [Imports and Modules](./17-imports.md).
