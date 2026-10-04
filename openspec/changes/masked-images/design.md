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
  data stage on the data source. The mask-first order is a choice, since
  the manual does not fix one. It is checked against the reference with a
  program whose two procedures read one shared file (black-box). If the
  reference reads the data first, the order flips, and the design's
  implementation notes record it.

A short delivery cuts the image to the rows both parts cover. For
interleave 3, the mask is cut to the same fraction of its height (rounded
up). For interleave 1 and 2, a cut on a block boundary keeps the blocks'
rows of each part. A decode filter is drained to its marker as today, so
a program that put the data inline continues after it.

**D3. Alignment.** Each dictionary's `ImageMatrix` and size give the map
from the unit square to user space, U = flip · scale(w, h) · ImageMatrix⁻¹.
The image is placed by the data dictionary's U. The mask must map onto the
same square: if its U equals the data's to 1e-4 of the square's extent,
it is used as read. If it equals the data's with one or both axes of
the unit square reversed, its rows and/or columns are reversed. Any other
U raises `typecheck`, which PLRM3 Table 4.24's alignment requirement
makes an inconsistency. The driver that motivated this change gives both
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
