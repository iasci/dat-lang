# DatLang Language Reference (WIP)

dat is a statically typed language with TypeScript-style syntax. During development, programs run on a fast interpreter. For production, they compile to Rust. Both run the same semantics, and the same tests run on both.

## Design principles

Every rule in this reference follows from one of these. When a rule looks odd, one of these is the reason.

1. **Values, not references.** Every value, whether a number, a string, a list or a struct, behaves the way a number does: assigning or passing it hands over an independent copy. Copies are cheap because they share memory until one of them changes (copy-on-write). A function can change the caller's value only when the caller passes it with `mut`. There is no garbage collector, no aliasing and no shared mutable state.
2. **Inspired by TypeScript.** Much of the syntax and many method names come from TypeScript, because coding agents and developers already know it. dat departs from TypeScript wherever that gives a simpler language, faster programs or clearer errors.
3. **One way to write each thing.** No `==` next to `===`, no classes next to structs, no `function` expressions next to arrow functions. Fewer choices give more uniform code and fewer mistakes.
4. **Failure is visible and recoverable.** A function that can fail in an expected way says so in its signature (`throws`), and every call to it is marked with `try`. Operations that can fail anywhere, such as an out-of-range index, a missing map key or a failed assertion, throw built-in runtime errors that need no marking. Both kinds can be caught, so one bad input doesn't have to stop the program.
5. **Every feature has a direct Rust equivalent.** No inheritance, no `any`, no runtime reflection. Structs, enums, unions, interfaces and generics compile to Rust structs, enums, traits and generics, so compiled programs run at close to hand-written Rust speed.

## Reading the Examples

Code examples are fragments of a function body, unless they declare top-level items such as functions, structs or tests. `// ❌ Error:` marks a compile error, and `// throws RangeError` marks an error thrown at runtime.

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

## Not yet specified

- Concurrency and async
- Shared references, for graphs, caches and state shared between tasks
- Standard library beyond the built-ins: I/O, JSON, time, randomness, HTTP
- Packages, dependencies and calling Rust crates
- Bitwise operators
