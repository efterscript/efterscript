# Design: Printer identity mechanism

See proposal.md and the spec deltas. This document fixes where the
dictionaries live, how seeding and the prelude run, what `exitserver`
does before job encapsulation exists, and how the implicit categories
are modelled.

## Context

- `systemdict` is built at construction from the operator tables and
  the standard dictionaries; `userdict`, `globaldict`, `errordict`,
  `$error` exist. `Config` has limits, streams, capabilities, quirks,
  and font settings. `distill` builds an interpreter from a `Config`.
- Resource categories are an enum with per-category dictionaries; the
  built-in tables for encodings and procedure sets are constants.
- The graphics state lives behind the backend trait; the VM keeps a
  few VM-side values already (the current font without a backend).
- No job encapsulation exists: one job per interpreter.

## Decisions

**D1. Dictionaries at construction.** `statusdict` (global VM,
writable, 64 entries) with `product` = `(EfterScript)`, `version` =
the crate version as a string, `revision` = 0; `serverdict` (global,
writable) with `exitserver`. Both are entries of `systemdict` and are
created before seeding and the prelude. *Alternative:* create them in
the prelude — then jobs that probe `statusdict` with `where` on a bare
interpreter would fail, and the dictionaries are reference-defined.

**D2. Seeding as data, then the prelude.** `Config::identity: Vec<(String,
MarkValue)>` (the existing value type; names, strings, numbers,
booleans, arrays, dictionaries become the corresponding objects) is
written into `statusdict` after construction; then
`Config::prelude: Option<Vec<u8>>` runs as a program through the normal
loop with a distinct outcome check: any error ends construction with
`Err(PreludeError { name, offending })`. The prelude runs with no
graphics backend requirement (it may define `setpage` in terms of
`setpagedevice`, which is defined without a backend already). `Config::
server_password: i32` default 0. The CLI maps `--identity K=V` (value
parsed like `--param`) and `--prelude <file>`.

**D3. `exitserver` today.** Pops the password, compares with the
configured one (`invalidaccess` on mismatch), sets `Interp::server_level
= true`, and returns; nothing is restored because no job `save` exists
yet. The session change will wrap each job in a `save` and make
`exitserver` restore to the server level and re-enter the loop outside
it. The contract this change fixes: password semantics, persistence of
what follows, and the operator living in `serverdict`. Recorded
explicitly in the spec's wording ("for the interpreter's life").

**D4. Implicit categories.** A second table beside the existing
built-ins: category name → member list derived from the interpreter's
capabilities (font types from `definefont`'s accepted set plus CID and
Type 0 as loadable, `FMapType` = [9], `Filter` = [], `ColorSpaceFamily`
= the six families, `Category` = every category name incl. the implicit
ones, `Generic` = []). `resourcestatus`/`findresource`/`resourceforall`
consult it; `defineresource`/`undefineresource` raise `invalidaccess`
(the members are not the program's to change). Integer keys are
compared as integers, names as names. *Alternative:* let the embedder
extend them — they describe the interpreter, so a claim the
interpreter cannot honour is a lie to the driver.

**D5. Screens and transfers.** Values stored in the VM-side graphics
state extension the font slot already uses when no backend exists, and
in the backend's `GState` when one does (so `gsave`/`grestore` honour
them): `screen: (f32, f32, Object)` per component set, `transfer:
Object` or four; defaults 60, 45, `{}`-like identity procedures. Never
emitted; the IR is untouched. `framedevice` validates operand count
and types and returns. `cexec` is registered as an alias of `exec`
with its own name for error attribution.

**D6. Report and private check.** `Report.identity: Vec<(String,
String)>` (the `statusdict` entries after seeding, printed forms) and
`Report.prelude_ran: bool`. The private check keeps a prelude beside
the captured job in the emulator's test data; the check runs the job
with that prelude and reports how far it gets; iterating the
interpreter until the page renders is this change's acceptance in the
private tier, with each construct fixed on the way getting a
clean-room scenario here.

## Risks / Trade-offs

- [Preludes hide interpreter gaps behind PostScript definitions] →
  only what the reference says is device-defined belongs in a prelude;
  language operators the prelude needs and lacks are fixed in the
  interpreter, and the notes list which.
- [`exitserver` without encapsulation surprises a job that expects a
  restore] → single-job runs never observe the difference; recorded.
- [Implicit category claims drift from reality] → the tables are
  derived from the code paths that accept each type, with a test that
  every claimed `FontType` defines and every listed category resolves.
