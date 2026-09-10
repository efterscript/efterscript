# Change: Printer identity mechanism — statusdict, serverdict, prelude, implicit resources

## Why

Printer drivers interrogate the device before drawing: they ask
`statusdict` who it is and what it can do, ask the implicit resource
categories which font types and filters exist, and download persistent
procedure sets through `serverdict`'s `exitserver`. The first real
driver job run through the interpreter stopped at the first of those
questions. EfterScript should answer them, but it must not become any
particular printer: the identity is the embedder's business. This
change adds the mechanism only. `statusdict` and `serverdict` exist and
are writable; an embedder seeds identity as data or, better, as a
PostScript prelude it owns, run once at session start and persisting
like a server-level download; the implicit resource categories answer
truthfully about this interpreter; and the operators such prologues
reach next — `cexec`, the screen and transfer setters, `framedevice` —
are accepted and recorded. Nothing in repo A names a product. A host
that emulates a printer supplies the prelude that makes the
interpreter look like one.

## What Changes

- **`statusdict` and `serverdict`** in `systemdict`: writable
  dictionaries created at startup, empty of identity except
  EfterScript's own `product`, `version`, and `revision`; `serverdict`
  holds `exitserver` (password-checked against a configured server
  password, default 0; on success the download that follows persists,
  see below) and `execjob`-style stubs are out of scope.
- **Seeding**: the interpreter configuration gains identity entries
  (name and value pairs written into `statusdict` before the prelude)
  and an optional **prelude** program executed once after startup at
  the server level, outside any job encapsulation, so its definitions
  persist for the interpreter's life. Errors in the prelude fail
  construction with the error reported.
- **Persistence model for now**: the interpreter runs one job per
  instance today, so "persists across jobs" means "persists after
  `exitserver` for the rest of the run"; the session work introduces
  per-job `save` encapsulation and will make `exitserver` pop it. The
  operator's contract is fixed here: password check, then continuation
  at the server level.
- **Implicit resource categories**: `FontType` (1, 3, 42, 2, 0 as
  supported), `FMapType` (9), `Filter` (none yet, an empty category
  until filters exist), `ColorSpaceFamily` (DeviceGray, DeviceRGB,
  DeviceCMYK, Separation, DeviceN, Indexed), `Category` (every
  category name), `Generic` (empty); `resourcestatus` answers status 0
  and size 0 for members, `findresource` returns the key itself,
  `resourceforall` enumerates; other unknown categories keep raising
  `undefined`.
- **Accepted and recorded**: `setscreen`, `currentscreen`,
  `settransfer`, `currenttransfer`, `setcolorscreen`,
  `currentcolorscreen`, `setcolortransfer`, `currentcolortransfer`,
  `framedevice`, and `cexec` (executed as `exec`); the screen and
  transfer values are kept in the graphics state and returned by their
  getters, and never applied.
- **Report**: `Report` lists the identity in effect and whether a
  prelude ran.
- Out of scope, with triggers: job encapsulation and multiple jobs per
  interpreter (the session change); `statusdict` entries that need
  device state such as page counts beyond a host-updated value; the
  `Filter` category's members (when filters land); `checkpassword` and
  `setpassword` beyond the server password.

## Capabilities

### New Capabilities
- `printer-identity`: `statusdict`, `serverdict`, seeding, the prelude,
  `exitserver`'s contract, and the accepted-and-recorded operators.

### Modified Capabilities
- `text`: "Encodings and resource categories" gains the implicit
  categories.

## Impact

- Code: `crates/ps-vm` (`ops/status.rs` for the dictionaries and
  `exitserver`, the prelude in `Interp` construction, `Config`
  additions, implicit categories in `ops/resource.rs`, `cexec`, screen
  and transfer operators), `crates/ps-graphics` (screen and transfer
  values in the graphics state, never emitted), `crates/remelt`
  (`Report` fields, `distill` passing the configuration),
  `crates/efterscript-cli` (`--prelude <file>` and `--identity
  Key=Value`), corpus scenarios seeding a fictional printer.
- The private check runs the captured driver job from the emulator's
  test data with a prelude kept beside it there; nothing about that
  prelude enters repo A.
- Depends on `distillation-policy` (archived) for the report shape.
