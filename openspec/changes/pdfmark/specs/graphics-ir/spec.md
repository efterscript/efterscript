# graphics-ir

## ADDED Requirements

### Requirement: Annotations and document marks

A page SHALL carry its link annotations (rectangle in default user
space, destination name or URI, border, colour, contents) and the sink
SHALL receive document-level marks (outline entries, named
destinations, document information, view settings, page attributes)
through a `document` method with a default that ignores them; the
current page index at the time of a mark SHALL be resolved by the
backend. The dump SHALL list a page's annotations as `annot link
<rect> dest=/<name>|uri=(<text>)` lines after the ops and document
marks in a `doc:` section after all pages, present only when marks
exist.

#### Scenario: Dump of a marked page

- **GIVEN** a page with one link and one bookmark
- **THEN** the dump shows the `annot link` line and a `doc:` section
  with `out` and the destination it resolved

#### Scenario: Existing goldens unchanged

- **WHEN** `difftest run` executes after this change
- **THEN** every pre-existing `.ir` and `.pdf` golden still matches

### Requirement: moveto replaces a pending moveto

A `moveto` immediately after another `moveto` (no segment between)
SHALL replace the subpath start; the path SHALL hold one subpath start,
not a one-point subpath followed by another.

#### Scenario: Two moves

- **GIVEN** `10 10 moveto 20 20 moveto 30 30 lineto stroke showpage`
- **THEN** the IR's stroke path is Move(20,20) Line(30,30)
