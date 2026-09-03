# remelt

## ADDED Requirements

### Requirement: CFF fonts in the PDF

An embedded-font resource whose program is CFF SHALL become a `Type1`
font dictionary with encoding differences, widths for the used codes,
ToUnicode from glyph names, and a descriptor whose `FontFile3` stream
has subtype `Type1C` and holds the subset program.

#### Scenario: Type1C embedded

- **GIVEN** the synthesised CFF font shown once
- **THEN** the font dictionary is `/Type1`, its descriptor's `FontFile3`
  has `/Subtype /Type1C`, the stream parses as CFF with the used glyph
  and `.notdef`, and a checker accepts the file and extracts the text
