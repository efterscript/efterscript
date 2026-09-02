# font-programs

## MODIFIED Requirements

### Requirement: charpath appends outlines

`charpath` SHALL append the outlines of the string's glyphs to the
current path, transformed by the font matrix, the current point, and the
CTM, and advance the current point as `show` would; the boolean operand
SHALL be accepted and, for fonts whose `PaintType` is 0, ignored. It
SHALL work for Type 1 and Type 42 fonts defined by the job and for
resident fonts backed by an outline asset; for Symbol, ZapfDingbats, and
Type 3 fonts it SHALL raise `invalidfont`.

#### Scenario: Filling a charpath

- **GIVEN** a synthesised Type 1 font with a square glyph, `100 100
  moveto (a) false charpath fill showpage`
- **THEN** the page's IR contains one fill of the square translated to
  (100,100) and scaled by the font size, and no text operation

#### Scenario: charpath advances

- **GIVEN** `0 0 moveto (aa) false charpath currentpoint` in that font
  at size 10
- **THEN** the results printed are `12 0`

#### Scenario: Resident charpath

- **GIVEN** `/Times-Roman findfont 48 scalefont setfont 72 72 moveto (Ab)
  false charpath fill showpage`
- **THEN** the page's IR contains fills of the two glyph outlines and no
  text operation, the current point after `charpath` is advanced by the
  AFM widths, and the dump's golden pins the outlines

#### Scenario: Symbol has no outlines

- **GIVEN** `/Symbol findfont 10 scalefont setfont 0 0 moveto (a) false
  charpath`
- **THEN** the error is `invalidfont`
