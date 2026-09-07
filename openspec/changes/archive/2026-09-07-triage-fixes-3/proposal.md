# Change: Triage fixes 3 — stroke colour, empty clip, gray images, angle reduction, arc precision, bitshift, invisible text

## Why

The first generated-program round against the reference converter
found what 145 hand-written files had not: every non-black stroke
renders black in our PDF output, a `clip` on an empty path keeps
painting, a gray image after a CMYK colour is distilled in CMYK, and
trigonometric operators lose digits on large angles. Each is a small,
well-understood fix; together they cover most of the 410 document
failures and 250 output differences the round reported. Two smaller
items ride along: a piece-count instability in arc construction and a
`bitshift` behaviour that is either a bug or a recorded divergence
depending on the reference text; and the harness learns to treat text
the reference dropped as invisible rather than as a failure.

## What Changes

- **Stroke colour in the PDF.** Every colour setting in the IR is
  written for both the fill and the stroke colour (the stroking
  operators alongside the non-stroking ones, colour spaces likewise),
  since PostScript has one current colour and PDF has two.
- **`clip` on an empty current path yields an empty clip.** Painting
  under it produces nothing until the clip is restored or reset; the IR
  records the empty clip and the writer emits it as an empty clipping
  path, so the output agrees.
- **Operand-form `image` samples are gray.** The five-operand `image`
  SHALL record DeviceGray samples whatever the current colour space;
  only the dictionary form names a space.
- **Angle reduction for `sin`, `cos`**, and the angle results of `atan`
  where relevant: degrees are reduced modulo 360 in double precision
  before conversion, so large angles keep full single precision.
- **Arc piece count is stable** at exact quarter-turn multiples, and the
  `arcto` tangent computation runs in double precision.
- **`bitshift`**: the reference text is checked in the vault; if it
  prescribes zero fill on right shifts, the difference from other
  interpreters is recorded as `bitshift-zero-fill`; if it prescribes
  sign extension, the operator is fixed. The delta carries the record
  and the implementation removes it if the check goes the other way.
- **Invisible text in the harness.** When the rasters agree and the
  reference extracted no text from a page, a text difference is a note,
  not a failure.
- **Regression corpus files** for each fix, with goldens, including the
  shrunk generated programs promoted as clean-room reproductions.

## Capabilities

### New Capabilities
- none.

### Modified Capabilities
- `remelt`: ADDED requirement for stroke colour.
- `graphics-ir`: ADDED requirements for the empty clip, operand-form
  image samples, and arc piece stability.
- `interpreter-core`: ADDED requirement for trigonometric argument
  reduction.
- `expected-divergences`: ADDED `bitshift-zero-fill`, conditional on the
  reference check.
- `oracle-testing`: "Comparison" gains the invisible-text rule.

## Impact

- Code: `crates/remelt` (content writer colour operators), `crates/ps-graphics`
  (empty clip, arc pieces), `crates/ps-vm` (image sample space,
  `sin`/`cos`, possibly `bitshift`), `tools/difftest` (text rule),
  corpus files and goldens; existing goldens that contain a stroke in a
  non-black colour change bytes (the stroke colour operators appear) —
  those are reviewed and re-pinned.
- The private oracle round over generated programs is re-run and its
  totals recorded.
- Depends on `psgen-v0` (archived).
