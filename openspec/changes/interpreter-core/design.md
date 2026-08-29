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

## 11. Implementation notes

Recorded where the code departs from, or pins down, the text above. Part 1
covers state, loop, operator table, standard dictionaries, and the stack,
arithmetic, dictionary, control, and error operator groups.

- **The job's source is borrowed, not owned.** `Interp::run(&mut dyn
  Source)` keeps the run boundary and a `Source` frame with a `Run` slot on
  the execution stack, and the loop reads the source it was handed; on
  `NeedMore` the frames and scanner state stay in place and
  `Interp::resume(&mut dyn Source)` continues. `SliceSource` borrows its
  bytes, so an owning frame would have forced every embedder to hand over a
  `'static` source. Frames for strings and files (`exec` on either) own
  their `StringSource`/`FileSource` as §3 describes.
- **Only frames holding a program object count toward the execution-stack
  limit** (`Object`, `Proc`, `Source`); `Loop`, `Stopped`, and `Marker`
  frames are the interpreter's bookkeeping and each accompanies a counted
  frame, so the stack stays bounded by roughly twice the limit. Counting
  every frame would make the spec's 200-level loop nest (two frames per
  level) exceed the Appendix B minimum of 250 that the same spec relies on.
- **Recursion through a name is not tail-eliminated**: a `Proc` frame is
  popped only when the loop revisits it exhausted, so `/f { f } def f`
  reaches `execstackoverflow` as the spec requires instead of looping
  forever on a flat stack.
- **The offending object is pushed before the `errordict` entry runs**, as
  PLRM3 §3.10 describes, which is what lets the spec's handler scenario
  begin with `pop`. The default entries are internal operators (one per
  error name, kept out of `systemdict`) that pop it, record `$error`, and
  `stop`; the machinery's own pushes bypass the stack limits so a full
  stack can still report its overflow. A `Marker::ErrorHandler` frame sits
  under a running handler; when `MAX_NESTED_ERROR_HANDLERS` (16) of them
  are live the next error records `$error` and unwinds to the run boundary
  instead of nesting, so a handler that re-raises its own error cannot
  loop.
- **`stop` unwinds to the nearest `Stopped` frame or run boundary**,
  whichever is closer; a nested `run` therefore acts as a job boundary and
  never lets a `stop` escape into the embedder's outer frames. `exit`
  treats `Stopped` and every marker as a barrier (`invalidexit`).
- **Reporting happens only for a job ended by `stop`.** At the boundary
  the loop checks `$error /newerror` only if `stop` unwound to it; a job
  that caught its errors and finished normally is `Outcome::Ok`, and a bare
  `stop` after a caught error reports that earlier error, which is the
  job-server behaviour of PLRM3 §3.10. The `errordict /handleerror` entry
  is run (so programs may replace it) and `newerror` is cleared afterwards.
- **`handleerror` writes to the error stream**, as §6 and the spec say; the
  list in §7 that puts it on `Io::stdout` is superseded. `Io` holds the two
  streams the embedder injects, `Interp` registers them in the file table
  at construction so part 2 can expose them as `%stdout`/`%stderr` file
  objects, and `print`/`=` write through `Interp::write_stdout`.
- **`Capabilities` is a construction-time bundle.** Its `file` capability
  is installed into `Memory` (which already owned that slot); `Interp`
  keeps no separate `caps` field until a capability exists that `Memory`
  does not hold.
- **Local standard dictionaries are inserted into `systemdict` raw.**
  `userdict`, `errordict`, and `$error` live in local VM (a program in
  local allocation mode must be able to store procedures into `errordict`)
  while `systemdict` is global; the global/local rule would reject the
  entries, but they predate every `save`, so nothing can dangle and the
  insert bypasses the check. `systemdict` is then made read-only.
- **The operator table is chained at first use** from each module's static
  slice (`ops::table()`, an `OnceLock`); indices are stable for the
  process, and part 2 appends its modules to `MODULES` without touching
  earlier entries. The `op_table!` macro accepts non-capturing closures, so
  the error handlers need no named function each.
- **Polymorphic `get`, `put`, `length`, and `copy` are complete now**
  (dictionary, array, string, and integer-count forms), since the memory
  API already provides them; `<<`/`>>` are registered with the dictionary
  group. `[`, `]`, `array`, and the rest of the array/string group remain
  for part 2.
- **`bind` descends only into executable arrays**, replaces names in
  writable ones, and leaves read-only or packed procedures untouched
  without error; it uses a work list and a visited set, so nesting depth
  and self-reference are safe. It does not change access attributes.
- **Numeric details pinned down**: integer `add`/`sub`/`mul` overflow
  yields the `i64` result rounded to `f32`; `neg`/`abs` of the minimum
  integer yield a real; `idiv` of the minimum integer by `-1` and any
  division by zero are `undefinedresult`; non-finite real results are
  `undefinedresult`; `sqrt`, `ln`, `log` outside their domain are
  `rangecheck`; `bitshift` is logical and a shift of 32 or more clears the
  value; `round` sends halves to the greater integer. `rand`/`srand`/
  `rrand` are not registered yet.
