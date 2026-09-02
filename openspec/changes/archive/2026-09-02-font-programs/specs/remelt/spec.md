# remelt

## ADDED Requirements

### Requirement: Embedded fonts in the PDF

An embedded-font resource SHALL become a font dictionary (`Type1` or
`TrueType`) whose descriptor embeds the subset program, with widths for
the used codes, encoding differences (Type 1) or a symbolic flag with the
synthesised cmap (TrueType), and a ToUnicode CMap from the program's
glyph names. The subset SHALL be computed over the whole document, so
font objects for embedded fonts SHALL be written when the document
finishes; determinism SHALL hold.

#### Scenario: Type 1 embedded

- **GIVEN** the synthesised Type 1 font shown on two pages with different
  glyphs
- **THEN** the document holds one font dictionary with a descriptor
  whose `FontFile` stream has `Length1`, `Length2`, `Length3`, contains
  exactly the glyphs used on both pages plus `.notdef`, and both pages
  reference it

#### Scenario: TrueType embedded

- **GIVEN** the synthesised TrueType font shown once
- **THEN** the font dictionary is `/TrueType` with a descriptor whose
  `FontFile2` is the subset program and flags marking it symbolic, and
  the content stream shows the code
