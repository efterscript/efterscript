# font-programs

## Purpose

Executing embedded font programs: `eexec`, the interpretation of Type 1
and Type 42 programs into glyph advances and outlines, `charpath`, and
the subsetting and embedding of those programs in the output.

## ADDED Requirements

### Requirement: eexec decrypts and executes

`eexec` SHALL take a file or string, decrypt its contents with the
standard `eexec` key in hexadecimal or binary form (detected from the
first four bytes), discard the four leading bytes, and execute the
plaintext as a source with `systemdict` pushed on the dictionary stack
for its duration. Inside the layer `currentfile` SHALL return the layer,
`readstring` and `token` SHALL read decrypted bytes, and `closefile` on
the layer SHALL end it, pop `systemdict`, and leave the underlying file
positioned after the last byte the layer consumed.

#### Scenario: A hexadecimal eexec section

- **GIVEN** a program whose `eexec` section, when decrypted, is
  `/x 42 def mark currentfile closefile`, followed by zeros and
  `cleartomark x =`
- **THEN** the output is `42`

#### Scenario: A binary eexec section

- **GIVEN** the same section in binary form
- **THEN** the output is `42`

#### Scenario: eexec on a string

- **GIVEN** a string holding an encrypted `(hi) print`
- **WHEN** `eexec` is applied to it
- **THEN** the output is `hi`

### Requirement: Type 1 programs measure and draw

After `definefont` of a Type 1 dictionary with `CharStrings` and
`Private`, `stringwidth` and the `show` family SHALL use each glyph's
charstring advance, and painting SHALL use its outline. The charstring
interpreter SHALL support the Type 1 operator set including `Subrs`,
`seac` composition through `StandardEncoding`, and flex through the
other-subroutine convention; hint operators SHALL be accepted and
ignored; a malformed charstring SHALL raise `invalidfont`.

#### Scenario: Width from the charstring

- **GIVEN** a synthesised Type 1 font whose glyph `a` has advance 600 in
  a 1000-unit em, defined through a hexadecimal `eexec` program, and
  `/Syn findfont 10 scalefont setfont (aa) stringwidth`
- **THEN** the results printed are `12 0`

#### Scenario: A composed glyph

- **GIVEN** the same font with `eacute` defined by `seac` from `e` and
  `acute`
- **WHEN** `/eacute glyphshow` runs at size 10 from (0,0)
- **THEN** the IR text operation carries the glyph with the `e`
  advance, and `charpath` of the same glyph yields the two components'
  outlines

#### Scenario: A malformed charstring

- **GIVEN** a font whose glyph `b` charstring ends mid-operator
- **WHEN** `(b) show` runs
- **THEN** the error is `invalidfont`

### Requirement: Type 42 programs measure and draw

After `definefont` of a Type 42 dictionary with `sfnts` and
`CharStrings`, glyphs SHALL be found by name through `CharStrings` to a
glyph index, advances SHALL come from the horizontal metrics scaled to
the font's units per em, and outlines from the glyph table with
composite glyphs resolved and quadratic contours converted to cubic
segments.

#### Scenario: Advance from the metrics

- **GIVEN** a synthesised TrueType font with 2048 units per em whose
  glyph `a` advances 1024, wrapped as Type 42, and `/SynTT findfont 20
  scalefont setfont (a) stringwidth`
- **THEN** the results printed are `10 0`

#### Scenario: Outline conversion

- **GIVEN** the same font whose glyph `o` is one quadratic contour
- **WHEN** `(o) true charpath pathbbox` runs
- **THEN** the bounding box matches the contour's control box scaled by
  the font size

### Requirement: charpath appends outlines

`charpath` SHALL append the outlines of the string's glyphs to the
current path, transformed by the font matrix, the current point, and the
CTM, and advance the current point as `show` would; the boolean operand
SHALL be accepted and, for fonts whose `PaintType` is 0, ignored. For a
resident font or a Type 3 font `charpath` SHALL raise `invalidfont`.

#### Scenario: Filling a charpath

- **GIVEN** a synthesised Type 1 font with a square glyph, `100 100
  moveto (a) false charpath fill showpage`
- **THEN** the page's IR contains one fill of the square translated to
  (100,100) and scaled by the font size, and no text operation

#### Scenario: charpath advances

- **GIVEN** `0 0 moveto (aa) false charpath currentpoint` in that font
  at size 10
- **THEN** the results printed are `12 0`

### Requirement: Embedded programs are subset and embedded

For every embedded font used on a page, the output SHALL contain the
font program restricted to the glyphs used in the document plus
`.notdef` and any `seac` components: a Type 1 program regenerated from
the font dictionary (cleartext entries, the private dictionary and
subroutines, the used charstrings re-encrypted) as `FontFile` with its
three lengths; a TrueType program rewritten with only the used glyphs,
their metrics, and a (3,0) cmap mapping each used code to its glyph, as
`FontFile2`. Each SHALL carry a `FontDescriptor` derived from the program
and widths for every used code. Subsetted fonts SHALL be named with a
six-letter tag prefix. The regenerated Type 1 program SHALL be accepted
by the interpreter itself and define the same glyphs.

#### Scenario: Type 1 subset round-trips

- **GIVEN** a synthesised Type 1 font with glyphs `a`, `b`, `c` where only
  `a` is shown
- **WHEN** the embedded `FontFile` is extracted and run through the
  interpreter
- **THEN** it defines a font whose `CharStrings` has exactly `.notdef`
  and `a`, and `a` has the same advance and outline

#### Scenario: TrueType subset has a cmap

- **GIVEN** a synthesised TrueType font where codes 65 and 66 are shown
- **THEN** the embedded `FontFile2` parses with two glyphs plus the
  notdef, a (3,0) cmap mapping 65 and 66, and the font dictionary's
  widths for 65 and 66

#### Scenario: A checker accepts embedded output

- **WHEN** `EFTERSCRIPT_PDF_CHECK` names a checker and `difftest run`
  executes
- **THEN** every fonts corpus golden is accepted and its text extracts
