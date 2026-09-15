## Purpose

User paths: the operators that build, append, and paint a path held
as a self-contained object, in the literal array form and the encoded
binary form.

## ADDED Requirements

### Requirement: User path operators

`uappend` SHALL append a user path to the current path, interpreting
its elements per PLRM3 §4.6; `ufill`, `ueofill`, and `ustroke` SHALL
paint a user path as a fresh path inside a saved graphics state, with
`ustroke` also accepting a matrix concatenated before the stroke;
`upath` SHALL return the current path as a user path array, beginning
with its bounding box and `setbbox`, and marked cacheable when its
operand is `true`; `setbbox` SHALL record the bounding box of the path
being built; `ucache` SHALL be accepted and ignored; `ucachestatus`
SHALL leave a mark and five integers and `setucacheparams` SHALL
consume operands down to a mark. A user path that is not an array or
packed array, or contains an element that is not a number or one of
the allowed operators, SHALL raise `typecheck`; a coordinate outside
the declared bounding box SHALL raise `rangecheck`; a path missing
`setbbox` first SHALL raise `typecheck`.

#### Scenario: A user path round-trips

- **WHEN** `{0 0 100 100 setbbox 10 10 moveto 90 90 lineto closepath} cvlit` is filled with `ufill` and then, after appending it with `uappend`, `false upath` is taken
- **THEN** the fill matches a `moveto`/`lineto`/`closepath`/`fill` of the same points and the returned user path begins with the path's own bounding box `10 10 90 90` and `setbbox`

#### Scenario: A stroke through a matrix

- **WHEN** a user path is stroked with `ustroke` and the matrix `[2 0 0 2 0 0]`
- **THEN** the stroke's geometry and line width are those of the path under a CTM doubled before the stroke

#### Scenario: Out of the box

- **WHEN** a user path declares `0 0 10 10 setbbox` and then `20 20 lineto`
- **THEN** `ufill` raises `rangecheck`

### Requirement: Encoded user paths

A user path given as an array of two strings — a homogeneous number
array of operands and a string of operator codes with repeat counts,
per PLRM3 §4.6.2 and §3.14.5 — SHALL be interpreted identically to
the literal array form, for every number representation the
homogeneous number array format allows.

#### Scenario: Encoded and literal agree

- **WHEN** the same square is painted from a literal user path and from its encoded form (32-bit fixed-point numbers, operator string with `moveto`, three `lineto`s, and `closepath`)
- **THEN** the two fills are identical in the delivered page
