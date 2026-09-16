## ADDED Requirements

### Requirement: Captured glyph metrics are in glyph space

A captured Type 3 glyph's width and bounding box in the IR SHALL be in
glyph space regardless of changes to the CTM inside the glyph
procedure before the metrics were declared, so the dump's glyph line
and the PDF's glyph procedure agree with the marks the glyph paints.

#### Scenario: Metrics agree with the marks

- **WHEN** a glyph procedure scales by 0.5 before declaring width 1200 and box `0 0 1200 1200` and paints a 1200-unit triangle in the scaled space
- **THEN** the dump's glyph line reads width 600 and box `0 0 600 600`, and the captured fill's coordinates lie within that box
