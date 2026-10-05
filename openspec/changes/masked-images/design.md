# Design: Masked images — image dictionary types 3 and 4

See proposal.md for the motivation and the spec deltas for the behaviour.
This document fixes where the mask lives at each layer, how the three
interleave types are read, and how a mask reaches the PDF.

## Context

- `ops/image.rs` parses the dictionary form in `from_dict`, which refuses
  any `ImageType` but 1 with `rangecheck` and `MultipleDataSources true`
  with `typecheck`. Samples are collected by an `ImageAcquisition`: at once
  for strings and files (a decode filter is then drained to its
  end-of-data marker), or through a `LoopFrame::ImageData` that calls a
  procedure until enough bytes have arrived. `finish` cuts a short
  delivery to whole rows. For a CIE space that does not collapse, it
  starts a conversion job (`cie::image_job`); otherwise it calls
  `Backend::image` / `imagemask` with an `ImageSpec` and the bytes.
- `ImageSpec` is the boundary type: width, height, depth, space, decode,
  matrix, interpolation, mask flag, and DCT encoding. `Resources::add_image`
  copies it into the IR's `Image`, the dump prints one `img` line per image,
  and `resources.rs` writes one image XObject per image. `downsample.rs`
  reduces samples in the IR before writing.
- The reference: PLRM3 §4.10.5–4.10.6 define the type 3 and 4
  dictionaries. ISO 32000-1 §8.9.5 (the `Mask` entry of the image
  dictionary) and §8.9.6.3–4 give the PDF forms: a stencil-mask XObject
  with its own resolution over the same unit square, and a colour-key
  array of 2n raw-sample ranges. The PDF output's minimum version (1.3) already
  allows both.

**Specification to read** (vault: `manuals/postscript/PLRM3_1999.pdf`,
`manuals/pdf/ISO32000-1_PDF1.7_2008.pdf`; PDF page numbers):

- PLRM3 §4.10.5 Image Dictionaries and Sample Decoding, pp311–315;
  §4.10.6 Masked Images with Tables 4.22–4.25, pp315–321.
- ISO 32000-1 §8.9.5 Image Dictionaries (Table 89, `Mask`, `Decode`),
  pp214–221; §8.9.6 Masked Images, pp221–222.

## Goals / Non-Goals

**Goals:** every interleave type and both colour key forms are read
exactly. The mask reaches the PDF in the PDF's own masking form, without
rasterising or resampling the image, and errors match the manual.

**Non-Goals:** `MultipleDataSources true` (see the proposal), soft masks,
and simulating masks with clipping paths.

## Decisions

**D1. The mask rides on `ImageSpec`.** `ImageSpec` gains
`mask: Option<ImageMask>`, with
`ImageMask::Stencil { width, height, decode_inverted: bool, interpolate, data: Vec<u8> }`
(one bit per sample, rows padded to bytes, covering the image's unit
square) and `ImageMask::ColorKey(Vec<(u16, u16)>)` (one inclusive range
per component, in raw sample values). The `Backend` trait is unchanged:
`image` receives a masked image as a spec with a mask. The mask needs no
matrix of its own, because D3 aligns it with the image's square.
*Alternative:* a new trait method `masked_image(spec, data, mask)`. A
backend that ignored it would still compile and silently paint unmasked
images, and every backend that forwards images (the pattern-cell
capture, the glyph capture, the CIE job) would need a second path.

**D2. Reading the data.** The acquisition learns a layout:

- **Interleave 1:** one source of (1 + n) components per sample at the
  data's depth. The acquisition collects whole rows of the combined
  layout. `finish` splits each sample into its colour components and one
  mask bit: 0 when the mask component is all zero bits, 1 otherwise
  (PLRM3 Table 4.22 treats any other value as all ones).
- **Interleave 2:** one source of blocks. Each block is k mask rows then
  one image row when the mask is taller, or one mask row then k image
  rows otherwise (k the height ratio). Mask rows are one bit per sample,
  and each row is padded separately. The needed count is the block size
  times the block count. `finish` takes the blocks apart.
- **Interleave 3:** two sources. The mask is read completely first, then
  the data. A procedure source runs as today: the `ImageData` loop frame
  carries a second stage, and when the mask stage completes it starts the
  data stage on the data source. The manual does not fix the order; the
  reference reads the mask first too (checked black-box with a program
  whose two procedures read one shared file; see the implementation
  notes). An empty string from the mask's procedure ends the mask's
  stage only; the data stage still runs, so a program reading both from
  one file stays in step.

