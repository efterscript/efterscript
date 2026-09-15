# Design: CIE-based colour spaces

See proposal.md and the spec deltas. This document fixes what crosses
the boundary, how a CIE space is recognised as collapsible, how the
conversion to L*a*b* is evaluated inside the VM, how image samples are
converted, and what the writer emits.

## Context

- Colour crosses the boundary as a `SpaceSpec` and a component vector;
  tint transforms are captured as calculator source, never evaluated:
  there is no evaluator for PostScript procedures outside the
  interpreter loop, and the graphics layer must stay free of them.
- The interpreter runs a procedure and resumes an operator through loop
  frames (`ImageData`, `FilterData`, `PatternCell`); the image
  acquisition delivers whole samples before the backend sees them.
- The reference vault's manuals define CIE colour in PLRM3 §4.8.3 (the
  four families, Table 4.5–4.8, the L*a*b* transformation in Example
  4.11) and the PDF calibrated spaces in ISO 32000-1 §8.6.5 (`CalGray`
  Table 63, `CalRGB` Table 64, `Lab` Table 65). `CalGray` and `CalRGB`
  are single-stage special cases of the CIE-based A and ABC spaces;
  `Lab` is the two-stage L*a*b* case with fixed procedures.
- The reference converter renders CIE colour through a rendering
  dictionary to device colour; the oracle compares pixels, so exact
  agreement is not expected for converted colours.

## Goals / Non-Goals

**Goals:** calibrated colour preserved as calibrated colour; the
common driver-emitted spaces recognised structurally so their
components pass through untouched; everything else preserved exactly
via L*a*b*; no procedures across the boundary.

**Non-Goals:** profile-based spaces; applying rendering dictionaries;
`UseCIEColor` remapping; recognising the L*a*b* procedure shape
structurally (conversion reproduces it numerically instead).

## Decisions

**D1. Three boundary variants, chosen on the VM side.** `SpaceSpec`
gains `CalGray { white: [f32; 3], black: [f32; 3], gamma: f32 }`,
`CalRGB { white, black, gamma: [f32; 3], matrix: [f32; 9] }`, and
`Lab { white, black, range: [f32; 4] }`. The VM keeps the parsed CIE
dictionary (`CieSpace`: family, ranges, the decode procedures as
objects, matrices, table, white, black) in its own graphics-state
colour slot beside the boundary spec, so `currentcolorspace` returns
the original array and `currentcolor` the program's components while
the backend only ever sees one of the three PDF spaces and PDF-ready
components. *Alternative:* carry the CIE dictionary across the
boundary with captured procedure source — the writer cannot emit it
(PDF has no procedural CIE space) and the IR would carry code.

**D2. The collapse test is structural.** A stage is *identity* when
its decode procedures are absent or empty and its matrix absent or
the identity; it is *gamma-shaped* when each decode procedure is
exactly `{ n exp }` (optionally bound) with `n` a positive number, or
empty (gamma 1). `CIEBasedA` collapses to `CalGray` when the A stage is
gamma-shaped, `MatrixA` equals `WhitePoint` (within 1e-4), the LMN
stage is identity, and `RangeA`/`RangeLMN` are the defaults;
`CIEBasedABC` collapses to `CalRGB` when exactly one of the two stages
carries gammas and a matrix and the other is identity, with default
ranges; the pure XYZ space (both stages identity) is `CalRGB` with unit
gammas and the identity matrix. `DEF`/`DEFG` never collapse. The test
looks at procedure *objects* (two elements: a number and the operator
`exp`), so a `bind`-ed or unbound procedure both qualify. *Alternative:*
sample the procedures and fit a gamma — approximate, and it would
misclassify a piecewise sRGB curve as a gamma; the manual's sRGB shape
therefore goes to `Lab`, which is exact.

