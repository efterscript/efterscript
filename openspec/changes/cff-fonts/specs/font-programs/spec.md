# font-programs

## ADDED Requirements

### Requirement: CFF programs measure and draw

A name-keyed CFF program SHALL yield each glyph's outline and advance
through its Type 2 charstrings: width from the nominal and default
widths, hints and hint masks consumed without effect, the four flex
forms, local and global subroutines with the bias rule, and the accent
form of `endchar` composed through the standard encoding. A CID-keyed
CFF program SHALL be parsed with its font dictionary array and select
table so that glyphs are found by CID with the private data of their
font dictionary. A malformed charstring or structure SHALL raise
`invalidfont` from the operator that needed it.

#### Scenario: Width from a Type 2 charstring

- **GIVEN** a synthesised CFF font with nominal width 500 whose glyph
  `a` encodes a width delta of 100, loaded as a FontSet, and `/SynCFF
  findfont 10 scalefont setfont (a) stringwidth`
- **THEN** the results printed are `6 0`

#### Scenario: Flex and hint mask

- **GIVEN** a glyph using `hintmask` with two stem hints and an `hflex`
  segment
- **WHEN** `(f) true charpath pathbbox` runs
- **THEN** the bounding box matches the flex curve's control box scaled
  by the size

#### Scenario: CID-keyed lookup

- **GIVEN** a synthesised CID-keyed CFF with two font dictionaries where
  CID 5 selects the second
- **WHEN** the engine is asked for CID 5
- **THEN** the outline is interpreted with the second dictionary's
  subroutines and widths

### Requirement: FontSet resources load CFF fonts

The `FontSetInit` procedure set SHALL be available through
`/FontSetInit /ProcSet findresource`; its `StartData` SHALL take a
resource name and a byte count, read exactly that many bytes from the
current file, parse them as CFF, define every name-keyed font in it as a
FontType 2 font (with `FontMatrix`, `FontBBox`, `Encoding`,
`CharStrings` mapping names to glyph indices, and `PaintType 0`), and
define the `FontSet` resource under the given name. `definefont` SHALL
accept FontType 2.

#### Scenario: A FontSet defines its fonts

- **GIVEN** a corpus file with a FontSet named `SynSet` holding one font
  `SynCFF`
- **WHEN** `/SynSet /FontSet resourcestatus` and `/SynCFF findfont
  /FontType get` run after the FontSet
- **THEN** the status is `true` and the type printed is `2`

#### Scenario: Short data

- **GIVEN** a `StartData` whose count exceeds the bytes remaining
- **THEN** the error is `invalidfont`

### Requirement: CFF subsets are embedded as Type1C

For a FontType 2 font used on a page, the output SHALL embed a
name-keyed CFF subset holding the used glyphs plus `.notdef`, a charset
naming them, no encoding, and only the local and global subroutines
those glyphs reach (renumbered with the bias rule), as `FontFile3` with
subtype `Type1C`; the pruned program SHALL yield the same outlines and
advances as the original.

#### Scenario: Type1C round-trips

- **GIVEN** the synthesised CFF font with glyphs `a`, `b`, `c` where
  only `a` is shown
- **WHEN** the embedded `FontFile3` is parsed by the engine
- **THEN** it holds exactly `.notdef` and `a`, `a`'s outline and advance
  match the original, and unreached subroutines are absent
