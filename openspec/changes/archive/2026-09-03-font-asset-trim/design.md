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

**D2. Subroutine pruning by trace, renumbered with re-encoded
operands; stubs as the fallback.** The interpreter gains a `trace`
collecting every `callsubr` index executed (nested calls included, since
they execute); the writer computes the union over kept charstrings, adds
indices 0–3, and renumbers the kept subroutines densely in index order —
0–3 map to themselves, so the flex and hint-replacement convention holds —
rewriting every call in the kept charstrings and kept subroutines: the
literal operand of `callsubr` (nested chains included, since every kept
subroutine is rewritten too), the subroutine number that precedes
`1 3 callothersubr` for hint replacement, and the number pushed before a
call to a helper subroutine whose body is `1 3 callothersubr pop callsubr
return` and performs hint replacement for its caller. Where a call's
operand is not a literal the rewrite can see, the numbering is left alone
and every unreached subroutine is written as the minimal charstring
`return` (encrypted with `lenIV` 4 like the others), so indices stay valid
either way. *Reason, measured:* the decision as proposed was stubs alone,
estimating renumbering to save "a few kilobytes"; on Pagella the two-glyph
subset reaches sixteen subroutines, but the hint-replacement ones sit at
indices 1437–1442 of 1762 and each stub line costs about twenty-two bytes
of syntax, so stubs floor at about 39 KB (47,065 bytes as written) against
the specification's 20 KB, while renumbering yields 7,949 bytes.
*Alternative removed:* stubs without renumbering — correct for any program
and kept only as the fallback.

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

## Implementation notes

Recorded where the code departs from, or pins down, the text above.

- **Metric tables (D1) as built.** `ps_fonts::metrics::{MetricTable,
  MetricsError, Derived, VERSION_LINE}`. A table is `metrics/1` on the
  first line, then `bbox llx lly urx ury`, `enc <code> /<name>` for each
  encoded code of the program file's own encoding in code order, and
  `w /<name> <advance>` for every charstring in byte order of the name;
  integers only. Comment lines (`#`) and blank lines are skipped after
  the version line — decided so the tables can carry the SPDX header and
  a `GENERATED-BY: cargo xtask fetch-fonts from <stem>.pfb` line as the
  corpus goldens do; the parser pays one condition for it. The reader
  requires exactly one `bbox`, a `u16` advance, and strictly increasing
  names (the lookup is a binary search); anything else is
  `Malformed(line)`. `MetricTable::derive(&ParsedFont)` reads the
  program's `FontBBox` (an error unless integral), its encoding, and every
  charstring's advance through the interpreter, rounding a non-integral
  advance to the nearest unit and reporting it in `Derived::rounded`;
  `render` writes the text. `cargo xtask fetch-fonts` derives the
  twenty-one tables from the extracted upstream programs in a pass after
  the archives, audits them with the same `ok`/`missing`/`differs`/
  `unlisted` verdicts (`--check` fails on anything else), prints a
  `rounded` line per rounded advance, and writes or refuses them like any
  asset; `xtask` now depends on `ps-fonts`. The AFMs are no longer
  archive members of the tool.
