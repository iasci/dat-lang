# List Methods

`list<T>` is an ordered sequence of values of type `T`. Lists are values: assigning or passing one gives an independent copy. Copying is cheap until one of the copies changes.

## Creating Lists

```
const empty: list<int> = [];
const nums = [3, 1, 4];
const more = [...nums, 1, 5];
const zeros = range(5).map((_) => 0);    // [0, 0, 0, 0, 0]
```

## Index Access

| Syntax | Description |
| --- | --- |
| `xs[i]` | The element at index `i`. Throws a `RangeError` if `i` is out of range. |
| `xs[i] = v` | Replace the element at index `i`. Throws a `RangeError` if `i` is out of range. |
| `xs.at(i)` | The element at index `i`. A negative `i` counts from the end, so `xs.at(-1)` is the last element. Throws a `RangeError` if `i` is out of range. |

```
let colors = ["red", "green", "blue"];
print(colors[0]);           // red
print(colors.at(-1));       // blue
print(try? colors.at(10));  // null
colors[1] = "yellow";
print(colors[5]);           // throws RangeError: index 5 out of range for length 3
```

## Length

`xs.length` is the number of elements.

## Mutating Methods

These need a list that can be modified: a `let` variable, a `mut` parameter, or a field or element of one.

| Method | Signature | Description |
| --- | --- | --- |
| `push(...items)` | `(...items: T) => void` | Add one or more elements at the end: `xs.push(4)`, `xs.push(...others)` |
| `pop()` | `() => T` | Remove and return the last element. Throws a `RangeError` if the list is empty. |
| `unshift(...items)` | `(...items: T) => void` | Add one or more elements at the front |
| `shift()` | `() => T` | Remove and return the first element. Throws a `RangeError` if the list is empty. |
| `insert(index, item)` | `(index: int, item: T) => void` | Insert before `index`; `index == length` appends. Throws a `RangeError` if `index` is out of range. |
| `removeAt(index)` | `(index: int) => T` | Remove and return the element at `index`. Throws a `RangeError` if `index` is out of range. |
| `remove(item)` | `(item: T) => bool` | Remove the first element equal to `item`. Returns `true` if one was removed. |
| `sort(compare?)` | `(compare: ((a: T, b: T) => int)?) => void` | Sort in place. The sort is stable. Without `compare`, it uses natural order, which requires a [comparable](./01-builtin-types.md#equality-hashing-and-ordering) `T`. `compare` returns a negative number, zero, or a positive number. |
| `sortBy(key)` | `(key: (item: T) => K) => void` | Sort in place by a comparable key |
| `reverse()` | `() => void` | Reverse in place |
| `clear()` | `() => void` | Remove all elements |

## Non-Mutating Methods

| Method | Signature | Description |
| --- | --- | --- |
| `isEmpty()` | `() => bool` | `true` if the list has no elements |
| `includes(item)` | `(item: T) => bool` | `true` if an element equals `item` |
| `indexOf(item)` | `(item: T) => int` | Index of the first element equal to `item`, or `-1` |
| `lastIndexOf(item)` | `(item: T) => int` | Index of the last element equal to `item`, or `-1` |
| `slice(start?, end?)` | `(start: int = 0, end: int?) => list<T>` | Elements from `start` (inclusive) to `end` (exclusive, default the end). Negative indexes count from the end, and out-of-range indexes are clamped. |
| `concat(other)` | `(other: list<T>) => list<T>` | A new list with `other`'s elements added at the end |
| `join(separator?)` | `(separator: string = ",") => string` | The elements' `toString()` texts joined with `separator` |
| `map(fn)` | `(fn: (item: T) => U) => list<U>` | Apply `fn` to every element |
| `filter(fn)` | `(fn: (item: T) => bool) => list<T>` | The elements for which `fn` returns `true` |
| `flatMap(fn)` | `(fn: (item: T) => list<U>) => list<U>` | Apply `fn` and join the resulting lists |
| `flat()` | `() => list<U>` | Join a `list<list<U>>` into a `list<U>` |
| `reduce(fn, initial)` | `(fn: (acc: U, item: T) => U, initial: U) => U` | Combine all elements into one value, starting from `initial` |
| `forEach(fn)` | `(fn: (item: T) => void) => void` | Call `fn` for every element |
| `find(fn, fallback?)` | `(fn: (item: T) => bool) => T?`<br>`(fn: (item: T) => bool, fallback: T) => T` | The first element for which `fn` returns `true`. If none does, `fallback`, or `null` when there's no fallback. |
| `findIndex(fn)` | `(fn: (item: T) => bool) => int` | The index of that element, or `-1` |
| `some(fn)` | `(fn: (item: T) => bool) => bool` | `true` if `fn` returns `true` for at least one element |
| `every(fn)` | `(fn: (item: T) => bool) => bool` | `true` if `fn` returns `true` for every element |
| `count(fn)` | `(fn: (item: T) => bool) => int` | The number of elements for which `fn` returns `true` |
| `entries()` | `() => list<[int, T]>` | Each element paired with its index |
| `toSorted(compare?)` | `(compare: ((a: T, b: T) => int)?) => list<T>` | A sorted copy |
| `toSortedBy(key)` | `(key: (item: T) => K) => list<T>` | A copy sorted by a comparable key |
| `toReversed()` | `() => list<T>` | A reversed copy |
| `unique()` | `() => list<T>` | A copy without duplicates, keeping first occurrences. Requires a hashable `T`. |
| `zip(other)` | `(other: list<U>) => list<[T, U]>` | Pairs of elements at the same index, as long as the shorter list |
| `groupBy(key)` | `(key: (item: T) => K) => map<K, list<T>>` | Elements grouped by a hashable key, keeping their order |

The callbacks of `map`, `filter`, `flatMap`, `forEach`, `find`, `findIndex`, `some`, `every` and `count` can take the element's index as a second parameter: `xs.map((x, i) => ...)`.

`at`, `pop` and `shift` throw instead of returning `null`, so a `null` result always means a stored `null`. To get `null` instead of the error, mark the call with [`try?`](./12-errors.md#discarding-or-asserting-try-and-try):

```
let queue = ["a", "b"];
const next = try? queue.shift();               // string?: null when the queue is empty
const last = try? queue.at(-1) ?? "none";      // "none" when the queue is empty
const admin = users.find((u) => u.isAdmin);    // User?
const top = scores.find((s) => s > 90, 0);     // int: 0 when no score is above 90
```

On a list of nullable values, `try? xs.pop()` can't tell an empty list from a stored `null`; check `xs.isEmpty()` first.

If a callback can fail, the method can fail too, and the call needs `try`. See [Errors in Callbacks](./12-errors.md#errors-in-callbacks).

## Examples

```
let nums = [3, 1, 4, 1, 5];

nums.push(9);                           // [3, 1, 4, 1, 5, 9]
nums.sort();                            // [1, 1, 3, 4, 5, 9]
print(nums.length);                     // 6
print(nums.includes(4));                // true
print(nums.indexOf(7));                 // -1
print(nums.unique());                   // [1, 3, 4, 5, 9]

const doubled = nums.map((x) => x * 2);
const evens = nums.filter((x) => x % 2 == 0);
const sum = nums.reduce((acc, x) => acc + x, 0);

const people = [
  Person { name: "Ada", age: 36 },
  Person { name: "Lin", age: 29 },
];
const youngestFirst = people.toSortedBy((p) => p.age);
const byAge = people.toSorted((a, b) => a.age - b.age);
const names = people.map((p) => p.name).join(", ");   // "Ada, Lin"
```
