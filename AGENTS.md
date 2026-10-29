# dat-lang

- Purpose: a simple, statically typed language whose syntax is inspired by TypeScript, with an agent-first experience: an agent should finish a task in the fewest turns.
- Fewest turns: familiar syntax that agents already know, and compiler errors that state the exact fix.
- Super fast tooling: type checks and tests should finish in milliseconds, for agents and for the people supervising them.
- Production speed: programs compile to Rust and should run close to hand-written Rust.
- Two run modes: a fast interpreter for development and Rust code generation for production, with the same semantics; every test runs in both.
- Rust compatibility: every language feature must map directly onto Rust. No GC, no `Rc<RefCell>` everywhere, no inheritance, no runtime reflection.
- Spec: `reference/` is the language reference
- Error tolerance: every runtime error can be caught; never turn a failure into an uncatchable crash.
- Don't remove or replace a feature the user designed without asking first.
- Outside libraries: use one only if it's secure, adds no significant performance cost and fits our needs as it is; if it would need workarounds, fork it or write our own.
- TypeScript: only the syntax is inspired by it. Decide semantics, errors and fixes by dat's own goals, never by what TypeScript does.
