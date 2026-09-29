# Design: interpreter-parameters

See proposal.md and the spec deltas. This document fixes where each
class of parameter lives, the parameter set and its defaults, the
password rules, and the handful of Level 1 operators carried along.

## Context

- PLRM3 Appendix C (§C.1–§C.4) defines the three classes of parameter
  and their properties; the operator entries in §8.2
  (`setuserparams`, `currentuserparams`, `setsystemparams`,
  `currentsystemparams`, `setdevparams`, `currentdevparams`,
  `cachestatus`, `setcachelimit`, `setcacheparams`,
  `currentcacheparams`, `setvmthreshold`, `vmreclaim`, `reversepath`,
  `writehexstring`, `status`, `echo`) fix the operand and error
  behaviour. The set of parameters is explicitly product-dependent: an
  implementation defines the keys it supports and ignores the rest.
- `Memory` snapshots local VM on `save`; `restore` reverts it. Global
  VM is not reverted by `restore` today, and the job-server change will
  make the job server's outermost save revert both.
- `exitserver` compares an integer with `Config::server_password`
  (`ops/status.rs`). The prelude runs at the server level
  (`Interp::run_prelude`).
- `vmstatus` already reports a fixed maximum (`VM_MAXIMUM`, 2^30);
  stack limits come from `Limits` (the PLRM3 Appendix B minimums by
  default).
- Paths live in the graphics backend in default user space; path
  operators that need the path are backend calls.

## Goals / Non-Goals

**Goals:** every operator a Level 2 program may call unguarded is
defined; parameters sit on the correct side of `save`/`restore` and of
the coming job-server restore; values are deterministic.

**Non-Goals:** caches, garbage collection, or rasterisation that the
parameters would govern; product-specific parameters supplied by the
host; device parameter sets; `startjob`; the interactive `executive`.

## Decisions

**D1. User parameters live in local VM.** At construction the
interpreter allocates one dictionary in local VM (not reachable from any
standard dictionary) and fills it with the defaults (D2). Because it is
local and older than every `save`, `restore` reverts its contents with
no new machinery (PLRM3 §C.1.1), and under a job server an encapsulated
job's changes vanish at its end while an unencapsulated job's become the
next job's defaults — exactly the appendix's rule, for free.
`setuserparams` validates every present key first (types; `JobName`
over 100 bytes truncated) and only then stores, so an error changes
nothing; strings are copied into new local strings whatever the
allocation mode, since a local dictionary may hold them and the
program's string must not alias the parameter. `currentuserparams`
allocates its result in the current allocation mode and copies
strings. *Alternative:* a Rust struct with its own save stack —
duplicates what the VM already does and would need the job server to
snapshot it separately.

**D2. The parameter set.** User parameters (Table C.1, all keys):
`AccurateScreens false`, `HalftoneMode 0`, `IdiomRecognition false`
(never set: the interpreter has no idiom recognition), `JobName ()`,
`MaxDictStack`/`MaxExecStack`/`MaxOpStack` = the configured `Limits`,
`MaxLocalVM` = 2^30 (the `vmstatus` maximum), `MaxFontItem 12500`,
`MaxFormItem 100000`, `MaxPatternItem 20000`, `MaxScreenItem 65536`,
`MaxSuperScreen 1016`, `MaxUPathItem 5000`, `MinFontCompress 100`,
`VMReclaim 0`, `VMThreshold 40000`. The limits are reported and
unchangeable (the entries allow the nearest achievable value); the rest
are recorded, since nothing they govern exists.