**D3. Conversion is evaluated in the VM at `setcolor`.** `setcolor` in a
non-collapsing CIE space pushes `LoopFrame::CieDecode { job }` whose
job lists the procedure calls to make (each decode procedure with its
clamped input) and collects results; each step runs one procedure as
a body frame and pops one number (`typecheck` otherwise); when all
results are in, the Rust side (`ops/cie.rs`) applies `Matrix*`, the
`RangeLMN` clamp, the table interpolation (multilinear over the
`Table` strings for `DEF`/`DEFG`, per PLRM3 Table 4.7/4.8), and the
XYZ→L*a*b* inverse relative to `WhitePoint` (the `g` function of
Example 4.11 inverted, with the linear branch below its knee), then
sets the boundary colour to the L*a*b* triple and re-dispatches
nothing (the operator is complete). The program's components are kept
for `currentcolor`. `setcolorspace` sets the initial colour the same
way (its conversion is the same job on the initial components).
*Alternative:* evaluate lazily at the first paint — every paint would
need the frame; converting at set time is simpler and `setcolor` is
already the operator that establishes the colour.

**D4. Images convert per sample, with caches.** After acquisition, an
image in a converting space runs one `CieDecode` job over the *set of
distinct input values per component* (at most 2^bits entries each;
sixteen-bit samples are reduced to eight before the cache so the job
stays bounded), then converts every sample natively through the cached
decode results, matrices, table, and XYZ→Lab, producing eight-bit
L*a*b* samples with `Decode [0 100 amin amax bmin bmax]`; the image is
then handed to the backend in the `Lab` space. Images in collapsed
spaces pass through unchanged with their original decode. *Alternative:*
convert nothing and emit the CIE dictionary — not expressible in PDF.

**D5. Rendering operators record, categories are regular.**
`setcolorrendering` validates `ColorRenderingType` 1 and stores the
dictionary in the graphics state (saved and restored); `find-
colorrendering` maps any intent name to `/DefaultColorRendering` and
`true`; the `ColorRendering` category holds one default instance (a
dictionary with `ColorRenderingType` 1 and the D65 white point) and
accepts defined ones with the same type check; `ColorSpace` is a plain
regular category. `UseCIEColor` is recorded by the page device like
other unknown keys and reported by `currentpagedevice`. The colour
getters in a CIE space return the device space's initial colour (PLRM3
§4.8.3), implemented in the existing getter path by matching the three
new variants.

**D6. IR and writer.** The three variants intern like any space; the
dump prints `cs n CalGray white=… gamma=…`, `CalRGB … gamma=… matrix=…`,
`Lab … range=…`; the writer emits `[/CalGray << /WhitePoint … /Gamma …
>>]`, `[/CalRGB << … /Gamma [...] /Matrix [...] >>]`, `[/Lab << …
/Range [...] >>]` (omitting `BlackPoint` when zero, `Gamma`/`Matrix`
when default), selected with `cs`/`CS` and set with `sc`/`SC`; an image
in `Lab` names it as its colour space with the decode array the VM
supplied. Numbers in the canonical real form.

**D7. Corpus.** `corpus/unit/cie/`: the spec scenarios, plus a
`CIEBasedA` that does not collapse (a non-white `MatrixA`) → `Lab`, an
sRGB-like piecewise space → `Lab` with a known colour, a `DEFG` space
with a 2×2×2×2 table, an image in a `CalRGB`-collapsing space (pass-
through) and in a `Lab`-converting space, a colour in a CIE space under
`save`/`restore`, the rendering operators and categories, the getters,
and the errors. IR and PDF goldens, the external checker on the PDFs.
Oracle: files that paint converted colour declare a tolerance or a
recorded divergence only after inspecting the pixel difference (the
reference renders through its rendering dictionary; small deltas are
expected and a large one is a bug of ours); `% backend: none` files
must pass.

## Risks / Trade-offs

- [XYZ→L*a*b* precision] → double precision in the conversion, rounded
  once to the canonical real form; the Lab round-trip scenario pins it.
- [The oracle disagreeing on converted colour] → compared by inspection
  first; a systematic small offset is registered as an expected
  divergence with the reason; anything else is investigated.
- [A decode procedure with side effects or that reads the job] → runs
  as a normal frame, so it can raise or suspend like any procedure;
  its results are cached per image, never across colours.
- [Sixteen-bit images reduced to eight before conversion] → a recorded
  limit; PDF `Lab` images are eight-bit in practice.

## Open Questions

- Whether `findcolorrendering` should look the intent up in the
  `ColorRendering` category before falling back to the default — decide
  by black-box observation and record it; it does not change the
  specs or tasks.