A short delivery cuts the image to the rows both parts cover. For
interleave 3, the mask is cut to the same fraction of its height (rounded
up). For interleave 1 and 2, a cut on a block boundary keeps the blocks'
rows of each part. The kept rows are always the image's leading rows, so
a mask whose rows run opposite to the image's (D3) covers any of them
only when it is complete; a shorter one cuts the image to no rows. Data
passed through encoded (DCT, interleave 3 only) cannot be cut: mask rows
that never arrived keep the image off the page instead. A decode filter
is drained to its marker as today, so a program that put the data inline
continues after it.

**D3. Alignment.** Each dictionary's `ImageMatrix` and size give the map
from the unit square to user space, U = flip · scale(w, h) · ImageMatrix⁻¹.
The image is placed by the data dictionary's U. The mask must map onto the
same square: if its U equals the data's to 1e-4 of the square's extent,
it is used as read. If it equals the data's with one or both axes of
the unit square reversed, its rows and/or columns are reversed. Any other
U raises `typecheck`, which PLRM3 Table 4.24's alignment requirement
makes an inconsistency; so does a singular mask matrix. The reference
paints such a mask, so the corpus file carries the expected divergence
`masked-mask-misaligned`. A data
dictionary whose square is degenerate (singular matrix or zero extent)
paints nothing and takes any mask as read. The driver that motivated this change gives both
dictionaries the same matrix form; the reversals cost little and cover a
program that writes the mask in the opposite row order.

**D4. Normalisation.** A stencil mask is stored at one bit per sample
with the polarity reduced to a flag. Each end of the mask's `Decode`
rounds to 0 or 1 (at 0.5). `[0 1]` is the PDF default, and `[1 0]` sets
`decode_inverted`. When both ends round alike, the mask is constant: its
bits are written to that value under `[0 1]`. Validation, in order: the
type 3 dictionary's own entries, then each sub-dictionary as a type 1
dictionary (reusing `from_dict`'s checks with the mask's depth rule),
then the cross-dictionary rules of Tables 4.22–4.24. All of it happens
before any data is read, so a structural error consumes nothing from the
source.

**D5. Colour key.** `MaskColor` of n integers becomes n ranges [v, v],
and one of 2n becomes n ranges. Values are clamped to 0..2^bpc − 1 as PDF
requires. A range left with min > max masks nothing in both languages
and is kept. The key compares raw samples, so it survives `Decode`
unchanged and is written as given. A DCT-encoded type 4 image keeps the
passthrough and its key, which the PDF allows.

