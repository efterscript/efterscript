# Design: Triage fixes 4

See proposal.md and the spec deltas. This document fixes how stroke
outlines are built, how randomness and time stay deterministic in a
library with no clock, how overprint reaches the PDF, how the page
device joins the graphics state, and the precision rules for path
readings.

## Context

- Library crates have no time source and no randomness; determinism
  of goldens is a project invariant. The embedder already supplies
  files through `FileCapability`.
- Strokes reach the IR as paths plus the CTM they were measured in; no
  outline geometry exists. `ps-graphics` flattens arcs to Béziers and
  keeps the flatness parameter but never flattens curves.
- The page device dictionary lives on the interpreter, outside the
  graphics-state stack; `restore` performs `grestoreall` and pops the
  state saved with `save`.
- `pathbbox` transforms the device-space box's corners through the
  inverse CTM in single precision; the generator found sixth-digit
  differences in `arcto`, `currentpoint`, and `pathbbox` readings.
- PLRM3: `strokepath` and `ustrokepath` entries (§8.2), `setflat`
  and `flattenpath` (§8.2), stroke geometry §4.5.2–4.5.3 and §7.5.2;
  `rand`/`srand`/`rrand`, `usertime`/`realtime` entries; §4.8.5
  overprint; §6.1.1 the page device across `restore`/`grestore`;
  `pathbbox` entry; Table 5.11 CID font `FontType` values. ISO 32000-1
  §8.4.5 extended graphics state (`OP`, `op`, `SA`).

## Goals / Non-Goals

**Goals:** every listed operator defined with the manual's semantics;
deterministic output; overprint preserved; readings agreeing with
the reference to the precision the generator checks.

**Non-Goals:** exact outline segment agreement with the reference;
applying stroke adjustment; `setoverprintmode`; a wall clock in the
library.

## Decisions

**D1. Stroke outlines are flattened polygons built in ps-graphics.**
`strokepath` is a backend call (`fn stroke_outline(&mut self) ->
Result<(), VmError>`) that replaces the current path: the backend
flattens each subpath's curves with the current flatness (a Bézier
subdivision to the tolerance in device pixels, as `setflat` defines
it), applies the dash pattern (segments and gaps along the flattened
polyline, phase honoured), and for each dash or subpath builds the
outline: offset polylines on both sides at half the line width in
*user space transformed by the CTM* (so a non-uniform CTM gives the
same ellipse-shaped pen `stroke` produces), joins per `setlinejoin`
(mitre with the mitre-limit cut-off, round as a flattened arc, bevel)
and caps per `setlinecap` (butt, round, projecting), closed subpaths
joined at their start, degenerate subpaths following `stroke`'s rules
(a zero-length open subpath with round caps gives a dot, with butt
caps nothing). The result is one closed polygon per dash/segment run
(joins closed into the run), which `fill` with the nonzero rule paints
as the stroke would. Paths in the IR are already in default user
space, so the outline is built there and stored as the new current
path (segments in default user space; `currentpoint` the last point).
`ustrokepath` = `newpath uappend [concat] strokepath [setmatrix]` per
its entry. *Alternative:* an exact offset of Bézier curves — far more
code for no distillation benefit; the manual permits any outline that
fills as the stroke.

**D2. Randomness and time are deterministic by default.** `rand`,
`srand`, `rrand` use a 31-bit linear congruential generator of the
project's own parameters recorded in the notes (multiplier and
increment chosen and tested for full period on 2^31), state in the
interpreter (per context; one context here), initial seed a fixed
constant so a job without `srand` is reproducible run to run; `rrand`
returns the state, `srand` sets it (any integer, masked). `usertime`
returns the interpreter's execution counter scaled to a millisecond-
like unit (the existing step budget counter, divided by a constant
recorded in the notes) — monotonic, deterministic, and legal per the
entry ("accuracy depends on the environment"). `realtime` asks a new
`Clock` capability (`fn realtime_ms(&mut self) -> i32`) installed by
the embedder like `FileCapability`; absent, it answers the same value
as `usertime`. Both wrap to the most negative integer as the entries
say. The CLI installs nothing in this change (deterministic goldens;
recorded as the trigger). *Alternative:* `std::time` in the library —
forbidden; a real clock would also make the corpus non-deterministic.

