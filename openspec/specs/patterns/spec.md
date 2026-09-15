# patterns Specification

## Purpose
Tiling patterns as colours: how a pattern instance is made from a
pattern dictionary and the CTM, how it becomes the current colour,
how its paint procedure is captured once and painted with, and the
`Pattern` resource category.

## Requirements

### Requirement: Pattern instances

`makepattern` SHALL take a pattern dictionary and a matrix and return
a pattern instance whose pattern space is the matrix concatenated with
the CTM in effect at the call, per PLRM3 §4.9.1. The dictionary SHALL
carry `PatternType` 1, `PaintType` 1 or 2, `TilingType` 1, 2, or 3,
`BBox`, `XStep`, `YStep`, and an executable `PaintProc`; a missing key
SHALL raise `undefined`, a wrongly typed value `typecheck`, a value out
of range (a `PatternType` other than 1, a zero step, an empty `BBox`)
`rangecheck`. The instance SHALL answer `currentcolor` after
`setpattern`, and `makepattern` SHALL leave the source dictionary
unchanged.

#### Scenario: A pattern instance is made

- **WHEN** `<< /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10 /PaintProc { pop 0 0 5 5 rectfill } >> matrix makepattern` is executed
- **THEN** the result is a dictionary whose `PatternType` is 1 and which `setpattern` accepts, and the operand dictionary keeps its entries

#### Scenario: A shading pattern is out of range

- **WHEN** a dictionary with `/PatternType 2` is given to `makepattern`
- **THEN** the error is `rangecheck`

### Requirement: A pattern as the current colour

`setpattern` SHALL make a pattern instance the current colour: a
coloured pattern (`PaintType` 1) with no components, an uncoloured
pattern (`PaintType` 2) with the components of the underlying colour
space given under it, per PLRM3 §4.9.2. `[/Pattern]` and `[/Pattern
base]` SHALL be accepted by `setcolorspace`, after which `setcolor`
SHALL take the pattern instance (and, with a base, its components)
the same way. `currentcolorspace` SHALL report the pattern space and
`currentcolor` the components and instance. A pattern colour SHALL be
saved and restored with the graphics state.

#### Scenario: Coloured pattern fill

- **WHEN** a coloured pattern instance is set with `setpattern` and a rectangle is filled, then `showpage`
- **THEN** the page's fill is painted with the pattern, and the page's resources hold one pattern whose cell contains the paint procedure's marks in pattern space

#### Scenario: Uncoloured pattern with components

- **WHEN** an uncoloured pattern is set with `[/Pattern /DeviceRGB] setcolorspace 1 0 0 <instance> setcolor` and a rectangle is filled
- **THEN** the fill is painted with the pattern in red, and colour operators inside the paint procedure raise `undefined`

### Requirement: Paint procedure capture

The first painting operation on a page with a given pattern instance
as colour SHALL execute the instance's `PaintProc` once, with the
pattern dictionary as its operand, inside a saved graphics state whose
CTM is the pattern space and whose clip is the `BBox`, with the marks
captured into the page's pattern resource rather than the page; later
paints with the same instance on the same page SHALL reuse the
resource. An error in the paint procedure SHALL propagate as that
error and leave the graphics state as it was before the paint. Page
operators inside a paint procedure SHALL raise `undefined`.

#### Scenario: One capture, two fills

- **WHEN** the same coloured pattern instance fills two rectangles on one page
- **THEN** the page's resources hold exactly one pattern and both fills reference it

#### Scenario: The cell is clipped to its box

- **WHEN** a paint procedure paints outside its `BBox`
- **THEN** the captured cell carries the box as its clip and the pattern's steps and box as given

### Requirement: The Pattern category

`Pattern` SHALL be a regular resource category: `defineresource` SHALL
accept a pattern dictionary as an instance, `findresource` SHALL
return it, `resourcestatus` SHALL report it, and the `Category`
category SHALL list `Pattern`.

#### Scenario: A pattern is defined and found

- **WHEN** a pattern dictionary is defined as `/Dots /Pattern defineresource` and then `/Dots /Pattern findresource matrix makepattern setpattern` is executed
- **THEN** the fill that follows is painted with the pattern and `/Dots /Pattern resourcestatus` leaves `true` on top