**D6. When samples change, the key becomes a stencil.** CIE conversion
(VM side) and averaging (remelt's downsampling) change sample values and
would break a raw-value key. In both places the key is first turned into
a stencil mask at the image's full resolution, from the samples as read.
One function on `ImageSpec` in `efterscript-vm` does this, and both
callers use it. A stencil mask is never resampled: downsampling reduces
the base image by its class and leaves the mask at its own resolution,
which PDF permits (the two need not share a resolution). Subsampling and
the other cases where samples keep their values (Indexed, 1-bit, DCT)
keep the key as is.

**D7. PDF form.** A stencil mask is written as its own image XObject:
`Subtype /Image`, `ImageMask true`, width, height, `BitsPerComponent 1`,
`Decode [1 0]` when inverted, `Interpolate` when set, Flate. It is written
before the base image, which names it in `/Mask`. It is not listed in the
page's `XObject` resources, because no content stream paints it directly.
A colour key is the `/Mask` integer array. Output stays deterministic:
the mask's object is allocated immediately before its image's.

**D8. Dump form.** The `img` line gains ` mask=<w>x<h> decode=[0 1]|[1 0]
<n> bytes`, plus ` interpolate` when set, or ` key=[min max …]`. Nothing
is added for an unmasked image, so every existing golden is unchanged.

**D9. Errors and contexts.** The `ImageType` check, `rangecheck` for an
unknown type or interleave type, and `typecheck` for structural
inconsistency all follow the spec. Inside an uncoloured pattern cell a
masked image is as undefined as any `image` (`colour_allowed`). In a
coloured cell, a form, or a glyph procedure it is captured with its mask
like any image. Interleave types 1 and 2 with a `DCTDecode` data source
raise `limitcheck`, registered as `masked-dct-interleaved`.

**D10. Corpus and acceptance.** New files under `corpus/unit/graphics/`
(`masked-*.ps`), each with an IR golden and a PDF golden, covering every
scenario of the `images` and `remelt` deltas. One of them reproduces the
construction that prompted the change, written in the project's own
words and samples: an Indexed 8-bit 32×32 image, a 1-bit mask with
`Decode [1 0]`, interleave type 2, `RunLengthDecode` on the current file,
and a save/restore around it. The oracle tier runs over the new files.
The private tier re-runs the real job: once the VM part is in, the job is
captured again from the emulator pairing (its hosting build pointed at
this branch's library), vaulted under `corpora/realworld-drivers/` with
its provenance, and distilled with the hosting application's prelude. It
must finish with outcome `ok` and pass the oracle.

## Risks / Trade-offs

- [Mask-first order for interleave 3 is a guess] → checked black-box in
  part 1 and corrected before the goldens are written.
- [A key turned into a stencil loses the PDF's key form in the two
  converting cases] → the rendering is identical, and both cases are
  rare (a converting CIE space; downsampling turned on).
- [`ImageSpec` gains a field that every constructor must set] → a
  compile-time change across the workspace; `None` everywhere but the
  new paths, so no golden moves. The unchanged-goldens check is the
  regression net.
- [Viewers differ on interpolation of stencil masks] → the flag is
  carried as given; the corpus does not depend on it.

## Implementation notes

### Part 1 (efterscript-vm)

As built, from PLRM3 §4.10.5 (type 1 dictionaries, sample decoding),
§4.10.6 with Tables 4.22–4.25, ISO 32000-1 §8.9.5 (Table 89 `Mask`,
`Decode`) and §8.9.6.3–4, and black-box observation of the reference.

**Boundary (D1, D6).** `ImageSpec` carries `mask: Option<ImageMask>`;
`ImageMask` (`Stencil { width, height, decode_inverted, interpolate,
data }` or `ColorKey(Vec<(u16, u16)>)`) is exported beside `ImageSpec`.
Every existing constructor in the workspace sets `mask: None`.
`ImageSpec::key_to_stencil(data)` is the one key-to-stencil function:
a full-resolution stencil under `[0 1]`, bit 1 where every component of
the raw sample lies in its range; rows the data lacks are not masked;
`None` without a key or for encoded data. Clippy's enum-size lint made
remelt's `downsample::Outcome::Reduced` box its image (the spec grew);
no behaviour changed there.

**Dictionaries (D4, D5, D9).** `ops/image.rs` dispatches on
`ImageType`: 1 for both operators, 3 and 4 for `image` only, anything
else `rangecheck`. The type 1 parser now takes a role (image, stencil,
type 3 mask) and returns the data source as optional, so the mask
dictionary goes through the same checks with any of the five depths.
Type 3 order: `DataDict`/`MaskDict` present and dictionaries
(`typecheck`), `InterleaveType` an integer (`typecheck`) in 1..3
(`rangecheck`), each sub-dictionary's `ImageType` 1 when present
(`typecheck`), both sub-dictionaries as type 1, then the cross rules and
the alignment in `ops/image/masked.rs` (`typecheck`). Type 4: the
`MaskColor` array (absent or not an array `typecheck`), integers
(`typecheck`), n or 2n of them (`rangecheck`), clamped. `ImageType`
absent still means 1, as before.

**Acquisition (D2, D3).** `ImageAcquisition` holds an optional
`MaskAcquisition`. Interleave 1 and 2 count the combined source in
`needed` and are taken apart in `finish` (interleave 1: one mask bit
per sample, 0 exactly when the mask component's bits are all 0; the
colour components repacked at their depth; interleave 2: whole blocks
only). Interleave 3 reads the mask from a string or file at once, or
through the `ImageData` frame, whose body moves from the mask's
procedure to the data's when the mask stage ends; a string or file data
source after a procedure mask is read when the frame finishes. A
`DCTDecode` data source keeps the passthrough under interleave 3 and is
`limitcheck` under 1 and 2, raised before any byte is read. The
frame's `restore` references now include both sources. Alignment
compares the images of image space's origin and the ends of its first
row and column under each dictionary's inverse matrix, in double
precision, to 1e-4 of the larger edge of the data's square; the flip of
the design's formula is common to both sides and drops out.
Normalisation: each end of the mask `Decode` rounds at 0.5; `[1 0]`
sets `decode_inverted`; equal ends write every delivered bit to that
value under `[0 1]`. Deviations, written into D2 and D3 above: a short
mask with reversed rows cuts the image to no rows; encoded data with a
short separate mask keeps its height and the missing mask rows keep it
off the page; an empty chunk ends only the mask stage; a degenerate
data square takes any mask, a singular mask matrix is `typecheck`.

**CIE (D6).** `cie::image_job` turns a key into a stencil from the raw
samples before decoding them; a stencil travels on the spec unchanged.

**Black-box observations (the reference, run as a subprocess).**
*Interleave 3 order:* two procedures each reading one byte from the
current file and printing it, mask 8×6 (six bytes) and data 1×6: the
reference called the mask procedure six times, then the data procedure
six times; with procedures returning constant strings, `MMMDDD` for a
three-call mask. Mask first, entirely — D2 stands unchanged. *Error
names, recorded for the oracle tier (not adopted; the spec follows
PLRM3's "inconsistency is typecheck"):* the reference raises
`rangecheck` for heights in no integral ratio, a mask `DataSource` under
interleave 2, a mask depth other than 1 under interleave 2, unequal
sizes or depths under interleave 1, a missing `DataDict`, and a
sub-dictionary of type 4; `undefined` for a missing `InterleaveType` or
a missing mask source under interleave 3; `typecheck` for `imagemask`
of a type 3 dictionary (the spec has `rangecheck`); `rangecheck` for
`MaskColor` values outside the sample range (D5 clamps) and for an
absent `MaskColor`. It accepts an offset or mirrored mask without error
and an `InterleaveType` of `2.0`. The error-case corpus files may need
divergence slugs or `oracle: skip` once part 2 runs the oracle tier.

**Corpus.** Error cases under `corpus/unit/graphics/`:
`masked-unknown-image-type.ps`, `masked-imagemask-type3.ps`,
`masked-heights-not-integral.ps`, `masked-mask-source-interleaved.ps`,
`masked-mask-offset.ps`, `masked-interleave-4.ps`,
`masked-colour-key-wrong-length.ps`, `masked-colour-key-real.ps`, and
`masked-dct-interleaved.ps` (`% divergence: masked-dct-interleaved`,
data inside `%%BeginData`/`%%EndData` so parse-survival scans it). The
oracle's slug registry reads open changes' deltas, so the slug resolves
now; its pinned per-slug inventory test gained the entry. Painting
files and goldens are part 2's.

**Tests.** 20 new: 9 unit tests in `ops/image/masked.rs` (interleave
rules, alignment and reversals, by-sample split at 8 and 4 bits, by-row
blocks in both ratios with a short delivery, separate cuts, reversed
rows and columns, decode flag and constants, encoded fill, key ranges),
1 in `graphics.rs` (key to stencil: exact, ranges, 1/2/4/8/12 bits,
full range, min > max, Indexed, missing rows, no key, encoded), 9 in
`tests/graphics.rs` (every interleave type and source combination
through the recording backend, the shared-file order, short data, the
Indexed filter case, reversal, colour keys, DCT, and the errors with
nothing consumed from the current file), 1 in `tests/cie.rs` (key
becomes a stencil before conversion; a stencil passes through).


**Gates.** `cargo test --workspace` green (1269 passed before, 1289
after; 4 ignored both times); `cargo clippy --workspace --all-targets`
0 warnings; `cargo fmt --check` clean; `difftest run` 373 files, 373
passed (every existing golden byte-identical, no golden written);
`parse-survival` 373 files, 0 failed; `fuzz-round` 1300 + 1300
programs, 0 failed; `lint-strings` clean; `openspec validate` valid.
`check-wasm` could not run: the `wasm32-unknown-emscripten` target is
not installed in this environment.

### Part 2 (efterscript-graphics, efterscript-remelt, corpus)

As built, from ISO 32000-1 §8.9.5 (Table 89: `ImageMask`, `Mask`,
`Decode`, `Interpolate`) and §8.9.6.1–4, and PLRM3 §4.10.6 with
Tables 4.22–4.25 for the corpus programs.

**IR and dump (D8).** `Resources::add_image` is the only path from
the backend into a page's images and it clones the whole spec, so the
mask reaches the IR on the page, in form bodies, and in pattern cells
without change. The dump's `img` line appends ` mask=<w>x<h>
decode=[0 1]|[1 0] <n> bytes` (then ` interpolate` when the mask's own
flag is set) or ` key=[min max …]`, after the image's own
` interpolate` and ` dct` flags; an unmasked image's line is
unchanged. Unit test in `dump.rs` (both polarities, the mask's
interpolation flag beside the image's, a key with a min > max range).

**PDF (D7).** `write_image` writes a stencil as its own Flate image
XObject (`/Type /XObject /Subtype /Image`, its size, `/ImageMask true`,
`/BitsPerComponent 1`, `/Decode [1 0]` only when inverted,
`/Interpolate true` only when set) allocated and written immediately
before its base image, whose dictionary names it in `/Mask`. Only the
base's reference enters `Objects::images`, so no `XObject` resource
dictionary (page, form, or cell) lists the mask. A key becomes the
`/Mask` integer array. A DCT-encoded base keeps its passthrough and
carries either form. Four tests in `tests/sink.rs` (the inverted
stencil's dictionary, object order, and absence from the resources;
the default polarity with interpolation; the key array; a DCT base
with a key and with a stencil).

**Downsampling (D6).** `downsample::reduce` keeps the spec's mask on
the reduced image. When it averages (gray or colour class, `/Average`)
it first replaces a key with `ImageSpec::key_to_stencil` of the
unreduced samples; subsampling, the one-bit class (always subsampled),
and the unreduced cases (Indexed, other depths, DCT) keep the key. A
stencil is never resampled. Two unit tests (a stencil kept at its
resolution under both methods; a key turned into a stencil when
averaged and kept when subsampled or one-bit).

**Corpus (D10).** Painting files under `corpus/unit/graphics/`, each
drawn over a striped backdrop so masked areas show it:
`masked-row-mask-taller.ps` and `masked-row-image-taller.ps`
(interleave 2, both ratios, hexadecimal data on the current file, a
stroke after it), `masked-sample-rgb.ps` (interleave 1 in DeviceRGB,
mask bytes 128 and 1 counting as 255), `masked-separate-sources.ps`
(interleave 3, 2×2 image, 8×8 mask), `masked-explicit-inverted.ps`
(a 4×4 mask with `Decode [1 0]`), `masked-colour-key-exact.ps`,
`masked-colour-key-ranges.ps`, `masked-colour-key-before-decode.ps`
(4-bit gray, `Decode [1 0]`, `MaskColor [0 3]`),
`masked-short-data.ps` (the data procedure ends after half the rows),
`masked-colour-key-cie.ps` (the monitor-like CIEBasedABC space of the
CIE corpus, with only a grey and a muted red painted, so it needs no
`cie-rendering-path` divergence), and `masked-indexed-runlength.ps`,
the construction that prompted the change: a 32×32 Indexed image over
an own four-colour palette, a disc-shaped mask with `Decode [1 0]`,
interleave 2 at equal heights, the run-length data binary inside a
`%%BeginData: … Binary Bytes` section, `save`/`restore` around it and
a stroked frame after it. `corpus/unit/policy/downsample-colour-key-averaged.ps`
is the remelt delta's averaged colour-key scenario: its PDF golden has
a 75×75 image whose `/Mask` is a 300×300 stencil. IR and PDF goldens
were written by `difftest run --update-ir --update-pdf` for these
twelve files only. Two error files changed: `masked-dct-interleaved.ps`
now carries the project's own baseline JPEG (`cargo xtask tiny-jpeg`)
so the reference decodes it and paints (its 256 samples are short of
the 288 bytes the interleaved layout needs, which only cuts its
image); `masked-mask-offset.ps` declares `masked-mask-misaligned`,
counted in the oracle's slug inventory test.

**Oracle tier** (profile `default`; the installed reference is an
earlier release than the one the profile pins). Pass: all eleven
painting files above, the policy file, and the seven error files
other than the two below (the reference raises an error there too).
Expected divergence: `masked-dct-interleaved.ps` (the reference
paints one page), `masked-mask-offset.ps` (the reference ends
normally). The policy file's plain run notes an error in the
reference, as `downsample-gray-300-to-72.ps` does: the plain run has
no distillation parameters; its pages still pass. The external
checker reports every new PDF golden clean. Each new golden was
rendered and inspected: masked cells show the backdrop, painted cells
show the expected samples (the Indexed file a ringed disc inside a
stroked frame; the short-data file only its upper half).

**Gates.** `cargo test --workspace` 1296 passed (1289 before), 4
ignored; clippy 0 warnings; `cargo fmt --check` clean; `difftest run`
385 files, 385 passed, only added goldens; `parse-survival` 385
files, 0 failed; `fuzz-round` 1300 + 1300 programs, 0 failed;
`lint-strings` clean; `openspec validate` valid. `check-wasm` could
not run here (no WebAssembly target installed).