**D3. Stroke adjustment is recorded; overprint is carried.** Both live
in the VM-side graphics state (saved/restored; `setstrokeadjust` is
not reset by `initgraphics` per its entry; set to `false` when a
glyph procedure starts, per its entry — hook the Type 3 begin). Only
overprint crosses the boundary: `fn set_overprint(&mut self, on:
bool)` on the trait; the emitter records it like line width — an
`IrOp::Overprint(bool)` written only where it takes effect and
differs; the dump prints `op true|false`. The writer keeps one
extended graphics state resource per distinct setting (`/GSn << /Type
/ExtGState /OP bool /op bool >>`) and emits `/GSn gs` inside the
saved state where the IR sets it; both `OP` and `op` are written with
the same value (no `setoverprintmode`). *Alternative:* write `SA` too
— stroke adjustment is a raster hint the project leaves to viewers;
recorded, not written.

**D4. The page device joins the graphics state.** The interpreter's
page-device dictionary becomes a field of the VM-side graphics state
(the object handle; `setpagedevice` installs a new dictionary in the
current state), so `gsave`/`grestore`/`grestoreall` and `restore`
(which already does the `grestoreall` and pops the saved state) bring
the previous page device back; `currentpagedevice` reads the current
state's. The backend's media box follows through the existing
`set_media_box` on each state change that alters it (the backend
already keeps `media_box` per state). A page device installed inside
`gsave` and dropped by `grestore` therefore reverts, as §6.1.1 says.
*Alternative:* special-casing `restore` only — the manual lists all
four operators.

**D5. Readings in double precision; `pathbbox` rules.** The path
construction operators keep their `f32` boundary but compute through
the CTM and its inverse in `f64` before rounding once (`arcto`'s
tangent points from the two vectors in `f64`, `currentpoint` and
`pathbbox` corners through an `f64` inverse). `pathbbox`: a declared
`setbbox` (from the user-path change) wins; otherwise the device-space
box encloses every segment end *and control point*, a trailing lone
`moveto` is excluded, the box's corners go through the inverse CTM and
the axis-aligned envelope is returned; an empty path is
`nocurrentpoint`. *Alternative:* tight curve extrema — the manual says
control points, and `flattenpath` is the documented way to tighten.

**D6. Identity.** `languagelevel` 3 in `systemdict`, with the notes
listing the LanguageLevel 3 features still absent (halftone types,
`setcolorrendering` application, `CCITTFax`, `DCT` decoding,
`setoverprintmode`, idiom recognition) so the claim is auditable;
`serialnumber` (an integer from the identity configuration, default
0), and `version`/`product`/`revision` defined in `systemdict` with
the same values `statusdict` carries, updated together when the
identity is configured. The `FontType` category lists `[0 1 2 3 9 11
42]` (Table 5.11: CIDFontType 0 → 9, CIDFontType 2 → 11; type 10 and
32 are not accepted, so not listed).

