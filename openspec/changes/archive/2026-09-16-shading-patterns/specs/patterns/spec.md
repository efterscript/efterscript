## MODIFIED Requirements

### Requirement: Pattern instances

`makepattern` SHALL take a pattern dictionary and a matrix and return
a pattern instance whose pattern space is the matrix concatenated with
the CTM in effect at the call, per PLRM3 §4.9.1. A type 1 dictionary
SHALL carry `PatternType` 1, `PaintType` 1 or 2, `TilingType` 1, 2, or
3, `BBox`, `XStep`, `YStep`, and an executable `PaintProc`; a type 2
dictionary SHALL carry `PatternType` 2 and a `Shading` dictionary; a
missing key SHALL raise `undefined`, a wrongly typed value
`typecheck`, a value out of range (a `PatternType` other than 1 or 2,
a zero step, an empty `BBox`) `rangecheck`. The instance SHALL answer
`currentcolor` after `setpattern`, and `makepattern` SHALL leave the
source dictionary unchanged.

#### Scenario: A pattern instance is made

- **WHEN** `<< /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10 /PaintProc { pop 0 0 5 5 rectfill } >> matrix makepattern` is executed
- **THEN** the result is a dictionary whose `PatternType` is 1 and which `setpattern` accepts, and the operand dictionary keeps its entries

#### Scenario: A shading pattern instance is made

- **WHEN** a dictionary with `/PatternType 2` and an axial `Shading` is given to `makepattern`
- **THEN** the result is an instance `setpattern` accepts

#### Scenario: A shading pattern is out of range

- **WHEN** a dictionary with `/PatternType 3` is given to `makepattern`
- **THEN** the error is `rangecheck`
