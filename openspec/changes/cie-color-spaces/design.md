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
twelve-bit samples — the deepest the operator accepts — are reduced to
eight before the cache so the job stays bounded), then converts every
sample natively through the cached decode results, matrices, table,
and XYZ→Lab, producing eight-bit L*a*b* samples with `Decode [0 100
amin amax bmin bmax]`; the image is then handed to the backend in the
`Lab` space. Images in collapsed
spaces pass through unchanged with their original decode. *Alternative:*
convert nothing and emit the CIE dictionary — not expressible in PDF.

**D5. Rendering operators record, categories are regular.**
`setcolorrendering` validates `ColorRenderingType` 1 and stores the
dictionary in the graphics state (saved and restored); `find-
colorrendering` composes the name of §7.1.3 and looks it up in the
`ColorRendering` category, answering that name and `true` when it is
held and `/DefaultColorRendering` with `false` otherwise (settled by
observation, see the implementation notes; the draft said `true` for
every intent); the `ColorRendering` category holds one default instance (a
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
- [Twelve-bit images reduced to eight before conversion] → a recorded
  limit; PDF `Lab` images are eight-bit in practice.

## Open Questions

- Whether `findcolorrendering` should look the intent up in the
  `ColorRendering` category before falling back to the default —
  settled by observation in part 1 (it does, under the composed name;
  see the implementation notes).

## Implementation notes

Recorded where the code departs from, or pins down, the text above.
Part 1 covered tasks 1.1, 2.1, 2.2, 2.5, and 3.1: the boundary types,
the four family dictionaries with the range clamp, the structural
collapse, the rendering operators and the two categories, and the
writer. Provenance: PLRM3 §4.8.3 (Tables 4.5–4.8 and the surrounding
text, the examples read for the shapes only), §3.9.2 (Table 3.7),
§6.2.5 (Table 6.6, `UseCIEColor`), §7.1.1–7.1.3 (Table 7.1 for the
rendering dictionary's required entries, Table 7.2 for the intents),
and the §8.2 entries for `setcolorspace`, `setcolor`, `currentcolor`,
`currentcolorspace`, `currentgray`, `currentrgbcolor`,
`currenthsbcolor`, `currentcmykcolor`, `setcolorrendering`,
`currentcolorrendering`, `findcolorrendering`, and `currentpagedevice`;
ISO 32000-1 §8.6.5 (Tables 63–65) and §8.6.8 (Table 74); and black-box
observation of the reference converter through `difftest oracle` with
scratch programs that never entered the repository ("observed" below).

- **D1 as built.** `SpaceSpec::CalGray { white, black, gamma }`,
  `CalRGB { white, black, gamma: [f32; 3], matrix: [f32; 9] }`, and
  `Lab { white, black, range: [f32; 4] }` in `ps-vm::graphics`, arity
  1/3/3, families `CalGray`/`CalRGB`/`Lab`, initial colour zero (a Lab
  space whose range excludes zero takes the nearest value). A new
  `SpaceSpec::component_limits(index)` gives the interval a component
  is clamped to — `[0, hival]` for Indexed, `[0, 100]` and the a*/b*
  range for Lab, the unit interval otherwise — and the graphics crate's
  `GState::clamped` uses it in place of its former Indexed-or-unit
  rule; nothing else in ps-graphics changed for the spaces, which intern
  structurally like the others. The dump prints `cs n CalGray white=x y
  z black=x y z gamma=g`, `cs n CalRGB white=… black=… gamma=g g g
  matrix=m1 … m9`, `cs n Lab white=… black=… range=amin amax bmin bmax`
  in the canonical real form; every existing IR golden is byte-identical.
- **Where the CIE space lives (D1's "VM-side colour slot").** The VM
  keeps parsed spaces in `Interp::cie_spaces`, an append-only table of
  `CieEntry { array, space: Option<CieSpace>, collapsed }` indexed by
  the colour-space array object (so a space set repeatedly from one
  object is one entry, on the `graphics_procs` argument: an entry a
  `restore` invalidated is never looked up, its graphics state having
  been restored too). The graphics state carries only a `CieColor {
  space: u32, components: [f32; 4] }` — the entry's id and the
  program's clamped components — through two new trait hooks,
  `set_cie_color`/`current_cie_color`, shaped like `set_pattern`/
  `current_pattern`: the backend stores the value beside the colour,
  clears it in `set_color_space`, keeps it through `set_color` and
  `set_pattern`, and saves and restores it with the state; a backend
  that keeps nothing accepts and ignores it. `setcolorspace` on an
  array that names a CIE family, *or merely contains one* (as the base
  of an Indexed or Pattern space, `[/Indexed [/CIEBasedABC …] …]`, which
  the reference accepts, observed), attaches the entry; `currentcolorspace`
  answers with the entry's array — the very object the program gave,
  so `cs setcolorspace currentcolorspace cs eq` is `true` where the
  reference answers `false` with a copy (observed; no corpus file tests
  `eq`) — and `currentcolor` with the program's components. The
  `space_object` rebuild for the three variants (a `CIEBasedA`/`ABC`
  dictionary with gamma procedures and the matrix; for `Lab` the ranges
  and points only) is reachable only through a backend without the
  hooks. `parse_space` now takes `&mut Interp` (dictionary access).
- **`CieSpace` as built** (`ops/cie.rs`, `pub(crate)`): `family`,
  `white`, `black`, `range_abc` (`RangeA`, two numbers, or `RangeABC`,
  six), `decode_abc` (`DecodeA` as a one-procedure vector, or
  `DecodeABC`; `None` when absent), `matrix_abc` (`MatrixA`, three, or
  `MatrixABC`, nine), `range_lmn`, `decode_lmn`, `matrix_lmn`, and
  `table: Option<TableStage { range_def, decode_def, range_hij, dims,
  data }>` for `DEF`/`DEFG`, the table's strings copied and
  concatenated in index order (entry `(h, i, j[, k])` at `3 × (((h × NI
  + i) × NJ + j) × NK + k)`). Defaults per Tables 4.5–4.8. Errors, all
  observed to match the reference unless noted: the array must be
  `[family dict]` — no parameter or more than one `rangecheck`, a
  non-dictionary `typecheck`; `WhitePoint` absent `undefined`, not an
  array of three numbers `typecheck`/`rangecheck`, Y not exactly 1
  (1.0001 is refused) or X or Z not positive `rangecheck`; `BlackPoint`
  negative `rangecheck`; a `Range*` pair with minimum above maximum
  `rangecheck`, a wrong length `rangecheck`, a non-array `typecheck`; a
  `Decode*` entry not an array `typecheck`, of the wrong count
  `rangecheck` (`DecodeABC {}` is an empty array of procedures, so
  `rangecheck`, as observed), holding a non-procedure `typecheck`;
  `DecodeA` must be one procedure (`[{}]` is `typecheck`); a `Matrix*`
  of the wrong length `rangecheck`; `Table` absent `undefined` — *the
  reference answers `rangecheck`*, the spec delta says `undefined`, the
  spec is followed and no corpus file exercises it — not an array
  `typecheck`, a wrong element count `rangecheck`, a dimension not an
  integer `typecheck`, below 2 `rangecheck`, a string of the wrong
  length or a row of the wrong count `rangecheck`, a non-string
  `typecheck`. The dictionary is left as it is (not made read-only; the
  reference leaves it writable, observed).
- **Clamp, initial colour, getters.** `setcolor` in a CIE space takes
  the family's component count (1/3/3/4), clamps each to its range —
  `RangeDEF(G)` for the table families, `RangeA`/`RangeABC` otherwise —
  without error (observed: `2 setcolor` in `RangeA [0 0.5]` reads back
  0.5, `-1` reads 0; `RangeDEFG [0 2 …]` keeps a 2), keeps the clamped
  components in the `CieColor`, and hands the boundary the components
  unchanged for a collapsed space or through the placeholder below. The
  initial colour is the clamp of zeros (`RangeA [0.5 1]` starts at 0.5,
  `[-2 -1]` at −1, both observed). `currentgray`, `currentrgbcolor`,
  `currenthsbcolor`, and `currentcmykcolor` fall to the existing
  "other space" arms: 0, `0 0 0`, `0 0 0`, and `0 0 0 1` — unchanged
  from what Separation and Indexed answer, and what the reference
  answers in a CIE space (observed, including the `1` for black).
- **D2 as built: the collapse.** `CieSpace::collapse(&Interp)` inspects
  procedure *objects*: a decode procedure is gamma-shaped when its body
  is exactly two objects, a finite positive number and `exp` — the
  operator object (`bind`ed) or the executable name — and the empty
  procedure is gamma 1; `{2.2 exp pop 1}`, `{-2 exp}`, `{0 exp}`,
  `{exp}`, `{2.2 /exp}`, and `{2.2 mul}` are not gammas. A stage is the
  identity when its procedures are absent or all empty and its matrix
  absent or the identity within 1e-4. `CIEBasedA` → `CalGray` when the
  A procedure is gamma-shaped, `MatrixA` is the white point within
  1e-4, and the LMN stage is the identity; `CIEBasedABC` → `CalRGB`
  from the ABC stage when the LMN procedures are the identity (the
  gammas from `DecodeABC`, the matrix `MatrixABC` followed by
  `MatrixLMN`, composed in double precision — the identity when both
  are absent, which is the pure XYZ space), else from the LMN stage
  when the ABC stage is the identity; `DEF`/`DEFG` never. *Ranges,
  refined from "the defaults":* `RangeA`/`RangeABC` must lie within the
  unit interval (the reader of a calibrated space clamps components
  there, and our `setcolor` has already clamped to the narrower range,
  so nothing changes) — the spec's XYZ scenario gives `RangeABC [0
  0.9505 0 1 0 1]`, which the literal rule would have refused; and
  `RangeLMN` must be the default, or an explicit range that contains
  the first stage's output outright (the decoded range box through the
  matrix, a cheap interval computation), so that the clamp the manual
  places between the stages can never act. The default is admitted
  without that check because a program that does not mention
  `RangeLMN` does not rely on the clamp, and the reference carries such
  a space as calibrated (observed: `MatrixABC` primaries with the
  default `RangeLMN` and with a wide one produce the same `CalRGB`
  dictionary in its output; a `CIEBasedA` and the LMN-stage form become
  a profile-based space there, where this interpreter writes `CalGray`
  and `CalRGB`). *Matrix ordering:* PLRM3 Table 4.5 lists `MatrixABC`
  as three elements per input component (`L = A·LA + B·LB + C·LC`), and
  ISO 32000-1 Table 64 lists `Matrix` the same way (`X = XA·A + XB·B +
  XC·C`); the elements are copied in order, no transposition.
- **Non-collapsing spaces** were carried through part 1 with their
  clamped components passed to the boundary unconverted; part 2's
  conversion (below) replaced that, and the image `Decode` of a
  collapsed space is resolved there too.
- **D5 as built: rendering.** `setcolorrendering`,
  `currentcolorrendering`, and `findcolorrendering` are public
  operators, defined with or without a backend like the screen
  operators. The dictionary is held by `ProcRef` (the transfer
  mechanism: `Interp::graphics_proc_ref` already keyed dictionaries) in
  a new `GState::color_rendering: Option<ProcRef>` behind
  `set_color_rendering`/`color_rendering` hooks, `None` meaning the
  default instance; kept across `initgraphics` like the screens; a VM
  without a backend keeps its own slot. `setcolorrendering` checks the
  three entries Table 7.1 marks required: `ColorRenderingType` an
  integer (`1.0` is `typecheck`) equal to 1 (2 is `rangecheck`), a
  valid `WhitePoint`, and `TransformPQR` as three procedures; an absent
  one is `undefined`, a non-dictionary `typecheck` (all observed; the
  reference demands more — `<< /ColorRenderingType 1 /WhitePoint W >>`
  alone is `undefined` there too, and with `TransformPQR` it is
  accepted, observed — and the optional entries are not looked at,
  since nothing applies the dictionary). `currentcolorrendering`
  returns the object set (`eq` to it, observed) or the default instance.
  *`findcolorrendering`, the open question, settled by observation:*
  the reference composes `intent.deviceconfig.halftone` from the
  operand (name or string), the page device's `PageDeviceName` or
  `none`, and `none`, and answers that name with `true` when the
  `ColorRendering` category holds it — defining `/Perceptual` alone
  changes nothing, `/Perceptual.none.none` is found, and after `<<
  /PageDeviceName /Foo >> setpagedevice` it looks for
  `Perceptual.Foo.none` — else `/DefaultColorRendering` with `false`.
  This is built as observed (the halftone part is always `none`; the
  page device's `PageDeviceName` is consulted when it is a name or
  string), and the design's and spec delta's "and `true`" were amended:
  the entry makes `false` mean an alternate is proposed. A non-name
  operand is `typecheck`.
- **The categories.** `ColorRendering` and `ColorSpace` are regular
  (`Interp::color_rendering_category`, `color_space_category`, a local
  and a global dictionary each). `ColorRendering` has the one built-in
  `DefaultColorRendering`: a read-only dictionary in global VM built on
  first use with `ColorRenderingType 1`, `WhitePoint [0.9505 1 1.089]`
  (D65-like), and `TransformPQR` as three copies of a procedure that
  discards the four point arrays and returns the component — the
  minimum Table 7.1 requires (the reference's carries eight entries,
  observed); it reports status 0 like the reference's (observed), is
  listed after the program's definitions by `resourceforall`, and
  survives `undefineresource` (observed). `defineresource` runs
  `cie::check_rendering_dict` with `typecheck` for an absent entry, as
  the Pattern and Form categories do — the reference accepts any
  dictionary (observed), so the check is this interpreter's; no corpus
  file exercises it against the oracle. A `ColorSpace` instance must be
  an array (packed included); a name, number, or dictionary is
  `typecheck` — observed, against the part brief's "an array or a
  name" — and the array's content is not checked (the reference defines
  `[/Bogus 1 2]`, observed). `COLOR_SPACE_FAMILIES` has the four CIE
  names (eleven entries; the reference also lists two families this
  interpreter does not carry), `CATEGORIES` sixteen, and the identity
  test counts them. `UseCIEColor`: no code change — `setpagedevice`
  records it like any unrecognised key and `currentpagedevice` reads it
  back; the reference accepts the key too, so the corpus file needs no
  divergence, and it accepts a non-boolean value as well (observed), so
  the key was not added to the typed set.
- **D6 as built: the writer.** `remelt::resources::Form` gained the
  three variants; the arrays are `[/CalGray << /WhitePoint [..] >>]`
  with `/BlackPoint` when non-zero and `/Gamma` when not 1, `[/CalRGB
  << … /Gamma [..] when not [1 1 1], /Matrix [..] when not the identity
  >>]`, `[/Lab << … /Range [..] when not [-100 100 -100 100] >>]`, as
  `/CSn` resources selected by `cs`/`CS`; colours are set with
  `scn`/`SCN`, the operators the writer already uses for every named
  space (Table 74 admits both `sc` and `scn` for the calibrated
  spaces). The reader's default image decode for `Lab` (`[0 100 amin
  amax bmin bmax]`, Table 90) is known to `default_decode`, so a `Lab`
  image writes `/Decode` only when its array differs. Every existing
  PDF golden is byte-identical.
- **Corpus, part 1.** `corpus/unit/cie/`: `calibrated-rgb-selected`,
  `white-point-required`, `components-clamped` (the three spec
  scenarios, *with* the backend — `setcolorspace` belongs to the
  graphics group, which is absent without one, so the part brief's
  `% backend: none` could not apply; they paint nothing and carry no
  goldens), `gamma-gray`, `lmn-stage-rgb`, `xyz-space` (IR and PDF
  goldens), `rendering-recorded` (goldens: a plain gray fill),
  `families-listed` and `usecieccolor-recorded` (`% backend: none`), and
  `colorspace-category`. Oracle tier (`difftest oracle --profile
  default corpus/unit/cie`): 10 files, 10 pass, 0 fail, 0 skipped,
  output same for all 10; the four painting files compare pixels with a
  differing fraction of 0 at 36 dpi — the reference renders the
  collapsed spaces through its rendering dictionary and this
  interpreter's document through the rasteriser's calibrated-space
  handling, and the gray renders agree exactly. Note the renders are
  gray, so a chromatic difference would not show at this tier.
- **Tests.** `crates/ps-vm/tests/cie.rs` (selection and getters, the
  clamp and `currentcolor`, `currentcolorspace` returning the original
  through `gsave`/`grestore`, `save`/`restore`, and inside Indexed and
  Pattern arrays, every dictionary error above, the gray and RGB
  collapse shapes bound and unbound with the rejected forms and the
  range rules, the rendering operators with and without a backend and
  their errors, `findcolorrendering`'s composed name, the two
  categories, `UseCIEColor`), the unit tests in `ops/cie.rs` (stage
  images, the LMN admission, matrix composition, decoded ranges, the
  placeholder), `graphics.rs` (arity, limits, initial colours),
  ps-graphics `state.rs` (Lab clamp, the attached colour across a
  colour change and `initgraphics`) and `dump.rs` (the three lines,
  nested), remelt `tests/sink.rs` (the arrays read back with their
  omission rules; the Lab image decode).

Part 2 covered tasks 2.3, 2.4, 4.0, 4.1, and 4.2: the conversion frame
and job, the inverse transformation, the table interpolation, image
and lookup-table conversion, the colour oracle directive, the rest of
the corpus, and the gates. Provenance: PLRM3 §4.8.3 (the two-stage
formulas of Table 4.5, the forward L*a*b* transformation of Example
4.11 — read for the formulas, its dictionary not copied — Table 4.6,
the `Table` layout and interpolation description of Tables 4.7 and
4.8), §4.8.4 (a string lookup's bytes as unit-range components),
§4.10.5 (sample decoding and Table 4.21); ISO 32000-1 §8.6.5.4 (Table
65 and the L*a*b*→XYZ formulas, inverted here), §8.6.6.3 (Indexed
bytes scaled to the base's ranges), §8.9.5.2 (`Decode`, Table 90); and
black-box observation of the reference converter through `difftest
oracle` with scratch programs that never entered the repository.

- **D3 as built: the frame and the job.** `LoopFrame::CieDecode {
  job: Box<CieJob> }`. A `CieJob` (`ops/cie.rs`) holds the space (an
  `Rc<CieSpace>` — `CieEntry.space` became `Rc` too, where part 1
  cloned the whole space, table included, on every `setcolor`), the
  operator to blame, a stage — `Table` (the `DEF`/`DEFG` families'
  `DecodeDEF(G)`), `Abc` (`DecodeA`/`DecodeABC`), `Lmn` — the values in
  flight (`width` per colour: one colour for `setcolor`, every sample
  of an image, every entry of a lookup table), the stage's calls, and
  the results. Preparing a stage clamps the inputs to the stage's
  ranges (`RangeDEF(G)`, `RangeABC`, `RangeLMN`) and lists, per
  component whose procedure is not empty, the distinct inputs
  ascending; an empty procedure is the identity and is never called.
  The loop step takes the number the procedure in flight left
  (`typecheck` for anything else, `stackunderflow` for nothing),
  truncates the operand stack to the depth beneath the input (a
  procedure leaving more than its result is cut back), delivers it,
  and issues the next call with its input pushed as a real; when a
  stage's results are in, the job completes it natively — the table
  lookup, `MatrixABC` (three elements per input, `MatrixA` likewise
  with one input), or `MatrixLMN` followed by XYZ→L*a*b* — into the
  next stage's inputs, and the frame finishes the operator: the
  boundary colour and the attached `CieColor` are set together at the
  end, so a procedure that raises leaves the colour as it was and the
  operand stack with the error's residue, as any operator error inside
  a procedure would (the operands were consumed before the frame; a
  test pins that the residue equals the procedure's run by itself).
  `start_job` runs a job with no calls to make — a table space without
  procedures, or all-empty procedures — to completion without a frame.
  `setcolorspace` runs the same job on the initial colour; the
  `CieColor` is attached before it so `setcolor` afterwards finds the
  space even if the initial conversion raised. The frame's references
  are the space's procedures (and the Indexed array), its body the
  procedure due next.
- **The inverse transformation and its precision.** Everything after
  the procedures is `f64`, rounded once to the boundary's `f32`. The
  inverse of `g`: the cube root above `(6/29)³`, the line `841/108 · y
  + 4/29` below it (continuous at the knee); then L* = 116·f(Y/Yw) −
  16, a* = 500·(f(X/Xw) − f(Y/Yw)), b* = 200·(f(Y/Yw) − f(Z/Zw)); L*
  clamped to `[0, 100]`, a* and b* to the Lab range, a NaN component
  read as zero. The a*/b* range is `[-128 127 -128 127]` (part 1 had
  the reader's default `[-100 100 -100 100]`): a monitor-like blue
  reaches b* ≈ −109, and an eight-bit sample then steps one unit; the
  writer emits `/Range` since it is not the default. The Lab-shaped
  round-trip scenario returns `50 20 -30` within 0.05 (the procedures
  compute in the language's single precision), and the unit tests pin
  hand-computed values on both branches within 2·10⁻³.
- **The table interpolation.** `TableStage::lookup`: each input
  clamped to its `RangeHIJ(K)` pair, which the lattice spans evenly;
  the lower lattice index and fraction per dimension; a multilinear
  sum over the 2ⁿ surrounding entries' bytes (entry `(h, i, j[, k])`
  at `3 × (((h × NI + i) × NJ + j) × NK + k)` of the concatenated
  strings); the byte scale mapped onto `RangeABC`. Pinned by unit
  tests on corners, edge midpoints, the centre, out-of-range inputs,
  a non-unit `RangeABC`, and a four-dimensional corner.
- **D4 as built: images.** After acquisition (`ops/image.rs::finish`)
  an image whose space is a non-collapsing CIE entry — captured on the
  acquisition at `image` time, so a data procedure changing the space
  cannot redirect it — is unpacked to integers, twelve-bit samples
  shifted to eight, decoded through the image's `Decode` (the space's
  ranges when the dictionary has none, per Table 4.21's remark that
  the arrays depend on the space's parameters), and run through a job
  whose first stage calls each procedure at most once per distinct
  sample value (≤ 2^bits ≤ 256, tested with a counting procedure);
  later stages take their distinct intermediate values exactly when
  there are at most 4096 per component and otherwise snap them to a
  4096-point grid over the stage's range first (tested: 16384 distinct
  LMN values make about a thousand calls). The result is eight-bit
  L*a*b* with `Decode [0 100 -128 127 -128 127]` in the `Lab` space;
  the writer omits the array as the reader's default. While the
  samples are acquired the spec carries the device space of the
  family's component count, not the boundary `Lab` — the byte count
  of a one- or four-component image was otherwise computed for three
  and the image truncated (found by the A-space test). Images in a
  collapsed space pass through with their decode, the default being
  the ranges: a collapsed space with `RangeABC [0 0.9505 0 1 0 1]`
  writes that as `/Decode`, so the reader maps the samples onto the
  range as `setcolor` would. *Deferred:* data that arrived DCT-encoded
  in a converting space cannot be converted and is handed on as it is
  in the stand-in device space, in the wrong colours — trigger: a job
  with a JPEG image in a CIE-based space.
- **Indexed over a CIE base.** Built: `[/Indexed [/CIEBased… dict]
  hival string]` over a non-collapsing base runs a job over the
  table's entries (bytes as unit-range components, PLRM3 §4.8.4,
  clamped to the base's ranges) and, when it completes, sets an
  Indexed space over the `Lab` base whose lookup holds the L*a*b*
  bytes over `[0 100]` and the Lab range (ISO 32000-1 §8.6.6.3), and
  registers the array so `currentcolorspace` answers with it. Over a
  collapsed base the lookup passes through (the bytes are the
  components). *Deferred:* a Separation or DeviceN whose alternate is
  a converting space, and an uncoloured pattern whose base is one —
  the tint transform's outputs and the pattern's components reach the
  `Lab` boundary space unconverted; triggers: a job painting such a
  separation where a reader falls back to the alternate, or an
  uncoloured pattern in a CIE space.
- **Downsampling in `remelt`.** A `Lab` image is an eight-bit
  three-component image, so it falls into the colour class and is
  averaged or subsampled like an RGB one; averaging the encoded bytes
  averages L*, a*, and b* linearly, which is as good a reduction as
  the RGB case. Left as it is, with a test pinning the class.
- **The colour oracle directive (task 4.0).** The profile grammar
  gained `render_color`, optional, with the placeholders of `render`
  validated the same way; a file carrying `% oracle: colour` has both
  documents rendered by it and its pages compared channel by channel
  by the existing `P6` comparison with the same threshold and limit,
  noted `compared in colour`; when the profile has no such rasteriser
  the file is reported `skipped` with a note naming the missing key.
  Files without the directive keep the grey path; the skip-list
  self-test is unchanged. Tests: the profile with and without the
  key, the placeholders, the directive beside `skip`, and an oracle
  self-test through a fake colour rasteriser.
- **Corpus, part 2.** `lab-round-trip`, `table-def`, `table-defg`,
  `converted-image`, `pass-through-image`, `gray-a-lab`,
  `monitor-like-lab`, `save-restore`, `indexed-cie-base` (IR and PDF
  goldens, `% oracle: colour`), and `missing-table-undefined` (the
  divergence below); the directive was added to part 1's four painting
  files. The monitor-like space uses this interpreter's own numbers (a
  linear toe under a 2.2 power curve, primaries summing to the white
  point). None of part 1's goldens changed (byte-identical after
  regeneration); eighteen goldens were added.
- **What the reference does, observed while choosing the corpus
  colours.** (1) A `CIEBasedA` component at the top of its range
  renders rolling off to white: with `MatrixA` half the white point,
  0.9 renders as this interpreter's 170, 0.95 as 178, 0.99 as 221,
  0.999 as 248, and 1.0 as 255, where the colorimetric value (L* 76)
  is 187; `gray-a-lab` therefore sets 0.9, not 1. (2) With `RangeA [0
  2]` it clamps the decoded value to the unit interval (1.2 and 1.4
  render as 1). (3) A `CIEBasedDEFG` colour of all zeros renders
  white, a four-component space apparently treated as ink-like;
  `table-defg` uses the corner `(0, 1, 0, 1)` instead. Each is the
  reference's rendering path; this interpreter writes the exact
  conversion.
- **Divergences.** `cie-missing-table-undefined`: kept `undefined`
  (the spec delta; a missing `WhitePoint` is `undefined` on both
  sides) where the reference raises `rangecheck`; the harness reports
  the file `divergence-closed` because its output comparison stops at
  the error marker, before the error names differ.
  `cie-rendering-path`, on `converted-image` and `indexed-cie-base`:
  the monitor-like blue (XYZ (0.18, 0.07, 0.95) relative to the white
  (0.95, 1, 1.09)) is L* 31.8, a* 81.1, b* −108.6 by hand, written as
  the bytes 81, 209, 19; its expected sRGB is (0, 0, 255), which the
  reference's own path produces from the monitor-like space (and (39,
  0, 254) from the Lab-shaped space for the same XYZ — its path varies
  by that much on one colour); rendering the written `Lab` it produces
  (91, 0, 253), so the blue pixel differs by 89/255 in one channel,
  2.063 % of the image page and 4.126 % of the Indexed page at 36 dpi;
  red, green, greys, and the moderate colours of the other files agree
  within 25/255. Registered as the reference's rendering of the `Lab`
  space, this interpreter's values being the exact conversion.
- **Oracle tier, part 2** (`difftest oracle --profile default
  corpus/unit/cie`): 20 files, 17 pass, 0 fail, 2 expected-divergence,
  1 divergence-closed, 0 skipped; output same for all 20. Fractions:
  0 for `gamma-gray`, `save-restore`, `gray-a-lab`, `table-def`,
  `table-defg`, `monitor-like-lab`, `pass-through-image`, `xyz-space`,
  `lab-round-trip`, and `rendering-recorded` (every compared pixel
  within the 48 threshold), 0.00245 for `lmn-stage-rgb` (the stroked
  rectangle's edges, within the limit), 0.02063 for `converted-image`
  and 0.04126 for `indexed-cie-base` (the blue above); the
  non-painting files compare no pixels. The captured driver job
  (`finder-sys608-lw70-print-directory` under the host prelude): pass,
  output same.
- **Tests, part 2.** `tests/cie.rs`: the Lab-shaped round trip (both
  branches of `g`), the table space at corners and between them, the
  non-white `MatrixA`, non-number and stack-underflow results, a
  raising procedure (colour and residue), a procedure leaving extra
  objects, `save`/`restore` and `gsave`/`grestore` with converted
  colour, the two-pixel image by hand, the per-component call bound
  for 8-, 4-, and 12-bit images, the second-stage grid, pass-through
  images with the range as default decode, and the Indexed lookup;
  `ops/cie.rs` unit tests: `g` inverse on both branches and at the
  knee, XYZ→L*a*b* against hand-computed values and its clamps,
  matrices, the 3-D and 4-D table lookup, the L*a*b* bytes and decode,
  sample unpacking with row padding; `remelt/downsample.rs`: the Lab
  class; difftest: the profile key, the directive, and the colour
  self-test.
- **Final gate record** (replacing part 1's): `cargo test --workspace`
  1092 passed, 0 failed; `cargo clippy --workspace --all-targets` 0
  warnings; `cargo fmt --check` clean; `difftest run` 282 files, 282
  passed (`git status --short corpus/golden` shows added files only);
  `parse-survival` 282 files, 0 failed, errors none; `fuzz-round` core
  1300 programs 0 failed, graphics 1300 programs 0 failed;
  `lint-strings` 990 files clean of 16 listed strings; `check-wasm`
  compiles for wasm32-unknown-emscripten; `cargo build --workspace
  --no-default-features` ok; `openspec validate cie-color-spaces`
  valid; the oracle tier and the captured job as above.
- **`cie-missing-table-undefined` withdrawn.** The slug was registered
  for the reference raising `rangecheck` where this interpreter raises
  `undefined` for a table family without its `Table`, but the harness
  compares only that both sides ended in an error, not which, so the
  oracle reported the divergence closed. An unobservable divergence is
  noise in the registry: the requirement and the file's header were
  removed, the observation stays here, and the file compares as a plain
  pass. If the harness ever compares error names, register it then.

