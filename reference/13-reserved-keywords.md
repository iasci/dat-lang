# Reserved Keywords

These keywords are reserved and can't be used as names:

```
let, const, function, return, if, else, for, while, break, continue,
try, throw, catch, finally, this, true, false, null
```

A keyword can still be a member name after a dot, as in `trip.return`.

`_` is a wildcard. It can be used as a parameter name or in a pattern for a value you don't need, and it can appear more than once.

## Words With a Meaning in One Place

Every other word is an ordinary name. Some words have a special meaning only where they appear:

| Words | Special |
| --- | --- |
| `int`, `float`, `string`, `bool`, `void`, `never`, `list`, `set`, `map` | in a type. A user-defined type can't take these names: see [User-defined Types](./01-builtin-types.md#user-defined-types). |
| `struct`, `enum`, `interface`, `type`, `import`, `export`, `test` | at the start of a top-level declaration |
| `from` | in an import |
| `of` | in a `for` loop, after the loop variable |
| `implements`, `extends`, `throws` | in a declaration's signature |
| `static`, `private`, `operator` | before a struct member |
| `mut` | before a parameter or an argument |
| `match` | before `(...) {` |
| `as`, `is`, `in` | between two values, or a value and a type |

```
function direction(from: int, to: int): string {
  const type = to > from ? "forward" : "back";
  return type;
}
```

New keywords are rare, because each one breaks the programs that use the word as a name. Where a new construct needs a word, dat gives it a meaning only where it's used, as the compiler does for [`await`](./22-concurrency.md#no-async-or-await).
