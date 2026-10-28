# Generics

Functions, structs and interfaces can take type parameters. The compiler generates specialized code for each set of type arguments, so generics cost nothing at runtime.

## Generic Functions

```
function first<T>(items: list<T>): T? {
  return try? items.at(0);
}

const n = first([1, 2, 3]);       // T = int, so n is int?
const s = first<string>([]);      // explicit when it can't be inferred
```

Type arguments are inferred from the call's arguments, and from the expected type when the arguments aren't enough.

## Generic Structs

```
struct Pair<A, B> {
  first: A;
  second: B;

  swap(): Pair<B, A> {
    return Pair { first: this.second, second: this.first };
  }
}

const p = Pair { first: "age", second: 36 };   // Pair<string, int>
const q: Pair<int, string> = p.swap();
```

## Generic Interfaces

```
interface Source<T> {
  mut next(): T?;
}

struct Countdown implements Source<int> {
  current: int;

  mut next(): int? {
    if (this.current == 0) {
      return null;
    }
    const value = this.current;
    this.current--;
    return value;
  }
}
```

## Constraints

`T extends I` restricts `T` to types that implement the interface `I`, which lets the function use `I`'s methods. `&` combines several constraints.

```
function describeAll<T extends Shape>(shapes: list<T>): list<string> {
  return shapes.map((s) => s.describe());
}

function largest<T extends Comparable>(items: list<T>): T? {
  let best: T? = null;
  for (const item of items) {
    if (best == null || item > best) {
      best = item;
    }
  }
  return best;
}

function dedupe<T extends Hashable>(items: list<T>): list<T> {
  let seen: set<T> = [];
  let result: list<T> = [];
  for (const item of items) {
    if (!seen.has(item)) {
      seen.add(item);
      result.push(item);
    }
  }
  return result;
}

function sortedLabels<T extends Shape & Hashable>(shapes: set<T>): list<string> { ... }
```

Without a constraint, a type parameter supports only what every type supports: assigning, passing, returning, storing in collections, and `toString()`. Using `==` needs `T extends Equatable`, `<` needs `T extends Comparable`, and set elements and map keys need `T extends Hashable`.

The built-in constraints are `Equatable`, `Hashable` and `Comparable`. See [Equality, Hashing and Ordering](./01-builtin-types.md#equality-hashing-and-ordering).
