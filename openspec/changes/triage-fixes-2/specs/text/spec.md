# text

## MODIFIED Requirements

### Requirement: Font dictionaries and the font directory

`definefont` SHALL validate a font dictionary (a `FontType` of 1, 2, 3,
or 42, a six-element `FontMatrix`, a 256-element `Encoding`; Type 3 also
a `BuildGlyph` or `BuildChar` procedure; Type 1 also `CharStrings` and
`Private` unless the dictionary is a resident face; Type 42 also `sfnts`
and `CharStrings`), add a unique `FID`, make the dictionary read-only,
register it under its key in `FontDirectory` (or `GlobalFontDirectory`
in global allocation mode), and return it. `findfont` SHALL return the
font registered under a key; `scalefont` and `makefont` SHALL return a
new font whose `FontMatrix` is the original's composed with the scale
or matrix, sharing every other entry; `setfont` SHALL make a font
current in the graphics state, `currentfont` SHALL return it, and
`selectfont` SHALL combine `findfont`, scaling, and `setfont`. The
current font SHALL be saved and restored by `gsave`/`grestore` and
`save`/`restore`.

#### Scenario: A defined font round-trips

- **GIVEN** a Type 3 font dictionary defined as `/Sq` and then
  `/Sq findfont 10 scalefont setfont currentfont /FontMatrix get`
- **THEN** the matrix printed is `[10 0 0 10 0 0]` and `/Sq findfont /FID
  known` is `true`

#### Scenario: Invalid font dictionaries are rejected

- **GIVEN** `/Bad 3 dict definefont`
- **THEN** the error is `invalidfont`

#### Scenario: A Type 1 dictionary without a program

- **GIVEN** a dictionary with `FontType 1`, a matrix, and an encoding
  but no `CharStrings`
- **WHEN** it is defined
- **THEN** the error is `invalidfont` at `definefont`

#### Scenario: The font follows the graphics state

- **GIVEN** `/Helvetica findfont 12 scalefont setfont gsave /Courier
  findfont 8 scalefont setfont grestore currentfont /FontName get`
- **THEN** the name printed is `Helvetica`

### Requirement: Font name substitution

`findfont` of a key not registered in the font directories SHALL resolve
it to one of the thirty-five resident faces through an alias table
covering the metrically paired families and their common
PostScript-name variants and, failing that, a style heuristic on the
name (monospace, serif, symbol, and dingbat hints; bold and italic
hints), defaulting to Helvetica. The returned font's `FontName` SHALL be
the substitute's name. The resource operators SHALL report exactly the
thirty-five names as resident: status 2 before a face has been loaded
into VM and 1 after. This substitution is an expected divergence from
the `invalidfont` error the reference specifies and SHALL be switchable
off in the interpreter configuration, in which case `findfont` SHALL
raise `invalidfont`.

#### Scenario: Metric-pair alias

- **GIVEN** `/Arial-BoldMT findfont /FontName get`
- **THEN** the name printed is `Helvetica-Bold`

#### Scenario: Style heuristic

- **GIVEN** `/Garamond-Italic findfont /FontName get` and
  `/LucidaConsole findfont /FontName get`
- **THEN** the names printed are `Times-Italic` and `Courier`

#### Scenario: LaserWriter family alias

- **GIVEN** `/Palatino-BoldItalic findfont /FontName get` and
  `/BookAntiqua findfont /FontName get`
- **THEN** the names printed are `Palatino-BoldItalic` and
  `Palatino-Roman`

#### Scenario: Substitution switched off

- **GIVEN** an interpreter configured without substitution and
  `/NoSuchFont findfont`
- **THEN** the error is `invalidfont`

#### Scenario: Resident set reported

- **GIVEN** `/Helvetica /Font resourcestatus`, then `/Helvetica findfont
  pop`, then `/Helvetica /Font resourcestatus`, and `/Arial /Font
  resourcestatus`
- **THEN** the statuses are 2, then 1, and the last leaves `false`

### Requirement: CMap and CIDFont categories

The resource operators SHALL also operate on the `CMap` category
(`Identity-H` and `Identity-V` predefined with status 2 until loaded
into VM and 1 after, embedded CMaps defined with status 0) and the
`CIDFont` category (defined by FontSets, `CIDInit` data, or
`defineresource`), and `CIDInit` SHALL be a `ProcSet` resource.

#### Scenario: CMap category listing

- **GIVEN** `(*) { == } 64 string /CMap resourceforall` before any
  embedded CMap
- **THEN** `Identity-H` and `Identity-V` are printed in sorted order

#### Scenario: Loaded CMap status

- **GIVEN** `/Identity-H /CMap resourcestatus` before and after
  `/Identity-H /CMap findresource pop`
- **THEN** the statuses are 2 then 1

## ADDED Requirements

### Requirement: Missing glyph advances by notdef

For a resident face, a code whose encoding name the face lacks SHALL
advance by the face's `.notdef` width: from its metric table when the
table carries one, else from the outline asset's `.notdef` glyph when
outlines are present, else 0.

#### Scenario: Re-encoded to a missing name

- **GIVEN** a re-encoded Palatino-Roman mapping a code to a name the
  face lacks, at size 10
- **THEN** `stringwidth` of that code is the table's `.notdef` width at
  size 10
