# shadings Specification

## Purpose
Gradient fills: function dictionaries as static colour functions,
shading dictionaries of the seven types with their data, `shfill`,
shading patterns as colours, smoothness, and how each reaches the PDF
as the same construct.

## Requirements

### Requirement: Function dictionaries

A function dictionary SHALL be accepted where a shading takes a
`Function`: type 0 (sampled) with `Domain`, `Range`, `Size`,
`BitsPerSample`, `DataSource` (a string or a positionable file holding
the packed samples), optional `Order`, `Encode`, `Decode`; type 2
(exponential) with `Domain`, `N`, optional `C0`, `C1`, `Range`; type 3
(stitching) with `Domain`, `Functions`, `Bounds`, `Encode`, optional
`Range`, per PLRM3 §3.10.1. A missing required entry SHALL raise
`undefined`, a wrongly typed one `typecheck`, inconsistent dimensions
(a `Range` whose length disagrees with `C0`, `Bounds` outside
`Domain` or out of order, a sample source shorter than `Size` and
`BitsPerSample` imply, a `FunctionType` other than 0, 2, or 3)
`rangecheck`. A shading's `Function` MAY be an array of n one-output
functions, n the colour space's component count.

#### Scenario: A sampled function with a short source

- **WHEN** a type 0 function declares `/Size [4]`, `/BitsPerSample 8`, one output, and a three-byte `DataSource`
- **THEN** the shading using it raises `rangecheck`

#### Scenario: A stitching function's bounds

- **WHEN** a type 3 function has `/Domain [0 1] /Bounds [0.7 0.3]`
- **THEN** the shading using it raises `rangecheck`

### Requirement: Shading dictionaries

A shading dictionary SHALL carry `ShadingType` 1 to 7 and `ColorSpace`
(any space but a pattern space; an `Indexed` space only for types 4
to 7 without a `Function`), with optional `Background`, `BBox`, and
`AntiAlias`, and the type's own entries per PLRM3 §4.9.3 Tables
4.12–4.19: type 1 `Function` (2-in) with optional `Domain` and
`Matrix`; types 2 and 3 `Coords`, `Function` (1-in), optional
`Domain` and `Extend`; types 4 to 7 `DataSource` (an array of numbers,
or a string or file with `BitsPerCoordinate`, `BitsPerComponent`,
`BitsPerFlag` for types 4, 6, and 7, and `Decode`), type 5
`VerticesPerRow`, optional `Function`. Mesh data SHALL be checked for
whole vertices, triangles, and patches with valid edge flags;
violations SHALL raise `rangecheck`. A missing required entry SHALL
raise `undefined`, a wrongly typed one `typecheck`. A `ColorSpace`
that is CIE-based and does not collapse to a calibrated space SHALL
raise `limitcheck` (an implementation limit, registered).

#### Scenario: An axial shading is accepted

- **WHEN** `<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [0 0 100 0] /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> /Extend [true false] >> shfill` is executed inside a clip
- **THEN** the page carries one shade operation of type 2 with those coordinates, function, and extension

#### Scenario: A triangle mesh from an array

- **WHEN** a type 4 shading's `DataSource` array holds three vertices with edge flags 0 0 0 and RGB colours
- **THEN** the page carries a mesh shading of one triangle with those vertices and colours

#### Scenario: An incomplete triangle

- **WHEN** a type 4 shading's data ends after two vertices of a new triangle
- **THEN** `shfill` raises `rangecheck`

#### Scenario: Indexed with a function

- **WHEN** a type 4 shading names an `Indexed` colour space and carries a `Function`
- **THEN** `shfill` raises `rangecheck`

### Requirement: shfill and shading patterns

`shfill` SHALL paint the shading in current user space subject to the
current clip, leaving the current path and colour untouched and
ignoring `Background`, per its entry in PLRM3 §8.2. `makepattern`
SHALL accept a dictionary with `PatternType` 2 and a `Shading` entry,
producing an instance whose shading is in pattern space (the matrix
concatenated with the CTM at the call); such an instance SHALL be
usable as the current colour for every painting operator like a
coloured tiling pattern, with `Background` honoured for the area
outside the shading's extent. A shading's data source that is a
non-positionable file SHALL be read once when the dictionary is used.

#### Scenario: A shading pattern fills text and a stroke

- **WHEN** a radial shading pattern is set with `setpattern` and a string is shown and a path stroked
- **THEN** the page's text run and stroke carry the pattern as colour and the page's resources hold one shading pattern with the radial shading and the instance's matrix

#### Scenario: Background applies in pattern use only

- **WHEN** an axial shading with `Background` is painted with `shfill` and, as a pattern, fills a rectangle larger than its extent
- **THEN** the shade operation ignores the background and the pattern's shading carries it

### Requirement: Smoothness

`setsmoothness` SHALL take a number, clamp it to 0 to 1, and record it
in the graphics state; `currentsmoothness` SHALL return it; the
initial value SHALL be the interpreter's default and neither SHALL
affect the output.

#### Scenario: Smoothness round-trips

- **WHEN** `0.05 setsmoothness currentsmoothness` is executed
- **THEN** the stack holds `0.05`
