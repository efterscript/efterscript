# Design: CFF fonts

See proposal.md for motivation and the spec deltas for the contract.
This document fixes the CFF engine's shape, how CFF enters the VM, and
how subsets are written.

## Context

- `ps_fonts::Program` is an enum over Type 1 and TrueType with
  `glyph(name)`, a per-program cache, and `ProgramKind` that the VM's
  snapshot and the PDF writer dispatch on. The Type 1 charstring
  interpreter has a trace mode and the writer renumbers subroutines.
- The VM snapshots a font's program from its dictionary by `FontType`
  (1: `CharStrings`/`Private`; 42: `sfnts`/`CharStrings`) on first glyph
  use, cached by `FID`. Resource categories are an enum of `Font` and
  `Encoding` with per-category dictionaries.
- PDF embeds CFF as `FontFile3` (subtype `Type1C` for name-keyed,
  `CIDFontType0C` for CID-keyed) per ISO 32000-1 §9.9; the writer's
  descriptor path takes a stream name.

## Goals / Non-Goals

**Goals:** a complete CFF reader (name- and CID-keyed) and a name-keyed
writer; FontType 2 fonts through the FontSet path working end to end.

**Non-Goals:** CID-keyed fonts visible to PostScript; CFF2; converting
other formats to CFF; hinting semantics.

## Decisions

**D1. `cff` module with one reader for both keyings.** `CffProgram`
parses eagerly into: strings, global subroutines, charset (glyph index
→ SID or CID), encoding (code → glyph index, name-keyed only), top
dictionary values, and either one private dictionary with its local
subroutines or, CID-keyed, a vector of them plus the select table.
`glyph_by_index`, `glyph(name)` (name-keyed, through the charset's
SIDs), and `glyph_by_cid` (CID-keyed, through the charset's CIDs);
advances from the charstring width or the private dictionary's default
width. `Program::Cff(CffProgram)` and `ProgramKind::Cff`. The Type 2
interpreter is its own module: operand stack of 48, the width rule on
the first stack-clearing operator, `hstemhm`/`vstemhm`/`hintmask`/
`cntrmask` counting stems to skip mask bytes, `callsubr`/`callgsubr`
with bias 107/1131/32768 by count, the four flex forms as two curves,
`endchar` with four arguments composing base and accent through the
standard encoding, and the arithmetic escapes accepted (`abs`, `add`,
`sub`, `div`, `neg`, `random` as 0, `mul`, `sqrt`, `drop`, `exch`,
`index`, `roll`, `dup`, `put`/`get` on a 32-entry transient array,
`and`/`or`/`not`/`eq`/`ifelse`) since fonts in the wild use them.
*Alternative:* two readers — the CID-keyed layout differs only in where
the private data lives, and one reader with an `enum Privates` keeps
the interpreter identical.

**D2. Loading through `FontSetInit`.** Resource categories gain
`ProcSet` (a table of built-in dictionaries; `FontSetInit` holds one
operator, `StartData`) and `FontSet` (defined resources only).
`StartData` pops a count and a name, reads that many bytes from the
innermost file frame's file (the run file or an `eexec`-free file; a
short read is `invalidfont`), parses the CFF, and for each name-keyed
font in it builds a read-only FontType 2 dictionary: `FontName`,
`FontMatrix` (the top dictionary's, default `[0.001 0 0 0.001 0 0]`),
`FontBBox`, `PaintType 0`, `Encoding` (the CFF encoding rendered to
names, or `StandardEncoding`), `CharStrings` mapping every charset name
to its glyph index (as Type 42 does), and `FID` through `definefont`.
The parsed program is cached on the interpreter under the `FID` before
`definefont` returns, so the snapshot path finds it without re-parsing;
a FontType 2 dictionary that reaches a glyph without a cached program
(one a job built by hand) is `invalidfont`, like a Type 1 dictionary
without charstrings. The `FontSet` resource is an array of the font
names defined. A CID-keyed font in the data is parsed and cached but
not defined as a font (the composite change defines it as a CIDFont).
*Alternative:* store the CFF bytes in the dictionary as a string — the
program is immutable data the job never needs to see, and Type 42's
`sfnts` precedent exists only because the reference specifies it.

**D3. Snapshot and show.** `snapshot` handles `FontType 2` by taking
the cached program; `FontKind::Embedded` needs no change: advances are
in charstring units under the font's own matrix, so `scale` is 1 (a
non-standard CFF `FontMatrix` is already in the dictionary's
`FontMatrix`). `charpath` follows.

**D4. Writer and subset.** `cff::write` emits a name-keyed CFF: header,
name index, top dictionary (charset and charstrings offsets, private
size and offset, `FontBBox`, `FontMatrix` when non-default, `Notice`
and `FullName` copied when present), string index for the names used,
global subroutines kept and renumbered, charset format 0 listing the
kept glyphs in order, no encoding, charstrings for kept glyphs with
`callsubr`/`callgsubr` operands re-encoded against the new biases, a
private dictionary carrying the numeric hinting entries and default
and nominal widths verbatim with its pruned local subroutines. Pruning
traces both subroutine kinds through the Type 2 interpreter (`hintmask`
inside a subroutine keeps working because the trace runs the real
interpreter). Offsets are computed in two passes with fixed-size
operand encodings for the offset fields. The round-trip test parses
the written program and compares every kept glyph's outline and
advance. *Alternative:* embed the original CFF whole — correct and
simple, but CJK-sized CFFs are megabytes and the composite change
would need the writer anyway.

**D5. PDF.** `ProgramKind::Cff` routes to a `Type1` dictionary with
`FontFile3 /Subtype /Type1C`; widths via the existing width path (the
font matrix is honoured); the descriptor's stem width from the private
dictionary's `StdVW`, italic angle and fixed pitch from the top
dictionary; flags as for Type 1.

**D6. Test assets.** `testing::CffFont` builds name-keyed and CID-keyed
programs from named outlines with a Type 2 encoder (including a
`hintmask` and an `hflex`); the corpus generator writes FontSet corpus
files with binary data after `StartData` — the corpus harness reads
files as bytes and parse-survival tokenises them, so the binary section
must not break either: the generator emits the CFF, and parse-survival
learns that a `StartData` count skips that many bytes (a small, honest
change recorded in the notes). `cargo xtask fetch-fonts --test-assets`
extracts one TeX Gyre OpenType file into `target/test-fonts/`; a test
parses its CFF table and interprets every glyph when the file is
present.

## Risks / Trade-offs

- [Type 2 width parsing is easy to get subtly wrong (odd argument
  counts per operator)] → unit tests per operator with and without a
  width, and the TeX Gyre OpenType check exercising a real font's every
  glyph.
- [Binary bytes in corpus files] → the difftest and parse-survival
  changes above; the files are generated, never hand-edited.
- [Renumbering with bias] → the same round-trip guard as Type 1, plus
  a test where a subroutine count crosses a bias boundary after
  pruning.

## Open Questions

- Whether `StartData` should also accept the hexadecimal form some
  tools emit. Not seen in the corpus; can be added without spec change.
