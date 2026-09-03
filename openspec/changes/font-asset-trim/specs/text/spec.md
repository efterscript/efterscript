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
the twenty-one with metric tables derived from their outline programs.

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
- **THEN** the results printed are the Pagella `a` width at size 10
  (`5 0`), not Times-Roman's `4.44 0`
