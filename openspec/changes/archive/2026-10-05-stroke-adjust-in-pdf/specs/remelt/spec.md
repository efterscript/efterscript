## ADDED Requirements

### Requirement: Stroke adjustment in the PDF

A stroke adjustment setting in the IR SHALL be written as an extended
graphics state resource carrying `SA` with that value, selected with
`gs` at the point the IR sets it, one resource per distinct value used,
listed by every content stream that selects it, per ISO 32000-1 §8.4.5
and §10.7.5. Overprint SHALL keep resources of its own, so selecting
one setting never changes the other.

#### Scenario: A stroked page states stroke adjustment

- **WHEN** a page strokes a line without calling `setstrokeadjust`
- **THEN** the page's resources hold an `ExtGState` with `/SA false`, the content stream selects it before the stroke, and a checker accepts the file

#### Scenario: Both values and overprint on one page

- **WHEN** a page strokes with stroke adjustment off, strokes again with it on, and fills with overprint on
- **THEN** the page's resources hold three extended graphics states, one with `/SA false`, one with `/SA true`, and one with `/OP true /op true`, none carrying another's keys, and a checker accepts the file

#### Scenario: A page without strokes is unchanged

- **WHEN** a page only fills and shows text in a font that is not Type 3
- **THEN** its PDF has no extended graphics state for stroke adjustment
