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

## Implementation notes

Recorded where the code departs from, or pins down, the text above.

- **Dictionaries (D1), amended: local VM.** `statusdict` (64 entries)
  and `serverdict` (8) are created in *local* VM, like `userdict` and
  `errordict`, and inserted raw into `systemdict`. D1 said global; the
  first run of the captured job showed why that cannot be: a job in
  local allocation mode stores strings into `statusdict` (its job name,
  say), and the global/local rule refuses a local string in a global
  dictionary — the prelude's own `/product (…) def` failed the same way.
  Being older than every `save`, local is as persistent as global for a
  prelude's definitions. `serverdict` holds `exitserver` through a new
  `Visibility::Server` (an `op_table!` arm `server`), so the operator is
  in no other dictionary. The default identity is `product
  (EfterScript)`, `version` = the crate version as a string (`0.0.1`
  today; note that a driver may `cvr` it — a host emulating a printer
  sets a numeric-looking one), `revision 0`; `ops::status::
  default_identity()` is public, and `Interp::statusdict_entries()`
  lists the dictionary in insertion order as `=` writes keys and `==`
  writes values.
- **Seeding and the prelude (D2) as built.** `Config::identity: Vec<
  (String, MarkValue)>`, `Config::prelude: Option<Vec<u8>>`, `Config::
  server_password: i32` (0). Values become objects through the same
  converter `setdistillerparams` seeding uses (`ops::distiller::object`,
  now crate-visible), allocated in local VM; a value the VM cannot hold
  (a name over the limit, nesting past 32) fails construction with
  `PreludeError { name: "limitcheck", offending: <the key> }`. The
  prelude runs through `Interp::run` on a `SliceSource` with
  `server_level` true and its output on the injected stdout; an
  `Outcome::Error` becomes `PreludeError { name, offending }` (the
  `$error` summary), a caught error is no failure, a `quit` ends the
  prelude alone (`has_quit` is cleared), and `Outcome::Suspended` —
  unreachable for a slice — is mapped to `syntaxerror`. **The
  constructor**: `Interp::try_with_config(Config) -> Result<Interp,
  PreludeError>` is new; `Interp::with_config` keeps its signature (25
  call sites across the workspace) and panics with the error's text
  when a prelude fails, documented as such, so only embedders that pass
  a prelude need the fallible form. `remelt::distill_into` uses
  `try_with_config` and gained `Error::Prelude(PreludeError)`; the CLI
  reports it as `efterscript: prelude failed: <name> in <offending>`
  with exit status 2, the document opened but unfinished.
- **`server_level`.** True while the prelude runs and after
  `exitserver` matched; false at the start of a job (there is no job
  encapsulation to flip it back, so the flag only says whether the
  password was given). `Interp::server_level()` and `Interp::
  prelude_ran()` are public.
