# graphics-ir

## ADDED Requirements

### Requirement: Clip with an empty path

`clip` and `eoclip` with no current path SHALL make the clip empty:
painting operations SHALL produce nothing until `initclip` or a
`grestore` restores an earlier clip, and the IR SHALL record the empty
clip so the output agrees.

#### Scenario: Nothing paints under an empty clip

- **GIVEN** `newpath clip 0 0 612 792 rectfill showpage`
- **THEN** the page's IR holds an empty clip and no fill, and the
  rendered page is blank

### Requirement: Operand-form image samples are gray

The five-operand `image` SHALL record its samples in DeviceGray
whatever the current colour space; `imagemask` paints the current
colour as before.

#### Scenario: Gray image after a CMYK colour

- **GIVEN** `0 0 0 1 setcmykcolor` followed by a five-operand `image`
- **THEN** the IR image resource is DeviceGray with one component per
  sample

### Requirement: Arc pieces are stable at quarter turns

Cutting a sweep into Bézier pieces SHALL not change piece count when
the sweep is within rounding of a quarter-turn multiple, and `arcto`
tangent points SHALL be computed in double precision.

#### Scenario: A quarter turn from rounding

- **GIVEN** an `arc` whose sweep computes to 90 degrees plus one unit
  in the last place
- **THEN** it produces one piece, identical to the exact quarter turn
