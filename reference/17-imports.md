# Imports and Modules

Every `.dat` file is a module. A module contains only declarations: `function`, `struct`, `enum`, `interface`, `type`, `const` and `test`. A module has no top-level statements and no module-level `let`, so importing a module never runs code and there's no global mutable state.

All import paths are explicit, with no magic resolution. File paths include the `.dat` extension and start with `./` or `../`. The standard library and packages start with `@`, as in `"@dat/tasks"`.

## Exporting

Only declarations marked with `export` are visible to other modules. Everything else is private to its file.

```
// math/vector.dat

export struct Vector {
  x: float;
  y: float;

  magnitude(): float {
    return Math.sqrt(this.x * this.x + this.y * this.y);
  }
}

export function dot(a: Vector, b: Vector): float {
  return a.x * b.x + a.y * b.y;
}

// Not exported: only usable within this file
const PRECISION = 10;
```

`export` can be applied to `function`, `struct`, `enum`, `interface`, `type` and `const`. Exported functions, and the methods of exported structs, must declare their return types, so the compiler can check each module without reading the bodies of the modules it imports.

## Module Constants

A module-level `const` is computed once, the first time it's used. Its initializer can't throw a declared error; use `try!`, which turns a failure into an `AssertionError`. A constant whose initializer depends on itself, directly or through other constants, throws an `AssertionError` when first used.

```
const DEFAULT_PORTS: set<int> = [80, 443];
const LIMITS = { free: 10, pro: 1000 };
```

## Folder Modules and `index.dat`

A folder can act as a module. Its entry point is `index.dat` inside the folder. Code **outside** the folder can access only the symbols exported from `index.dat`.

```
math/
  index.dat       ← entry point for the "math" module
  vector.dat
  matrix.dat
  helpers.dat     ← internal, not re-exported
```

```
// math/vector.dat
export struct Vector { ... }
export function dot(a: Vector, b: Vector): float { ... }
```

```
// math/matrix.dat
export struct Matrix { ... }
```

```
// math/helpers.dat
export function clamp(value: float, low: float, high: float): float { ... }
```

```
// math/index.dat: controls what the outside world sees
export { Vector, dot } from "./vector.dat";
export { Matrix } from "./matrix.dat";
// helpers.dat is NOT re-exported, so clamp() is internal to the math folder
```

```
// app/main.dat
import { Vector, Matrix } from "../math";          // ✅ exported from index.dat
import { clamp } from "../math";                   // ❌ E1701: 'clamp' is not exported
                                                   //    fix (review, math/index.dat): export { clamp } from "./helpers.dat";
import { clamp } from "../math/helpers.dat";       // ❌ E1702: can't reach into a folder past its index.dat
                                                   //    fix (review): import { clamp } from "../math";
                                                   //    fix (review, math/index.dat): export { clamp } from "./helpers.dat";
```

### Sibling Access

Files in the same folder can import each other's exports directly, without going through `index.dat`:

```
// math/matrix.dat
import { Vector } from "./vector.dat";  // sibling import: allowed

export struct Matrix {
  // can use Vector here
}
```

## Named Imports

Import specific exported symbols from a module:

```
import { Vector, dot } from "../math";

const v = Vector { x: 3.0, y: 4.0 };
const d = dot(v, v);
```

Only imported symbols are available. Using one that wasn't imported is a compile error:

```
import { Vector } from "../math";

const d = dot(v, v);  // ❌ E1401: 'dot' is not defined
                      //    fix (import): import { Vector, dot } from "../math";
```

## Namespace Imports

Import all exports under one name:

```
import * as math from "../math";

const v = math.Vector { x: 3.0, y: 4.0 };
const d = math.dot(v, v);
```

## Aliased Imports

Rename an import to avoid a conflict or to make code clearer:

```
import { Vector as MathVector } from "../math";
import { Vector as PhysicsVector } from "../physics";
```

## Re-exporting

An `index.dat` file re-exports symbols from its sibling files with `export ... from`:

```
// math/index.dat
export { Vector, dot } from "./vector.dat";
export { Matrix } from "./matrix.dat";
```

This is the only way to make a folder's internal symbols visible outside it.

## Module Resolution

- All paths are explicit, with no implicit resolution or extension guessing.
- File imports always include the `.dat` extension, as in `"./vector.dat"`.
- `./` starts a same-folder (sibling) import.
- `../` starts a parent-relative import.
- Importing a folder, such as `"../math"`, resolves to the `index.dat` inside it. This is the only implicit resolution, similar to a web server's `index.html`.
- `@` starts a module that isn't a file. `"@dat/..."` is the standard library, as in `"@dat/tasks"`. Any other `"@owner/name"` is a package, such as `"@acme/postgres"`: every package name starts with its owner, so two owners can each publish a `postgres`, and the standard library can add a module without taking a name from anyone. No package can use the owner names `dat` and `std`, so nothing can pass itself off as the standard library. Packages aren't specified yet.
- A path that starts with none of `./`, `../` and `@` is an error. When the standard library has a module of that name, the fix adds `@dat/`:

```
import { Shared } from "tasks";   // ❌ E1703: an import path starts with `./`, `../` or `@`
                                  //    fix: import { Shared } from "@dat/tasks";
```

## Program Entry

The file passed to `dat run` must define a `main` function, which is where the program starts. See [The `main` Function](./06-functions.md#the-main-function).

## Import Rules

- Imports must appear at the top of the file, before any declarations.
- Circular imports are allowed. Two modules can import from each other, because importing never runs code.
- Code outside a folder can access only what its `index.dat` exports, never the files inside it.
- Tests in a module can use that module's non-exported declarations. See [Testing](./20-testing.md).
