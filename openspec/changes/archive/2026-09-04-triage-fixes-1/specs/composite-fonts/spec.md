# composite-fonts

## MODIFIED Requirements

### Requirement: CID-keyed fonts

A `CIDFont` resource category SHALL exist. `StartData` of a FontSet
whose CFF is CID-keyed SHALL define a `CIDFontType 0` resource named
by the font, with `CIDSystemInfo`, `CIDCount`, `FontMatrix`, and
`FontBBox`. The `CIDInit` `StartData` form (font dictionary array,
`GlyphData` with `CIDMap`, `FDBytes`, `GDBytes`, Type 1 charstrings)
SHALL define a `CIDFontType 0` resource too: it SHALL take the form
string (`Binary` or `Hex`) and the byte count as operands with the
CIDFont dictionary as the current dictionary, and after defining the
resource SHALL end that dictionary and the procedure set's, so a
CIDFont file in its canonical form (no trailing `end`) leaves the
dictionary stack as it found it. A `CIDFontType 2` dictionary with
`sfnts` and `CIDMap` SHALL define through `defineresource` and
`definefont`. Glyphs SHALL be found by CID through the program's
charset, the glyph data map, or the CID map respectively, with advances
from the program.

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

#### Scenario: CIDInit StartData restores the dictionary stack

- **GIVEN** `countdictstack` before `/CIDInit /ProcSet findresource
  begin` and after the CIDFont's `StartData` has consumed its data
- **THEN** the two counts are equal and the operand stack holds nothing
  else
