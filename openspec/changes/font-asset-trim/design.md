# Design: Font asset trim

See proposal.md for motivation. Two small decisions.

## Context

- `ResidentFace::metrics()` returns an `Afm` for all thirty-five, from
  `include_str!` of Core 14 or TeX Gyre AFM text; widths, bounding box,
  and built-in encoding are read from it. The TeX Gyre AFMs exist only to
  feed that, and their kerning is never read.
- `type1::parse_file` yields each extra face's program, whose charstrings
  give advances through the interpreter, whose cleartext gives
  `FontBBox` and the `Encoding`, so everything the AFM supplied is
  derivable.
- The charstring interpreter executes `callsubr` for ordinary calls and
  for hint replacement (the other-subroutine leaves the index for `pop`
  and the following `callsubr` runs), and flex uses the conventional
  first four subroutines. The Type 1 writer copies `Subrs` whole.

## Goals / Non-Goals

**Goals:** the same widths to the unit with 7 MB less; embedded extras
an order of magnitude smaller; no behaviour change for the fourteen.

**Non-Goals:** changing the AFM path for the Core 14; renumbering
subroutines; kerning.

## Decisions

**D1. A derived metric table per extra face, generated at intake.**
Format, one file per face under `data/outlines/tex-gyre/<face>.metrics`:
a header line with the format version, `bbox llx lly urx ury`, then
`enc <code> <name>` lines for the file's encoding and `w <name>
<advance>` lines for every charstring, sorted by name, integers only
(the programs' advances are integral). `cargo xtask fetch-fonts` writes
them from the `.pfb` after intake; a test in `ps-fonts` regenerates and
compares byte-for-byte, so the tables cannot drift from the programs.
`ResidentFace::metrics()` returns a `Metrics` trait object or enum over
`Afm` and the table; callers use `width`, `bbox`, `builtin_encoding`
only. The AFMs are deleted and the provenance lists the tables as
derived from the named program files. *Alternative:* strip kerning from
the AFMs — leaves modified third-party files that need a modification
notice, for no gain over a format the project owns.

**D2. Subroutine pruning by trace, stubs not renumbering.** The
interpreter gains a `trace` collecting every `callsubr` index executed
(nested calls included, since they execute); the writer computes the
union over kept charstrings, adds indices 0–3, and writes unreached
subroutines as the minimal charstring `return` (encrypted with `lenIV`
4 like the others). Indices are untouched, so charstrings and the
other-subroutine convention stay valid. *Alternative:* renumber and
rewrite `callsubr` operands — smaller by a few kilobytes at the cost of
re-encoding numbers inside every charstring.

## Risks / Trade-offs

- [A glyph reaches a subroutine only through a path the interpreter
  does not execute] → the interpreter executes every operator except
  hints, and hint subroutines are reached through `callsubr` too; the
  round-trip test compares outlines before and after pruning for every
  glyph of every extra face.
- [Advance disagreement between AFM and program for a few `div`-based
  advances noted in resident-outlines] → the program is now the single
  source; the resident-outlines corpus widths are re-checked and any
  changed expectation is listed in the notes.
