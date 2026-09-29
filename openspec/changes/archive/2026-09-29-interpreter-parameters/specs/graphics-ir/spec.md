# graphics-ir

## ADDED Requirements

### Requirement: reversepath

`reversepath` SHALL replace the current path with one whose subpaths
appear in the same order but each traverse their segments in reverse,
per its entry in PLRM3 §8.2: each subpath SHALL begin at the point its
original ended, a curve's control points SHALL be exchanged, a closed
subpath SHALL remain closed, and the current point SHALL become the end
of the last reversed subpath. A path with no segments SHALL be left
unchanged. Painting the reversed path SHALL produce the same IR as
painting the original, apart from segment order.

#### Scenario: A reversed open subpath

- **WHEN** `newpath 10 20 moveto 30 40 lineto 50 20 lineto reversepath currentpoint exch = =` is executed
- **THEN** the output is `10.0` then `20.0`

#### Scenario: Reversal twice is the identity

- **WHEN** a path with a curve and a closed subpath is reversed twice and filled
- **THEN** the page's IR equals that of filling the original path
