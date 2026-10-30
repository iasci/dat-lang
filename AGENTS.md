# dat-lang

- Purpose: a simple, statically typed language whose syntax is inspired by TypeScript, with an agent-first experience: an agent should finish a task in the fewest turns.
- Fewest turns: familiar syntax that agents already know, and compiler errors that state the exact fix.
- Super fast tooling: type checks and tests should finish in milliseconds, for agents and for the people supervising them.
- Production speed: programs compile to Rust and should run close to hand-written Rust.
- Speed trade-offs: aim for within about 4% of hand-written Rust on everyday code (loops, strings, collections, recursion that does work per call), not on worst cases like `fib`, which only calls itself. Don't aim to beat Rust, and don't give up simplicity, exact traces or catchable errors for speed that only a worst case would see.
- Two run modes: a fast interpreter for development and Rust code generation for production, with the same semantics; every test runs in both.
- Rust compatibility: every language feature must map directly onto Rust. No GC, no `Rc<RefCell>` everywhere, no inheritance, no runtime reflection.
- Spec: `reference/` is the language reference
- Error tolerance: every runtime error can be caught; never turn a failure into an uncatchable crash.
- Don't remove or replace a feature the user designed without asking first.
- Reserved keywords: keep the list as short as possible, and add one only with the owner's approval. Every new keyword breaks programs that use the word as a name. Prefer a built-in, a standard library module, or a word recognized only where it's used, such as `await` before a call, so the word stays free as a name.
- Error codes: add a new one without asking whenever an error that no existing code describes needs it, and list it in `crates/dat-check/CODES.md`.
- Fixes: every fix an error offers follows `FIXES.md`.
- Outside libraries: use one only if it's secure, adds no significant performance cost and fits our needs as it is; if it would need workarounds, fork it or write our own.
- TypeScript: only the syntax is inspired by it. dat aims to be consistent and free of quirks on its own terms. Decide semantics, errors and fixes by dat's own goals and rules, never by what TypeScript or JavaScript does, and don't compare dat with them when explaining a choice or asking about one. Code written the TypeScript way isn't a category of its own: it gets the same errors and fixes as any other mistake.
