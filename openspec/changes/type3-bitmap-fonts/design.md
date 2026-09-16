# Design: Type 3 glyph metrics under a changed CTM, and resident fonts in FontDirectory

See proposal.md and the spec deltas. This document fixes where the
metric transform happens, what "glyph space" is in the capture, and
how `findfont` registers a face.

## Context

- The backend captures a Type 3 glyph between `begin_glyph` and
  `end_glyph`: geometry is taken back through the CTM in effect at
  `begin_glyph` (glyph space), so a `scale` inside the procedure is
  reflected in the captured marks. The VM passes the metric operands
  of `setcachedevice`/`setcharwidth` to `end_glyph` as given.
- Diagnosis on the captured driver job: the IR holds all 31 glyphs
  with correct marks; widths of about 16 glyph units and boxes 18
  units tall come from operands declared after a `Gnormsize dup scale`
  in the procedure, while the marks were painted at about one unit.
  The PDF writes those as `d1` and `Widths`, so the text spreads and
  the checker flags the boxes. A minimal font with `0.5 0.5 scale`
  before `setcachedevice` reproduces it.
- PLRM3 §5.4: metrics are in the glyph coordinate system; the
  `setcachedevice` entry: the operands are numbers in the glyph
  coordinate system. The reference interprets them through the CTM in
  effect at the call — the only reading under which the driver's font
  (and Example 5.6's) both work — and enters a found font in
  `FontDirectory` (observed: `/Helvetica findfont` makes
  `FontDirectory /Helvetica known` true).
- `findfont` today resolves resident faces (and substitutes) without
  touching `FontDirectory`; `definefont` is what writes it.

## Goals / Non-Goals

**Goals:** driver bitmap fonts distil with correct spacing and boxes;
metrics declared in a transformed space are carried into glyph space
exactly; resident faces appear in `FontDirectory` after `findfont`.

**Non-Goals:** a font cache; changing how marks are captured; the
`smooth4` branch of the driver's procedure.

## Decisions

**D1. Transform at the operator, on the VM side, using the backend's
CTM.** When `setcachedevice`, `setcachedevice2`, or `setcharwidth`
executes inside a glyph, the VM asks the backend for the current CTM
(`current_matrix`) and for the glyph-space CTM the capture recorded at
`begin_glyph` (new trait getter `glyph_matrix() -> Option<Matrix>`,
default `None`); the metric transform is `current × glyph⁻¹`: the width
vector goes through its delta transform, the box's four corners through
the full transform, then the axis-aligned envelope. The results in glyph
space are what the VM keeps for the advance and hands to `end_glyph`.
A singular glyph matrix (a degenerate font matrix) leaves the operands
as given. `stringwidth`'s measuring pass uses the same rule. *Alternative:*
transform in the backend at `end_glyph` — the backend would need the
CTM at the *call*, which only the VM knows at that moment; passing it
along is the same work with a wider interface.

**D2. `findfont` registers.** After resolving a key through the
resident set or substitution, `findfont` enters the resulting font
dictionary in `FontDirectory` (or `GlobalFontDirectory` in global
allocation mode) under the requested key, without inserting a new
`FID` if the dictionary already carries one; a key the program has
defined is found first as today. `resourceforall` over `Font` is
unchanged (it already lists the resident set). Substituted faces are
registered under the requested name, matching the reference's
observed behaviour, and the substitution report is unaffected.
*Alternative:* pre-registering every resident face at startup — the
manual says the directory holds fonts *loaded into VM*, and eager
registration would make a driver think 35 faces are loaded and
inflate every `FontDirectory` walk.

**D3. Corpus.** `corpus/unit/text/`: the scale-before-metrics
scenario; a synthetic bitmap font in the driver's shape (`save`,
normalisation scale, `setcachedevice`, `translate`, `scale`,
`imagemask` from a string, `restore`) with the project's own bitmaps
and numbers; a rotate-before-metrics case; metrics declared before any
change (unchanged goldens elsewhere); `setcharwidth` under a scale;
the `FontDirectory` scenario; the private tier re-runs the captured
bitmap-font job (`gs-test-data/printjobs/finder-sys608-lw70-print-
directory-bitmapfont.ps`) with the host prelude — its text extraction
must match the reference's and the checker must accept the font.

## Risks / Trade-offs

- [Existing Type 3 goldens change] → only fonts that transform before
  declaring metrics are affected; every existing golden is checked
  byte-identical and any change listed with its reason.
- [Registering substituted faces under the requested name] → matches
  the reference and the manual's "loaded into VM"; recorded.
- [`stringwidth` on such fonts] → it runs the procedure with the same
  transform rule, so widths agree with `show`.

## Open Questions

- None.
