# text Specification

## Purpose
Font dictionaries and the font operators in the VM, the resident standard
fonts' metrics and name substitution, Type 3 glyph execution, and what
`stringwidth` and the `show` family observably do.

## Requirements

### Requirement: Font dictionaries and the font directory

`definefont` SHALL validate a font dictionary (a `FontType` of 1, 3, or
42, a six-element `FontMatrix`, a 256-element `Encoding`; Type 3 also a
`BuildGlyph` or `BuildChar` procedure), add a unique `FID`, make the
dictionary read-only, register it under its key in `FontDirectory` (or
`GlobalFontDirectory` in global allocation mode), and return it.
`findfont` SHALL return the font registered under a key; `scalefont` and
`makefont` SHALL return a new font whose `FontMatrix` is the original's
composed with the scale or matrix, sharing every other entry; `setfont`
SHALL make a font current in the graphics state, `currentfont` SHALL
return it, and `selectfont` SHALL combine `findfont`, scaling, and
`setfont`. The current font SHALL be saved and restored by
`gsave`/`grestore` and `save`/`restore`.

#### Scenario: A defined font round-trips

- **GIVEN** a Type 3 font dictionary defined as `/Sq` and then
  `/Sq findfont 10 scalefont setfont currentfont /FontMatrix get`
- **THEN** the matrix printed is `[10 0 0 10 0 0]` and `/Sq findfont /FID
  known` is `true`

#### Scenario: Invalid font dictionaries are rejected

- **GIVEN** `/Bad 3 dict definefont`
- **THEN** the error is `invalidfont`

#### Scenario: The font follows the graphics state

- **GIVEN** `/Helvetica findfont 12 scalefont setfont gsave /Courier
  findfont 8 scalefont setfont grestore currentfont /FontName get`
- **THEN** the name printed is `Helvetica`

### Requirement: Resident fonts with correct metrics

The fourteen standard fonts (Courier, Helvetica, and Times in four
styles each, Symbol, and ZapfDingbats) SHALL be available to `findfont`
without being defined by the job, as Type 1 font dictionaries with the
`FontMatrix` `[0.001 0 0 0.001 0 0]`, their font bounding box, and their
built-in encoding (`StandardEncoding` for the text fonts, the font's own
for Symbol and ZapfDingbats). `stringwidth` SHALL return the sum of the
glyph widths of the string's characters through the current encoding,
transformed by the font matrix into user space, and the `show` family
SHALL advance the current point by the same amount.

#### Scenario: Helvetica widths

- **GIVEN** `/Helvetica findfont 12 scalefont setfont (Hello) stringwidth`
- **THEN** the results printed are `27.336 0`

#### Scenario: Show advances the current point

- **GIVEN** `/Courier findfont 10 scalefont setfont 100 100 moveto (abc)
  show currentpoint`
- **THEN** the results printed are `118 100`

#### Scenario: Re-encoding changes widths

- **GIVEN** a copy of Helvetica whose `Encoding` maps code 65 to `/W`,
  defined as `/H2`, and `/H2 findfont 10 scalefont setfont (A) stringwidth`
- **THEN** the results printed are `9.44 0`

### Requirement: Font name substitution

`findfont` of a key not registered in the font directories SHALL resolve
it to one of the fourteen standard fonts through an alias table covering
the metrically paired families and their common PostScript-name variants
and, failing that, a style heuristic on the name (monospace, serif,
symbol, and dingbat hints; bold and italic hints), defaulting to
Helvetica. The returned font's `FontName` SHALL be the substitute's name.
The resource operators SHALL report exactly the fourteen names as
resident. This substitution is an expected divergence from the
`invalidfont` error the reference specifies and SHALL be switchable off
in the interpreter configuration, in which case `findfont` SHALL raise
`invalidfont`.

#### Scenario: Metric-pair alias

- **GIVEN** `/Arial-BoldMT findfont /FontName get`
- **THEN** the name printed is `Helvetica-Bold`

#### Scenario: Style heuristic

- **GIVEN** `/Garamond-Italic findfont /FontName get` and
  `/LucidaConsole findfont /FontName get`
