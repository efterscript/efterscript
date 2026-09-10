# printer-identity Specification

## Purpose
How an embedder makes the interpreter present itself as a device:
`statusdict` and `serverdict`, identity seeding, the startup prelude,
`exitserver`'s contract, and the device operators accepted for
compatibility.

## Requirements

### Requirement: statusdict and serverdict

`systemdict` SHALL hold a writable `statusdict` containing `product`,
`version`, and `revision` describing this interpreter and nothing else
by default, and a writable `serverdict` containing `exitserver`.
`exitserver` SHALL take a password, compare it with the configured
server password (default 0), raise `invalidaccess` on mismatch, and on
match continue execution at the server level so that subsequent
definitions persist for the interpreter's life.

#### Scenario: Default identity

- **GIVEN** `statusdict /product get = statusdict length =`
- **THEN** the output is the interpreter's own product string followed
  by `3`

#### Scenario: exitserver with the right password

- **GIVEN** `serverdict begin 0 exitserver /persist 1 def` and later
  `persist =`
- **THEN** the output is `1`: the dictionary stack is back at the
  permanent dictionaries and the definition landed in `userdict`

#### Scenario: exitserver with the wrong password

- **GIVEN** `serverdict begin 1 exitserver`
- **THEN** the error is `invalidaccess`

### Requirement: Identity seeding and prelude

The interpreter configuration SHALL accept identity entries written
into `statusdict` before any program runs and an optional prelude
program executed once at startup at the server level; a prelude error
SHALL fail construction with the error reported. The command-line tool
SHALL accept `--identity Key=Value` and `--prelude <file>`.

#### Scenario: Seeded identity

- **GIVEN** an interpreter configured with `product` = `(Fictional
  Press)` and `manualfeed` = `false`
- **WHEN** `statusdict /product get = statusdict /manualfeed get =`
  runs
- **THEN** the output is `Fictional Press` then `false`

#### Scenario: Prelude defines the device

- **GIVEN** a prelude `statusdict begin /waittimeout 300 def /setpage {
  pop 2 array astore << /PageSize 3 -1 roll >> setpagedevice } def end`
- **WHEN** a job runs `statusdict /waittimeout get =` and `612 792 0
  setpage` is used before a page
- **THEN** the output is `300` and the page is 612 by 792

#### Scenario: Prelude failure

- **GIVEN** a prelude `1 0 div`
- **THEN** construction fails with `undefinedresult` reported

### Requirement: Accepted device operators

`setscreen`, `setcolorscreen`, `settransfer`, `setcolortransfer` SHALL
accept their operands, record them in the graphics state, and apply
nothing; their `current…` counterparts SHALL return what was recorded
(the defaults before any set); `framedevice` SHALL accept its four
operands and do nothing; `cexec` SHALL behave as `exec`. None of these
SHALL affect the IR.

#### Scenario: Screen round-trips

- **GIVEN** `60 45 { pop } setscreen currentscreen pop pop =`
- **THEN** the output is `60.0` (the frequency comes back as a real)
  and the page's IR is unchanged

#### Scenario: cexec

- **GIVEN** `{ 1 2 add = } cexec`
- **THEN** the output is `3`
