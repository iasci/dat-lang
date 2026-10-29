# DatLang Language Reference (WIP)

dat is a statically typed language with TypeScript-style syntax. During development, programs run on a fast interpreter. For production, they compile to Rust. Both run the same semantics, and the same tests run on both.

## Design principles

Every rule in this reference follows from one of these. When a rule looks odd, one of these is the reason.

1. **Values, not references.** Every value, whether a number, a string, a list or a struct, behaves the way a number does: assigning or passing it hands over an independent copy. Copies are cheap because they share memory until one of them changes (copy-on-write). A function can change the caller's value only when the caller passes it with `mut`. There is no garbage collector, no aliasing and no shared mutable state.
2. **Syntax inspired by TypeScript.** Much of the syntax and many method names come from TypeScript, because coding agents and developers already know them. What the code means follows dat's own goals: a simple language, fast programs and clear errors.
3. **One way to write each thing.** No `==` next to `===`, no classes next to structs, no `function` expressions next to arrow functions. Fewer choices give more uniform code and fewer mistakes.
4. **Failure is visible and recoverable.** A function that can fail in an expected way says so in its signature (`throws`), and every call to it is marked with `try`. Operations that can fail anywhere, such as an out-of-range index, a missing map key or a failed assertion, throw built-in runtime errors that need no marking. Both kinds can be caught, so one bad input doesn't have to stop the program.
5. **Every feature has a direct Rust equivalent.** No inheritance, no `any`, no runtime reflection. Structs, enums, unions, interfaces and generics compile to Rust structs, enums, traits and generics, so compiled programs run at close to hand-written Rust speed.

## Reading the Examples

Code examples are fragments of a function body, unless they declare top-level items such as functions, structs or tests. `// throws RangeError` marks an error thrown at runtime, and `// ❌` marks a compile error, with the fix the compiler suggests below it:

```
"Total: " + 42     // ❌ E0301: can't add string and int
                   //    fix: `Total: ${42}`
```

- **The code** names the rule that was broken and never changes. Its first two digits are the section that defines the rule, so `E0301` is in [Operators](./03-operators.md). Codes that start with `E00` are [general](./21-command-line.md#general-codes). The same mistake has the same code wherever it appears.
- **`fix:`** shows the code as it reads after the fix. `dat fix` applies it.
- **`fix (review):`** is a fix that changes what the code means, such as turning a `const` into a `let`. The compiler shows it, but `dat fix` leaves it to you.
- **`option:`** lines appear when the right fix depends on what the author meant. `dat fix` applies none of them. When two fixes mean the same thing, the compiler gives only one.
- **A place in parentheses**, such as `(declaration)` or `(math/index.dat)`, says where the edit goes when it isn't the marked line. A fix that needs several edits has one `fix` line per edit, and they're applied together.
- **`_`** in a fix stands for a value or type only the author knows. A fix that contains `_` is always `review`.

The code, the line the error is reported on and the fix are part of the language definition. The message wording isn't: the compiler may phrase it more clearly.

## Sections

1. [Built-in Types](./01-builtin-types.md)
2. [Variable Declaration](./02-variable-declaration.md)
3. [Operators](./03-operators.md)
4. [Primitive Methods](./04-primitive-methods.md)
5. [Type Narrowing and Casts](./05-type-narrowing.md)
6. [Functions](./06-functions.md)
7. [Control Flow](./07-control-flow.md)
8. [Structs](./08-structs.md)
9. [Built-in Functions](./09-builtin-functions.md)
10. [List Methods](./10-list-methods.md)
11. [Set Methods](./11-set-methods.md)
12. [Errors](./12-errors.md)
13. [Reserved Keywords](./13-reserved-keywords.md)
14. [Scope](./14-scope.md)
15. [Interfaces](./15-interfaces.md)
16. [Map Methods](./16-map-methods.md)
17. [Imports and Modules](./17-imports.md)
18. [Enums and Unions](./18-enums-and-unions.md)
19. [Generics](./19-generics.md)
20. [Testing](./20-testing.md)
21. [Command Line](./21-command-line.md)

## Not yet specified

- Concurrency and async
- Shared references, for graphs, caches and state shared between tasks
- Standard library beyond the built-ins: I/O, JSON, time, randomness, HTTP
- Packages, dependencies and calling Rust crates
- Bitwise operators
