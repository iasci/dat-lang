# Command Line

The `dat` command checks, runs, tests, builds and fixes programs. Code runs on the interpreter unless `--compiled` asks for Rust.

## Commands

| Command | What it does |
| --- | --- |
| `dat check [path]` | Type checks without running anything, and reports every error in one run |
| `dat run <file>` | Runs a program on the interpreter, starting at its [`main` function](./06-functions.md#the-main-function) |
| `dat test [path]` | Runs tests on the interpreter. See [Testing](./20-testing.md). |
| `dat build <file>` | Compiles a program and its imports through Rust into a native executable |
| `dat fix [path]` | Applies every safe fix, then reports the errors that are left |

`path` is a file or a folder, and defaults to the current folder.

```
dat check                   # every file in the current folder
dat run src/main.dat        # on the interpreter
dat run --compiled src/main.dat
dat test --filter clamp
dat fix src/users.dat
```

## Options

| Option | Commands | Effect |
| --- | --- | --- |
| `--compiled` | `run`, `test` | Compile to Rust and run that instead of the interpreter. The results are the same; only speed differs. |
| `--json` | all | Print diagnostics as JSON instead of text. See [Diagnostics](#diagnostics). |
| `--filter <text>` | `test` | Run only the tests whose name contains `text` |
| `--verbose` | `test` | Also list each test that passes |

## Output and Exit Status

stdout carries results: what the program prints, and the `dat test` report. stderr carries problems: compile errors, and an error that escapes `main`. So `dat run app.dat > out.txt` keeps the program's output clean and still shows its errors.

| Status | Meaning |
| --- | --- |
| `0` | Success |
| `1` | The command ran and found a problem: `check`, `build` or `fix` found errors, a test failed, or an error escaped `main` |
| `2` | The command couldn't do its job: `run` or `test` hit a compile error, an option or file doesn't exist, or a `--filter` matches no test |

## Diagnostics

Every compile error has a code, a location, a message and usually a fix. In a terminal:

```
error[E0701]: condition must be bool
  --> src/users.dat:14:7
14 |   if (user) {
   |       ^^^^ `user` is `User?`
fix: compare with null
14 |   if (user != null) {
1 error, 1 fixable with dat fix
```

- Errors are sorted by file and position, capped at 50, and end with a summary line.
- Each root cause is reported once.
- A fix is marked the same way as in the spec's ❌ lines: `fix:` for a safe fix, `fix (review):` for one that changes what the code means, and `option:` when the right fix depends on intent. See [Reading the Examples](./README.md#reading-the-examples).

### General Codes

A code's first two digits name the section that defines its rule. Two codes belong to no section:

- **`E0001`** is a syntax error. Where the fix is certain, such as a missing `;` or TypeScript's `===`, the error comes with it:

  ```
  const answer = 42;
  if (answer === 42) { }   // ❌ E0001: dat has no `===`; `==` compares contents
                           //    fix: if (answer == 42) { }
  ```

- **`E0002`** marks valid dat that this compiler doesn't support yet. It never means the program is wrong, and it disappears once the compiler implements the feature.

With `--json`, diagnostics go to stderr as one JSON document:

```
{
  "schemaVersion": 1,
  "diagnostics": [{
    "code": "E0701",
    "severity": "error",
    "message": "condition must be bool",
    "file": "src/users.dat",
    "span": { "byteStart": 312, "byteEnd": 316,
              "line": 14, "col": 7, "endLine": 14, "endCol": 11 },
    "labels": [{ "message": "`user` is `User?`" }],
    "fixes": [{
      "title": "compare with null",
      "applicability": "safe",
      "isPreferred": true,
      "edits": [{ "file": "src/users.dat", "byteStart": 312, "byteEnd": 316,
                  "before": "user", "after": "user != null" }]
    }],
    "rendered": "error[E0701]: condition must be bool\n…"
  }],
  "summary": { "errors": 1, "warnings": 0, "fixable": 1 }
}
```

- `applicability` is `safe`, `review` or `option`. `dat fix` applies only `safe` fixes. When there are several options, `isPreferred` marks the one to show first.
- Each edit names its file and carries the exact `before` text, so a tool can detect stale offsets, and an agent can apply the edit as a plain string replacement.
- `byteStart` and `byteEnd` are byte offsets into the file. `line` and `col` count from 1, and columns count Unicode scalar values. `endLine` and `endCol` point just past the end of the span.
- `rendered` is the terminal form of the same diagnostic.
