# graphics-ir

## ADDED Requirements

### Requirement: Composite glyph runs

Glyphs in a text operation SHALL carry a code of one to four bytes with
its byte length and a CID (equal to the code for simple fonts), and a
font resource SHALL also be a composite font: the CMap's name, writing
mode, whether the CMap is Unicode-based, and the descendant's program
snapshot with its kind. The dump SHALL write a run's codes in
hexadecimal when any code is longer than one byte and SHALL list a
composite resource as `font <n> composite <CMapName> wmode=<m>
<descendant kind> <FontName> glyphs=<count>`; dumps of pages without
composite text SHALL be unchanged.

#### Scenario: Composite run in the dump

- **GIVEN** the two-byte show scenario
- **THEN** the dump's text line writes `<00010002>` and the displacements
  500 and 700, and the resource line names `Identity-H`

#### Scenario: Existing goldens unchanged

- **WHEN** `difftest run` executes after this change
- **THEN** every pre-existing `.ir` and `.pdf` golden still matches
