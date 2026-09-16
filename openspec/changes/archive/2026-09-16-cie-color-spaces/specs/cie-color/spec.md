## Purpose

Calibrated colour: how the CIE-based colour space families are
accepted and validated, how their colours map to the PDF calibrated
spaces without loss of device independence, and the colour-rendering
operators and categories that accompany them.

## ADDED Requirements

### Requirement: CIE-based colour spaces are accepted

`setcolorspace` SHALL accept `[/CIEBasedA dict]`, `[/CIEBasedABC
dict]`, `[/CIEBasedDEF dict]`, and `[/CIEBasedDEFG dict]` with the
dictionary entries of PLRM3 §4.8.3: `WhitePoint` required (its Y
component 1, X and Z positive), `BlackPoint`, the `Range*` arrays,
the `Decode*` procedure arrays, the `Matrix*` arrays, and for the
`DEF`/`DEFG` families the `Table` array, each with its default when
absent. A missing `WhitePoint` or `Table` SHALL raise `undefined`, a
wrongly typed entry `typecheck`, an ill-formed value (a white point
with Y ≠ 1, a table whose dimensions or string lengths disagree)
`rangecheck`. The initial colour SHALL be zero in every component, or
the nearest value the component's range allows. `setcolor` SHALL clamp
components to their ranges without error. `currentcolorspace` SHALL
return the array as given and `currentcolor` the program's components;
`currentgray`, `currentrgbcolor`, `currentcmykcolor`, and
`currenthsbcolor` SHALL return the initial value of their device space.

#### Scenario: A calibrated RGB space is selected

- **WHEN** `[/CIEBasedABC << /WhitePoint [0.9505 1 1.089] >>] setcolorspace currentcolor` is executed
- **THEN** the stack holds `0 0 0` and `currentrgbcolor` also leaves `0 0 0`

#### Scenario: A white point is required

- **WHEN** `[/CIEBasedA << >>] setcolorspace` is executed
- **THEN** the error is `undefined`

#### Scenario: Components are clamped to their range

- **WHEN** a `CIEBasedA` space with `/RangeA [0 0.5]` is selected and `2 setcolor currentcolor` is executed
- **THEN** the stack holds `0.5`

### Requirement: Collapsible spaces map to CalGray and CalRGB

A `CIEBasedA` space whose transformation is a single stage — `DecodeA`
absent or a gamma procedure, `MatrixA` the white point, `DecodeLMN`
and `MatrixLMN` absent or identities, ranges the defaults — SHALL be
carried as a `CalGray` space with that gamma; a `CIEBasedABC` space
whose transformation is a single gamma-and-matrix stage (either the
ABC stage with the LMN stage identity, or the ABC stage identity with
the LMN stage carrying gammas and a matrix) SHALL be carried as a
`CalRGB` space with those gammas and that matrix. A gamma procedure is
one that raises its operand to a positive constant power and nothing
else; the identity procedure is the empty procedure. White and black
points SHALL be carried unchanged and the components SHALL pass
through unconverted.

#### Scenario: A gamma gray space

- **WHEN** a `CIEBasedA` space with `/DecodeA {2.2 exp}`, `/MatrixA` equal to the white point, and no LMN stage is selected and a rectangle is filled with `0.5 setcolor`
- **THEN** the page's resource is a `CalGray` space with gamma 2.2 and the fill's component is 0.5

#### Scenario: A calibrated RGB space through the LMN stage

- **WHEN** a `CIEBasedABC` space with default ABC entries, `/DecodeLMN` of three `{1.8 exp}` procedures, and a `/MatrixLMN` is selected and a colour is set
- **THEN** the page's resource is a `CalRGB` space with gammas 1.8 and that matrix, and the components are those set

#### Scenario: The XYZ space

- **WHEN** a `CIEBasedABC` space with only `WhitePoint` and `Range*` entries is selected
- **THEN** it is carried as a `CalRGB` space with unit gammas and the identity matrix

### Requirement: Other spaces convert to Lab

A CIE-based space that does not collapse SHALL be carried as a `Lab`
space with the CIE space's white and black points and the default
a*/b* range; each colour set in it SHALL be converted by running the
space's decode procedures with the component on the operand stack
(a result that is not a number is `typecheck`), applying the
matrices, interpolating the lookup table for the `DEF`/`DEFG`
families, clamping the intermediate values to their ranges, and
converting the resulting XYZ to L*a*b* relative to the white point by
the inverse of the transformation of PLRM3 §4.8.3 (Example 4.11).
Images in such a space SHALL have their samples converted the same
way to eight-bit L*a*b* samples with a matching decode array; images
in a collapsed space SHALL pass through. Decode procedures SHALL be
run once per distinct component value and the results cached for the
image.

#### Scenario: A Lab-defined space round-trips

- **WHEN** a `CIEBasedABC` space defined as L*a*b* (the decode procedures, matrix, and ranges of the L*a*b* transformation) is selected and `50 20 -30 setcolor` fills a rectangle
- **THEN** the page's resource is a `Lab` space with the same white point and the fill's components are `50 20 -30` within rounding

#### Scenario: A table-driven space

- **WHEN** a `CIEBasedDEF` space with a 2×2×2 table is selected and a colour at a table corner is set
- **THEN** the fill's L*a*b* components equal the conversion of that corner's ABC entry

#### Scenario: A converted image

- **WHEN** an eight-bit RGB image is drawn in a non-collapsing `CIEBasedABC` space
- **THEN** the page's image is in the `Lab` resource with converted samples and a decode array of `0 100` for L* and the a*/b* range

### Requirement: Rendering operators and categories

`setcolorrendering` SHALL take a dictionary carrying `ColorRenderingType`
1 and record it in the graphics state without applying it;
`currentcolorrendering` SHALL return it; `findcolorrendering` SHALL
take a rendering-intent name or string and leave a colour rendering
name and a boolean: the name of an instance the `ColorRendering`
category holds under the composed name of PLRM3 §7.1.3 with `true`, or
the default instance's name with `false` when there is none.
`ColorRendering` SHALL be a regular category with a default
instance, `ColorSpace` a regular category, and `ColorSpaceFamily` SHALL
list `CIEBasedA`, `CIEBasedABC`, `CIEBasedDEF`, and `CIEBasedDEFG`. The
`UseCIEColor` page-device key SHALL be accepted and recorded.

#### Scenario: A rendering dictionary is recorded

- **WHEN** `/DefaultColorRendering /ColorRendering findresource setcolorrendering currentcolorrendering /ColorRenderingType get` is executed
- **THEN** the stack holds `1` and a fill afterwards is unchanged

#### Scenario: An intent without an instance proposes the default

- **WHEN** `/Perceptual findcolorrendering` is executed with no instance defined for it
- **THEN** the stack holds `/DefaultColorRendering false`

#### Scenario: The families are listed

- **WHEN** `/CIEBasedDEFG /ColorSpaceFamily resourcestatus` is executed
- **THEN** the stack holds `0 0 true`
