# Change: Composite fonts — CID-keyed fonts, CMaps, Type 0, and PDF Type 0

## Why

Every CJK job, and many Latin jobs from modern drivers, draws text
through composite fonts: a Type 0 font whose CMap turns byte sequences
into CIDs that a CID-keyed font resolves to glyphs. None of that path
exists: `definefont` rejects FontType 0, `CIDInit` is unknown, CID-keyed
CFF data loaded by the previous change is parsed and parked, and the
IR's glyph codes are one byte wide. This change completes the font
milestone by building composite text end to end and fixes the shapes
the rest of the system inherits: a CMap model with codespace decoding,
glyph runs carrying both the original code bytes and the CID, and PDF
Type 0 output with Identity encodings. Widening the glyph code is the
part that is hard to retrofit, which is why it lands here with the
first consumer rather than later.

## What Changes

- **CMaps** (`ps-fonts`, `ps-vm`): a CMap model (codespace ranges of
  one to four bytes, CID ranges and single mappings, notdef ranges,
  chained `usecmap`, writing mode) with the standard decoding rule for
  partial matches; the `CIDInit` procedure set whose operators build a
  CMap from an embedded CMap program; a `CMap` resource category;
  `Identity-H` and `Identity-V` shipped from Adobe's BSD-licensed CMap
  resources with licence and provenance, loaded through the same
  operators.
- **CID-keyed fonts**: a `CIDFont` resource category; `CIDFontType 0`
  from CID-keyed CFF data (defined by `StartData` of a FontSet) and from
  the CIDInit `StartData` form with Type 1 charstrings in `GlyphData`
  addressed through `CIDMap`; `CIDFontType 2` from `sfnts` with
  `CIDMap`; glyphs found by CID with advances from the program.
- **Type 0 fonts**: `FMapType 9` dictionaries with `CMap`, `FDepVector`,
  and `Encoding`; `composefont`; `definefont` accepts FontType 0 of that
  map type and rejects the older map types with `invalidfont`.
- **Composite text**: the show family, `stringwidth`, and `charpath`
  decode strings through the CMap, select the descendant, and take
  widths and outlines by CID; vertical writing mode advances downward
  with the default vertical metrics.
- **IR**: glyph runs carry a code of up to four bytes with its length
  and the CID; the dump writes multi-byte codes in hexadecimal; font
  resources gain a composite kind naming the CMap and the descendant.
- **PDF**: Type 0 fonts with `Identity-H`/`Identity-V`, a
  `CIDFontType0` descendant embedding a CID-keyed CFF subset as
  `FontFile3`/`CIDFontType0C`, or a `CIDFontType2` descendant embedding a
  TrueType subset as `FontFile2` with a `CIDToGIDMap` stream, `W` (and
  `DW`) from advances, ToUnicode from the job's CMap when its name marks
  it Unicode-based, else from the TrueType cmap, else omitted; a
  CID-keyed font with Type 1 charstrings (no PDF embedding form) is
  written as a Type 3 font from its outlines so the output stays
  self-contained.
- **Out of scope, with triggers**: FMapTypes 1–8 (a corpus job using
  one); predefined CJK CMaps beyond Identity (an asset change with a
  loading capability, since the Unicode CMaps are hundreds of
  kilobytes each); explicit vertical metrics `W2` (a corpus job with
  them); `cshow` (with a consumer); converting Type 1 charstrings to
  CFF for embedding (a policy, replacing the Type 3 fallback).

## Capabilities

### New Capabilities
- `composite-fonts`: CMaps and their resources, CID-keyed fonts of both
  types and all loading forms, Type 0 fonts and `composefont`, and how
  composite text measures, draws, and outlines.

### Modified Capabilities
- `graphics-ir`: ADDED requirement for composite glyph runs and font
  resources.
- `remelt`: ADDED requirement for Type 0 output with CID-keyed
  descendants and the Type 3 fallback.
- `text`: ADDED requirement for the `CMap` and `CIDFont` resource
  categories.

## Impact

- Code: `crates/ps-fonts` (`cmap` module, CID-keyed CFF writer,
  CIDFont Type 1-charstring reader, `data/cmap/` assets), `crates/ps-vm`
  (`CIDInit`, resource categories, CIDFont and Type 0 dictionaries,
  `composefont`, composite decoding in the show frame), `crates/ps-vm`
  and `crates/ps-graphics` (the widened `Glyph`), `crates/remelt` (Type
  0 output, CID subsets, the Type 3 fallback), `tools/difftest`,
  `xtask` (asset intake), corpus files under `corpus/unit/fonts/` with
  goldens.
- The `Glyph` value type changes shape; every consumer (dump, content
  writer, tests) is touched, and every existing golden must stay
  byte-identical.
- Dependencies: none new.
- Depends on `cff-fonts` (archived).
