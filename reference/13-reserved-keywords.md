# Reserved Keywords

These keywords are reserved and can't be used as identifiers:

```
int, float, string, bool, null, void, never, list, set, map, true, false,
let, const, mut, function, return, if, else, for, of, while, break, continue, match,
struct, enum, interface, type, implements, extends, static, private, this,
throw, throws, try, catch, finally, operator,
as, is, in,
import, export, from, test
```

Any keyword can be used as a member name after a dot, as in `m.delete(key)`.

`_` is a wildcard. It can be used as a parameter name or in a pattern for a value you don't need, and it can appear more than once.
