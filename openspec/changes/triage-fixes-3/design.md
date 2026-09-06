# Design: Triage fixes 3

See proposal.md. Seven small items; the decisions below are the ones
with a choice in them.

## Context

- The content writer maps the IR's colour settings to `g`/`rg`/`k`/
  `scn` with `cs` for named spaces; nothing sets the stroking colour,
  so strokes use PDF's initial black.
- `clip` with an empty path records a clip with no segments; the
  emitter and the writer then behave as if no clip existed.
- The operand-form `image` takes its sample space from the current
  colour space; the reference text gives the operand form DeviceGray
  samples.
- `sin`/`cos` convert degrees to radians in single precision before
  evaluation; the arc splitter computes the piece count from
  `ceil(sweep / 90)`.
- `bitshift` shifts the 32-bit pattern with zero fill in both
  directions.
- The harness's text channel compares extracted text whenever both
  sides have a PDF.

## Decisions

**D1. Stroke colour: emit pairs.** The writer emits, for each colour
change the emitter recorded, the non-stroking and the stroking form
back to back (`g G`, `rg RG`, `k K`, `cs CS` with `scn SCN`), sharing the
resource name. Doubling the operators costs a few bytes per colour
change and keeps the emitter untouched. *Alternative:* track fill and
stroke colour separately in the emitter and emit only what a paint
needs — smaller streams, but the IR would grow a distinction PostScript
does not have.

**D2. Empty clip is a real clip.** The backend records a clip entry
with no segments; the emitter's clip synchronisation writes it as `W n`
with no path (an empty clipping path in PDF clips everything); painting
under it is still recorded in the IR — the writer does not need to
suppress it, the clip does — but the dump shows the empty `W n` so
goldens pin the behaviour. `initclip` and `grestore` clear it as any
clip. *Alternative:* suppress paints in the backend — hides the clip
from the IR and differs from how every other clip is handled.

**D3. Operand-form image space.** `sample_space` returns DeviceGray for
the operand form when not a mask; the dictionary form is unchanged.

**D4. Angle reduction in `f64`.** `v.rem_euclid(360.0)` on the `f64`
widened value, then the exact quarter cases (0, 90, 180, 270) return
exact results, else convert and evaluate in `f64` and narrow. `atan`
is unchanged (its result reduction is already correct).

**D5. Arc pieces.** The piece count uses `(sweep.abs() / 90.0 - 1e-9)
.ceil().max(1)`, so a sweep within rounding above a multiple of 90
does not gain a piece; `arcto`'s tangent geometry moves to `f64`
throughout, narrowing only at the output points.

**D6. `bitshift`.** The implementer reads the operator's description in
the reference manual in the vault (reading the reference is what the
vault is for). Zero fill prescribed → keep the operator, keep the
registry entry, add the header to the corpus file. Sign extension
prescribed → fix the operator, delete the registry requirement from
this change's delta, and record the reason. Either way the corpus file
`interp/bitshift-negative-right.ps` pins the chosen behaviour.

**D7. Invisible text in the harness.** In the text comparison: if the
page's pixel comparison passed and the reference's extracted text for
that page is empty after normalisation while ours is not, emit a note
`text: invisible (reference extracted nothing)` and do not count a
failure. The asymmetry is deliberate: the reference drops text it
deems invisible, we keep it; the rasters agreeing is the evidence.

**D8. Corpus promotion.** Each shrunk generated program becomes a
`corpus/unit/` file rewritten by hand into the corpus style (SPDX,
scenario comment, expectation headers) with goldens; the generator's
output is never committed as is.

## Risks / Trade-offs

- [Existing goldens with coloured strokes change] → expected and
  reviewed: the `stroke-ctm` and colour goldens gain `RG`/`K`/`G`
  lines; every change is listed in the notes.
- [Empty `W n` confuses a viewer] → PDF defines it; the external
  checker and the reference converter's rendering are the check.
