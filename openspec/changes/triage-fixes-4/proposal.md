# Change: Triage fixes 4 — the recorded language gaps

## Why

Three rounds of triage and the generator left a list of small gaps
that each stop or skew a real job on its own: `strokepath` (and
`ustrokepath`) are undefined, so a job that clips or fills a stroke's
outline stops; `rand`, `srand`, `rrand`, `usertime`, and `realtime` are
undefined, so a job that seeds a pattern, times itself, or scatters
marks stops at the first call; `setstrokeadjust`, `setoverprint`, and
their getters are undefined, so any job that touches them stops,
and overprint — the one of those a distiller must preserve — is lost;
`languagelevel` still answers 2 although the interpreter now carries
shadings, CIE colour, user paths, and reusable streams; `serialnumber`
and the `systemdict` copies of the identity values are missing; the
`FontType` category does not list the CID-keyed font types the
interpreter accepts; the page device does not follow `restore` and
`grestore` as PLRM3 §6.1.1 requires; `pathbbox` does not honour the
control-point rule and a declared `setbbox`; and path readings such as
`arcto`'s tangent points differ from the reference in the sixth digit.
None warrants a change of its own, all are specified precisely, and
together they close the language-level gaps a LanguageLevel 3 job
expects, which is the bar the next milestone — the first harvested
LaserWriter 8 job — is measured against.

## What Changes

- **Stroke outlines** (`ps-vm`, `ps-graphics`): `strokepath` replaces
  the current path with the outline of the stroke the current
  parameters would draw (width, cap, join, mitre limit, dash,
  flattened by the flatness parameter), per its entry in PLRM3 §8.2;
  `ustrokepath` in both forms per its entry. The outline is a set of
  closed polygons suitable for `fill`, `clip`, and `pathbbox`.
- **Random numbers and clocks** (`ps-vm`): `rand`, `srand`, `rrand`
  with a documented generator of the project's own and a fixed initial
  seed; `usertime` counting interpreter execution deterministically;
  `realtime` answered by an embedder clock capability that defaults to
  the execution clock, so library output stays deterministic.
- **Stroke adjustment and overprint** (`ps-vm`, `ps-graphics`,
  `remelt`): `setstrokeadjust`/`currentstrokeadjust` recorded in the
  graphics state; `setoverprint`/`currentoverprint` recorded and
  carried to the IR as a state setting written to the PDF as an
  extended graphics state so separations overprint as the job asked.
- **Identity** (`ps-vm`): `languagelevel` answers 3; `serialnumber`,
  `version`, `product`, and `revision` are defined in `systemdict` with
  the configured identity's values; the `FontType` category lists 9
  and 11 beside the existing types.
- **Page device across state restoration** (`ps-vm`): the page device
  dictionary is part of the graphics state, so `grestore`,
  `grestoreall`, and `restore` bring back the page device in effect at
  the matching `gsave`/`save`, per PLRM3 §6.1.1.
- **Path readings** (`ps-vm`, `ps-graphics`): `pathbbox` encloses curve
  control points, honours a declared `setbbox`, and ignores a trailing
  `moveto`, per its entry; `arcto`'s tangent points, `currentpoint`,
  and `pathbbox` are computed in double precision through the CTM and
  its inverse.
- Out of scope, with triggers: stroke adjustment applied to geometry
  (never — a raster concern); `setoverprintmode`/`OPM` (when a job
  uses it); `realtime` wall-clock accuracy from the command line
  (the CLI may supply a real clock later); exact agreement of
  `strokepath` outlines with the reference's (the fill they produce is
  compared, not the segments).

## Capabilities

### New Capabilities
- none.

### Modified Capabilities
- `interpreter-core`: ADDED requirements for the random-number
  operators and the clocks; MODIFIED `languagelevel` to answer 3.
- `graphics-ir`: ADDED requirements for stroke outlines, stroke
  adjustment and overprint in the state, the page device across
  restoration, and the `pathbbox` rules with double-precision readings.
- `remelt`: ADDED requirement for overprint in the PDF.
- `printer-identity`: ADDED requirement for the `systemdict` identity
  entries and the CID font types in the `FontType` category.

## Impact

- Code: `crates/ps-vm` (`ops/arith.rs` or a new `ops/random.rs`, a
  `Clock` capability beside `FileCapability`, `ops/graphics.rs` for
  the new state operators and `pathbbox`, `ops/pagedevice.rs` and the
  graphics-state stack for the page device, `ops/status.rs` and
  `interp/mod.rs` for identity, `ops/resource.rs` for the category,
  `ops/upath.rs` for `ustrokepath`), `crates/ps-graphics` (`strokepath`
  outline construction in a new `outline.rs`, the overprint state
  setting and `IrOp::Overprint`, double-precision readings, dump),
  `crates/remelt` (extended graphics state resources and `gs`),
  `crates/efterscript-cli` (may install a real clock later; not now),
  `tools/psgen` (may add the new operators to its grammar; deferred),
  corpus under `corpus/unit/{graphics,interp,identity}/` with goldens.
- No new dependencies.
- Depends on `patterns-and-forms` (user paths), `printer-identity-
  mechanism`, `shading-patterns`, archived.
