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

## Implementation notes

- **D1, stroke colour** (`remelt/src/content.rs`). `ColorOp` gained
  `stroking_operator` (`G`, `RG`, `K`, `SCN`); `set_color_space` writes
  `/name cs` then `/name CS` and `SetColor` writes the components with
  both operators, one operation per line as before. Tests
  `every_colour_setting_is_written_for_both_painting_operations` (the
  red RGB stroke and a Separation stroke sharing `/CS0`) and the
  updated `colour_operators_follow_the_space_across_save_and_restore`.
  Corpus: `graphics/stroke-colour-red.ps` (the spec's red stroke) and
  `graphics/stroke-colour-separation.ps` (a 0.6 tint of `Spot`), IR
  and PDF goldens. Re-pinned PDF goldens, every one for the stroking
  pair now beside each colour setting and nothing else (no IR moved):
  `graphics/clip-and-pages.pdf` (`0.5 G` before the dashed stroke,
  `1 0 0 RG`), `graphics/imagemask-in-separation.pdf` (`/CS0 CS`,
  `0.6 SCN`), `graphics/separation-survives.pdf` (the same),
  `text/text-under-clip-and-colour.pdf` (`1 0 0 RG`). The render check:
  the red stroke distilled to `target/triage3/` and rasterised through
  the profile's render command (a colour variant of it, in the shell
  only) gives red pixels (`ff0000`, anti-aliased edges `ff1010`,
  `ffcccc`) and no black; in the oracle run the file passes.
- **D2, empty clip.** The backend already recorded the entry — `clip`
  clones the current path's segments, empty or not — and the emitter
  already wrote `W n` with no segments, which the dump shows; the
  difference was in the content writer, whose bare `W n` with no path
  the rasteriser ignored (the generated-round finding painted 5 629
  pixels through it). D2's "an empty clipping path in PDF clips
  everything" holds only for a path: the writer now precedes `W n`
  (`W* n`) of a clip without segments with the zero-area rectangle
  `0 0 0 0 re`, and nothing else changed; the fill under the clip stays
  in the IR and the PDF. Test `a_clip_without_segments_is_a_zero_area_
  rectangle`. Corpus `graphics/clip-empty-path.ps`: page one is the
  spec's scenario, pages two and three paint again after `grestore` and
  `initclip`. Rendered through the profile's rasteriser: page one has
  no non-white pixel, pages two and three 625 each (the 50×50 square at
  36 dpi); the oracle passes the file.
- **D3** (`ps-vm/src/ops/image.rs::sample_space`): a `from_operands`
  flag; the operand form records DeviceGray unless a mask, the
  dictionary form keeps the current space. Corpus
  `graphics/image-operand-form-gray.ps`: `cs 0 DeviceGray`, `decode=
  [0 1]`, 4 bytes for the 2×2 image after `setcmykcolor`; the finding's
  program now dumps the same.
- **D4** (`ps-vm/src/ops/arith.rs::sin_cos_degrees`): the `f32` widened,
  `rem_euclid(360.0)`, the four quarter turns exact, otherwise
  `to_radians().sin_cos()` in `f64` narrowed once. `atan` untouched.
  Unit test `angles_are_reduced_before_conversion` (a million degrees
  is 280: cosine narrows to 0.17364818, sine to the nearest single of
  −0.984807753) and the `math_operators` scenario in
  `tests/interp.rs`. Corpus `interp/trig-large-angle.ps` pins
  `0.7313537`, `-0.6819984`, `0.17364818` — the reference prints the
  same three lines (output: same). A quarter-turn `90 cos` prints `0.0`
  here and a value of order 10⁻⁸ there; the file leaves it out rather
  than pin a rounding artefact either way.
- **D5** (`ps-graphics/src/arc.rs`, `backend.rs`). Pieces:
  `(sweep.abs() / 90.0 - 1e-9).ceil().max(1.0)`; test
  `a_quarter_turn_from_rounding_is_one_piece` (90° plus one ulp is one
  piece within 10⁻³ of the exact one; 90.001° is two; −180° and 360°
  plus 10⁻¹⁰ keep two and four). The geometry is `f64` throughout:
  `sweep`, `point_at`, `curves`, and `append_arc` take `f64` (a
  `Center` pair, `center_of` widens a `Point`), `Tangent` carries the
  centre and start angle unnarrowed and narrows only `t1`/`t2`; the
  right-angle test checks the centre to 10⁻⁹. The finding
  `arcto-quarter-sweep-split` now passes the generator's `translate`
  relation (one piece either way) and is promoted as
  `graphics/arcto-quarter-sweep.ps` with goldens (one `c` after the
  tangent line). `arcto-acute-tangent-precision` still fails that
  relation by the same (0.007, 0.009): the error is in its inputs —
  the current point is stored in single precision in device space and
  read back through the CTM, and the 6° corner's lever
  (r/tan(θ/2), some 950 units, growing 18 000 units per radian of
  angle) turns that rounding into hundredths of a unit — which `f64`
  arithmetic after the fact cannot reach. It is not promoted: the
  difference is a hundredth of a unit, invisible at the profile's
  resolution and below the IR tolerance in every relation but
  `translate`; a follow-up would keep the current point in `f64`.
