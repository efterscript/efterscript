# font-programs Specification

## Purpose
Executing embedded font programs: `eexec`, the interpretation of Type 1
and Type 42 programs into glyph advances and outlines, `charpath`, and
the subsetting and embedding of those programs in the output.

## Requirements

### Requirement: eexec decrypts and executes

`eexec` SHALL take a file or string, decrypt its contents with the
standard `eexec` key in hexadecimal or binary form (detected from the
first four bytes), discard the four leading bytes, and execute the
plaintext as a source with `systemdict` pushed on the dictionary stack
for its duration. The section SHALL be a file object whichever operand
form began it: inside it `currentfile` SHALL return that file,
`readstring` and `token` SHALL read decrypted bytes, and `closefile` on
it SHALL end the section, pop `systemdict`, and resume the enclosing
source — for a file operand, positioned after the last byte the layer
consumed.

#### Scenario: A hexadecimal eexec section

- **GIVEN** a program whose `eexec` section, when decrypted, is
  `userdict /x 42 put mark currentfile closefile`, followed by zeros and
  `cleartomark x =`
- **THEN** the output is `42`

#### Scenario: A binary eexec section

- **GIVEN** the same section in binary form
- **THEN** the output is `42`

#### Scenario: eexec on a string

- **GIVEN** a string holding an encrypted `(hi) print`
- **WHEN** `eexec` is applied to it
- **THEN** the output is `hi`

#### Scenario: A string section closes only itself

- **GIVEN** a string whose plaintext is
  `userdict /inside 1 put currentfile closefile`, executed by `eexec`
  between two statements of the job
- **THEN** the statement after the section runs and `inside` is `1` —
  the section's `closefile` ended the section, not the job

### Requirement: Type 1 programs measure and draw

After `definefont` of a Type 1 dictionary with `CharStrings` and
`Private`, `stringwidth` and the `show` family SHALL use each glyph's
charstring advance, and painting SHALL use its outline. The charstring
interpreter SHALL support the Type 1 operator set including `Subrs`,
`seac` composition through `StandardEncoding`, and flex through the
other-subroutine convention; hint operators SHALL be accepted and
ignored; a malformed charstring SHALL raise `invalidfont`.

#### Scenario: Width from the charstring

- **GIVEN** a synthesised Type 1 font whose glyph `a` has advance 600 in
  a 1000-unit em, defined through a hexadecimal `eexec` program, and
  `/Syn findfont 10 scalefont setfont (aa) stringwidth`
- **THEN** the results printed are `12 0`

#### Scenario: A composed glyph

- **GIVEN** the same font with `eacute` defined by `seac` from `e` and
  `acute`
- **WHEN** `/eacute glyphshow` runs at size 10 from (0,0)
- **THEN** the IR text operation carries the glyph with the `e`
  advance, and `charpath` of the same glyph yields the two components'
  outlines

#### Scenario: A malformed charstring

- **GIVEN** a font whose glyph `b` charstring ends mid-operator
- **WHEN** `(b) show` runs
- **THEN** the error is `invalidfont`

### Requirement: Type 42 programs measure and draw

After `definefont` of a Type 42 dictionary with `sfnts` and
`CharStrings`, glyphs SHALL be found by name through `CharStrings` to a
glyph index, advances SHALL come from the horizontal metrics scaled to
the font's units per em, and outlines from the glyph table with
composite glyphs resolved and quadratic contours converted to cubic
segments.

#### Scenario: Advance from the metrics

- **GIVEN** a synthesised TrueType font with 2048 units per em whose
  glyph `a` advances 1024, wrapped as Type 42, and `/SynTT findfont 20
  scalefont setfont (a) stringwidth`
- **THEN** the results printed are `10 0`

#### Scenario: Outline conversion

- **GIVEN** the same font whose glyph `o` is one quadratic contour
- **WHEN** `(o) true charpath pathbbox` runs
- **THEN** the bounding box is that of the converted cubic segments'
  points and control points, scaled by the font size, and lies within
  the quadratic contour's control box

### Requirement: charpath appends outlines

