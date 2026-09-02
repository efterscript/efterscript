# graphics-ir

## ADDED Requirements

### Requirement: Text produces IR

The graphics state SHALL hold the current font as an opaque font
reference and matrix that `gsave`/`grestore` save and restore. Showing
text SHALL emit one text operation carrying a font resource, the matrix
mapping glyph space to default user space at the start of the run, and
the glyphs shown, each with the displacement applied after it; the
graphics settings a text paint depends on (colour) SHALL be emitted
lazily as for fills. Font resources SHALL be either a resident standard
font with its encoding, or a Type 3 font with its font matrix, encoding,
and the captured procedure of every glyph shown on the page, and SHALL be
interned per page.

#### Scenario: Text operation shape

- **GIVEN** `/Helvetica findfont 12 scalefont setfont 100 700 moveto
  (Hi) show showpage`
- **THEN** the page holds one font resource for Helvetica with the
  standard encoding, and one text operation with matrix
  `[0.012 0 0 0.012 100 700]`, glyph codes 72 and 105 with displacements
  722 and 222 in glyph space

#### Scenario: Type 3 glyphs are captured in glyph space

- **GIVEN** a Type 3 font with `FontMatrix [0.01 0 0 0.01 0 0]` whose
  glyph fills `0 0 50 50` in glyph units, shown at `2 2 scale`
- **THEN** the captured glyph procedure's fill is the rectangle 0 0 50 50
  in glyph space, and the text operation's matrix is `[0.02 0 0 0.02 tx ty]`

#### Scenario: Text under a clip and colour

- **GIVEN** a clip, `1 0 0 setrgbcolor`, then `show`
- **THEN** the dump shows `q`, the clip, the colour setting, then the
  text operation, and nothing else

### Requirement: Text in the dump

The `ir/1` dump SHALL list font resources as `font <n> …` lines (a
resident font by name with its encoding differences from the built-in
encoding; a Type 3 font by matrix followed by one `glyph` block per
captured glyph with its width and procedure lines indented) and text
operations as `text <font> <a> <b> <c> <d> <tx> <ty> (<bytes>) <dx> <dy>…`
lines; dumps of pages without text SHALL be unchanged.

#### Scenario: Existing goldens unchanged

- **WHEN** `difftest run` executes after this change
- **THEN** every pre-existing `.ir` and `.pdf` golden still matches
