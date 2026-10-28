# Interfaces

An interface names a set of methods. A struct that implements those methods can be used wherever the interface is expected.

## Defining an Interface

```
interface Shape {
  area(): float;
  name(): string;
}
```

Interfaces declare methods only, never fields. For a data shape, use a [struct](./08-structs.md). If code needs a value from every implementing struct, declare a method that returns it.

```
interface User { name: string; }   // ❌ E1501: interfaces can't declare fields
                                   //    option: struct User { name: string; }
                                   //    option: interface User { name(): string; }
```

## Implementing an Interface

A struct implements an interface by declaring `implements` and defining the interface's methods:

```
struct Circle implements Shape {
  radius: float;

  area(): float {
    return Math.PI * this.radius * this.radius;
  }

  name(): string {
    return "circle";
  }
}
```

Implementation is explicit. A struct that happens to have matching methods doesn't implement the interface unless it says so. A struct can implement several interfaces: `struct Circle implements Shape, Named { ... }`.

## Default Methods

An interface method with a body is a default. Implementing structs get it automatically and can define their own version to replace it.

```
interface Shape {
  area(): float;
  name(): string;

  describe(): string {
    return `${this.name()} with area ${this.area().toFixed(2)}`;
  }
}

print(Circle { radius: 1.0 }.describe());   // circle with area 3.14
```

Inside a default method, `this` has the interface type, so it can call only the interface's methods. Default methods are how dat shares behaviour between types instead of inheritance.

## `mut` Methods

An interface can require methods that change the value:

```
interface Counter {
  count(): int;
  mut increment(): void;
}
```

## Extending Interfaces

An interface can extend one or more other interfaces:

```
interface Animal {
  name(): string;
  sound(): string;
}

interface Pet extends Animal {
  owner(): string;
}
```

A struct that implements `Pet` must also define `Animal`'s methods, and can be used wherever an `Animal` is expected.

## Interface Types

A variable, parameter, field or collection element can have an interface type. It holds a value of any implementing struct, and method calls go to that struct's methods:

```
const shapes: list<Shape> = [
  Circle { radius: 1.0 },
  Square { side: 2.0 },
];

for (const shape of shapes) {
  print(shape.describe());
}
```

Interface values are values too: copying one copies the struct inside it.

`is`, `as?` and `match` find out which struct an interface value holds:

```
if (shape is Circle) {
  print(shape.radius);   // shape is narrowed to Circle
}
```

Interface values can't be compared with `==`, and can't be set elements or map keys.

## Interfaces as Generic Constraints

An interface can constrain a type parameter:

```
function totalArea<T extends Shape>(shapes: list<T>): float {
  return shapes.reduce((sum, s) => sum + s.area(), 0.0);
}
```

The difference from `list<Shape>`: a `list<Shape>` can mix struct types, and each call is dispatched at runtime. A `list<T>` with `T extends Shape` holds one struct type, and the compiler generates specialized code for it, which is faster. See [Generics](./19-generics.md).

## Built-in Interfaces

`Equatable`, `Hashable` and `Comparable` are implemented automatically by every type that supports `==`, hashing or ordering. See [Equality, Hashing and Ordering](./01-builtin-types.md#equality-hashing-and-ordering). They can be used only as generic constraints. A struct can't list them after `implements`; it gets them from its fields or from its [operator methods](./08-structs.md#operator-overloading).
