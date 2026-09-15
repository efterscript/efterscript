## ADDED Requirements

### Requirement: Tiling patterns and forms in the PDF

A pattern resource SHALL be written as a tiling pattern object (ISO
32000-1 §8.7.3) with its paint type, tiling type, bounding box, steps,
matrix, its own resource dictionary listing what its content uses, and
the content stream of its cell; a paint with a pattern colour SHALL
select the `Pattern` colour space (with the underlying space for an
uncoloured pattern) and set the pattern by name with any components,
for filling and stroking alike. A form resource SHALL be written as a
form XObject (§8.10) with its bounding box, the identity matrix, its
own resource dictionary, and its content; a placement SHALL become the
XObject painted under its matrix. Resources inside patterns and forms
SHALL be named consistently with the page's.

#### Scenario: A pattern object with a cell

- **WHEN** a page fills a rectangle with a coloured tiling pattern
- **THEN** the page's resources name a pattern whose object has `/PatternType 1 /PaintType 1`, the box, steps, and matrix, and whose stream paints the cell; the content stream selects `/Pattern cs` and the pattern by name before the fill, and a checker accepts the file

#### Scenario: A form XObject placed twice

- **WHEN** a page executes one form twice
- **THEN** the page's resources name one form XObject with `/Subtype /Form` and the bounding box, and the content stream paints it twice under two matrices

#### Scenario: Uncoloured pattern components

- **WHEN** a page fills with an uncoloured pattern in red
- **THEN** the content stream selects a `[/Pattern /DeviceRGB]` colour space resource and sets `1 0 0` with the pattern name
