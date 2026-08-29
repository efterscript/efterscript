# Design: Interpreter core

Execution semantics are the PostScript Language Reference's (PLRM3 §3.5
execution, §3.6 overview of operators, §3.10 errors, §3.11 early name
binding, §3.12 stack limits and Appendix B). This document records how
EfterScript executes and why.

## 1. Goals and constraints

- **No native recursion.** Depth of PostScript recursion, loop nesting, and
  `exec` nesting is bounded only by the execution-stack limit, never by the
  Rust stack. Unwinding (`exit`, `stop`, errors) is a stack operation.
- **Exact error semantics.** Jobs redefine `errordict` entries, inspect
  `$error`, and rely on `stopped` returning after an error inside it.
- **No ambient authority.** Output, files, and later devices are handles the
  embedder injects; the interpreter has no `println!`.
- **Deterministic.** Same program, same capabilities, same bytes out.
- **Sessions later, not never.** The state must be reusable across jobs
  (persistent VM, encapsulated job) without redesign.

## 2. State

```
Interp
├── mem:      Memory                 arenas, names, files, save stack
├── ostack:   Vec<Object>            operand stack        (limit → stackoverflow)
├── dstack:   Vec<Object>            dictionary stack     (limit → dictstackoverflow)
├── estack:   Vec<Frame>             execution stack      (limit → execstackoverflow)
├── ops:      &'static [OpEntry]     operator table
├── io:       Io                     injected stdout / stderr streams
├── caps:     Capabilities           file, and later font/device capabilities
└── quirks:   Quirks                 tolerance policy (empty in this change)
```

Stack limits are the PLRM3 Appendix B minimums by default and configurable
upward by the embedder.

## 3. The execution stack

`Frame` is an enum; the loop always acts on the top frame:

```
Frame
├── Object(Object)                 a single object to execute (from exec, names, operators)
├── Proc { array, next: u32 }      an executable array being run, element by element
├── Source { src, scanner }        a file or string being scanned and executed (run, exec)
├── Loop(LoopFrame)                for / repeat / loop / forall / pathforall continuations
├── Stopped                        marker pushed by `stopped`
└── Marker(Kind)                   run boundary, job boundary, and similar barriers
```

Decisions:

- **Control operators are frames, not Rust loops.** `for` pushes
  `Loop(For { proc, cur, inc, limit })`; each iteration pushes the operand
  and a `Proc` frame; when the `Proc` frame completes the loop frame advances.
  `exit` pops frames down to and including the nearest `Loop`. This keeps
  the Rust stack flat and makes `exit` inside nested procedures trivial.
- **`Proc` frames hold the array object and an index**, not a copy, so
  executing a procedure allocates nothing and mutation of the array during
  execution behaves as the spec describes (the interpreter reads elements as
  it goes).
- **`Source` frames own a scanner state** and pull one token at a time; a
  `NeedMore` result suspends the interpreter (for sessions) rather than
  erroring. `currentfile` finds the innermost `Source` frame that is a file.
- **Executing an operator pops nothing implicitly**: the operator function
  receives `&mut Interp` and performs its own checks through helpers that
  produce `stackunderflow`/`typecheck` with the operator's name attached.

## 4. Execution loop

```
loop:
  frame = estack.top or return Done
  match frame:
    Proc  -> obj = array[next]; next += 1 (pop frame if exhausted); execute(obj)
    Source-> scan one token (NeedMore → Suspend; End → pop; Token → execute(obj))
    Loop  -> advance loop state (push next iteration or pop)
    Object-> pop; execute(obj)
    Stopped / Marker -> pop (normal completion)

execute(obj):
  literal, or executable array/string encountered directly in the operand flow
    → push on ostack   (arrays/strings pushed literally; PLRM3 §3.5.5)
  executable name → lookup through dstack; undefined → error; else execute(value)
  executable array (reached via a name or exec) → push Proc frame
  operator → call; map Err(VmError) to the error machinery
  executable file/string via exec → push Source frame
```

