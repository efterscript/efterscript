# remelt

## ADDED Requirements

### Requirement: Composite fonts in the PDF

Text in a composite font SHALL be written through a Type 0 font
dictionary with `Identity-H` (or `Identity-V` for writing mode 1) whose
content-stream strings are two-byte CIDs, a `CIDSystemInfo`, a `DW` and
`W` array from the used glyphs' advances, and one descendant: a
`CIDFontType0` with a `FontFile3` of subtype `CIDFontType0C` holding a
CID-keyed CFF subset, or a `CIDFontType2` with a `FontFile2` subset and
a `CIDToGIDMap` stream. ToUnicode SHALL map CIDs through the job's CMap
when the CMap is Unicode-based, else through the TrueType cmap, else be
omitted. A CID-keyed font with Type 1 charstrings SHALL be written as a
Type 3 font whose CharProcs are its used glyphs' outlines.

#### Scenario: CIDFontType0C embedded

- **GIVEN** the two-byte show scenario
- **THEN** the document holds a Type 0 font with `Identity-H`, a
  `CIDFontType0` descendant whose `FontFile3` has `/Subtype
  /CIDFontType0C` and parses as CID-keyed CFF with CIDs 1 and 2, a `W`
  array giving 500 and 700, and the content stream shows `<00010002>`

#### Scenario: CIDFontType2 with a CID map

- **GIVEN** the `CIDFontType 2` scenario
- **THEN** the descendant is `CIDFontType2` with a `FontFile2` subset and
  a `CIDToGIDMap` stream mapping CID 3 to the subset's glyph index

#### Scenario: Unicode-based CMap gives ToUnicode

- **GIVEN** a corpus CMap named `Syn-UCS2-H` mapping `<0041>` to CID 1
- **THEN** the Type 0 font's ToUnicode maps CID 1 to U+0041

#### Scenario: Type 1 charstring CID font falls back to Type 3

- **GIVEN** the Type 1 charstring CIDFont scenario shown once
- **THEN** the page's font is a Type 3 font with one CharProc holding
  the glyph's outline, and a checker accepts the file
