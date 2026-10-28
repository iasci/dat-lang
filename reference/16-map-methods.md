# Map Methods

`map<K, V>` holds key-value pairs in insertion order. `K` must be [hashable](./01-builtin-types.md#equality-hashing-and-ordering); `V` can be any type.

## Creating Maps

```
const ages = { alice: 30, bob: 25 };                 // map<string, int>; identifier keys are strings
const headers = { "content-type": "text/html" };     // quoted keys
const byKey = { [key]: value };                      // computed key
const names: map<int, string> = { 1: "one", 2: "two" };
let empty: map<string, int> = {};
const merged = { ...defaults, ...overrides };        // later entries win
```

## Access

| Syntax | Description |
| --- | --- |
| `m[key]` | The value for `key`. Throws a `KeyError` if `key` is missing. |
| `m[key] = value` | Insert or replace the value for `key` |
| `m.get(key, fallback)` | The value for `key`, or `fallback` if `key` is missing |
| `key in m` | `true` if `key` exists |

```
let scores = { alice: 10, bob: 20 };

print(scores["alice"]);              // 10
print(scores["nobody"]);             // throws KeyError: key "nobody" not found
print(scores.get("nobody", 0));      // 0
print("bob" in scores);              // true
scores["carol"] = 30;
```

A lookup never returns `null` for a missing key, so a `null` result always means the key exists and holds `null`:

```
const nicknames: map<string, string?> = { ada: null };
print(nicknames["ada"]);             // null: the key exists
print(nicknames["lin"]);             // throws KeyError
```

Values can be changed in place through the key:

```
let teams: map<string, list<string>> = { red: ["Ada"] };
teams["red"].push("Lin");            // throws KeyError if "red" is missing
if ("blue" in teams) {
  teams["blue"].push("Kai");
}
```

Maps have no dot access: `scores.alice` is an error. For a fixed set of named fields, use a [struct](./08-structs.md).

## Size

`m.size` is the number of entries.

## Mutating Methods

These need a map that can be modified: a `let` variable, a `mut` parameter, or a field or element of one.

| Method | Signature | Description |
| --- | --- | --- |
| `set(key, value)` | `(key: K, value: V) => void` | Same as `m[key] = value` |
| `delete(key)` | `(key: K) => bool` | Remove `key`. Returns `true` if it existed. |
| `clear()` | `() => void` | Remove all entries |

## Non-Mutating Methods

| Method | Signature | Description |
| --- | --- | --- |
| `get(key, fallback)` | `(key: K, fallback: V) => V` | The value for `key`, or `fallback` if it's missing. The fallback is required; use `m[key]` when the key must exist. |
| `has(key)` | `(key: K) => bool` | `true` if `key` exists; same as `key in m` |
| `isEmpty()` | `() => bool` | `true` if the map has no entries |
| `keys()` | `() => list<K>` | All keys, in insertion order |
| `values()` | `() => list<V>` | All values, in insertion order |
| `entries()` | `() => list<[K, V]>` | All entries as `[key, value]` tuples, in insertion order |

## Iteration

`for...of` over a map goes through its entries as `[key, value]` tuples, in insertion order:

```
for (const [name, score] of scores) {
  print(`${name}: ${score}`);
}
```

## Examples

Counting words:

```
let counts: map<string, int> = {};
for (const word of text.split(" ")) {
  counts[word] = counts.get(word, 0) + 1;
}
```

Grouping, by hand or with `groupBy`:

```
let byTeam: map<string, list<string>> = {};
for (const person of people) {
  let members = byTeam.get(person.team, []);
  members.push(person.name);
  byTeam[person.team] = members;
}

const grouped = people.groupBy((p) => p.team);   // map<string, list<Person>>
```

Other operations:

```
let scores = { alice: 10, bob: 20, charlie: 30 };

print(scores.has("alice"));       // true
print(scores.delete("bob"));      // true
print(scores.delete("bob"));      // false: already removed
print(scores.keys());             // ["alice", "charlie"]
print(scores.values());           // [10, 30]
print(scores.size);               // 2
```