- **THEN** the names printed are `Times-Italic` and `Courier`

#### Scenario: Substitution switched off

- **GIVEN** an interpreter configured without substitution and
  `/NoSuchFont findfont`
- **THEN** the error is `invalidfont`

#### Scenario: Resident set reported

- **GIVEN** `/Helvetica /Font resourcestatus` and `/Arial /Font
  resourcestatus`
- **THEN** the first leaves `status size true` with status 2 and the
  second leaves `false`

### Requirement: Type 3 fonts execute natively

For a Type 3 font, each character shown SHALL run `BuildGlyph` (or
`BuildChar` with the character code) with the font dictionary and the
glyph name (or code) on the operand stack, inside its own graphics state
whose CTM is the font matrix composed with the current CTM translated to
the current point; `setcachedevice`, `setcachedevice2`, and
`setcharwidth` SHALL record the glyph's width, and the current point
SHALL advance by that width through the font matrix. `stringwidth` on a
Type 3 font SHALL run the glyph procedures without painting anything.
A glyph procedure that paints SHALL produce marks on the page attributed
to that glyph.

#### Scenario: A square glyph

- **GIVEN** a Type 3 font whose `BuildGlyph` sets width 1000 and fills
  the unit-em square, `FontMatrix [0.001 0 0 0.001 0 0]`, shown once at
  size 20 from (10,10)
- **THEN** the page's IR contains one text operation for the glyph whose
  captured procedure fills a square, `currentpoint` afterwards is
  `30 10`, and the dump's golden pins the glyph procedure

#### Scenario: stringwidth paints nothing

- **GIVEN** the same font and `(a) stringwidth` before any painting
- **THEN** the results are `20 0` and the page has no operations

#### Scenario: A glyph shown twice yields one procedure

- **GIVEN** the same font and `(aa) show`
- **THEN** the page's resources hold one glyph procedure for the font and
  the text operation lists the glyph twice with its displacement

### Requirement: The show family and glyph names

`show`, `ashow`, `widthshow`, `awidthshow`, `kshow`, `xshow`, `yshow`,
`xyshow`, and `glyphshow` SHALL paint glyphs per the reference's
semantics for their extra displacements and per-glyph procedures, each
character's displacement SHALL be recorded with its glyph, and `kshow`
SHALL run its procedure between consecutive glyphs with the two codes on
the stack. `charpath` SHALL be registered and raise `invalidfont` until
outlines are available.

#### Scenario: widthshow adds to spaces

- **GIVEN** `/Courier findfont 10 scalefont setfont 0 0 moveto 5 0 32
  (a b) widthshow currentpoint`
- **THEN** the results printed are `23 0`

#### Scenario: xshow positions each glyph

- **GIVEN** `/Helvetica findfont 10 scalefont setfont 0 0 moveto (abc)
  [10 20 30] xshow currentpoint`
- **THEN** the results printed are `60 0` and the IR's text operation
  carries displacements of 1000, 2000, and 3000 glyph units

#### Scenario: kshow runs between glyphs

- **GIVEN** `{ exch = = } (ab) kshow` in a resident font
- **THEN** the output is `97` then `98`

### Requirement: Encodings and resource categories

`StandardEncoding` and `ISOLatin1Encoding` SHALL be read-only 256-element
name arrays in `systemdict`. `findresource`, `resourcestatus`,
`defineresource`, `undefineresource`, and `resourceforall` SHALL operate
on the `Font` category (resident fonts and `definefont` results) and the
`Encoding` category (the two encodings); an unknown category SHALL raise
`undefined`; a resource not found SHALL raise `undefined` from
`findresource` and leave `false` from `resourcestatus`.

#### Scenario: Encoding resource

- **GIVEN** `/ISOLatin1Encoding /Encoding findresource 233 get`
- **THEN** the name printed is `eacute`

#### Scenario: Font category lists the resident set

- **GIVEN** `(*) { == } 128 string /Font resourceforall`
- **THEN** the fourteen standard names are printed in sorted order
