# Change: Interpreter core

## Why

With objects, memory, and the scanner in place, the next component is the
execution loop: the three stacks, name lookup, procedure execution, the
control operators, and the error machinery. Two properties are hard to
retrofit and must be built in from the start: the execution stack must be
an explicit data structure rather than Rust recursion (PostScript programs
recurse and loop far deeper than a native stack allows, and `stop`, `exit`,
and error handlers unwind it in ways that must be exact), and errors must be
real PostScript errors — `errordict` handlers, `$error`, `stopped`,
`handleerror` — because real jobs redefine them. This change makes the
project able to run programs end to end, which turns the corpus into an
executable test suite.

## What changes

- Defines the interpreter's state (operand, dictionary, execution stacks,
  `Memory`, injected capabilities) and the frame types on the execution
  stack.
- Defines the execution loop, name lookup, the operator table with typed
  signatures, and the control-flow operators as native frames.
- Defines the error machinery and `stopped`/`stop`/`exit` unwinding.
- Defines output (`print`, `=`, `==`) as an injected stream, not stdout.
- Brings in the operator set needed to run the corpus: stack, arithmetic,
  relational, boolean, dictionary, array, string, type-conversion, control,
  VM (`save`/`restore`/`setglobal`), and file/scanner operators (`exec`,
  `run`, `token`, `readline`, `readstring`, `currentfile`, `eexec` reserved).
  No graphics operators; those dispatch to a trait defined by the next
  change.

## Impact

- New capability spec: `interpreter-core`.
- Code: `crates/ps-vm` (`interp`, `ops/*`, `errors` modules), `efterscript-cli`
  gains a `run` mode that executes a file and reports errors, `difftest`
  gains its first mode: run every `corpus/unit/**/*.ps` and compare output
  and final error against the expectations recorded in the file.
- Depends on `vm-object-model` and `scanner`.