System parameters (Table C.2 and the entries on p. 752–753 of the
appendix, as keys this implementation defines): read-only `ByteOrder
false`, `RealFormat (IEEE)`, `Revision` (= `systemdict` `revision`),
`BuildTime 0` (deterministic output outranks a build stamp),
`PageCount` (pages shown by `showpage` and `copypage` in the
interpreter's life), and `CurDisplayList`, `CurFontCache`,
`CurFormCache`, `CurOutlineCache`, `CurPatternCache`,
`CurScreenStorage`, `CurSourceList`, `CurStoredScreenCache`,
`CurUPathCache` all 0; writable `MaxDisplayAndSourceList`,
`MaxDisplayList`, `MaxFontCache`, `MaxFormCache`, `MaxImageBuffer`,
`MaxOutlineCache`, `MaxPatternCache`, `MaxScreenStorage`,
`MaxSourceList`, `MaxStoredScreenCache`, `MaxUPathCache` (defaults
recorded in the code, each a round figure, negative values stored as
0), `PrinterName` (= `product` until set; empty resets it),
`StartupMode 0`, `FactoryDefaults false` (accepted, no effect: there is
no non-volatile storage), `FontResourceDir (%null)`,
`GenericResourceDir (%null)`, `GenericResourcePathSep (/)` (§C.3.6
names `%null` for a product with no external resources); write-only
`SystemParamsPassword`, `StartJobPassword`. `LicenseID` is not
defined: it names a vendor's licensing scheme.

**D3. System parameters live outside VM.** A `SystemParams` struct on
`Interp` holds them as Rust values; `currentsystemparams` builds a new
dictionary in the current allocation mode. No `restore` — including the
job server's outermost one — reverts them, as §C.1.2 requires of a
systemwide, permanent setting. *Alternative:* a global-VM dictionary —
the job-server change reverts global VM between jobs, which would undo
a permitted change made by an encapsulated job.

**D4. Passwords.** Both passwords are byte strings initialised from
`Config::server_password` formatted as `cvs` formats an integer; the
configuration type does not change. A password operand is a string or
an integer (converted the same way); comparison is exact (§C.3.1).
`setsystemparams` and `setdevparams` are permitted when (a) `Password`
equals `SystemParamsPassword`, (b) `SystemParamsPassword` is empty,
(c) `FactoryDefaults` is the only other entry, or (d) the prelude is
running — the host configuring its device is the system administrator.
The system-administrator job of §C.3.1 arrives with `startjob` in the
job-server change and becomes (e). `exitserver` accepts a match with
either password (§C.3.1 note: what applies to `startjob` applies to
`exitserver`); its behaviour on a match is unchanged here.

**D5. Device parameters: none.** The appendix leaves parameter sets and
their names to the product (§C.4), and a library has no devices of its
own. Both operators check operand types and raise `undefined` for any
name, which is what a program probing a communication channel inside
`stopped` handles. *Trigger* for host-supplied sets and host-supplied
system keys (a memory size, a channel's parameters): a program that
fails without one. The mechanism would be a configuration list parallel
to the identity entries; it is not built now because no job needs it.

**D6. The cache and VM operators are views.** `setcachelimit`,
`setcacheparams`, `setvmthreshold`, and `vmreclaim` write the user
parameters they alias through the same validation path as
`setuserparams`; `cachestatus` and `currentcacheparams` read them.
`setcacheparams`'s first value names `MaxFontCache`, a system
parameter: it is not applied (the nearest achievable value), so a
program need not hold the system password to call a Level 1 operator.
`vmreclaim` 1 and 2 request a collection; there is no collector, so
they do nothing.

**D7. `reversepath` is a backend call.** `GraphicsBackend` gains
`reverse_path`, implemented in `efterscript-graphics` over its path
representation: subpaths keep their order, each is rebuilt from its
last point back to its first, curve control points swap, a
`closepath` stays at the end of its subpath, and the current point
becomes the last subpath's new end. The operator has graphics
visibility like the other path operators.

**D8. `writehexstring` and `status`.** `writehexstring` writes lowercase
digits through the file's stream. `status` on a file object reports
whether the file table entry is open. `FileCapability` gains a provided
method `fn status(&mut self, name: &[u8]) -> Option<FileStatus>` whose
default returns `None`; `FileStatus` carries the four integers. No
shipped embedder installs a file capability, so `status` of a name
answers `false` everywhere today; the test suite's capabilities
exercise the other branch.

**D9. `echo` stays undefined.** Its entry defines it only where
`executive` exists; the interpreter has none.

**D10. Corpus.** Default values are implementation-dependent and differ
between interpreters, so corpus files print properties (a key is known,
a value survives or reverts, a type) rather than defaults, except for
this interpreter's own configured limits, which are declared `% oracle:
skip` with the reason. Files: `corpus/unit/params/` for the parameter
scenarios, the `reversepath` scenarios under `graphics/`, and
`writehexstring`/`status` under `vm/`.

## Risks / Trade-offs

- [A program branches on a default value] → the defaults are plausible
  magnitudes; a program that needs a specific product's value is D5's
  trigger.
- [`PageCount` increases across a job server's jobs] → intended: a
  device's page count is systemwide; the job-server change relies on it.
- [A driver sets a system parameter without a password] → it gets
  `invalidaccess`, as on a device whose administrator set a password;
  the default password is the configured one, so hosts choose.

## Migration Plan

Additive: new operators, new provided trait method, one widened operand
type (`exitserver` accepts strings). No golden changes expected; any
change to an existing golden is a defect.

## Open Questions

- None blocking. The round-figure defaults for the cache limits are
  recorded in the implementation notes when chosen.
