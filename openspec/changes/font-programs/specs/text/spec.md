# text

## MODIFIED Requirements

### Requirement: The show family and glyph names

`show`, `ashow`, `widthshow`, `awidthshow`, `kshow`, `xshow`, `yshow`,
`xyshow`, and `glyphshow` SHALL paint glyphs per the reference's
semantics for their extra displacements and per-glyph procedures, each
character's displacement SHALL be recorded with its glyph, and `kshow`
SHALL run its procedure between consecutive glyphs with the two codes on
the stack. These operators SHALL work with resident fonts, Type 3 fonts,
and Type 1 and Type 42 fonts defined by the job; a font dictionary with
a `FontType` the interpreter cannot draw SHALL raise `invalidfont`.
`charpath` SHALL append outlines for Type 1 and Type 42 fonts and raise
`invalidfont` for resident and Type 3 fonts.

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

#### Scenario: Showing an embedded font

- **GIVEN** a synthesised Type 1 font defined by the job and `(a) show`
- **THEN** the page's IR has one text operation over an embedded-font
  resource and no error