- **D6, `bitshift`.** The operator's description in PLRM3 §8.2
  (page 539 of the Level 3 reference, read through a text extraction
  in the shell) prescribes that bits shifted in are zero and states the
  result is arithmetically right only for a non-negative first
  operand; the operator is kept as it is, the registry requirement
  `bitshift-zero-fill` stays in this change's delta, and
  `interp/bitshift-negative-right.ps` carries the header, pinning
  `-663 -7 bitshift` → 33554426, `-1 -1 bitshift` → 2147483647, and a
  left shift of a negative (the same in both, −1416). The oracle
  reports it `expected-divergence`, output differs (−6 there).
- **D7** (`difftest/src/oracle.rs::compare_documents`). A
  `pixels_agree` flag set false by a page-count, media-box, size, or
  pixel-fraction reason; when the texts differ after normalisation,
  the reference's is empty, and the flag holds, the note `text:
  invisible (reference extracted nothing)` replaces the reason. One
  deviation from D7's wording: the profile's extractor yields one text
  per document, not per page, so the rule is applied per document —
  every page's pixels must agree and the reference must have extracted
  nothing from the whole document. Fake-profile test
  `text_the_reference_did_not_extract_is_invisible_when_the_rasters_
  agree` (the extractor learned `%fake-silent`): a note and `pass`;
  with differing pixels, an extra page, or text on both sides, a
  reason as before. Corpus `text/invisible-offpage-text.ps` (Helvetica
  at −200, −200): blank on both sides, one text run in the IR, the
  oracle notes it and passes.
- **D8.** Every corpus file above was written by hand from the
  finding's root cause; none is the generator's output.
- **Corpus oracle totals.** Before: 145 files, 108 pass, 0 fail, 32
  expected-divergence, 1 divergence-closed, 4 skipped; output 110
  same, 31 differs. After: **153 files, 115 pass, 0 fail, 33
  expected-divergence, 1 divergence-closed, 4 skipped; output 117
  same, 32 differs** — the eight new files are seven passes and the
  `bitshift` divergence; the four re-pinned files pass as before.
- **Generated round.** Regenerated with `psgen gen` (seed 300, 2 000
  programs per profile, ill-typed share 0) under
  `target/psgen/oracle2/`, run as eight `difftest oracle --profile
  default` processes over four chunks per profile (about four
  minutes wall). Before (the psgen-v0 notes): `core` 2 000 pass,
  0 fail, output 1 751 same / 249 differs; `graphics` 1 590 pass, 410
  fail, output 1 668 same / 332 differs. After: **`core` 2 000 pass,
  0 fail, output 1 801 same / 199 differs; `graphics` 1 962 pass, 38
  fail, output 1 690 same / 310 differs**; 140 `graphics` files carry
  the invisible-text note. The 410 document failures are 38, and the
  four fixed classes are gone: no failing file loses a stroke colour,
  paints through an empty clip, or expands a gray image, and no output
  difference is a `sin`/`cos` of a large angle.
- **The 38 document failures, by root cause** (each pixel case shrunk
  with `psgen shrink --predicate`, the predicate from the earlier
  round; the shrunk programs under `target/psgen/oracle2/shrink/`):
  - *Partly invisible text (25 text-only, 5 with pixels).* A run
    partly off the page, or one visible run beside an invisible one:
    the reference extracts the visible glyphs (`jmzd` for our
    `jmzdnty`, `uelkmf` for `juelkmf`), so its text is neither empty
    nor equal. Five of these are also extractor layout differences —
    the same glyphs grouped or ordered differently
    (`ckbizqexckbizqex…` against `ckbizqexvzdfyunn…`). The document
    rule of D7 cannot reach them; a per-glyph-run rule against the
    page and clip geometry remains the follow-up it was.
  - *Thin strokes and stroke adjustment (4 pixel cases: 1822, 0342,
    1238, and the pixel half of 0512).* A single one-unit
    `147 172 96 113 rectstroke` differs in 0.522 % of the pixels at 36
    dpi (its perimeter is 209 pixels of 40 000, so any such rectangle
    crosses the 0.5 % limit) while `100 100 100 100 rectstroke` and
    the same rectangle stroked twice agree. The reference's document
    sets an ExtGState with stroke adjustment off; ours sets nothing,
    and the rasteriser then snaps our thin lines to whole pixels
    (darker: 22 against 73). Follow-up: write the page's stroke-adjust
    state (or `/SA false` unconditionally, as the reference does) into
    an ExtGState, then re-check.
  - *Consecutive `moveto`s (1252, 3.2 % of pixels).* `229 199.23 moveto
    77 257 moveto pathbbox` gives `77 199.23 229 257` here and
    `77 257 77 257` there: a `moveto` following a `moveto` replaces
    it (PLRM3 §8.2 `moveto`), which our path keeps as two subpaths;
    the bounding box then fed an `rcurveto`. Follow-up: replace a
    trailing lone `moveto` in `Path::move_to` (a small `ps-graphics`
    fix with an IR golden to review, out of this change's scope).
  - *`pathbbox` of a curve (0390, 0.66 %).* For an arc ours encloses
    the Bézier control points (`363.69 370.32` for the upper corner),
    the reference the curve itself (`360 367`); the box then placed a
    second arc. The manual describes the control-point box (PLRM3 §8.2
    `pathbbox`); whether to flatten first or record a divergence is a
    decision for a later change.
  - *Resident-font differences (0729, 4.1 %; the rest of 0512).*
    Rotated `Times-Roman` text and `Courier` charpath fills in the
    reference's substitute faces; the `resident-inventory` class.
  - *Page rotated to its text (3: 0234, 1878, 0775).* As before; the
    converter feature noted in the vault.
- **The 509 output differences, by root cause** (the tally script's
  first differing token, checked by hand on samples): 237 sixth-digit
  rounding (`0.9986295` against `0.99862951`, `-95020728.0` against
  `-9.50207e+07`) and 103 numbers glued to text by `print`, both the
  known unavoidable class; 61 `bitshift` of a negative shifted right,
  now the declared divergence `bitshift-zero-fill` (the generated
  programs carry no header, so they still count); 58 `stringwidth`
  advances (`resident-inventory`); 15 readings through the CTM
  (`pathbbox`, `currentpoint`, `arcto`) in the sixth digit or, for
  `pathbbox`, the two causes above; 11 in programs with `sin`/`cos`
  whose first differing value is a reading, not the trigonometric
  result (`-5.0` against `-4.99892521`); 22 other — `==` of a
  procedure holding `=` or `[` (`--=--`, `--[--` against `=`,
  `--mark--`), two booleans, and two integer overflows into reals
  printed as digits here and in exponent form there.
- **Tests updated for the pairs and the gray operand form.** The
  content-stream expectations in `remelt/tests/scenarios.rs`
  (Separation fill, direct device colour, image mask) and
  `remelt/tests/sink.rs` (device spaces, Separation, DeviceN and
  Indexed, image mask, compressed stream, Type 3 charproc) gained the
  stroking line after each colour line; `ps-vm/tests/graphics.rs::
  image_data_from_strings_and_files` now expects one component from
  the operand form under DeviceRGB and checks three through the
  dictionary form.
- **Gates.** `cargo test --workspace` 758 passed, 0 failed, 2 ignored,
  `cargo clippy --workspace --all-targets` 0 warnings, `cargo fmt`
  clean, `difftest run` 153/153, `parse-survival` 153 files,
  `fuzz-round` 2 600 programs 0 failed, `lint-strings` 636 files
  clean, `openspec validate triage-fixes-3` valid; `difftest`'s
  registry-count test gained `bitshift-zero-fill`. The vault README
  carries a dated note (unstaged); the profile is unchanged.
- **Follow-ups, in priority order.**
  1. Stroke adjustment in the content stream (the thin-stroke class).
  2. `moveto` after `moveto` replaces it.
  3. `pathbbox` of curves: flatten or declare.
  4. Per-run invisible-text rule in the harness (page and clip
     geometry of each glyph run), or text compared only where the
     rasters show it.
  5. The current point in `f64` for `arcto`'s acute corners.
  6. A `bitshift` header for generated programs, or the harness
     treating a registry slug's known pattern as declared, so the 61
     stop counting as differences.
