# Change: Masked images — image dictionary types 3 and 4

## Why

`languagelevel` answers 3, but `image` accepts only type 1 image
dictionaries: any other `ImageType` raises `rangecheck`. The two
LanguageLevel 3 forms, the explicit mask (type 3) and colour key masking
(type 4, PLRM3 §4.10.6), are what a driver that believes the device is
LanguageLevel 3 uses to place an image with a transparent background.
A desktop print captured through the emulator pairing from a later
classic driver does exactly that. It asks for the language level, gets
3, and draws every icon as a type 3 image: an Indexed 8-bit image whose
1-bit mask is interleaved by row and run-length encoded. The first icon
ends the job with `rangecheck` in `image` and no page. The same job
completes once the device is made to answer 2, which shows that masked
images are the only gap in it. On that path, though, the driver drops the
masks and paints each icon's transparent area opaque.

This matters now for two reasons. A device that advertises LanguageLevel
3 and refuses its image types fails every driver that trusts the answer,
which is worse than advertising 2. And a hosting application can only work
around it by lowering the language level, which costs exactly the
fidelity this change provides. PDF has an equivalent for both forms (a
stencil-mask image XObject in `/Mask`, and a colour key array in
`/Mask`, ISO 32000-1 §8.9.6.3–4), so the masks carry into the PDF
unchanged, vector-preserving, with no rasterising.

## What Changes

- **`image` accepts type 3 and type 4 dictionaries** (`efterscript-vm`).
  Type 3 takes a `DataDict`, a `MaskDict`, and all three
  `InterleaveType`s: by sample (the mask component in the data source
  ahead of the colour components), by row (blocks of mask rows and image
  rows in one source, the heights in an integral ratio), and separate
  sources. Type 4 takes a type 1 dictionary plus `MaskColor` (n
  exact values or n ranges, compared with the samples before decoding).
  Structural inconsistencies among the dictionaries raise `typecheck`,
  as the manual prescribes. An unknown interleave type raises `rangecheck`.
  `imagemask` still accepts only type 1, and image types other than 1, 3,
  and 4 still raise `rangecheck`.
- **The boundary carries the mask** (`efterscript-vm`, `efterscript-graphics`):
  an image's spec carries either a stencil mask (its own width, height,
  1-bit samples, polarity, and interpolation, aligned with the image's
  unit square) or a colour key (per-component ranges). The IR records it
  and the dump prints it. Pages without masked images dump exactly as
  before.
- **The PDF carries the mask** (`efterscript-remelt`): a stencil mask
  becomes its own image XObject with `ImageMask true`, which the base
  image names in `/Mask`. A colour key becomes the `/Mask` integer array.
  A colour key on samples that are later converted (a CIE-based space
  converted to Lab) or averaged by downsampling is first turned into a
  stencil mask computed from the raw samples, so masking stays exact.
- **Corpus**: synthetic programs under `corpus/unit/graphics/` for each
  interleave type, height ratios in both directions, both mask
  polarities, Indexed and DeviceRGB data, the colour key forms, and the
  error cases. One of them reproduces the driver's construction in the
  project's own words and samples. The captured job goes to the vault and
  is re-run in the private tier.
- **Expected divergence** `masked-dct-interleaved`: a type 3 image whose
  data source is a `DCTDecode` filter and whose mask is interleaved with
  the data (types 1 and 2) is refused, because the interpreter decodes
  no JPEG and so cannot separate the mask from the colour samples.

Out of scope: `MultipleDataSources true`, which a type 1 dictionary
already refuses with `typecheck` and which type 3 interleave type 3 and
type 4 inherit unchanged (trigger: a job that uses it); soft masks and
transparency groups (no PostScript-language source for them).

## Capabilities

### New Capabilities

- `images`: the image operators' dictionary forms as the VM accepts
  them. This change adds the masked image types; it is the home for image
  operator behaviour from now on.

### Modified Capabilities

- `graphics-ir`: images carry a stencil mask or colour key, and the
  dump prints them.
- `remelt`: image XObjects carry `/Mask`; downsampling of masked and
  colour-keyed images.
- `expected-divergences`: the `masked-dct-interleaved` slug.

## Impact

- `efterscript-vm`: `ops/image.rs` (dictionary parsing, acquisition of
  interleaved and separate mask data, mask normalisation), `graphics.rs`
  (`ImageSpec` gains the mask; every constructor of `ImageSpec` sets it),
  `ops/cie.rs` (the conversion job keeps the mask and turns a colour key
  into a stencil mask before converting).
- `efterscript-graphics`: `ir.rs` (the image resource carries its mask),
  `dump.rs` (the `img` line).
- `efterscript-remelt`: `resources.rs` (the mask XObject and `/Mask`),
  `downsample.rs` (masked and colour-keyed images).
- `efterscript-platen`: no API change; a job from a driver that asks for
  LanguageLevel 3 now completes, which a hosting application only sees
  in a new library release.
- Corpus and goldens: new files only; no existing golden may change.
