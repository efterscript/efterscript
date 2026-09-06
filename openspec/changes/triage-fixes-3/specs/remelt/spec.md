# remelt

## ADDED Requirements

### Requirement: Stroke colour follows the current colour

Every colour-space and colour setting written to a content stream SHALL
be written for both painting operations: the non-stroking operators for
fills, text, and image masks, and the stroking operators for strokes,
so a stroke is painted in the colour current when it was painted.

#### Scenario: A red stroke

- **GIVEN** `1 0 0 setrgbcolor 4 setlinewidth 100 100 moveto 300 300
  lineto stroke showpage`
- **THEN** the content stream sets the stroking colour to red before the
  stroke, and the rendered page shows a red line

#### Scenario: A Separation stroke

- **GIVEN** a stroke in a Separation colour space
- **THEN** the stroking colour space and colour are set from the same
  resource as the non-stroking ones