Name lookup walks `dstack` top-down; each dict lookup is the ordered-map
hash lookup from the object-model change. `bind` replaces executable names
that resolve to operators with the operator objects, recursively into
nested procedures, and never binds names that resolve to anything else.

## 5. Operator table

A static array of `OpEntry { name, func, sig }` built at compile time by a
small macro. `sig` is an optional stack signature (types of the top n
operands) used by a shared prologue to produce `stackunderflow` and
`typecheck` uniformly; operators with polymorphic operands check by hand.
`systemdict` is populated from the table at startup, plus the standard
dictionaries (`errordict`, `$error`, `userdict`, `globaldict`,
`statusdict` stub) and constants (`true`, `false`, `null`, `languagelevel`
= 2 by default). Operator functions are plain `fn(&mut Interp) ->
Result<(), VmError>`. Operators that need to run a procedure and continue
(`forall`, `stopped`, `loop`) push frames and return; they do not call the
loop recursively.

## 6. Errors

An operator's `Err(e)` enters the error machinery with the operator object
as the offending command:

1. Look up `e.name()` in `errordict`; execute the value (the program may have
   replaced it). The default entries behave as the spec describes: record
   `newerror`, `errorname`, `command`, and snapshots of the stacks in
   `$error`, then execute `stop`.
2. `stop` unwinds `estack` to the nearest `Stopped` frame, pops it, and
   pushes `true` on the operand stack; `stopped` pushed that frame and
   arranged for `false` on normal completion.
3. With no `Stopped` frame, `stop` unwinds to the run boundary; the embedder
   sees the job end with the `$error` contents, which is what a printer would
   print via `handleerror`. `handleerror` itself is the default reporter,
   writing the conventional `%%[ Error: name; OffendingCommand: cmd ]%%`
   line to the injected error stream so session mode gets the back-channel
   format for free.

Decisions:

- **Errors never use Rust panics or unwinding.** `VmError` values flow up
  through `Result` to the loop, which drives the machinery.
- **`stackoverflow` and friends are checked by the push helpers**, so every
  operator gets them without thinking about it.
- **Interrupt and timeout** are frames the embedder can inject (session
  mode's `^C` and job time limits) — reserved as `Marker` kinds, unused now.

## 7. Output and files

`print`, `=`, `==`, `pstack`, `flush`, and `handleerror` write to
`Io::stdout`; the embedder supplies both streams (a terminal, a capture
buffer for tests, a session back channel). `file` opens through the
capability layer from the object-model change; the standard streams
`%stdin`, `%stdout`, `%stderr` map to the injected streams. `eexec` is
registered but returns `undefined` until the font change implements it; its
position semantics are already guaranteed by the scanner.

## 8. Jobs

`Interp::run(source) -> Outcome` pushes a run-boundary marker and a `Source`
frame, executes to completion, and returns `Outcome::{Ok, Error($error
summary), Suspended}`. Job encapsulation (`save` on entry, `restore` on
exit, `exitserver` escape) is a thin wrapper the session change adds; the
persistent-VM shape is already what `Memory` provides.

## 9. Verification

- Every corpus file becomes executable: `difftest run` executes each
  `corpus/unit/**/*.ps`, capturing stdout and the final outcome, and compares
  against expectations declared in the file's header comments
  (`% expect-output:` lines, `% expect-error: name`). The object-model and
  scanner corpus files gain expectations in this change.
- Property tests: random nesting of `for`/`loop`/`exit`/`stopped`/`stop`
  never touches the Rust stack depth (checked with a recursion counter) and
  always leaves the stacks in the state the spec predicts.
- Deep-recursion test: a procedure that calls itself 100 000 times through
  `exec` fails with `execstackoverflow`, not a crash.

## 10. Alternatives considered

- **Recursive `execute` calling itself for procedures** — the obvious shape;
  rejected because `exit`/`stop`/error unwinding then needs Rust unwinding
  or a result-threading discipline, and deep PostScript recursion crashes.
- **Continuation-passing operators** — unnecessary generality; frames cover
  every control operator in the language.
- **Bytecode compilation of procedures** — deferred; the frame design does
  not prevent a later tier from replacing `Proc` frames with compiled ones.