`charpath` SHALL append the outlines of the string's glyphs to the
current path, transformed by the font matrix, the current point, and the
CTM, and advance the current point as `show` would; the boolean operand
SHALL be accepted and, for fonts whose `PaintType` is 0, ignored. It
SHALL work for Type 1 and Type 42 fonts defined by the job and for
resident fonts backed by an outline asset; for Symbol, ZapfDingbats, and
Type 3 fonts it SHALL raise `invalidfont`.

#### Scenario: Filling a charpath

- **GIVEN** a synthesised Type 1 font with a square glyph, `100 100
  moveto (a) false charpath fill showpage`
- **THEN** the page's IR contains one fill of the square translated to
  (100,100) and scaled by the font size, and no text operation

#### Scenario: charpath advances

- **GIVEN** `0 0 moveto (aa) false charpath currentpoint` in that font
  at size 10
- **THEN** the results printed are `12 0`

#### Scenario: Resident charpath

- **GIVEN** `/Times-Roman findfont 48 scalefont setfont 72 72 moveto (Ab)
  false charpath fill showpage`
- **THEN** the page's IR contains fills of the two glyph outlines and no
  text operation, the current point after `charpath` is advanced by the
  AFM widths, and the dump's golden pins the outlines

#### Scenario: Symbol has no outlines

- **GIVEN** `/Symbol findfont 10 scalefont setfont 0 0 moveto (a) false
  charpath`
- **THEN** the error is `invalidfont`

### Requirement: Embedded programs are subset and embedded

For every embedded font used on a page, the output SHALL contain the
font program restricted to the glyphs used in the document plus
`.notdef` and any `seac` components: a Type 1 program regenerated from
the font dictionary (cleartext entries, the private dictionary and
subroutines, the used charstrings re-encrypted) as `FontFile` with its
three lengths; a TrueType program rewritten with only the used glyphs,
their metrics, and a (3,0) cmap mapping each used code to its glyph, as
`FontFile2`. Each SHALL carry a `FontDescriptor` derived from the program
and widths for every used code. Subsetted fonts SHALL be named with a
six-letter tag prefix. The regenerated Type 1 program SHALL be accepted
by the interpreter itself and define the same glyphs.

#### Scenario: Type 1 subset round-trips

- **GIVEN** a synthesised Type 1 font with glyphs `a`, `b`, `c` where only
  `a` is shown
- **WHEN** the embedded `FontFile` is extracted and run through the
  interpreter
- **THEN** it defines a font whose `CharStrings` has exactly `.notdef`
  and `a`, and `a` has the same advance and outline

#### Scenario: TrueType subset has a cmap

- **GIVEN** a synthesised TrueType font where codes 65 and 66 are shown
- **THEN** the embedded `FontFile2` parses with two glyphs plus the
  notdef, a (3,0) cmap mapping 65 and 66, and the font dictionary's
  widths for 65 and 66

#### Scenario: A checker accepts embedded output

- **WHEN** `EFTERSCRIPT_PDF_CHECK` names a checker and `difftest run`
  executes
- **THEN** every fonts corpus golden is accepted and its text extracts

### Requirement: Subroutines are pruned in embedded subsets

An embedded Type 1 subset SHALL keep only the subroutines reachable from
its kept charstrings, transitively, plus the first four, renumbered
densely with every call in the kept charstrings and subroutines
rewritten to the new numbering; where a call's operand cannot be
rewritten, the subset SHALL instead keep the original numbering with
every other subroutine replaced by a stub that returns, so subroutine
indices remain valid. The pruned program SHALL define the same glyph
outlines and advances as the unpruned one.

#### Scenario: Pagella subset shrinks

- **GIVEN** the Palatino scenario embedding two glyphs
- **THEN** the embedded `FontFile` is under 20 KB and the interpreter
  reading it back yields the same outlines and advances as before

#### Scenario: Hint replacement survives

- **GIVEN** a synthesised font whose glyph reaches a subroutine only
  through the hint-replacement other-subroutine
- **THEN** the pruned program keeps that subroutine and the glyph's
  outline is unchanged

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
