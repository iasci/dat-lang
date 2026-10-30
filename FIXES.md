# Fixes

A wrong fix is worse than none: `dat fix` applies safe fixes without anyone looking.

## Levels

- `fix:` (safe): the one obvious fix, very likely what the author meant; `dat fix` applies it.
- `fix (review):` changes what the code means; shown, applied only by hand.
- `option:` one of several fixes, where the right one depends on intent; never applied.
- No fix: when no fix is clearly right, give none rather than a guess.
- Unsure between two levels: pick the more careful one.

## Safe only when

- There's exactly one fix the author could have meant.
- It keeps all of the author's code: no argument, value, statement or comment is dropped.
- It's right in every edge case, not only the usual one.
- It has no `_` placeholder.
- It maps an exact match, never a similar name: `@std/math` → `@dat/math` is safe, `@dat/maths` → `@dat/math` is review.

## Review when

- It changes what the code means: `const` → `let`, `==` → `=`.
- It deletes code: a statement that does nothing is likely a typo, and deleting it hides the real mistake.
- It guesses a similar name: `flor` → `floor`.
- It contains `_`.

## Every fix, at every level

- Compiles: after it, the code parses, the error is gone and no new error appears.
- Settles: it never brings its own error back, so `dat fix` reaches an end.
- Does something: no no-op such as `total = total`.
- Fires only on the shape it was made for: a rule that also fires on a misspelled name is wrong.
- Doesn't clash: fixes that would edit the same place become one fix.
- Touches only the text it's about, and carries that text exactly as `before`.
- Has a title that says what it does, such as ``use `floor` ``, never a question.
- After the error, the checker goes on as if the fix were applied, so one mistake gives one error.

## Tests

- Every new fix gets an example; a safe one also gets a `.fixed` file that checks clean and runs the same in both modes.
- `dat-conformance examples` checks that every example's safe fixes leave code that parses.
- Try every new fix on near misses: a misspelled name, a `const`, an extra argument, the same name twice, and the fix applied twice.
