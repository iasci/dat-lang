# Functions

## Named Functions

Functions are declared with the `function` keyword at the top level of a module. Parameter types are required.

```
function add(a: int, b: int): int {
  return a + b;
}

function greet(name: string): void {
  print(`Hello, ${name}!`);
}
```

Named functions can't be declared inside other functions. For a local function, assign an arrow function to a `const`.

## Return Types

A function that returns nothing has return type `void`, and can use `return;` to exit early.

The return type can be left out and inferred from the body, with two exceptions: exported functions, and methods of exported structs, must declare it. This keeps a module's interface readable without its bodies, and lets the compiler check each module independently.

```
function double(x: int) { return x * 2; }          // inferred: int
export function triple(x: int): int { return x * 3; }
```

Every path through a function that returns a value must end in `return`, `throw`, or a call to a `never` function such as `fail()`. Nothing returns `null` implicitly:

```
function sign(x: int): string {
  if (x > 0) {
    return "positive";
  }
  if (x < 0) {
    return "negative";
  }
}   // ❌ E0601: missing return when x == 0
    //    fix (review, end of sign): return _;
```

A function that can fail declares the failure with `throws`. See [Errors](./12-errors.md).

```
function parsePort(text: string): int throws ParseError { ... }
```

## Parameters

Arguments are passed by position. A parameter with a default value can be left out, and a nullable parameter defaults to `null`. Parameters with defaults go after the required ones.

```
function connect(host: string, port: int = 5432, label: string?): void {
  print(`${label ?? "db"} at ${host}:${port}`);
}

connect("localhost");                   // port = 5432, label = null
connect("localhost", 6543);             // label = null
connect("localhost", 6543, "primary");
```

Parameters are constants: the function can't reassign or modify them. To work with a changed version, copy it into a `let`:

```
function normalize(name: string): string {
  let result = name.trim();
  if (result.isEmpty()) {
    result = "anonymous";
  }
  return result;
}
```

## `mut` Parameters

A `mut` parameter gives the function direct access to the caller's value, so its changes are visible to the caller. The caller must write `mut` before the argument, so every call shows which values it may change.

```
function addItem(mut cart: Cart, item: Item): void {
  cart.items.push(item);
  cart.total += item.price;
}

let cart = Cart { items: [], total: 0.0 };
addItem(mut cart, Item { name: "pen", price: 1.5 });
print(cart.total);   // 1.5
```

Rules:

- The argument must be something that can be modified: a `let` variable, a `mut` parameter, or a field or element of one, such as `addItem(mut order.cart, item)` or `bump(mut counts[0])`.
- One call can't pass the same value as `mut` twice, including a value and a part of it: `swap(mut a, mut a)` and `merge(mut order, mut order.cart)` are errors. The receiver of a [`mut` method](./08-structs.md#methods) counts as a `mut` argument.

Returning a new value usually reads better. Use `mut` for in-place updates of large values, or when a function updates several things.

## Arrow Functions

Arrow functions are function values. They can be passed as callbacks, stored in variables, or returned. The parameter list always has parentheses. The body is either a single expression or a block.

```
const double = (x: int): int => x * 2;

const greet = (name: string) => {
  print(`Hello, ${name}`);
};

const answer = () => 42;
```

The return type is optional and inferred from the body.

### Callback Type Inference

When an arrow function is passed where a function type is expected, its parameter types can be left out:

```
const evens = [1, 2, 3, 4].filter((n) => n % 2 == 0);
const doubled = [1, 2, 3].map((x) => x * 2);
const total = [1.5, 2.5].reduce((sum, x) => sum + x, 0.0);
```

Without that context, parameter types are required:

```
const fn = (x) => x * 2;        // ❌ E0602: can't infer the type of x
                                //    option: const fn = (x: int) => x * 2;
                                //    option: const fn = (x: float) => x * 2;
const fn = (x: int) => x * 2;   // ✅
```

## Closures

An arrow function can use variables from the scope around it, but only ones that can't change: `const` variables, parameters other than `mut` ones, `const` loop variables, and `this` in methods that aren't `mut`. The closure keeps its own copy of each, so it can safely be stored and called later.

```
const rate = 0.2;
const withTax = (price: float) => price * (1.0 + rate);   // ✅

let total = 0.0;
items.forEach((item) => {
  total += item.price;     // ❌ E0603: closures can't use the `let` variable `total`
                           //    fix (whole forEach): for (const item of items) { total += item.price; }
});

for (const item of items) {
  total += item.price;     // ✅ use a loop...
}
const sum = items.reduce((acc, item) => acc + item.price, 0.0);   // ✅ ...or reduce
```

## Function Types

Functions are values. A function type is written like an arrow function signature. A named function can be passed by name.

```
function apply(x: int, transform: (value: int) => int): int {
  return transform(x);
}

function square(n: int): int { return n * n; }

apply(3, square);            // 9
apply(3, (n) => n + 1);      // 4
```

## The `main` Function

A program starts at the `main` function in the entry file. Statements aren't allowed outside functions.

```
function main(): void {
  print("Hello, World!");
}
```

`main` may declare `throws`. If an error escapes it, declared or runtime, the program prints the error and exits with status 1. See [Errors](./12-errors.md).

## Generic Functions

Functions can take type parameters: `function first<T>(items: list<T>): T?`. See [Generics](./19-generics.md).
