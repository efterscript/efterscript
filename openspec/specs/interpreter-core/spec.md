# interpreter-core Specification

## Purpose
Defines how EfterScript executes PostScript: an explicit execution stack
with control operators as frames (never native recursion), name lookup and
binding, stack limits, the error machinery (`errordict`, `$error`,
`stopped`, `handleerror` reporting on the injected error stream), output
only through embedder-supplied streams, and the requirement that every
corpus file is an executable test via `difftest run`. Execution semantics
are the PostScript Language Reference's; this spec records the behaviours
the corpus pins down. The design rationale lives in the archived change
`2026-08-29-interpreter-core`.

## Requirements

### Requirement: Execution without native recursion

The interpreter SHALL execute procedures, loops, and `exec` through an
explicit execution stack bounded by a configurable limit; exhausting it
SHALL raise `execstackoverflow` and SHALL never overflow the host stack.

#### Scenario: Deep recursion is an error, not a crash

- **GIVEN** `/f { f } def` and an execution-stack limit of 250
- **WHEN** `f` is executed
- **THEN** the job ends with `execstackoverflow`

#### Scenario: Deep loop nesting completes

- **GIVEN** a 200-level nest of `{ … } loop` bodies each running `exit`
- **THEN** execution completes normally

### Requirement: Control operators

`for`, `repeat`, `loop`, `forall`, `if`, `ifelse`, `exit`, `stop`,
`stopped`, and `exec` SHALL behave as the spec describes, with `exit`
leaving the innermost enclosing loop regardless of procedure nesting.

#### Scenario: exit from a nested procedure

- **GIVEN** `0 1 1 10 { dup 5 eq { exit } if add } for`
- **THEN** the operand stack holds 10 and 5 (the sum so far and the loop
  variable at exit)

#### Scenario: for with real control

- **GIVEN** `0 0.5 2 { } for`
- **THEN** the operand stack holds the reals 0.0, 0.5, 1.0, 1.5, 2.0

### Requirement: Error machinery

An error SHALL execute the corresponding `errordict` entry; the default
entries SHALL record `newerror`, `errorname`, and `command` in `$error` and
execute `stop`; `stopped` SHALL catch it and push `true`.

#### Scenario: stopped catches an error

- **GIVEN** `{ 1 0 div } stopped`
- **THEN** the operand stack holds `true`, `$error /errorname get` is
  `/undefinedresult`, and `$error /command get` is the `div` operator

#### Scenario: Program-defined handler runs

- **GIVEN** `errordict /undefined { pop (caught) print } put`
- **WHEN** `nosuchname` is executed outside any `stopped`
- **THEN** the output is `caught` and execution continues

#### Scenario: Uncaught error ends the job with a report

- **GIVEN** `1 0 div` executed with no `stopped` and no handler override
- **THEN** the job outcome is an error named `undefinedresult` and the error
  stream receives a line containing `Error: undefinedresult` and
  `OffendingCommand: div`

### Requirement: Name lookup and binding

Executable names SHALL resolve through the dictionary stack top-down;
`bind` SHALL replace names that resolve to operators with operator objects
recursively and leave other names untouched.

#### Scenario: bind replaces operators only

- **GIVEN** `/x 1 def /p { x add } bind def`
- **THEN** element 0 of `p` is the name `x` and element 1 is the `add`
  operator object

### Requirement: Stack limits

Pushing beyond the operand, dictionary, or execution stack limit SHALL raise
`stackoverflow`, `dictstackoverflow`, or `execstackoverflow` respectively;
popping an empty operand stack SHALL raise `stackunderflow`.

#### Scenario: operand stack overflow

- **GIVEN** an operand-stack limit of 500
- **WHEN** `{ 1 } loop` is executed
- **THEN** the job ends with `stackoverflow`

### Requirement: Output through injected streams

`print`, `=`, `==`, and `pstack` SHALL write to the embedder-supplied
output stream; a VM constructed without one SHALL discard output rather than
touch host stdout.

#### Scenario: Captured output

- **GIVEN** a VM with a capture buffer as stdout
- **WHEN** `2 3 add = (x) print` is executed
- **THEN** the buffer holds `5\nx` and nothing is written to host stdout

### Requirement: Executable corpus

`difftest run` SHALL execute every `corpus/unit/**/*.ps` file and compare
captured output and final outcome with `% expect-output:` and
`% expect-error:` declarations in the file; every existing corpus file SHALL
carry such declarations and pass.

#### Scenario: object-model corpus runs

- **GIVEN** `corpus/unit/vm/restore-reverts-local.ps`
- **WHEN** `difftest run` executes it
- **THEN** its declared expectations pass

### Requirement: type yields an executable name

`type` SHALL return the type name as an executable name object.

#### Scenario: Executable attribute

- **GIVEN** `1 type xcheck =` and `1 type ==`
- **THEN** the output is `true` then `integertype`

### Requirement: == writes null in its syntactic form

`==` SHALL write a null object as `null`; `=` SHALL be unchanged.

#### Scenario: null through both writers

- **GIVEN** `null == null =`
- **THEN** the output is `null` then `--nostringval--`

### Requirement: Execution budget

The interpreter's limits SHALL include an optional budget of executed
objects; when set and exceeded, execution SHALL stop with `limitcheck`
attributed to the object being executed, and the job SHALL report that
outcome. Unset, execution is unbounded as before.

#### Scenario: Budget exceeded

- **GIVEN** an interpreter with a budget of 10000 and the program
  `{ } loop`
- **THEN** the outcome is the `limitcheck` error and the interpreter
  returns

### Requirement: Trigonometric argument reduction

`sin` and `cos` SHALL reduce their argument modulo 360 degrees in
double precision before conversion to radians, so results for large
angles keep single precision's digits.

#### Scenario: A large angle

- **GIVEN** `1000000 cos =`
- **THEN** the output is `0.17364818` within one unit in the last place
