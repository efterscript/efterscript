# Change: Type 3 glyph metrics under a changed CTM, and resident fonts in FontDirectory

## Why

The first live print from the emulated Macintosh through the session
library exposed two defects with one visible symptom: the page's text
came out as scattered fragments. The driver, finding no fonts listed
in `FontDirectory`, downloaded its own bitmap font — a Type 3 font
whose `BuildChar` scales the coordinate system before calling
`setcachedevice` and painting the bitmap with `imagemask`. The
interpreter records the glyph's width and bounding box as the raw
operands, in the scaled space, while it captures the painted marks
correctly; the PDF then advances each glyph by the unscaled width and
declares a bounding box many times larger than the glyph, so the text
is spread across the line and a checker rejects the glyph procedures.
The same job through the reference converter yields the expected
page. Separately, `findfont` on a resident face returns the font
without entering it in `FontDirectory`, so a driver's font inventory
sees nothing resident and takes the bitmap path in the first place.
Both are interpreter defects: the bridge and the command line produce
byte-identical output for the captured job. Fixing them now matters
because the joint milestone — a print from the emulated Macintosh
yielding a correct PDF — is measured on exactly this job, and because
the first fix changes what the text and IR specs promise about Type 3
metrics, which every later Type 3 job depends on.

## What Changes

- **Glyph metrics follow the CTM at the call** (`ps-vm`,
  `ps-graphics`): the operands of `setcachedevice`, `setcachedevice2`,
  and `setcharwidth` are interpreted in the coordinate system in
  effect when the operator executes, and carried into glyph space (the
  system in effect when the glyph procedure began) — the width vector
  through the delta transform, the bounding box through the full
  transform and an axis-aligned envelope — so a procedure that scales,
  translates, or rotates before declaring its metrics gets the
  displacement and box the reference produces. Metrics declared with
  no intervening change are unaffected.
- **`findfont` registers resident faces** (`ps-vm`): a font obtained
  from the resident set (or by substitution) is entered in
  `FontDirectory` under its key, as `definefont` would, so
  `FontDirectory` enumerates it afterwards and `findfont` finds it
  there first; a font already defined by the program is unaffected.
- **Corpus**: the driver's bitmap-font job pattern reduced to a
  synthetic Type 3 font (own bitmaps, own numbers) under
  `corpus/unit/text/`, the metric-transform scenarios, and the
  `FontDirectory` scenario; the captured live job re-run in the
  private tier.
- Out of scope, with triggers: the font cache semantics of
  `setcachedevice` (never — no cache); `setcachedevice2` writing-mode-1
  metrics beyond carrying them (when a vertical Type 3 job appears);
  the driver's smoothing branch (`smooth4`) — it is the job's own
  procedure and needs nothing.

## Capabilities

### New Capabilities
- none.

### Modified Capabilities
- `text`: ADDED requirements for glyph metrics under a changed CTM and
  for `findfont` entering resident faces in `FontDirectory`.
- `graphics-ir`: ADDED requirement that a captured glyph's width and
  bounding box are in glyph space regardless of CTM changes inside the
  procedure.

## Impact

- Code: `crates/ps-vm` (`ops/show.rs` or wherever `setcachedevice`/
  `setcharwidth` hand metrics to the backend; `ops/font.rs` `findfont`),
  `crates/ps-graphics` (`backend.rs` `end_glyph`: the metrics
  transformed through the capture's CTM; `begin_glyph` recording it),
  `crates/remelt` (no change expected: it writes what the IR carries),
  corpus under `corpus/unit/text/` with goldens.
- No new dependencies.
- Depends on `text-core`, `resident-outlines`, `printer-identity-
  mechanism`, `platen` (archived).
