## ADDED Requirements

### Requirement: Calibrated colour in the PDF

A `CalGray`, `CalRGB`, or `Lab` resource SHALL be written as the
corresponding colour-space array of ISO 32000-1 §8.6.5 with its
dictionary entries (`WhitePoint`, `BlackPoint` when not the default,
`Gamma`, `Matrix`, `Range`), named in the page's resources and
selected by name; colours in it SHALL be written with their components
as given, and an image in it SHALL name it as its colour space with
the decode array the interpreter supplied.

#### Scenario: A Lab fill

- **WHEN** a page fills a rectangle in a converted CIE-based space
- **THEN** the page's resources hold `[/Lab << /WhitePoint … /Range … >>]`, the content stream selects it by name and sets `L* a* b*` components, and a checker accepts the file