**D7. Corpus.** `corpus/unit/graphics/`: `strokepath` of a line with
each cap and join, a dashed curve, a closed rectangle, a degenerate
subpath, compared against the reference by filling the outline (the
fill's raster is what the oracle sees; the IR golden pins our
outline); `ustrokepath` both forms; `pathbbox` with control points,
after `flattenpath`… (no `flattenpath` yet — record; use a curve
whose control points stick out), with `setbbox`, with a trailing
`moveto`; overprint set and painted with a Separation fill (PDF
golden with the ExtGState; the oracle in colour); strokeadjust
getter/setter; page device across `gsave`/`grestore` and `save`/
`restore` (`currentpagedevice /PageSize get`); `arcto` readings at the
generator's found cases (from its findings, re-typed). `corpus/unit/
interp/`: `rand` sequence reproducibility after `srand`, `rrand`
round-trip, `usertime` monotonic, `realtime` an integer — values never
printed where the oracle compares output (the reference's sequence
differs; files print booleans about the values instead). `corpus/unit/
identity/`: `languagelevel`, `serialnumber`, the `systemdict` entries,
the category members. Goldens; the oracle; the captured job.

## Risks / Trade-offs

- [Outline construction bugs show as wrong fills] → the oracle
  compares the filled outline's raster against the reference's fill
  of its own outline; joins and caps each get a corpus file; the
  generator gains `strokepath` in a later round.
- [A different random sequence from the reference] → corpus files
  print properties, not values; no divergence needed.
- [Page device in the state changes `restore` behaviour for existing
  jobs] → the captured driver job and every corpus file re-run; the
  manual mandates it.
- [`languagelevel` 3 lets jobs take paths that hit remaining gaps] →
  the notes list them; a job hitting one raises `undefined` where it
  did before as well, now further in.

## Open Questions

- The `usertime` scale constant and the `realtime` fallback: any
  choice satisfies the entries; record the one taken.

## Implementation notes

Recorded as built, part 1 (tasks 1.1–1.4 and 2.1). Provenance is the
PLRM3 entries and sections the design cites; each note names the one
it rests on.

- **Random numbers (D2) as built** — `ops/random.rs`, from the `rand`,
  `srand`, and `rrand` entries (§8.2). A mixed linear congruential
  recurrence modulo 2^31 with the project's own parameters: multiplier
  `0x3A47_0C25` (one more than a multiple of four, as the full period
  modulo a power of two requires; five more than a multiple of eight, so
  the potency is as high as the modulus allows; about 0.46 of the
  modulus), increment `0x1B4F_2C63` (odd), initial state `0x1D2B_4A99`.
  The unit tests check the conditions on the constants, walk the whole
  period for the moduli 2^12 and 2^20 (the conditions are the same for
  every 2^k, so the 2^31 period follows), and check the outputs. The
  value `rand` returns is not the raw state: the low bits of a
  power-of-two recurrence cycle quickly (bit k has period 2^(k+1)), so
  the state is folded once with its own high half (`state ^ (state >>
  15)`, invertible, hence a permutation of the states and uniform over a
  period); a test asserts the low bit does not simply alternate, that
  every low nibble appears within five percent of its share over 65 536
  draws, and that the lag-1 correlation over 100 000 draws is below
  0.02. `rrand` returns the raw state (0 to 2^31 − 1), `srand` masks any
  integer to 31 bits and installs it, so `srand` of an `rrand` value
  continues the sequence exactly and seeds equal under the mask start
  the same sequence. State: `Interp::random_state`, one per interpreter
  (one context).
- **Clocks (D2) as built** — from the `usertime` and `realtime`
  entries. `usertime` is `steps / 10 000`, where `steps` is the
  interpreter's execution counter, now counted whether or not a budget
  is set (`Interp::steps` was zero without a budget; the test
  `no_budget_leaves_execution_unbounded_but_counted` records the
  change). The scale is a measurement, not a promise: a release build on
  the development machine ran a loop of `pop`s and dictionary stores at
  some 10 600 objects per millisecond (a debug build at 200–400), so
  the clock ticks at roughly real-millisecond rate in the shipped build
  and identically in every build. The value wraps by truncation to 32
  bits, so `i32::MAX + 1` reads as `i32::MIN` (unit test on the pure
  function `usertime_of`). `realtime` reads `Capabilities::clock`, a
  `Box<dyn Clock>` with `fn realtime_ms(&mut self) -> i32`, installed
  like `FileCapability`; without one it answers exactly what `usertime`
  would. The CLI installs no clock (trigger: a job that needs wall-clock
  `realtime`; the corpus stays deterministic until then); `platen` and
  `remelt` likewise. The corpus files print properties of the values
  only, since the reference's sequence and rates differ.
- **`languagelevel` 3 (D6)** and the features a Level 3 claim implies
  that are still absent, found by surveying the operator table:
  `sethalftone`/`currenthalftone` (every halftone type, 1–5 as well as
  the Level 3 types 6, 10, 16; `setscreen` and friends are recorded
  only); applying a colour rendering dictionary (`setcolorrendering`
  records, `findcolorrendering` answers, nothing converts);
  `CCITTFaxDecode` (absent from the `Filter` category) and `DCTDecode`
  (a placeholder); `setoverprintmode`/`currentoverprintmode`; idiom
  recognition (`IdiomSet` resources and the `bind` behaviour they
  drive); `clipsave`/`cliprestore`; `setblackgeneration`/
  `setundercolorremoval` and their getters; the parameter operators
  `setsystemparams`/`currentsystemparams`, `setuserparams`/
  `currentuserparams`, `setdevparams`/`currentdevparams`;
  `startjob`; the file-system operators `deletefile`, `renamefile`,
  `filenameforall`; the page-device procedures `GetHalftoneName`,
  `GetPageDeviceName`, `GetSubstituteCRD`; the insideness operators
  `infill`, `ineofill`, `instroke`, `inufill`, `inueofill`,
  `inustroke`; user objects (`defineuserobject`, `execuserobject`,
  `undefineuserobject`); `cshow`; `findencoding`; `setvmthreshold`;
  CIDFont types 1 and 4 (`FontType` 10 and 32); and `setpagedevice`'s
  implicit `initgraphics` and `erasepage` (its entry), which the
  operator still does not perform. A job reaching one of these raises
  `undefined` where it did before, only later in the job.
- **Identity in `systemdict` (D6) as built** — from the `product`,
  `version`, `revision`, `serialnumber` entries. `serialnumber` is 0 in
  `systemdict` from construction (`populate`); `ops::status::
  seed_identity` writes every configured entry into `statusdict` as
  before and, for the four identity keys, into `systemdict` as well
  (raw insert into the read-only global dictionary, safe because the
  objects predate every `save`), so the default seeding puts `product
  (EfterScript)`, `version` (the crate version), and `revision 0`
  there, and `Config::identity` overrides all four. A `serialnumber`
  that is not an integer fails construction with `PreludeError {
  typecheck, "serialnumber" }`. The mirror happens at seeding only: a
  prelude that redefines `product` in `statusdict` does not change what
  the `product` operator answers (it may define `product` in `userdict`
  to shadow `systemdict`'s, as the captured host prelude's job never
  needs). Plumbing: the CLI's `--identity Key=Value` and `platen`'s
  `identity` entries (PostScript literals) already carry an integer, so
  `serialnumber=123` reaches the VM through the existing paths; the CLI
  help text names the four keys; **the C ABI is unchanged** (the entry
  fits `platen_entry`). `statusdict` gains no `serialnumber` unless
  configured (`default-identity.ps` still counts three entries). The
  reference keeps only `product` in its `statusdict`, so the corpus
  file compares `version` and `revision` only where `statusdict` has
  them.
- **`FontType` 9 and 11 (D6)** — `FONT_TYPES` is `[0, 1, 2, 3, 9, 11,
  42]`. Beyond D6, `define` now inserts `FontType` 9 or 11 into a
  CIDFont dictionary by its `CIDFontType` (Table 5.12: inserted by
  `definefont`/`defineresource`, a Type 2 CIDFont's 42 replaced by 11),
  so the category lists exactly the types a defined font can carry;
  the identity test defines one of each and reads 9 and 11 back. The
  reference lists 10 as well (it accepts `CIDFontType` 1); this
  interpreter does not, so `cid-font-types.ps` differs there by design
  (a capability, not a divergence; the file says so).
- **Stroke adjustment and overprint (D3, VM half) as built** — from
  the `setstrokeadjust`, `currentstrokeadjust`, `setoverprint`,
  `currentoverprint`, and `initgraphics` entries and §4.8.5. Both are
  booleans in a new VM-side graphics state `VmGState { page_device,
  stroke_adjust, overprint }` (`interp/mod.rs`), saved and restored in
  step with the backend's stack (below). `initgraphics` resets neither:
  its entry lists what it resets and says the rest, stroke adjustment
  named among them, is left unchanged; overprint is not in the list
  either, and the corpus file checks both survive it (the reference
  agrees, output same). Initial values `false` (the stroke-adjustment
  entry makes the initial value device-dependent, `false` for printers).
  A glyph procedure starts with stroke adjustment `false`: `begin_glyph`
  clears it right after its `gsave`, the procedure may set it, and the
  glyph's `grestore_to` brings the caller's value back (test
  `a_glyph_procedure_starts_without_stroke_adjustment`). The trait hook
  `fn set_overprint(&mut self, on: bool) -> Result<(), VmError>` has a
  default of `Ok(())`; the VM calls it on every `setoverprint` and on a
  restoration whose value differs from the one before (`set_line_width`
  is not replayed on `grestore` because the backend keeps its own copy
  per state; overprint is replayed on change so a backend keeping
  nothing still hears it, and one keeping its own copy hears a value it
  already has). The recording test backend logs the calls. The four
  operators have `graphics` visibility, like `setsmoothness`: the
  getter scenario could not be a `% backend: none` file because
  `gsave`/`grestore` are undefined without a backend, so
  `overprint-round-trip.ps` runs with the backend and no marks.
- **The page device in the graphics state (D4) as built** — from
  §6.1.1 and the `setpagedevice`, `restore`, `grestore`, `gsave`
  entries. The dictionary handle is `VmGState::page_device`;
  `currentpagedevice` answers the current state's. `setpagedevice`
  builds a fresh global read-only dictionary (the current one's
  entries merged with the request's, all copied to global VM as
  before) and installs it, so a saved state keeps its own dictionary
  and restoring the state brings the earlier contents back; the old
  in-place merge would have let a `restore` bring back a handle whose
  contents had moved on. Stack discipline: the VM keeps `vm_gstates`,
  one entry per state on the backend's stack, moved only through
  `Interp::gsave`, `grestore`, and `grestore_to`, which every former
  direct call site now uses (`save`/`restore`, the operators, glyph
  procedures, user-path painting, pattern cells, forms); the backend's
  own `gsave` inside `begin_pattern_cell`/`begin_form` is mirrored by
  `Interp::align_vm_gstates` right after those calls; a debug assertion
  checks the two depths agree after every move. On a restoration that
  changes the page-device handle, the VM calls `set_media_box` with the
  restored dictionary's `PageSize` (the backend already keeps its media
  box per state, so for `ps-graphics` the call repeats its own restore;
  a backend keeping nothing is told). `restore` reaches this through
  the existing `grestore_to`; `grestoreall` on an empty stack does
  nothing, as its entry says. `set_graphics_backend` clears the mirror
  (a fresh backend has an empty stack). Consequences: `pagedevice-
  merges.ps` had asserted that a page size set between `save` and
  `restore` survives the `restore`; the entry for `setpagedevice` and
  §6.1.1 say the opposite, and the reference prints `[612.0 792.0]`
  after the `restore`, so the file's expectation is corrected (it now
  checks the merge inside the `save` and the reversion after). The
  `pagedevice-records-unknown-keys` divergence is unaffected (the
  merge is the same, into a new dictionary); the printer-identity
  page-device requirements (the host prelude's `setpage` procedures)
  still hold, and the captured driver job passes with output identical
  to the reference's.
- **`pathbbox` (D5) as built** — from the `pathbbox` and `setbbox`
  entries. `GState::path_bbox` takes the device-space box over every
  segment end and control point, leaving out a `Move` that ends the
  path unless it is the whole path (a `Move` after a `Move` already
  replaces it), sends the four corners through the `f64` inverse CTM,
  and returns the axis-aligned envelope rounded once; an empty path is
  `nocurrentpoint`. A `setbbox` outside a user path wins: `Interp::
  set_declared_path_bbox` now also stores the box's device-space
  envelope under the CTM at the declaration, and the operator derives
  the answer from that envelope through the inverse of the current
  CTM, after checking for a current point. A `setbbox` inside a user
  path (`uappend`) does not declare a box for `pathbbox`; recorded, a
  follow-up if a job asks. Corrected expectation: `fonts/type42-
  charpath-bbox.ps` (generated by `ps-fonts`' corpus generator, so the
  template changed) asserted that the trailing `moveto` `charpath`
  leaves at the advance widens the box; the entry excludes a trailing
  `moveto`, and the reference prints `10.75` (its own outline) where the
  file expected `11.719` — now `10.742`.
- **Readings in double precision (D5) as built** — `Path` carries
  `current64`/`start64`, the current point as the construction
  operator computed it through the CTM in `f64` before rounding
  (`move_to_at`, `line_to_at`, `curve_to_at`); `currentpoint` reads it
  through the `f64` inverse and rounds once, so a point read back under
  a rotation is the point that was set; `arcto` starts from it and
  `arc::tangent` works in `f64` throughout (`P64` points), returning
  the tangent points rounded once. **Deviation from D5's wording:** the
  stored geometry (the IR's device-space segments) is still the CTM
  applied in single precision, so that every IR and PDF golden stays as
  it was; only the shadow and the readings moved to `f64`. Moving the
  geometry too changes `arcto-quarter-sweep` in the sixth digit
  (`m 11.7798 -0.0995612` → `-0.099561`; the exact value is −0.0995606)
  and nothing else; left for a change that wants it. One golden did
  move regardless: `arcto`'s arc (centre, start angle, sweep) now comes
  from the exact corner, and `corpus/golden/pdf/graphics/arcto-quarter-
  sweep.pdf` changed in one number, `2.85282 8 23 28.1472 23 53 c` →
  `2.85281 …` — the exact control point is −22 + 45·4/3·tan(22.5°) =
  2.852813, so the new reading is the correctly rounded one and the old
  was an artefact of the twice-rounded current point. Its IR golden is
  byte-identical (the difference is below the dump's six digits). Every
  other pre-existing golden is byte-identical. The generator's recorded
  cases are unit tests in `ps-graphics/tests/backend.rs`: the acute
  corner `-45 rotate 440 404 moveto 464 404 371 414 50 arcto` agrees
  with an independent `f64` construction to six significant digits
  (−468.680, 404, −463.335, 503.713; the reference prints −468.692,
  404.001, −463.346, 503.715 — its own single-precision corner, so the
  corpus file's output differs from the reference's while the page
  passes), and the quarter turn under an anisotropic scale is one
  Bézier piece. `pathforall` still reads coordinates through the
  single-precision inverse; not a reading the generator checks;
  recorded.
- **Stroke outlines (D1) as built** — `ps-graphics/src/outline.rs`,
  from the `strokepath`, `ustrokepath`, `stroke`, `setflat`,
  `flattenpath`, `setlinewidth`, `setlinecap`, `setlinejoin`,
  `setmiterlimit`, and `setdash` entries (§8.2) and §4.5.1; §7.5.2 was
  read and stroke adjustment is not applied. Steps, in order:
  1. *Flattening.* The stored path is in default user space, which is
     this backend's device space, so the flatness (device pixels,
     clamped to `setflat`'s range by the setter) applies to it directly:
     every cubic is halved at its parameter midpoint until both control
     points lie within the tolerance of the chord (the hull then bounds
     the curve's deviation from the chord), at most sixteen halvings
     deep. Straight segments are untouched. A `closepath` marks the
     polyline closed and starts a fresh one at the subpath's start, as
     the operator leaves the current point there.
  2. *Into the stroke's space.* Each polyline goes through the inverse
     CTM in double precision, runs of coincident points collapsed and a
     closed run's repeated end point dropped; a singular CTM yields an
     empty outline, as its stroke marks nothing.
  3. *Dashing*, in that space, where dash lengths are measured. A
     walker over the array starts at the state the phase reaches
     (cycling through the elements without marking; a negative phase
     counts back through the cycle) and cuts each polyline into "on"
     runs; a run that reaches a corner continues round it, so corners
     inside a dash are joined and only the run's ends are capped. A
     closed subpath whose start lies inside a dash has its last run
     joined to its first, and one with no gap at all stays one closed
     run. A dash of zero length is a one-point run that remembers its
     direction of travel.
  4. *Widening*, in that space, as a union of simple convex pieces
     rather than one contour per run: a rectangle per segment, a wedge
     on the outer side of each corner — the mitre quadrilateral
     (corner, the two offset points, the tip at `(n_a + n_b) /
     (1 + d_a·d_b)`) when `1/sin(φ/2) ≤ miterlimit`, else the bevel
     triangle; a fan of the pen's arc for a round join, or the whole
     disc when either adjoining segment is shorter than half the width,
     since the rectangles then leave part of the disc bare — and the
     cap beyond each open end: nothing, a half-disc fan, or a square of
     half the width. The pen's circle is cut so that its chords stay
     within the flatness on the page: the number of points is
     `π / acos(1 − f/R)` (at least four), with `R` the half width
     scaled by the CTM's largest singular value.
  5. *Degenerate pieces*, per the `stroke` entry: a subpath that had a
     segment but covers no distance (two coincident points, or a
     `closepath` after a lone `moveto`) is a disc with round caps and
     nothing with butt or projecting caps; a subpath of one `moveto`
     is nothing, so a trailing `moveto` leaves no mark. A zero-length
     dash inside a segment has a direction, so with projecting caps it
     is a square of the line width centred on the point (and a disc
     with round caps).
  6. *Back to the page.* Every piece goes through the CTM, is oriented
     counter-clockwise on the page (reversed when its signed area comes
     out negative, which a mirroring CTM causes), and becomes one
     closed subpath of `Move`, `Line`s, `Close`. Under the nonzero rule
     every piece adds to the winding, so overlaps are harmless and no
     inner-corner loop can cancel a region; this is why the output is
     many small rings rather than one contour per run. **Deviation from
     D1's wording** ("one closed polygon per dash/segment run"): a
     single contour would need the inner side's self-intersections
     resolved; the union of pieces fills identically and the manual
     permits any outline that fills as the stroke (`strokepath` says
     the result may hold interior segments and disconnected subpaths).
  The backend's `stroke_outline` builds the outline from the stored
  segments with the state's width, cap, join, mitre limit, dash,
  flatness, and CTM, and installs it as the current path
  (`Path::from_segments`, so the current point is the outline's last
  point and an empty outline leaves none); `strokepath` also drops a
  `setbbox` declared for the old path. `ustrokepath` (`ops/upath.rs`)
  is `newpath uappend strokepath` with, in the second form, the matrix
  concatenated between the walk and the outline and the CTM put back
  with `set_matrix` afterwards; unlike `ustroke` nothing is `gsave`d,
  since the outline is meant to remain, and a failed walk leaves the
  operands on the stack with the CTM untouched. The recording backend
  logs a `StrokeOutline` call. *Empty path, observed black-box:* the
  reference does not raise on `newpath strokepath`; afterwards
  `currentpoint` is `nocurrentpoint` and so is `pathbbox`, and this
  interpreter does the same (`strokepath-pathbbox.ps`, output `false`
  and `/nocurrentpoint` on both sides). The same file shows the one
  reading that differs: for a line 4 wide the reference's outline box
  is 5 tall (`7.5 … 11.5` where ours prints `8 … 12`) — its outline is
  widened by half a device pixel, the stroke-adjustment effect §7.5.2
  describes, while the `setlinewidth` entry gives half the line width
  each side, which is what the outline honours; the page verdict is
  `pass` and the printed difference is recorded here, as part 1 did for
  `arcto-acute-tangent.ps`.
- **Overprint in the IR (D3, emitter half) as built** — from the
  `setoverprint` entry and §4.8.5. `GState::overprint` is the value the
  VM last told (`set_overprint`), saved and restored with the state and
  kept by `initgraphics` and `showpage` like the VM's copy; `Emitted::
  overprint` is what the IR last set, starting `false` on a page and
  inherited by a glyph or form capture, `false` again in a pattern
  cell. `flush` records `IrOp::Overprint` first, before the colour and
  line parameters, whenever the state's value differs from the
  emitted one, for every kind of paint — fills, strokes, text, images,
  masks, form placements, and shade operations (`shade` now calls
  `flush(Needs::Nothing)`, which records overprint alone) — and a
  `Restore` brings the emitted value back with the rest, so a setting
  inside a clip is re-recorded after it when it still differs. The dump
  line is `op true` / `op false`. Every pre-existing IR golden is
  byte-identical (the default is never written). The backend test
  `overprint_is_emitted_where_it_changes_and_restored_with_the_clip`
  pins the dedup and the restore; the scenario
  `overprint_is_recorded_before_the_fills_it_bears_on` and
  `overprint-separation.ps` pin the order.
- **Overprint in the PDF (D3, writer half) as built** — from ISO
  32000-1 §8.4.4 (`gs`), §8.4.5 (Table 58: `OP`, `op`), and §8.6.7.
  `resources.rs` writes one `<< /Type /ExtGState /OP b /op b >>` per
  distinct value selected anywhere on the page (its operations, every
  cell, body, and glyph procedure, gathered through `Refs`), named
  `GS0` (off) and `GS1` (on), and lists them under `ExtGState` in the
  page's resource dictionary and in the dictionary of every content
  whose operations select one (`Refs::overprints`, walked by
  `collect`). The content writer emits `/GSn gs` where the IR sets the
  value. `SA` is not written (D3's alternative, kept). Every
  pre-existing PDF golden is byte-identical apart from part 1's listed
  `arcto-quarter-sweep.pdf`. The sink tests read the resource back
  from the page and from a form body's own dictionary.
- **Corpus (D7) as built** — `corpus/unit/graphics/`: `strokepath-
  cap-{butt,round,square}.ps`, `strokepath-join-{miter,round,bevel}.
  ps`, `strokepath-join-miter-limit.ps` (a corner of about 11°, a
  bevel under the default limit and the spike under 20, each as an
  outline page and a stroke page), `strokepath-dashed-curve.ps`,
  `strokepath-closed-rectangle.ps`, `strokepath-degenerate.ps` (dots
  with round caps; nothing with butt caps or from a lone `moveto`, so
  two blank pages), `strokepath-scaled.ps` (the pen under a 3×1
  scale), `strokepath-pathbbox.ps` (the box after `strokepath`, the
  empty path), `ustrokepath-plain.ps` and `ustrokepath-matrix.ps`
  (each dumping the same outline as the hand-built path; the matrix
  form compares the CTM before and after element by element, since
  the reference's default CTM is its device matrix), and `overprint-
  separation.ps` (`% oracle: colour`). Every outline file fills the
  outline on one page and strokes the same path on the next, so the
  oracle compares each against the reference's own fill and stroke.
  The rest of D7's list was already covered by part 1 (`pathbbox-
  control-points.ps`, `pathbbox-rules.ps`, `overprint-round-trip.ps`,
  `pagedevice-in-gstate.ps`, `pagedevice-merges.ps` for `save`/
  `restore`, `arcto-acute-tangent.ps`, `rand-seed-reproduces.ps`,
  `clocks-move-forward.ps`, the identity files); `pathbbox` after
  `flattenpath` waits for `flattenpath`, which does not exist.
- **Oracle, this round** (`difftest oracle --profile default` over
  `corpus/unit/{graphics,interp,identity}`, 36 dpi, limit 0.5 %;
  fractions are the differing share per page, outline page first):
  `strokepath-cap-butt` pass 0 / 0; `-cap-round` pass 0.00027 / 0;
  `-cap-square` pass 0 / 0; `-join-miter` pass 0 / 0.00135; `-join-
  round` pass 0.0001 / 0.00125; `-join-bevel` pass 0 / 0.00124;
  `-join-miter-limit` pass 0 / 0.00124 / 0 / 0.00165; `-dashed-curve`
  pass 0.00159 / 0.00012; `-degenerate` pass 0.00066 / 0.00026 / 0 /
  0; `-scaled` pass 0 / 0.00264; `-closed-rectangle` pass 0 / 0.00345;
  `-pathbbox` pass (no pages; output differs as recorded above);
  `ustrokepath-plain` pass 0.00027 / 0.00027; `ustrokepath-matrix`
  pass 0.00027 / 0.00027; `overprint-separation` pass 0 (colour).
  Every other file in the three directories kept its verdict (the
  two recorded expected divergences and the `% backend: none` skips
  included). The one verdict that needed inspection: `strokepath-
  closed-rectangle.ps` first failed at 0.527 % on its *stroke* page
  with a 340×300 rectangle — 638 pixels, all on the four axis-aligned
  edges, ours darker than theirs by one anti-aliasing step; our
  *outline* page was pixel-identical to the reference's stroke page
  and to its own outline page. The reference's document selects an
  extended graphics state with `SA false` for the stroke and ours
  writes no `SA`, so the rasteriser applies its default stroke
  adjustment to ours alone; the edge count scales with the perimeter,
  so the rectangle was reduced to 240×180 (0.345 %) rather than
  writing `SA`, which D3 leaves to viewers. A trigger for revisiting
  that alternative: a job whose axis-aligned strokes fail the oracle
  on this account.
- **Captured job:** `finder-sys608-lw70-print-directory.ps` under the
  host prelude, `pass`, output same.
- **Remaining gaps** (part 1's list, still open): `setpagedevice`'s
  implicit `initgraphics` and `erasepage`; `pathforall` reads through
  the single-precision inverse; `setbbox` inside `uappend` declares no
  box for `pathbbox`; `flattenpath` is undefined (its entry is read;
  the flattening routine is in `outline.rs` should it be wanted);
  `SA` is not written to the PDF. The generator does not yet emit
  `strokepath`.
- **Final gate record** (replacing part 1's): `cargo test --workspace`
  1183 passed, 0 failed, 3 ignored over 67 targets (new tests: 13 in
  `ps-graphics/tests/outline.rs`,
  2 backend, 2 scenario, 2 upath, 2 sink, 4 unit tests in `outline.rs`,
  and one name test in `resources.rs`); `cargo clippy --workspace
  --all-targets` 0 warnings; `cargo fmt --check` clean; `difftest run`
  337 files, 337 passed (322 at HEAD plus the 15 new ones), `git
  status --short corpus/golden` showing only added files beside part
  1's `arcto-quarter-sweep.pdf`; `parse-survival` corpus 337 files, 0
  failed; `fuzz-round` core 1300 and graphics 1300 programs, 0 failed;
  `lint-strings` 1145 files clean; `check-wasm` compiles for
  `wasm32-unknown-emscripten`; `cargo build --workspace
  --no-default-features` finished; `openspec validate triage-fixes-4`
  valid; the oracle and the captured job as above.
