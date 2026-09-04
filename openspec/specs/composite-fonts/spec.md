# composite-fonts Specification

## Purpose
CMaps and their resources, CID-keyed fonts of both types and all loading
forms, Type 0 fonts and `composefont`, and how composite text measures,
draws, and outlines.

## Requirements

### Requirement: CMaps

The `CIDInit` procedure set SHALL be available through `/CIDInit
/ProcSet findresource`, and an embedded CMap program using its
operators (codespace ranges, CID ranges and single mappings, notdef
ranges, `usecmap`, `CMapName`, `WMode`, `CIDSystemInfo`) SHALL define a
`CMap` resource. `Identity-H` and `Identity-V` SHALL be predefined
`CMap` resources. Decoding SHALL follow the codespace rule: the byte
length is that of the codespace range containing the bytes; bytes
matching no range decode as a notdef of the shortest partially matching
length, else one byte.

#### Scenario: An embedded CMap

- **GIVEN** a corpus CMap with a one-byte range `<20>-<7E>` mapping to
  CIDs from 1 and a two-byte range `<8140>-<817E>` mapping from 200,
  defined as `/Syn-H`
- **WHEN** `/Syn-H /CMap resourcestatus` runs
- **THEN** it leaves `true` with status 0

#### Scenario: Identity is predefined

- **GIVEN** `/Identity-H /CMap findresource /WMode get` and
  `/Identity-V /CMap findresource /WMode get`
- **THEN** the values printed are `0` and `1`

#### Scenario: Partial match decodes as notdef

- **GIVEN** the corpus CMap and a string whose second byte falls
  outside every two-byte range after a valid lead byte
- **WHEN** it is shown
- **THEN** the run records one notdef glyph of two bytes and continues
  with the next code

### Requirement: CID-keyed fonts

A `CIDFont` resource category SHALL exist. `StartData` of a FontSet
whose CFF is CID-keyed SHALL define a `CIDFontType 0` resource named
by the font, with `CIDSystemInfo`, `CIDCount`, `FontMatrix`, and
`FontBBox`. The `CIDInit` `StartData` form (font dictionary array,
`GlyphData` with `CIDMap`, `FDBytes`, `GDBytes`, Type 1 charstrings)
SHALL define a `CIDFontType 0` resource too. A `CIDFontType 2`
dictionary with `sfnts` and `CIDMap` SHALL define through
`defineresource` and `definefont`. Glyphs SHALL be found by CID through
the program's charset, the glyph data map, or the CID map respectively,
with advances from the program.

#### Scenario: CID-keyed CFF from a FontSet

- **GIVEN** a corpus FontSet holding a CID-keyed CFF `SynCID` with CIDs
  1 and 2 in different font dictionaries
- **WHEN** `/SynCID /CIDFont resourcestatus` runs
- **THEN** it leaves `true`, and `/SynCID /CIDFont findresource
  /CIDCount get` prints the count

#### Scenario: CIDFontType 2 by CID map

- **GIVEN** a synthesised TrueType wrapped as `CIDFontType 2` with a
  `CIDMap` mapping CID 3 to glyph 1
- **WHEN** CID 3 is shown through `Identity-H`
- **THEN** the advance is glyph 1's, scaled by the units per em

#### Scenario: Type 1 charstring CID font

- **GIVEN** a corpus CIDFont in the `CIDInit` `StartData` form with two
  font dictionaries and three glyphs
- **THEN** each CID's advance and outline come from its own
  dictionary's private data, and `charpath` yields the outline

### Requirement: Type 0 fonts

`composefont` SHALL take a name, a CMap (or its name), and a descendant
array and define a Type 0 font; `definefont` SHALL accept a FontType 0
dictionary with `FMapType 9`, a `CMap`, an `FDepVector`, and an
`Encoding`, and SHALL raise `invalidfont` for other map types. The
font's `FontMatrix` composes with the descendant's.

#### Scenario: composefont

- **GIVEN** `/SynComposite /Identity-H [ /SynCID /CIDFont findresource ]
  composefont pop` and `/SynComposite findfont /FontType get`
- **THEN** the value printed is `0`

#### Scenario: Older map types rejected

- **GIVEN** a dictionary with `FontType 0` and `FMapType 2`
- **WHEN** it is defined
- **THEN** the error is `invalidfont`

### Requirement: Composite text

With a Type 0 font current, `show` and its family, `stringwidth`,
`glyphshow` (by CID name form is not required), and `charpath` SHALL
decode the string through the CMap, select the descendant by the CMap's
font number through `Encoding`, and measure, paint, or outline each CID
through the descendant; `xshow`, `yshow`, and `xyshow` SHALL consume one
displacement per decoded code. In writing mode 1 the current point
SHALL advance downward by the default vertical advance with the glyph
positioned by the default vertical origin.

#### Scenario: Two-byte show through Identity

- **GIVEN** `SynCID` with CIDs 1 and 2 of advances 500 and 700,
  `/SynComposite findfont 10 scalefont setfont 0 0 moveto <00010002>
  show currentpoint`
- **THEN** the results printed are `12 0`

#### Scenario: Mixed byte lengths

- **GIVEN** the corpus CMap `Syn-H` over `SynCID` and the string
  `<41 8140 42>` (one-, two-, one-byte codes)
- **THEN** the run holds three glyphs with codes of lengths 1, 2, and 1
  and CIDs from the ranges

#### Scenario: Vertical writing

- **GIVEN** the same font through `Identity-V` at size 10, `0 100 moveto
  <0001> show currentpoint`
- **THEN** the results printed are `0 90`

#### Scenario: charpath through a composite font

- **GIVEN** `<0001> false charpath` in `SynComposite`
- **THEN** the current path holds CID 1's outline and the current point
  advanced by its width
