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
— stroke adjustment is a raster hint the charter leaves to viewers;
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
