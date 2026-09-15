## ADDED Requirements

### Requirement: Calibrated colour spaces are resources

The IR SHALL carry `CalGray` (white point, black point, gamma),
`CalRGB` (white point, black point, three gammas, matrix), and `Lab`
(white point, black point, a*/b* range) as interned colour-space
resources, with colours and image samples in them unconverted, and
the dump SHALL print each with its parameters.

#### Scenario: A CalRGB space in the dump

- **WHEN** a page fills in a collapsed calibrated RGB space
- **THEN** the dump's resource line names `CalRGB` with the white point, gammas, and matrix, and the fill's colour line carries the three components
