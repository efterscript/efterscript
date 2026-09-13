## Purpose

Forms as reusable artwork: how `execform` runs a form dictionary, how
its body is captured once per page and placed at each execution, and
the `Form` resource category.

## ADDED Requirements

### Requirement: Form execution

`execform` SHALL take a form dictionary carrying `FormType` 1, `BBox`,
`Matrix`, and an executable `PaintProc`, and paint it per PLRM3 §4.7:
inside a saved graphics state, with `Matrix` concatenated to the CTM,
the clip intersected with `BBox`, and the dictionary as the
procedure's operand. A missing key SHALL raise `undefined`, a wrongly
typed value `typecheck`, a `FormType` other than 1 or an empty `BBox`
`rangecheck`. An error in the procedure SHALL propagate and restore
the graphics state. Page operators inside a form procedure SHALL raise
`undefined`.

#### Scenario: A form paints where its matrix puts it

- **WHEN** a form with `BBox [0 0 10 10]` and `Matrix [1 0 0 1 100 100]` whose procedure fills the unit box is executed under a CTM scaled by 2, then `showpage`
- **THEN** the page shows the form's marks placed by the matrix `[2 0 0 2 200 200]` and clipped to the box

#### Scenario: A bad form is refused

- **WHEN** `execform` is given a dictionary without `PaintProc`
- **THEN** the error is `undefined` and nothing is painted

### Requirement: Forms are captured once per page

The first `execform` of a form dictionary on a page SHALL execute the
procedure with its marks captured into a form resource in form space;
every `execform` of that dictionary on the page, the first included,
SHALL emit one placement of the resource carrying the matrix that maps
form space to the page. Later executions on the same page SHALL NOT
run the procedure again. A form executed inside another form's
procedure SHALL be captured and placed inside that form's resource.

#### Scenario: Three placements, one body

- **WHEN** one form dictionary is executed three times on a page at different translations
- **THEN** the page's resources hold one form and the page carries three placements with the three matrices

#### Scenario: Nested forms

- **WHEN** a form's procedure executes another form
- **THEN** the outer form's resource contains a placement of the inner form, and the page carries one placement of the outer form

### Requirement: The Form category

`Form` SHALL be a regular resource category: `defineresource` SHALL
accept a form dictionary, `findresource` SHALL return it,
`resourcestatus` SHALL report it, and the `Category` category SHALL
list `Form`.

#### Scenario: A form is defined and found

- **WHEN** a form dictionary is defined as `/Logo /Form defineresource` and `/Logo /Form findresource execform` is executed
- **THEN** the form is painted and `/Logo /Form resourcestatus` leaves `true` on top
