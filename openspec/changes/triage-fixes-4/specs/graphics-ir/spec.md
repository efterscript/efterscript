## ADDED Requirements

### Requirement: Stroke outlines

`strokepath` SHALL replace the current path with closed subpaths
enclosing the area `stroke` would paint with the current line width,
cap, join, mitre limit, and dash, flattened by the current flatness,
so that a nonzero `fill` of the result paints what the stroke would;
a degenerate subpath SHALL follow `stroke`'s rules. `ustrokepath` in
both forms SHALL do the same for a user path, the second form
concatenating its matrix for the outline only, per their entries in
PLRM3 §8.2.

#### Scenario: A stroke's outline fills as the stroke

- **WHEN** a line of width 20 with round caps is outlined with `strokepath` and filled, and the same line is stroked on another page
- **THEN** both pages paint the same area, and the first page's IR carries a fill of closed subpaths and no stroke

#### Scenario: A user path's outline

- **WHEN** a user path is outlined with `ustrokepath` and the matrix `[2 0 0 2 0 0]`
- **THEN** the outline has twice the width and the CTM is unchanged afterwards

### Requirement: Stroke adjustment and overprint in the state

`setstrokeadjust` and `currentstrokeadjust` SHALL keep a boolean in
the graphics state that `initgraphics` does not reset and a glyph
procedure starts with `false`; `setoverprint` and `currentoverprint`
SHALL keep a boolean in the graphics state, default `false`, saved
and restored with it, and the IR SHALL carry the overprint setting in
effect where a paint occurs, dumped as a state line only where it
changes.

#### Scenario: Overprint is recorded

- **WHEN** `true setoverprint` precedes a fill in a Separation space and `false setoverprint` a second fill
- **THEN** the dump shows an overprint setting before the first fill and its reset before the second, and `currentoverprint` after a `gsave … grestore` round trip is unchanged

### Requirement: The page device follows the graphics state

The page device dictionary SHALL be part of the graphics state:
`grestore`, `grestoreall`, and `restore` SHALL bring back the page
device in effect at the matching `gsave` or `save`, per PLRM3 §6.1.1,
and `currentpagedevice` SHALL answer the current state's.

#### Scenario: A page size set inside gsave reverts

- **WHEN** `gsave << /PageSize [200 200] >> setpagedevice grestore currentpagedevice /PageSize get` is executed after a default page device
- **THEN** the result is the default page size and a page shown afterwards has the default media box

### Requirement: pathbbox rules and reading precision

`pathbbox` SHALL enclose curve control points as well as segment
ends, SHALL ignore a trailing `moveto`, SHALL answer from a declared
`setbbox` when one is in effect, and SHALL raise `nocurrentpoint` on
an empty path; `pathbbox`, `currentpoint`, and `arcto`'s tangent
points SHALL be computed in double precision through the CTM and its
inverse before being rounded once.

#### Scenario: Control points widen the box

- **WHEN** `0 0 moveto 0 100 100 100 100 0 curveto pathbbox` is executed
- **THEN** the box is `0 0 100 100`, and with `50 50 moveto` appended it is unchanged

#### Scenario: Tangent points at a rotated corner

- **WHEN** `-45 rotate 440 404 moveto 464 404 371 414 50 arcto` is executed
- **THEN** the four readings agree with the double-precision computation to six significant digits
