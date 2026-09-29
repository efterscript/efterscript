# Change: interpreter-parameters — user, system, and device parameters, and the font-cache operators

## Why

`languagelevel` answers 3, and a program that sees a Level 2 or 3 answer
is entitled to the Level 2 interpreter-parameter operators. Printer
drivers use them unguarded: a driver's setup query reads
`currentsystemparams` to size the device's memory, and its document
prologue names the job with `setuserparams` before the first page.
Neither operator exists, so the query fails with `undefined` and the
driver falls back to asking its user to configure the device by hand,
and the document job stops before its first page. The gap was recorded
when the Level 3 claim was made (the notes of the change that raised
`languagelevel`); a harvested job from a current driver has now reached
it, and it is the whole of what stops that job.

Three Level 1 operators the same drivers reach belong with them:
`cachestatus` (the font-cache report the setup query falls back on),
`setcachelimit`, and `setcacheparams`/`currentcacheparams`, which are
the Level 1 spellings of font-cache parameters (PLRM3 §C.3.2). Two
Level 1 gaps found by the same survey of the operator table are cheap
and fit here: `reversepath` and `writehexstring`; and `status`, whose
file-object form needs nothing new and whose file-name form needs one
optional capability method.

It matters now because the parameters decide where state lives. User
parameters follow `save` and `restore`; system and device parameters do
not. The next change, a long-lived job server, restores the whole VM at
the end of every job, so parameters built on the wrong side of that line
would either leak between jobs or be lost. Settling them first keeps the
job server's snapshot a mechanical question.

## What Changes

- **User parameters** (PLRM3 §C.1.1, Table C.1): `setuserparams` and
  `currentuserparams`. Held in a dictionary in local VM, so `save` and
  `restore` revert them with no extra machinery. Every Table C.1 key is
  defined; `JobName` is stored; the stack limits and `MaxLocalVM` report
  the interpreter's configured limits and stay there (the nearest
  achievable value, as the operator entries allow); the cache and
  halftone policies are recorded and have no effect, since nothing is
  cached and nothing is rasterised. Unknown keys are ignored.
- **System parameters** (PLRM3 §C.1.2, Table C.2): `setsystemparams`
  and `currentsystemparams`. Held by the interpreter outside VM, so no
  `restore` reverts them. A defined, deterministic set: the read-only
  reports (`ByteOrder`, `RealFormat`, `Revision`, `BuildTime`, the
  current cache consumptions at zero, `PageCount`), the cache limits,
  `PrinterName`, `StartupMode`, `FactoryDefaults`, the resource
  directories (`%null`: the library has no external resources), and the
  two write-only passwords.
- **Passwords** (PLRM3 §C.3.1): `SystemParamsPassword` and
  `StartJobPassword`, both seeded from the configured server password as
  a string. `setsystemparams` needs the right `Password` entry unless
  the only other entry is `FactoryDefaults`, the password is empty, or
  the prelude is running. `exitserver` accepts an integer or a string
  and compares it with either password.
- **Device parameters** (PLRM3 §C.4): `setdevparams` and
  `currentdevparams`, with no parameter sets defined — both raise
  `undefined` for every device name, which is what a program probing for
  a device inside `stopped` expects.
- **Font-cache operators**: `cachestatus`, `setcachelimit`,
  `setcacheparams`, `currentcacheparams`, reading and writing the same
  parameters; `setvmthreshold` and `vmreclaim` likewise for the VM user
  parameters.
- **Level 1 gaps**: `reversepath` (a backend path operation);
  `writehexstring`; `status` for a file object, and for a file name
  through a new `FileCapability::status` method whose default finds
  nothing.
- **Not added**: `echo` stays undefined, as its entry requires of an
  interpreter without `executive`; `startjob` belongs to the job-server
  change; host-supplied product-specific parameters (a device's memory
  size, its communication-channel parameter sets) are out of scope, with
  the trigger being a program that fails without one — the programs
  seen so far test for them with `known` or inside `stopped`.

## Capabilities

### New Capabilities
- `interpreter-parameters`: user, system, and device parameters, their
  passwords, and the font-cache and VM operators that alias them.

### Modified Capabilities
- `interpreter-core`: ADDED `writehexstring` and `status`.
- `graphics-ir`: ADDED `reversepath`.
- `printer-identity`: MODIFIED `statusdict and serverdict` — the
  `exitserver` password is compared as a string with either password
  parameter.
- `expected-divergences`: MODIFIED `server-password-default` — it
  covers `setsystemparams` as well as `exitserver`.

## Impact

- Code: `crates/efterscript-vm` (a new `ops/params.rs`; the parameter
  store on `Interp`; `ops/status.rs` for the password check;
  `ops/file.rs` for `writehexstring` and `status`; `files.rs` for the
  capability method), `crates/efterscript-graphics` (a backend method
  for `reversepath`). No change to `remelt`, `platen`, or the C ABI.
- Corpus: new files under `corpus/unit/params/`, plus `reversepath`,
  `writehexstring`, and `status` scenarios in the existing directories.
- `FileCapability` gains a provided method; existing implementations
  compile unchanged.
- No new dependencies.