- **`exitserver` (D3) as built.** `[Int]` operand; `invalidaccess` on
  mismatch with the operand left in place; on match the operand is
  popped, `server_level` set, and the dictionary stack cut back to the
  three permanent dictionaries, which is what "continue at the server
  level" means for the dictionary stack: a definition after `serverdict
  begin 0 exitserver` lands in `userdict`, where later jobs (and the
  rest of this one) find it, instead of in `serverdict` under the
  `begin` — the idiom every download uses without a matching `end`.
  The spec scenario's trailing `end` was therefore a slip and is
  amended. The reference converter refuses password 0 (its own server
  password is not 0), recorded as the expected divergence
  `server-password-default`; `cexec`, which it lacks, is
  `cexec-defined`.
- **Report (D6).** `Report.identity` is taken *after* the prelude, not
  only after the data seeding: the proposal's "identity in effect" is
  what the job sees, and a prelude that adds `waittimeout` is part of
  it. `Report.prelude_ran` mirrors the interpreter's flag. The CLI:
  `--identity Key=Value` (repeatable; the value parsed like `--param`,
  now with `(…)` for a string) and `--prelude <file>`, for `pdf`.
- **Implicit categories (D4) as built.** `Kind::Implicit(Implicit)` in
  `ops::resource` with constant member tables: `FontType` [0, 1, 2, 3,
  42], `FMapType` [9], `Filter` [], `ColorSpaceFamily` the six families
  sorted, `Category` the twelve category names sorted, `Generic` [].
  **FontType 0 and 2 are listed**, verified by the test that builds a
  dictionary of each claimed type and defines it: a FontType 0
  dictionary defines with `FMapType 9` and a CMap, a FontType 2
  dictionary defines by hand with the structural checks of 1 and 42 (its
  glyphs are reachable only when a FontSet loaded it, which is the
  reference's own mechanism for CFF); 9 and 11 are *not* listed although
  a CIDFont dictionary is accepted whatever its `FontType`, since
  `definefont` identifies those by `CIDFontType`, not by font type — a
  follow-up if a driver asks. `findresource` returns the key;
  `resourcestatus` answers `0 0 true` for a member (a key of another
  type is `false`); `defineresource`/`undefineresource` are
  `invalidaccess`; `resourceforall` enumerates through the new
  `ResourceKey` (`Name` | `Int`): name keys are matched against the
  template and written into the scratch string, **integer keys are
  pushed as integers and match every template**, which is what the
  reference converter does too (`(*) … /FontType resourceforall` lists
  its integers). The reference's answers for the scenario agree except
  for its own member lists.
- **Screens and transfers (D5) as built: in the backend's `GState`,
  behind opaque ids.** The boundary keeps its rule of no objects: the
  VM holds the procedures in an append-only table (`Interp::
  graphics_proc_ref` / `graphics_proc`, deduplicated by composite
  reference, entry 0 the empty procedure, on the font-instance argument
  that a `restore` also restores the state that referred to them) and
  the backend stores `[Screen { frequency, angle, spot: ProcRef }; 4]`
  and `[ProcRef; 4]` in red, green, blue, gray order. `setscreen`/
  `settransfer` set all four, `currentscreen`/`currenttransfer` read the
  gray one, the colour forms take and give four; the trait methods
  `set_screens`/`screens`/`set_transfers`/`transfers` have defaults (the
  defaults, nothing stored) so the recording mock needs nothing;
  `ps_graphics::Graphics` stores them, `gsave`/`grestore`/`restore`
  follow for free, and `initgraphics` keeps them (like the font: PLRM3
  §8.2 lists what it resets; the halftone and transfer are device
  setup). Without a backend the VM keeps its own slots, as for the font,
  and the eight operators are `Public`, so a prelude can call them. The
  frequency and angle are stored as `f32` and come back as reals —
  `60.0`, which is also what the reference prints — so the spec
  scenario's `60` is written `60.0`. A spot function may be a procedure
  or a dictionary (the Level 2 halftone form); a transfer must be a
  procedure; the default of both is `{}`. `framedevice` checks a
  six-number matrix, two non-negative integers, and an executable
  procedure, and pops them. `cexec` is a second table entry for the
  `exec` function.
- **Gaps the private check exposed, fixed here** (each with a clean-room
  scenario the reference converter agrees with):
  `pathforall` (§8.2) was undefined — the driver's setup code turns the
  clip path into a procedure list with it. It is a `LoopFrame::PathForAll`
  over the segments the new backend method `current_path` reports in
  user space (the CTM at the call; a procedure that changes the CTM does
  not move the rest), pushing reals; the loop step's value slots grew
  from two to six for a curve. `colorimage` (§8.2) was undefined — the
  driver loads it when `setcmykcolor` exists. Built on the image
  acquisition: one source for 1, 3, or 4 components in the matching
  device space at any depth; one source per component collects planes,
  the procedures called in rotation and the planes interleaved at the
  end, 8-bit samples only (`limitcheck` otherwise); sources of mixed
  kinds are `typecheck`, a component count other than 1, 3, 4
  `rangecheck`. Both graphics-visibility entries sit in a table appended
  after the earlier groups so no operator index moved. The resident
  faces' `FontBBox` was a literal array where a Type 1 program defines
  it as a procedure of four numbers; the driver re-encodes a face by
  executing `FontBBox` after `begin` and rebuilding it with `astore`, so
  the resident dictionaries now carry it executable (the reference's
  faces do too; its numbers differ). The resident faces lacked
  `CharStrings` and `Private`, required of a Type 1 dictionary (§5.2)
  and read by the driver (`/CharStrings get /space get`): each face now
  carries a read-only `CharStrings` mapping every glyph name of its
  metrics (`.notdef` first) to its index, as a Type 42 dictionary
  numbers glyphs — the faces have no Type 1 program to quote — and an
  empty read-only `Private`; `ps_fonts::resident::Metrics::glyph_names`
  is new.
- **The private check (D6).** The prelude beside the captured job in
  the emulator's test data (and its clone) seeds the product and a
  numeric version string, the manual-feed, timeout, job-name, and
  job-state entries, `setpage`/`setpageparams` over `setpagedevice`,
  and the page-type procedures in `userdict`. The job's probes, in our
  words: it classifies the device by whether the product string starts
  with the vendor's family name and what follows; decides whether to
  download by `eexec` and `cexec` being known, the class, and free VM
  from `vmstatus`; asks `42 /FontType resourcestatus`; converts
  `version` with `cvr`; reads and writes `waittimeout`, `manualfeed`,
  `manualfeedtimeout`, `jobname`; looks for `processcolors`,
  `setpage`/`setpageparams` and the page-type names; and expects
  `currentcolorscreen`/`setcolorscreen`, `settransfer`/
  `currenttransfer`, `pathforall`, `colorimage`, an executable
  `FontBBox`, and `CharStrings` on a resident face. Progression: (1)
  prelude `invalidaccess` at `def` → dictionaries moved to local VM;
  (2) `undefined` at `load` → `colorimage`; (3) `undefined` at `get` →
  resident `CharStrings`; then the page rendered (`pathforall` and the
  executable `FontBBox` were found by reading the job and fixed
  beforehand). Its download sections decrypt to a `readstring` of a
  binary string handed to `cexec` — native code for the printer's own
  processor — which `exec` of a literal pushes back, so the driver's
  smoothing procedures stay undefined and its plain `imagemask` path
  draws the icon; the reference converter, whose identity the job does
  not recognise, skips the sections by reading lines and takes the same
  path. Verdict over the job with the prelude on our side (`difftest
  oracle --prelude`, added to the harness for this and applied to our
  side only): **pass**, output same, one page each, differing pixel
  fraction 0 at 36 dpi and at 72 dpi. The rasterised page shows a
  double rule across the top with the window title centred on it, the
  row `1 item — 2,086K in disk — 17,086K available` under it, a folder
  icon (a 48 by 38 mask painted twice, white then outline) with `System
  Folder` beneath, and a double rule at the foot with the page number
  `1` centred below it. Nothing about the prelude entered this
  repository.
- **Not this change's.** A `Metrics` dictionary in a re-encoded resident
  face is not consulted for widths (the job defines one on a face it
  does not show); `strokepath` and `usertime` are still undefined (the
  job defines procedures over them it never calls); `version`,
  `product`, `revision` as `systemdict` operators (PLRM3 §8.2) — the
  job reaches them through `statusdict begin`; `FontType` 9 and 11 in
  the implicit table.
- **Goldens.** New: `identity/screen-round-trip` (`.ir`, `.pdf`: the fill
  alone), `graphics/colorimage-rgb` and `graphics/colorimage-planes`
  (the same image resource). Every pre-existing golden is
  byte-identical.
- **Gates.** `cargo test --workspace` 886 passed, 0 failed, 2 ignored
  (from 866); clippy clean on all targets; fmt clean; `difftest run`
  183 of 183; `parse-survival` 183 files, no failures; `fuzz-round` 2 600 programs (300 psgen, 1 300 core, 1 300 graphics), 0 failed;
  `lint-strings` clean; `openspec validate printer-identity-mechanism`
  valid. Oracle over the new corpus files: 9 pass, 2 expected
  divergences (`cexec`, `exitserver-ok`); output same for `screen-round-
  trip`, `pathforall-enumerates`, both `colorimage` files, and
  `resident-charstrings`.