- **`=` formats integral reals with one decimal** (`5.0`) and other reals
  with the shortest round-trip form; the full `cvs` rules come with the
  type-conversion group.
- **`quit` clears the whole execution stack** and sets a flag readable via
  `Interp::has_quit`; the run returns `Outcome::Ok`, and the CLI decides
  the exit code from the flag.
- **`countexecstack`/`execstack` report the counted frames' objects**, with
  a procedure shown as its unexecuted remainder; the job's own source has
  no object and is omitted.

### Part 2

Covers the array, type, VM, and file operator groups, the CLI, and the
executable corpus.

- **The job's source is read through a file-table entry.** `currentfile`
  at the top level must return a file object whose reads share the
  scanner's cursor, and the source handed to `run` is borrowed, so the loop
  moves the bytes the source has available (`Source::drain_into`, one copy
  for a slice) into a permanent entry opened at construction — the *run
  file* — and scans from that entry; the borrowed source's
  `more_may_come` is carried across so `NeedMore`/`resume` behave as in
  part 1. `currentfile` returns the innermost `File` frame's object or, when
  the innermost file frame is the `Run` slot, the run file. The entry is
  older than every `save`, so `restore` never closes it; `closefile` on it
  discards the unread remainder (the job ends when the scanner reaches the
  end), and `run` discards whatever a `quit` left unread. Operator-level
  reads (`readline`, `readstring`, …) cannot suspend: a chunked job source
  that ends inside data an operator is reading sees end of file, which is
  the session change's problem to solve if it needs to.
- **`forall` is a `Loop` frame** (`LoopFrame::ForAll { body, container,
  next }`) stepping by index, dictionaries through `Memory::dict_entry_at`
  in insertion order, so `exit` and nesting work as for the other loops
  and a container that becomes unreadable mid-loop raises with `forall`
  as the command.
- **`restore` scans the operand and dictionary stacks and
  `Interp::exec_references`**: `Object` frames, procedures with elements
  left, source objects, loop bodies, and `forall` containers. An exhausted
  procedure is excluded — it is typically the one that called `restore` —
  and the loop pops an exhausted `Proc` frame without touching storage, so
  a body discarded by `restore` does not fault when revisited. `save`
  records a graphics-state depth of 0 and `restore` ignores the depth it
  returns; the graphics change will wire both.
- **`setpacking` is a `Memory` flag the scanner honours**
  (`Memory::alloc_procedure`), so it affects scanned procedures only; `]`
  and `array` always build ordinary arrays, per PLRM3 §3.3.2, and
  `packedarray` always builds packed ones. Since part 1's `bind` leaves
  packed procedures untouched, a program that packs and binds gets
  late-bound names; if a compatibility case needs binding into packed
  procedures, `bind` is where to extend.
- **`type` returns a literal name**, so `type ==` prints `/integertype`,
  as the corpus expects.
- **Access only tightens.** `readonly`/`executeonly`/`noaccess` on an
  array, packed array, string, or file return a copy with the new
  attribute (`invalidaccess` if it would loosen the current one); on a
  dictionary they change the shared storage and return the same object.
  `executeonly` rejects dictionaries with `typecheck`. `file` gives
  read-mode files the read-only attribute, so `write` on them is
  `invalidaccess`.
- **Conversions**: `cvi`/`cvr` on a string scan one number token with the
  scanner's own `parse_number` (whitespace trimmed; a non-number token is
  `typecheck`, malformed input its scan error); `cvi` of a real outside the
  integer range is `rangecheck`. `cvs` writes the `=` form. `cvrs` with
  radix 10 is `cvs`; any other radix (2–36, else `rangecheck`) truncates a
  real and prints the 32-bit two's-complement pattern in upper-case
  digits. `cvn` interns through the name table, so a string over 127
  bytes is `limitcheck`.
- **`vmstatus` reports the save depth, the number of slots in both arenas
  as "used", and a fixed 2^30 maximum**; memory is not metered yet.
- **Standard files**: `%stdin` is the optional injected `Io::stdin` (absent
  → `undefinedfilename`), `%stdout`/`%stderr` the part-1 streams; the mode
  must agree with the direction (`invalidfileaccess`). Every other name goes
  to the file capability. `Stream` gained a defaulted `flush`, which
  `flush` and `flushfile` call; `flushfile` on an input file discards the
  rest of it.
- **`==` prints the syntactic form** through `ops::output::full`, which
  recurses into arrays no deeper than 64 levels and prints `...` beyond, so
  a self-containing array terminates; objects without read access print as
  `--nostringval--`. `pstack` and `stack` print top-down.
- **`eexec` raises `undefined`** so `errordict` sees the operator as the
  offending command; the font change replaces the body.
- **`difftest run`** treats each `% expect-output:` line as one output line
  (joined with newlines, trailing newline included); a job's output must
  therefore end in a newline to be expressible, which every corpus file
  does by ending with `=` or `==`. `% expect-error:` compares only the
  error name of the outcome.
- **The CLI injects host stdin as `%stdin`** alongside stdout and stderr;
  exit status is 0 for `Ok` (including after `quit`), 1 for `Error`, 2 for
  usage and unreadable input.
