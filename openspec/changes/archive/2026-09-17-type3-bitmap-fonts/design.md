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

## Implementation notes

As built, from PLRM3 §5.4, §5.7, and the `setcachedevice`,
`setcachedevice2`, `setcharwidth`, `findfont`, `FontDirectory`, and
`definefont` entries of §8.2, with the reference observed black-box
through the oracle tier.

**The transform rule (D1).** `ps-vm/src/ops/show.rs` holds
`Metrics` (width, optional box, optional writing-mode-1 width and
origin) and `into_glyph_space(current, glyph, metrics)`: the carry
matrix is `current × glyph⁻¹`, composed in double precision
(`compose64`, new in `graphics.rs`, next to `apply64`/`envelope64`);
every vector goes through its delta part, the box's four corners
through the full matrix and then `envelope64`, and each result rounds
to `f32` once. The three operators in `ops/font.rs` build a `Metrics`
and call `declare`, which asks the backend for `current_matrix()` and
the new trait getter `glyph_matrix()` (default `None`; the capture
backend answers the CTM recorded at `begin_glyph` for the innermost
glyph capture, `Capture.ctm`, so a pattern cell or nested glyph does
not confuse it) and stores the carried width and box in the running
glyph, where `finish_glyph` reads them for the advance and hands them
to `end_glyph`. Without a backend nothing is carried, since no CTM
exists. *Identity guard:* when `current == glyph` the operands are
returned untouched, so a procedure that declares before transforming
sees no rounding — every pre-existing Type 3 golden is byte-identical.
*Singular:* a glyph matrix without an inverse (a degenerate font
matrix) leaves the operands as given. *`stringwidth`:* the measuring
pass runs the same frame and the same `declare`, and `begin_glyph`
with `measure` still opens a capture, so `glyph_matrix()` is present
and widths agree with `show` (unit-tested: scale, translate, rotate,
identity, singular, and `setcachedevice2`). *`setcachedevice2`:* all
three vectors are carried; the running glyph keeps the mode-0 width
and the box, as before, since the VM shows in writing mode 0.

**Blank glyphs.** A glyph that declares a width and paints nothing (a
space) used to be dropped by `end_glyph` because its operation list
was empty, so the PDF's `Encoding`/`Widths` named a code with no
`CharProcs` entry and the checker reported a bad Type 3 CharProc — on
the synthetic bitmap font and on the driver job alike. `end_glyph`
now stores such a glyph with an empty procedure (`glyph /space 0.5 0
[0 0 0 0] {}` in the dump, a bare `d1` line in the PDF). Only glyphs
that were measured, abandoned (`(0, 0)` and no box), or run under the
null device are still dropped. No existing golden had a blank glyph.

**`findfont` registers (D2).** `ops/font.rs::find` resolves the key
through the directories, the resident set, or substitution as before
and, for the latter two, calls `find_resident`, which enters the
face's (global-VM, cached) dictionary in the directory of the current
allocation mode under the requested key through the same `register`
helper `define` uses (both directories in global mode). No `FID` is
minted: the materialised dictionary carries one. `findresource` on
the `Font` category goes through `find_resident` too, as the
`findresource` entry describes (load, then define). A program-defined
font under the same key replaces the entry and wins. `resourcestatus`
keeps answering 1 (a loaded built-in) for such an entry — the face's
own dictionary under its own name or under a substituted name it
resolved — and 0 for the same dictionary the program defined under a
key of its own (`resident-set-reported.ps` unchanged);
`resourceforall` already deduplicated directory and built-in names.
*`save`/`restore`:* the registration is a dictionary change in local
VM and goes with `restore`, like any `definefont` in local mode;
*`undefinefont`* removes it and the next `findfont` enters it again.
Both confirmed by unit test. The reference behaves differently on
both (observed: the key stays known after `restore` and after
`undefinefont`, consistent with it loading faces into global VM as the
`findfont` entry permits); D2's choice is kept, the corpus scenario
avoids the two lines, and the unit test pins ours. Also observed: the
reference reports a substituted key as a Font resource even before
`findfont`; ours answers `false` until `findfont` has entered it (a
pre-existing difference, unchanged in kind).

**`FontBBox` consistency (remelt).** `write_type3` writes the font's
own `FontBBox` (the dictionary entry, in glyph space) and each glyph's
`d1` box from the IR. Before this change the glyph boxes of a font
that transforms before declaring were in the transformed space while
`FontBBox` was in glyph space; both are now in glyph space, so the
two agree (the driver's font computes its `FontBBox` through the same
normalisation its procedure applies). No remelt change was needed.

**Corpus (D3).** `corpus/unit/text/type3-scale-before-metrics.ps`,
`type3-rotate-before-metrics.ps` (the quarter turn as an exact
`concat` matrix, so the golden carries no trigonometric residue —
`90 rotate` yields a width of `-0.0000262268 600` from the `f32`
CTM), `type3-setcharwidth-under-scale.ps`, and
`type3-bitmap-glyph.ps` (the driver's shape with the project's own
8×8 and 6×10 bitmaps, a normalisation of 1/8 and pixel metrics chosen
so every width and box is exact in binary; `save`, scale,
`setcachedevice`, `translate`, `scale`, `imagemask` from a string data
procedure, `restore`, and a blank space glyph), each with IR and PDF
goldens; `corpus/unit/fonts/findfont-registers-resident.ps` as a
`% backend: none` scenario. The four PDFs were checked from the shell
with the vault's PDF tools: no syntax warnings, renders as expected.

**Golden changes.** None: all 337 pre-existing corpus files pass
byte-identical; the five new files add five goldens (four IR + four
PDF).

**Oracle.** `corpus/unit/text` + `corpus/unit/fonts`: 86 files, 61
pass, 0 fail, 23 expected-divergence, 1 divergence-closed, 1 skipped;
output 61 same, 24 differs (the 24 are the expected-divergence files
plus the skipped one's kin, as before). The five new files: all
`pass`, output `same`. The one `divergence-closed` is
`substitution-arial.ps` (`font-substitution`): a substituted face
rendering within the pixel limit at the profile's resolution. The
same file gives the same verdict from an untouched checkout of the
parent commit, so it predates this change (nothing here touches how
substituted glyphs are drawn); the marker is left for the
expected-divergences owner. Captured jobs with
the host prelude: `finder-sys608-lw70-print-directory-bitmapfont.ps`
now `pass`, output `same` (before this change: `fail`, text
extraction of ours timing out and the render sparse);
`finder-sys608-lw70-print-directory.ps` `pass`, output `same`. The
bitmap-font job distilled with the CLI (`efterscript pdf --prelude`)
lists one embedded Type 3 font, renders the full directory listing
(header, folder icon and label, footer), extracts the listing's text,
and the shell tools report no syntax errors.

**Gates.** `cargo test --workspace` all green (new: 5 unit tests in
`show.rs`, 8 integration tests in `ps-vm/tests/text.rs`, 1 in
`ps-graphics/tests/backend.rs`); `cargo clippy --workspace
--all-targets` 0 warnings; `cargo fmt --check` clean; `difftest run`
342 files, 342 passed; `parse-survival` 342 files, 0 failed;
`fuzz-round` 1300 + 1300 programs, 0 failed; `lint-strings` 1179 files
clean; `check-wasm` passes; `cargo build --workspace
--no-default-features` clean; `openspec validate` valid.
