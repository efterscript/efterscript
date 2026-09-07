# pdfmark

## Purpose

The `pdfmark` operator: how a mark's objects become values for the
graphics layer, which mark kinds are honoured and with what meaning,
and how everything else is tolerated.

## ADDED Requirements

### Requirement: The operator and its values

With a graphics backend installed, `pdfmark` SHALL be defined; it SHALL
pop every object down to the nearest mark, take the last object as the
mark kind (a name), and convert the others to values: names, strings,
integers, reals, booleans, arrays (recursively), and dictionaries
(name-keyed); any other object type in a mark SHALL raise `typecheck`;
no mark on the stack SHALL raise `unmatchedmark`. Without a backend the
name SHALL be undefined.

#### Scenario: A guarded job

- **GIVEN** `/pdfmark where { pop } { userdict /pdfmark /cleartomark
  load put } ifelse` followed by a mark
- **THEN** with a backend installed the mark is honoured, and without
  one the job runs with `cleartomark` in its place

#### Scenario: Kind must be a name

- **GIVEN** `[ /Title (x) (OUT) pdfmark`
- **THEN** the error is `typecheck`

### Requirement: Honoured kinds

`OUT` SHALL create a bookmark with `/Title`, `/Count` (children, sign
for closed), and a destination (`/Dest` name, or `/Page` with `/View`,
defaulting to the current page); `DEST` SHALL define a named
destination for the current or given page with its view; `ANN` with
`/Subtype /Link` SHALL create a link annotation on the current page
with `/Rect` in the user space in effect, `/Dest` or `/Action` with a
URI, optional `/Border`, `/Color`, `/Contents`; `DOCINFO` SHALL set
document information entries (`/Title`, `/Author`, `/Subject`,
`/Keywords`, `/Creator`, `/CreationDate`, `/ModDate`, and any other
string-valued key); `DOCVIEW` SHALL set `/PageMode`, `/PageLayout`, and
an open action from `/Page` and `/View`; `PAGES` SHALL set defaults and
`PAGE` the current page's `/CropBox` and `/Rotate`. Views SHALL accept
`[/Fit]`, `[/FitH top]`, and `[/XYZ left top zoom]`.

#### Scenario: Nested bookmarks

- **GIVEN** `[ /Title (Chapter) /Count 2 /OUT pdfmark`, two `OUT` marks
  with `/Count 0`, then a `showpage`
- **THEN** the IR's document marks hold three outline entries, the
  first with two children, all pointing at page 1

#### Scenario: A link with a named destination across pages

- **GIVEN** a `DEST` mark `/Dest /top` on page 1 and, on page 2, an
  `ANN` link whose `/Dest` is `/top` and whose `/Rect` is given under
  `2 2 scale`
- **THEN** page 2 carries a link annotation whose rectangle is the
  scaled one and whose destination resolves to page 1

#### Scenario: Document information

- **GIVEN** `[ /Title (Report) /Author (Someone) /DOCINFO pdfmark`
- **THEN** the document marks hold both entries

### Requirement: Tolerance

Unknown mark kinds and annotation subtypes SHALL be accepted, counted,
and dropped; the count SHALL be reported. A mark with a key the kind
does not use SHALL be honoured for the keys it does.

#### Scenario: Unknown kind

- **GIVEN** `[ /Foo 1 /NOSUCH pdfmark` followed by a page
- **THEN** no error is raised and the report counts one ignored mark
