# Scope

dat uses lexical scoping. A variable declared within a block, such as a function body, a loop or an `if` branch, is accessible only within that block and the blocks nested inside it.

```
function main(): void {
  let x = 10;

  if (x > 0) {
    let y = 20;
    print(x + y);  // 30
  }

  print(y);        // ❌ Error: y is not defined
}
```

Declaring a variable in an inner scope with the same name as one in an outer scope shadows the outer variable:

```
let x = 10;

if (x > 0) {
  let x = 20;
  print(x); // 20
}

print(x); // 10
```

Declaring the same name twice in one scope is an error:

```
let x = 10;
let x = 20; // ❌ Error: x is already declared
```

## Module Scope

A module's top level contains only declarations: functions, structs, enums, interfaces, type aliases, `const` values and tests. Each one is visible everywhere in the module, including before its declaration. There's no module-level `let`. See [Imports and Modules](./17-imports.md).

## Closures

An arrow function can use variables from the scopes around it, but only ones that can't change. See [Closures](./06-functions.md#closures).