- **What the tables replaced, measured.** Before deleting the AFMs every
  derived width was compared with them: 0 differences over 26,803
  glyphs, bounding boxes and encodings identical; the only extra row is
  `.notdef` (Pagella's advances 500). Thirty-four advances in ten faces
  are computed with `div` and are not integral — `tie`, `undertie`,
  `undertieinverted` in Bonum Italic (500.667), Bonum BoldItalic
  (613.588), Schola Italic (505.492), Schola BoldItalic (517.848), Heros
  Condensed Italic (543.161) and BoldItalic (623.8); `hyphen.alt` and
  `hyphendbl.alt` in the four Schola faces (166.5) and the four Heros
  Condensed faces (136.5) — and the tables carry them rounded to the
  value the AFMs had, so no width moved and no corpus expectation
  changed; `tests/resident_assets.rs` pins that exact set. Sizes: the
  twenty-one tables total 564 KB (17–30 KB each) against 7.4 MB of AFMs;
  `crates/ps-fonts/data` 16 MB → 9.2 MB; the release `efterscript`
  binary 18.6 MB → 11.6 MB.
- **`Metrics` as built.** `ps_fonts::Metrics<'a>` is the enum
  `Afm(Afm) | Table(MetricTable)` with `width`, `bbox`, `encoding`,
  `glyph_count`, `afm()`, and `table()`; `ResidentFace::metrics()`
  returns it (`OnceLock` per face, tables included unconditionally),
  `StdFont::metrics()` still returns the `Afm`, and `builtin_encoding`
  is unchanged (`StandardEncoding` for the text faces; a table's
  `encoding()` is the file's own and is not used by the VM).
- **A caller D1 missed (deviation).** `remelt::fonts::write_descriptor`
  and `flags` read the AFM's `IsFixedPitch`, `ItalicAngle`, `Ascender`,
  `Descender`, `CapHeight`, and `StdVW` for an unembedded resident face,
  which the design's "callers use `width`, `bbox`, `builtin_encoding`
  only" did not cover. The fourteen are unchanged (every golden is
  byte-identical). An extra face reaches that writer only in a build
  without its outline asset, the degraded path the previous change left
  unpinned; there it now describes itself by its bounding box and face
  flags alone: `ItalicAngle` 0, no `CapHeight`, `Ascent`/`Descent` from
  the box, `StemV` 0 (the TeX Gyre AFMs carried no `StdVW`, so this was
  0 before too). The alternative — descriptor lines in the table format —
  was not taken, to keep the format as D1 fixes it.
- **Provenance, REUSE, tests.** `PROVENANCE.md` lists the tables in
  their own section with a `Derived from` column (the row parsers read
  the first two cells) and the source `.pfb` checksums stay in the
  section above; the `.afm` rows are gone. `REUSE.toml` names the TeX
  Gyre upstream files by extension (`*.pfb`, `*.txt`), so the tables
  keep the MIT header they carry. `tests/provenance.rs` lists the
  derived files beside the fetched ones and checks each manifest's
  version against the program's `FontInfo` `version` instead of the
  AFM's. `tests/resident_assets.rs` regenerates every table byte for
  byte, and the AFM-versus-program width test became that.
- **Trace and pruning (D2) as built.** The charstring interpreter records
  every `callsubr` index it executes (`Interpreted::subrs`, the components
  of a `seac` glyph merged in), exposed as
  `Type1Program::reached_subrs(name)`; hint replacement is seen through
  `pop` + `callsubr`, both inline and through the helper subroutine the
  TeX Gyre programs use (`subr# 4 callsubr` with subroutine 4 being
  `1 3 callothersubr pop callsubr return`). `type1::write::reachable_subrs`
  is the union over the kept glyphs plus indices 0–3 bounded by the
  array, `None` when a kept charstring cannot be interpreted (then every
  subroutine is kept as it is). `type1::write::renumber` maps the kept
  indices densely in order (0–3 to themselves) and rewrites every kept
  charstring and kept subroutine token by token (`charstring::{tokens,
  encode}`, numbers re-encoded in canonical form): the literal before
  `callsubr`, the literal three tokens before `1 3 callothersubr`, and
  the literal before `<helper> callsubr` for a kept subroutine whose body
  is exactly the helper; a `callsubr` fed by `pop` is accepted only after
  other-subroutine 3. Any other shape — an operand computed with `div`, a
  `pop` after another other-subroutine, a call in dead code naming an
  unkept subroutine — makes `renumber` answer `None`, and the writer
  falls back to `pruned_subrs`: the original numbering with every
  unreached entry the `return` stub, encrypted with `lenIV` 4. Every
  glyph of all twenty-one faces renumbers (no fallback is taken by any
  shipped program). Safety net: `tests/resident_assets.rs` compares
  outline and advance for every glyph of every extra face against the
  renumbered program and the stubbed program in memory, and against the
  whole face written by the writer and read back through `parse_file`;
  `type1::write` unit tests cover hint replacement inline and through
  the helper, a nested chain (8 → 7 renumbered to 6 → 5), the three
  fallback shapes, an uninterpretable kept charstring, and a program
  with fewer than four subroutines.
- **Sizes behind D2's amendment.** Pagella's `P` and `a` reach sixteen
  subroutines — 0–4, 52, 53, 63–65 and the hint-replacement entries
  1437–1442 of 1762. With stubs the `FontFile` was 47,065 bytes
  (103,632 before this change): each `dup <k> 5 RD <5 bytes> NP` line
  costs about twenty-two bytes, and truncating the unreached tail would
  not have helped since hint-replacement subroutines sit near the top of
  every face's array (floor about 39 KB). Renumbered, the `FontFile` is
  7,949 bytes and the program carries sixteen subroutines.
- **Goldens.** Only `fonts/palatino-embeds-pagella.pdf` (105,328 →
  9,639 bytes) and `fonts/extra-face-with-helvetica.pdf` (108,701 →
  13,013 bytes) changed; every other `.ir` and `.pdf` golden is
  byte-identical. The external checker's font listing lists the Pagella subset as `emb yes sub yes
  uni yes` beside the unembedded Helvetica, the external checker accepts both,
  its text extraction extracts `Pa` and `Hi`, and its rasteriser renders the glyphs.
- **Verification.** `cargo test --workspace`: 554 passed, 2 ignored
  (543 before; eleven new tests); `cargo test -p ps-fonts
  --no-default-features`: 74 passed, 1 ignored; `cargo clippy --workspace
  --all-targets` and `cargo clippy -p ps-fonts --all-targets
  --no-default-features` clean; `cargo fmt --check` clean; `difftest run`
  115 of 115; `cargo xtask parse-survival` 115 files, no failures;
  `cargo xtask fetch-fonts --check` reports all 64 files (43 fetched, 21
  derived) as `ok`; `openspec validate font-asset-trim` valid.
