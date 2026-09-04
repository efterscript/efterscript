# font-programs

## MODIFIED Requirements

### Requirement: FontSet resources load CFF fonts

The `FontSetInit` procedure set SHALL be available through
`/FontSetInit /ProcSet findresource`; its `StartData` SHALL take a
resource name and a byte count, read exactly that many bytes from the
current file, parse them as CFF, define every name-keyed font in it as a
FontType 2 font (with `FontMatrix`, `FontBBox`, `Encoding`,
`CharStrings` mapping names to glyph indices, and `PaintType 0`),
define the `FontSet` resource under the given name, and then pop the
procedure set's dictionary from the dictionary stack, so a FontSet file
in its canonical form (no trailing `end`) leaves the dictionary stack as
it found it. `definefont` SHALL accept FontType 2.

#### Scenario: A FontSet defines its fonts

- **GIVEN** a corpus file with a FontSet named `SynSet` holding one font
  `SynCFF`, written in the canonical form
- **WHEN** `/SynSet /FontSet resourcestatus` and `/SynCFF findfont
  /FontType get` run after the FontSet
- **THEN** the status is `true` and the type printed is `2`

#### Scenario: Dictionary stack restored

- **GIVEN** `countdictstack` before `/FontSetInit /ProcSet findresource
  begin` and after `StartData` has consumed the data
- **THEN** the two counts are equal

#### Scenario: Short data

- **GIVEN** a `StartData` whose count exceeds the bytes remaining
- **THEN** the error is `invalidfont`
