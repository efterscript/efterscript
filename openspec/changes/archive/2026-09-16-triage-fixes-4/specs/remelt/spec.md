## ADDED Requirements

### Requirement: Overprint in the PDF

An overprint setting in the IR SHALL be written as an extended
graphics state resource carrying `OP` and `op` with that value,
selected with `gs` inside the saved state where the setting applies,
one resource per distinct value, per ISO 32000-1 §8.4.5.

#### Scenario: An overprinted separation fill

- **WHEN** a page fills in a Separation space with overprint on
- **THEN** the page's resources hold an `ExtGState` with `/OP true /op true`, the content stream selects it before the fill, and a checker accepts the file
