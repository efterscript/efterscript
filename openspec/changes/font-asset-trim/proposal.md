# Change: Font asset trim — derived metrics and subroutine pruning

## Why

The resident-outlines change doubled its size estimate: the twenty-one
TeX Gyre AFM files weigh 7.4 MB, of which more than nine tenths are
kerning pairs nothing reads, and they are included unconditionally, so
even a build without outlines carries them. At the same time an embedded
Pagella subset is about 100 KB for two glyphs because every one of the
font's 1762 subroutines is kept. Both should be fixed before a crate is
published or a WASM build is measured, and both are cheap now: the
outline programs already yield every advance, bounding box, and encoding
the AFMs provide, and the charstring interpreter already walks every
subroutine a glyph calls.

## What Changes

- **Derived metric tables replace the TeX Gyre AFMs.** `cargo xtask
  fetch-fonts` generates, for each of the twenty-one faces, a small
  text table (glyph name and advance for every charstring, the font
  bounding box, and the file's encoding) from the Type 1 program, and
  the AFM files are removed from the repository. The tables are the
  project's own derived data; a test regenerates them from the programs
  and requires equality. Widths for the extras are unchanged to the
  unit (the values come from the same source the AFMs were made from).
- **Subroutine pruning on Type 1 embedding.** The charstring interpreter
  gains a trace mode recording every subroutine index a charstring
  reaches, transitively; the writer keeps those plus the first four
  (the flex and hint-replacement convention), renumbers them densely,
  and re-encodes the call operands in the kept charstrings and
  subroutines, so a subset carries only the subroutines it uses.
- **Feature-off behaviour unchanged**: with outlines disabled the
  extras keep their metrics from the tables, which are always included.
- Out of scope: compressing the outline programs themselves; pruning
  TrueType tables further; run-time loading of assets.

## Capabilities

### New Capabilities
- none.

### Modified Capabilities
- `resident-fonts`: "Glyph resolution" — advances come from the face's
  metric table, which for the extras is derived from the program;
  "Feature-gated assets" — the metric tables, not AFMs, are unconditional.
- `text`: "Resident fonts with correct metrics" — the twenty-one measure
  with tables derived from their outline programs.
- `font-programs`: ADDED requirement for subroutine pruning in embedded
  Type 1 subsets.

## Impact

- Code: `crates/ps-fonts` (metric table format and parser, resident
  metrics for the extras, charstring trace, writer pruning), `xtask`
  (table generation and audit), `crates/ps-fonts/data/PROVENANCE.md`
  (AFMs removed, tables listed as derived), corpus goldens for embedded
  extras change (smaller `FontFile` streams) — those goldens are
  regenerated and reviewed; every other golden stays byte-identical.
- Repository and default binary shrink by about 7 MB; embedded extras
  shrink by roughly an order of magnitude.
- Depends on `resident-outlines` (archived).
