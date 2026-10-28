# Set Methods

`set<T>` holds unique values in insertion order. Adding a value that's already present does nothing. Membership checks take constant time. `T` must be [hashable](./01-builtin-types.md#equality-hashing-and-ordering).

## Creating Sets

A list literal builds a set wherever a set type is expected:

```
const tags: set<string> = ["go", "rust", "go"];   // {"go", "rust"}
let seen: set<int> = [];
const all: set<string> = [...frontend, ...backend];
const uniqueIds: set<int> = [...ids];             // from a list
const asList = [...tags];                         // back to a list
```

## Size

`s.size` is the number of elements.

## Mutating Methods

These need a set that can be modified: a `let` variable, a `mut` parameter, or a field or element of one.

| Method | Signature | Description |
| --- | --- | --- |
| `add(item)` | `(item: T) => void` | Add `item`; does nothing if it's already present |
| `delete(item)` | `(item: T) => bool` | Remove `item`. Returns `true` if it was present. |
| `clear()` | `() => void` | Remove all elements |

## Non-Mutating Methods

| Method | Signature | Description |
| --- | --- | --- |
| `has(item)` | `(item: T) => bool` | `true` if `item` is present |
| `isEmpty()` | `() => bool` | `true` if the set has no elements |
| `union(other)` | `(other: set<T>) => set<T>` | Elements in either set |
| `intersection(other)` | `(other: set<T>) => set<T>` | Elements in both sets |
| `difference(other)` | `(other: set<T>) => set<T>` | Elements in this set but not in `other` |
| `symmetricDifference(other)` | `(other: set<T>) => set<T>` | Elements in exactly one of the two sets |
| `isSubsetOf(other)` | `(other: set<T>) => bool` | `true` if every element is also in `other` |
| `isSupersetOf(other)` | `(other: set<T>) => bool` | `true` if every element of `other` is in this set |
| `isDisjointFrom(other)` | `(other: set<T>) => bool` | `true` if the sets have no elements in common |

Results that are sets keep this set's order first, followed by `other`'s.

Sets have no index access and no `map` or `filter`. Convert to a list first: `[...tags].filter((t) => t.length > 2)`.

## Iteration

`for...of` goes through a set in insertion order:

```
for (const tag of tags) {
  print(tag);
}
```

## Examples

```
let tags: set<string> = ["go", "rust", "python"];

tags.add("zig");
tags.add("go");                  // no change: already present
print(tags.size);                // 4

print(tags.has("rust"));         // true
print(tags.delete("python"));    // true
print(tags.delete("python"));    // false: already removed

const systems: set<string> = ["rust", "zig", "c"];
print(tags.intersection(systems));   // {"rust", "zig"}
print(tags.difference(systems));     // {"go"}
```
