# graphics-ir

## ADDED Requirements

### Requirement: Embedded-font resources

A font resource SHALL also be an embedded font: the program snapshot
(Type 1 or TrueType) with its encoding, interned per page by the font's
identity and encoding; text operations over it SHALL carry glyph codes
and displacements as for resident fonts. The dump SHALL list such a
resource as `font <n> embedded <kind> <FontName> glyphs=<count>` with
encoding differences, and SHALL never print program bytes.

#### Scenario: Embedded font in the dump

- **GIVEN** a synthesised Type 1 font `Syn` shown once
- **THEN** the dump's resources contain `font 0 embedded type1 Syn`
  followed by the glyph count, and the text line references font 0
