# text

## MODIFIED Requirements

### Requirement: Resident fonts with correct metrics

Thirty-five faces SHALL be available to `findfont` without being defined
by the job: the fourteen standard fonts (Courier, Helvetica, and Times
in four styles each, Symbol, and ZapfDingbats) and the twenty-one
LaserWriter faces (AvantGarde-Book/BookOblique/Demi/DemiOblique,
Bookman-Light/LightItalic/Demi/DemiItalic, Helvetica-Narrow and its
Bold/Oblique/BoldOblique, NewCenturySchlbk-Roman/Italic/Bold/BoldItalic,
Palatino-Roman/Italic/Bold/BoldItalic, ZapfChancery-MediumItalic), as
Type 1 font dictionaries with the `FontMatrix` `[0.001 0 0 0.001 0 0]`,
their font bounding box, and their built-in encoding (`StandardEncoding`
for the text fonts, the font's own for Symbol and ZapfDingbats).
`stringwidth` SHALL return the sum of the glyph widths of the string's
characters through the current encoding, transformed by the font matrix
into user space, and the `show` family SHALL advance the current point
by the same amount; the fourteen measure with the Core 14 metrics and
the twenty-one with the TeX Gyre metrics.

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

#### Scenario: An extra face measures with its own metrics

- **GIVEN** `/Palatino-Roman findfont 10 scalefont setfont (a) stringwidth`
- **THEN** the results printed are the TeX Gyre Pagella `a` width at
  size 10 (`5 0`), not Times-Roman's `4.44 0`

### Requirement: Font name substitution

`findfont` of a key not registered in the font directories SHALL resolve
it to one of the thirty-five resident faces through an alias table
covering the metrically paired families and their common
PostScript-name variants and, failing that, a style heuristic on the
name (monospace, serif, symbol, and dingbat hints; bold and italic
hints), defaulting to Helvetica. The returned font's `FontName` SHALL be
the substitute's name. The resource operators SHALL report exactly the
thirty-five names as resident. This substitution is an expected
divergence from the `invalidfont` error the reference specifies and
SHALL be switchable off in the interpreter configuration, in which case
`findfont` SHALL raise `invalidfont`.

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

- **GIVEN** `/Helvetica /Font resourcestatus`, `/Palatino-Roman /Font
  resourcestatus`, and `/Arial /Font resourcestatus`
- **THEN** the first two leave `status size true` with status 2 and the
  third leaves `false`

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
- **THEN** the thirty-five resident names are printed in sorted order
