# Structs

A struct groups named fields and can have methods. Structs are values: assigning or passing one gives an independent copy. See [Value Semantics](./02-variable-declaration.md#value-semantics).

## Declaring a Struct

```
struct User {
  name: string;
  email: string?;            // nullable: defaults to null
  active: bool = true;       // default value
  tags: list<string> = [];
}
```

## Creating Values

A struct literal names the struct and sets its fields. Every field without a default must be set, and unknown fields are errors.

```
const ada = User { name: "Ada" };                 // email: null, active: true, tags: []
const lin = User { name: "Lin", email: "lin@example.com", active: false };
const mo = User { email: "mo@example.com" };                   // ❌ E0850: User needs a `name`
                                                               //    fix (review): const mo = User { email: "mo@example.com", name: _ };
const kai = User { name: "Kai", emial: "kai@example.com" };    // ❌ E0851: User has no field `emial`
```

When the type is already known from context, the struct name can be left out:

```
const kai: User = { name: "Kai" };

function register(user: User): void { ... }
register({ name: "Mo" });
```

A field with the same name as a variable can use the shorthand:

```
const name = "Ada";
const user = User { name };
```

Spreading a struct copies it with some fields changed:

```
const inactive = { ...ada, active: false };   // User
```

There's no `new` and no constructor. To validate input or compute fields, write a [static](#static-functions) factory function.

## Fields

Fields are public unless marked `private`. A field can be changed through any value that can be modified: a `let` variable, a `mut` parameter, or `this` inside a `mut` method.

```
let user = User { name: "Ada" };
user.name = "Ada L.";
user.tags.push("admin");

const fixed = User { name: "Lin" };
fixed.name = "X";      // ❌ E0201: `fixed` is const and can't be modified
                       //    fix (review, declaration): let fixed = User { name: "Lin" };
```

## Methods

```
struct Rectangle {
  width: float;
  height: float;

  area(): float {
    return this.width * this.height;
  }

  mut scale(factor: float): void {
    this.width *= factor;
    this.height *= factor;
  }

  static square(side: float): Rectangle {
    return Rectangle { width: side, height: side };
  }
}

let r = Rectangle.square(2.0);
print(r.area());    // 4.0
r.scale(1.5);
print(r.area());    // 9.0
```

- Methods use `this` to read the struct's fields and call its other methods. `this` is required.
- A regular method can't change `this`.
- A `mut` method can change `this`. It can be called only on a value that can be modified: a `let` variable, a `mut` parameter, or a field or element of one.

```
const fixed = Rectangle.square(1.0);
fixed.scale(2.0);    // ❌ E0201: scale() is a mut method and `fixed` is const
                     //    fix (review, declaration): let fixed = Rectangle.square(1.0);
```

### Static Functions

`static` functions belong to the struct itself rather than to a value, so they have no `this`. Call them through the struct's name: `Rectangle.square(2.0)`.

## Private Members

A `private` field or method can be used only inside the struct's own methods and static functions.

```
struct Account {
  owner: string;
  private balance: int;

  static open(owner: string): Account {
    return Account { owner, balance: 0 };
  }

  currentBalance(): int {
    return this.balance;
  }

  mut deposit(amount: int): void {
    assert(amount > 0, "deposit must be positive");
    this.balance += amount;
  }
}

let account = Account.open("Ada");
account.deposit(100);
print(account.currentBalance());    // 100
print(account.balance);             // ❌ E0801: `balance` is private
                                    //    fix: print(account.currentBalance());
const fake = Account { owner: "Eve", balance: 1_000_000 };   // ❌ E0802: can't set private field `balance`
                                                             //    fix (review): const fake = Account.open("Eve");
```

A struct literal must set every field that has no default. So a struct with a private field that has no default can only be created by its own static functions, which makes them the place to validate input.

## Custom `toString()`

Every struct gets a `toString()` that prints its name and fields, such as `User { name: "Ada", active: true }`. Define `toString(): string` to replace it. See [Primitive Methods](./04-primitive-methods.md#tostring).

## Equality and Hashing

By default, structs compare field by field with `==`, and a struct whose fields are all hashable can be a set element or a map key.

```
struct Point {
  x: int;
  y: int;
}

const a = Point { x: 1, y: 2 };
const b = Point { x: 1, y: 2 };
print(a == b);                    // true

const seen: set<Point> = [a, b];
print(seen.size);                 // 1
```

To define equality differently, write [`operator equals`](#operator-overloading). Values that are equal must hash the same, so custom equality and hashing go together:

| The struct defines | Result |
| --- | --- |
| Neither | Automatic `==` and hashing, field by field |
| `operator equals` and `hash()` | Custom `==` and hashing. The struct is hashable even if its fields aren't, such as floats. |
| Only `operator equals` | Custom `==`. The struct isn't hashable: using it as a set element or map key is a compile error. |
| Only `hash()` | Compile error: custom hashing needs custom equality |

```
struct Temperature {
  celsius: float;

  operator equals(other: Temperature): bool {
    return Math.round(this.celsius * 10) == Math.round(other.celsius * 10);
  }

  hash(): int {
    return Math.round(this.celsius * 10);
  }
}

print(Temperature { celsius: 21.04 } == Temperature { celsius: 21.0 });   // true
const readings: set<Temperature> = [];                                    // ✅ hashable
```

## Operator Overloading

A struct can define what built-in operators do with its values by writing `operator` methods. In the signatures below, `Self` stands for the struct itself.

| Method | Signature | Used by |
| --- | --- | --- |
| `operator equals` | `(other: Self): bool` | `==`, `!=` |
| `operator compare` | `(other: Self): int` | `<`, `<=`, `>`, `>=`, and `sort()` without a comparator |
| `operator add` | `(other: A): R` | `a + b`, `a += b` |
| `operator subtract` | `(other: A): R` | `a - b`, `a -= b` |
| `operator multiply` | `(other: A): R` | `a * b`, `a *= b` |
| `operator divide` | `(other: A): R` | `a / b`, `a /= b` |
| `operator modulo` | `(other: A): R` | `a % b`, `a %= b` |
| `operator negate` | `(): R` | `-a` |
| `operator get` | `(index: I): V` | `a[i]` |
| `mut operator set` | `(index: I, value: V): void` | `a[i] = v` |
| `operator contains` | `(item: E): bool` | `item in a` |

```
struct Vector {
  x: float;
  y: float;

  operator add(other: Vector): Vector {
    return Vector { x: this.x + other.x, y: this.y + other.y };
  }

  operator subtract(other: Vector): Vector {
    return Vector { x: this.x - other.x, y: this.y - other.y };
  }

  operator multiply(factor: float): Vector {
    return Vector { x: this.x * factor, y: this.y * factor };
  }

  operator negate(): Vector {
    return Vector { x: -this.x, y: -this.y };
  }
}

const a = Vector { x: 1.0, y: 2.0 };
const b = Vector { x: 3.0, y: 4.0 };
print(a + b);      // Vector { x: 4.0, y: 6.0 }
print(a * 2.0);    // Vector { x: 2.0, y: 4.0 }
print(-a);         // Vector { x: -1.0, y: -2.0 }
```

Indexing and `in`:

```
struct Histogram {
  private counts: map<string, int> = {};

  operator get(key: string): int {
    return this.counts.get(key, 0);
  }

  mut operator set(key: string, value: int): void {
    this.counts[key] = value;
  }

  operator contains(key: string): bool {
    return key in this.counts;
  }
}

let words = Histogram {};
words["dat"] += 1;          // calls get, then set
print(words["dat"]);        // 1
print(words["rust"]);       // 0
print("dat" in words);      // true
```

Ordering:

```
struct Version {
  major: int;
  minor: int;

  operator compare(other: Version): int {
    if (this.major != other.major) {
      return this.major - other.major;
    }
    return this.minor - other.minor;
  }
}

let versions = [Version { major: 1, minor: 4 }, Version { major: 1, minor: 2 }];
print(Version { major: 2, minor: 0 } > versions[0]);   // true
versions.sort();                                        // 1.2, then 1.4
```

Rules:

- `!=` is the opposite of `operator equals`. `operator compare` returns a negative number, zero or a positive number, and provides all four ordering operators. It also makes the struct [comparable](./01-builtin-types.md#equality-hashing-and-ordering).
- `==` doesn't use `operator compare`. If the two should agree, define both.
- The struct must be the left operand: `v * 2.0` calls `operator multiply`, and `2.0 * v` is a compile error.
- A struct defines each operator at most once. dat has no overloading by parameter type, so `Vector * float` and `Vector * Vector` can't both exist.
- `operator set` must be `mut`, and the other operator methods can't be. They can't be `static` and can't declare `throws`; to fail, they throw a [runtime error](./12-errors.md#built-in-error-types).
- Operator methods are called only through their operators, not by name.

## Implementing Interfaces

A struct implements interfaces with `implements` and must define every method they require that has no default:

```
struct Circle implements Shape, Named {
  radius: float;

  area(): float {
    return Math.PI * this.radius * this.radius;
  }

  name(): string {
    return "circle";
  }
}
```

See [Interfaces](./15-interfaces.md).

## No Inheritance

Structs can't extend other structs. Inheritance does three jobs, and each has a direct replacement.

**Sharing data: composition.** Put the shared fields in their own struct:

```
struct Address {
  city: string;
  zip: string;
}

struct Customer {
  name: string;
  address: Address;
}

struct Supplier {
  company: string;
  address: Address;
}
```

**Sharing behaviour: interfaces with default methods.**

```
interface Named {
  name(): string;

  greet(): string {
    return `Hello, ${this.name()}`;
  }
}
```

**A fixed set of variants: a union.**

```
type Account = PersonalAccount | BusinessAccount;
```

See [Enums and Unions](./18-enums-and-unions.md).

## Recursive Structs

A struct can contain itself through a list, a map or a nullable field:

```
struct TreeNode {
  value: int;
  children: list<TreeNode> = [];
}

struct ListNode {
  value: int;
  next: ListNode?;
}
```

## Generic Structs

Structs can take type parameters: `struct Pair<A, B> { first: A; second: B; }`. See [Generics](./19-generics.md).
