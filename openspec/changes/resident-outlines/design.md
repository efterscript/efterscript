# Design: Resident outlines

See proposal.md for motivation and the licence decision. This document
fixes how the assets enter the repository, how a shipped font becomes a
program the engine understands, and how the resident set grows.

## Context

- The glyph engine (`ps_fonts::Program`) interprets Type 1 charstrings
  and TrueType glyphs; the Type 1 writer regenerates a subset program
  from a snapshot plus the dictionary entries the PDF needs. Both were
  built for fonts that arrive through the interpreter, which executes
  the font program and hands over the resulting dictionaries.
- The resident set is `StdFont`, an enum of fourteen with AFM metrics,
  substitution to one of them, and `ResidentFont <index>` markers in
  materialised dictionaries. `charpath` for resident fonts raises
  `invalidfont`.
- Liberation is TrueType with post-table glyph names and a Unicode cmap;
  TeX Gyre ships Type 1 (PFB) files with AFM metrics and OpenType (CFF)
  files, of which only the Type 1 form is readable today. Both are
  redistributable; the repository has a `LICENSES/` directory with the
  MIT text and a provenance file format from the AFM promotion.
- `ps-fonts` must not read files at run time; assets are embedded.

## Goals / Non-Goals

**Goals:**
- Correct `charpath` for the thirty-three faces with outlines; correct
  widths for all thirty-five.
- One asset-intake discipline: exact release, checksums, licence text in
  two places, a tool that audits.
- Zero run-time file access; builds without the assets still work.

**Non-Goals:**
- Rendering fidelity of the substitutes versus the originals beyond
  metric compatibility.
- Any use of TeX Gyre's OpenType files (they need the CFF parser).
- Changing PDF output for the fourteen.

## Decisions

**D1. Assets in the crate, embedded behind a feature.**
`crates/ps-fonts/data/outlines/liberation/` (twelve `.ttf`, the OFL
text as `LICENSE`) and `crates/ps-fonts/data/outlines/tex-gyre/`
(twenty-one `.pfb`, twenty-one `.afm`, the GUST licence text and the
upstream manifest), all listed in `data/PROVENANCE.md` with URL, release,
retrieval date, and SHA-256, and `LICENSES/OFL-1.1.txt` and
`LICENSES/LPPL-1.3c.txt` added for REUSE. The `.ttf` and `.pfb` are
`include_bytes!`'d behind the feature `resident-outlines` (default on);
the AFMs are `include_str!`'d unconditionally, since widths are small
and always needed. *Alternative:* a run-time font directory through a
capability — right for a WASM build that fetches on demand, but it
changes the library's contract and is deferred to when that build
exists.

**D2. `cargo xtask fetch-fonts`.** Downloads
`liberation-fonts-ttf-2.1.5.tar.gz` and CTAN's `tex-gyre.zip`, verifies
the archives' SHA-256 against constants in the xtask, extracts exactly
the listed files, and compares them with provenance. `--check` reports
without writing; without it, differing files are refused unless
`--force`, which also prints the new checksums to paste into provenance.
System `curl` and `tar`/`unzip` via `std::process` — a developer tool,
never run by a build. The committed files are the source of truth; the
tool is the audit trail.

**D3. A standalone Type 1 file parser.** `ps_fonts::type1::parse_file`
accepts PFB (segment headers) and PFA (hexadecimal or binary `eexec`),
finds the cleartext entries it needs (`FontName`, `FontMatrix`,
`FontBBox`, `Encoding` as `StandardEncoding` or `dup … put` lines,
`FontInfo` entries), decrypts the private section, reads `lenIV`,
`Subrs` and `CharStrings` through the `RD`/`ND`/`NP` convention (any
procedure names — the parser keys on the `<index> <length> <token>
<binary>` shape), and produces the same `Type1Program` plus `Type1Dict`
text the interpreter's snapshot produces, so the writer embeds shipped
fonts and job fonts identically. It is a recogniser for the layout the
Type 1 format prescribes, not an interpreter; a file it cannot read is
a test failure at intake, not a run-time condition. *Alternative:* run
each `.pfb` through the interpreter at first use — correct but slow, and
it would make the font layer depend on the VM.

**D4. The resident set.** `StdFont` stays the fourteen (their standard-14
status matters to the PDF writer); a new `ResidentFace` enum covers all
thirty-five with `metrics()` (Core 14 AFM or TeX Gyre AFM),
`outline_asset()` (Liberation face, TeX Gyre face, or none),
`std_font()` (`Some` for the fourteen), and the PostScript name. The
`ResidentFont` marker index now ranges over the thirty-five;
`StdFont::from_index` becomes `ResidentFace::from_index`. TeX Gyre AFM
glyph names follow the Adobe Glyph List, as the Core 14 do, so the
encoding path is unchanged. *Alternative:* keep fourteen and map the
extras to Times/Helvetica as now — wrong widths, the thing the resident
set exists to get right.

**D5. Outline lookup.** `ResidentFace::outline(glyph_name) ->
Option<Rc<Glyph>>` with a per-face cache: Type 1 assets by charstring
name; TrueType assets by post name, else the name's Unicode value
(`ps_fonts::unicode`) through the (3,1) cmap; TrueType outlines scaled
from units per em to the 1000-unit space the AFM metrics use. The
advance returned to the VM is always the AFM width; the asset's advance
is ignored (the sets are metric-compatible, and one authority beats two
that agree to rounding). A missing glyph yields no outline and the AFM
width, or 0 when the AFM lacks the name too.

**D6. `charpath` on resident fonts** runs the same outline mode as
embedded fonts with the outline from D5; Symbol and ZapfDingbats (no
asset) and Type 3 fonts keep `invalidfont`, as does everything when the
feature is off. The corpus files for resident `charpath` carry a
`% requires: resident-outlines` header the harness skips when the
feature is off.

**D7. Substitution.** Aliases extend to the LaserWriter families
(`Palatino`, `Book Antiqua`, `Palladio` → Palatino; `Bookman`,
`ITC Bookman` → Bookman; `AvantGarde`, `ITC Avant Garde`, `Gothic` →
AvantGarde; `NewCenturySchlbk`, `Century Schoolbook`, `Schoolbook` →
NewCenturySchlbk; `ZapfChancery`, `Chancery` → ZapfChancery;
`Helvetica-Narrow`, `Arial Narrow` → Helvetica-Narrow), with style
mapping to each family's four names (Bookman: Light/Demi; AvantGarde:
Book/Demi; Chancery: one face). Heuristics unchanged otherwise.

**D8. PDF output for extras.** The VM describes an extra face to the
backend as `FontSource::Embedded` built from the asset's program at
first show (the shipped `.pfb` parsed once, cached by face), so the
graphics layer and the writer treat it exactly like a job-embedded
Type 1 font: interned per page by face and encoding, subset and
embedded at finish, `BaseFont` the TeX Gyre name with a subset tag.
The fourteen keep `FontSource::Resident`. Widths in the PDF come from
the AFM through the existing width path. *Alternative:* embed nothing
and name Palatino unembedded — viewers substitute unpredictably; the
whole point of shipping outlines is that the output is self-contained.

**D9. Provenance test.** `tests/provenance.rs` grows to hash every
outline and metric file against `PROVENANCE.md` entries (no vault
involved: these files never passed through it, being freely
redistributable), and checks that each set's licence file is present
and byte-identical to the copy under `LICENSES/` where one exists.

## Risks / Trade-offs

- [Seven megabytes in the repository and the default binary] → the
  feature flag; a WASM build turns it off until a run-time loading
  capability exists. Git stores the files once; they change only with
  a deliberate release bump.
- [TeX Gyre names differ from the Adobe names in the embedded PDF
  (`TeXGyrePagella-Regular` for Palatino)] → intentional: the embedded
  program is that font, and naming it otherwise would misattribute the
  outlines. The PostScript-level name stays `Palatino-Roman`.
- [Post-table names missing in a future Liberation release] → the
  Unicode fallback covers every name the Adobe Glyph List knows; the
  intake test checks that all Core 14 glyph names resolve in the
  matching Liberation face.
- [The GUST licence's renaming request] → it is a request, not a
  condition, and applies to derived works; the files ship unmodified,
  with the upstream manifest beside them.

## Open Questions

- Whether to ship TeX Gyre's OpenType files as well once the CFF parser
  exists, for smaller embeddings. Does not affect this change.

## Implementation notes

Recorded where the code departs from, or pins down, the text above.

- **Assets and their sizes (D1).** `crates/ps-fonts/data/outlines/liberation/`
  (twelve `.ttf`, 4.2 MB, and `LICENSE`) and `outlines/tex-gyre/`
  (twenty-one `.pfb`, 3.6 MB; twenty-one `.afm`, 7.4 MB; the GUST licence;
  six manifests), `LICENSES/OFL-1.1.txt` (the SPDX text, pinned to the
  licence-list tag v3.27.0) and `LICENSES/LPPL-1.3c.txt` (the LaTeX
  project's text), all listed in `PROVENANCE.md` with URL, release,
  retrieval date, and SHA-256 — the two licence texts too, under a
  `LICENSES/` section whose paths are repository-relative. REUSE
  annotations are one `REUSE.toml` at the repository root (two
  `[[annotations]]` blocks, aggregate precedence) rather than sixty-six
  `.license` sidecars. The design's "widths are small" premise does not
  hold for the TeX Gyre AFMs: their kerning sections make the metrics
  twice the size of the outlines, and they are embedded unconditionally
  as D1 says, so the default binary grows by about fifteen megabytes,
  not seven, and a build without the feature still carries 7.4 MB of
  metrics. A compact width table generated at intake (its own file, its
  own provenance row) is the fix if size matters; not done here.
- **Release versions.** The CTAN collection carries no single version:
  Adventor and Pagella are 2.501, Bonum 2.004, Schola 2.005, Chorus
  2.003, Heros 2.004 (the font headers, the AFMs, and the manifests
  agree per family, and the provenance test checks each manifest
  against its AFM). The proposal's "TeX Gyre 2.501" names the two
  families that carry it; `PROVENANCE.md` records the six. The Heros
  Condensed faces name themselves `TeXGyreHerosCondensed-Regular`,
  `-Italic`, `-Bold`, `-BoldItalic`.
- **Licence-file identity (D9, deviation).** Neither set's licence file
  is byte-identical to a canonical text: Liberation's `LICENSE` is the
  OFL preceded by Google's and Red Hat's copyright statements and
  re-wrapped (one em dash even differs), and the GUST licence is a short
  text that places the fonts under the LPPL by reference rather than
  containing it. The provenance test therefore checks presence and
  checksum of each set's licence file, presence of the two texts under
  `LICENSES/`, and that the right clauses and version statements appear
  in each, instead of the byte identity D9 asked for.
- **`cargo xtask fetch-fonts` (D2) as built.** `xtask/src/fetch_fonts.rs`
  with its own SHA-256 (`xtask/src/sha256.rs`; no dependency). Four
  upstreams: the two archives and the two licence texts, each with its
  checksum as a constant; downloads go to `target/fetch-fonts/` through
  `curl -sSL --fail` and are reused when their checksum still holds;
  members are extracted with `tar xzf … -C` and `unzip -q -o … -d`. Per
  file the verdict is `ok`, `missing`, `differs`, or `unlisted` (identical
  to upstream but the provenance row is absent or stale); `--check`
  writes nothing and fails on anything but `ok`; without it, missing
  files are written, differing ones refused unless `--force`; the rows to
  paste are printed whenever something was written or is unlisted. The
  intake was done through the tool; `--check` passes on the committed
  tree and was seen to report `differs` and fail on a modified file.
- **The Type 1 file parser (D3) as built.** `type1::parse_file(bytes) ->
  Result<ParsedFont, FontError>` with `ParsedFont { font_name,
  font_matrix, encoding: FileEncoding::{Standard, Custom(names)},
  program: Type1Program }`, the program carrying its `Type1Dict`. PFB
  segments are recognised by the `0x80` header (ASCII segments before the
  first binary one are the cleartext, binary segments the cipher, later
  ASCII the trailer); anything else is PFA, split after the `eexec`
  token and one whitespace byte, the section's form decided by
  `decrypt_section` (four hexadecimal digits). A small PostScript
  tokenizer (comments, `(…)` strings with nesting and escapes, `<…>`,
  names, numbers including radix forms, brackets) reads the cleartext
  for `FontName`, `FontMatrix`, `FontBBox` (braces or brackets),
  `PaintType`, `Encoding` (`StandardEncoding` or `dup <code> /<name> put`
  triples up to the entry's `def`; the files write `dup 223/germandbls
  put` without a space), and `FontInfo`; the decrypted private section
  for `lenIV`, `Subrs` (`dup <index> <length> <token>` then one separator
  byte and the bytes, ending at the first token that is not `dup`) and
  `CharStrings` (`/<name> <length> <token>` likewise, up to `end`). Every
  other entry of both dictionaries is kept as `(key, source text)`: the
  value's tokens joined by single spaces with comments dropped and
  strings verbatim, the trailing `readonly`/`noaccess` attributes
  removed, so the writer prints it back as `/<key> <value> def`; an
  `executeonly` procedure (the reading procedures) is left out, as the
  interpreter's snapshot leaves them out, and `Subrs`, `lenIV`, and the
  procedure names are structural. `OtherSubrs` therefore travels as its
  full flex and hint-replacement procedures. Tested against the
  synthesised corpus font in PFA hexadecimal, PFA binary, and PFB form
  (`testing::Type1Font::pfb` is new), against the writer's own output
  (parse, subset, write, parse again), and against every TeX Gyre file:
  all twenty-one parse, every charstring interprets, and every AFM width
  agrees with its charstring advance within one unit (a few glyphs
  compute their advance with `div`, e.g. Bonum Italic `tie` 500.667
  against the AFM's 501 — D5's single-authority rule earns its keep).
- **`ResidentFace` (D4) as built.** `ps_fonts::ResidentFace`, thirty-five
  variants in sorted PostScript-name order, `ALL`, `COUNT`, `index`,
  `from_index`, `postscript_name`, `from_postscript_name`, `std_font()`,
  `family()`, `is_bold`, `is_italic`, `styled(family, bold, italic)`
  (Bookman Light/Demi, AvantGarde Book/Demi, one Chancery face),
  `is_symbolic`, `metrics()` (Core 14 or TeX Gyre AFM, `OnceLock` per
  face), `width`, `bbox`, `builtin_encoding` (`StandardEncoding` for the
  text faces — the TeX Gyre AFMs' own codes are not used), and
  `outline_asset() -> Option<OutlineAsset>` (`Liberation(stem)` or
  `TexGyre(stem)`, none for Symbol and ZapfDingbats). `Family` grew the
  six new families and `is_serif()`; `StdFont` stays the fourteen and
  delegates to its face (`StdFont::face()`), and `StdFont::styled` maps
  the new families to the nearest standard font for callers that still
  want one of the fourteen. The `ResidentFont` marker is the face index,
  so the fourteen's marker values changed; nothing observable depends on
  them.
- **Outline lookup (D5) as built.** `ps_fonts::outlines`:
  `ResidentFace::outlines() -> Option<Rc<ResidentOutlines>>` parses the
  asset once per thread (a `thread_local!` table: `Program` holds
  `RefCell` caches and is not `Sync`, so a process-wide `OnceLock` is
  out) and answers `None` for a face without an asset or a build
  without the feature; `has_outlines()` is that test;
  `ResidentFace::outline(name) -> Result<Option<Rc<Glyph>>, FontError>`
  gives the outline in thousandths of the em (TrueType scaled by
  `1000 / unitsPerEm`) with the advance set to the AFM width (0 when the
  metrics lack the name), cached per face by name. A TrueType name is
  found through the `post` table, else the name's single Unicode value
  through the `(3,1)` cmap (a name mapping to several characters, a
  ligature `f_i` say, has no fallback). `ResidentOutlines` also exposes
  `program()` and `font_name()` for D8. Coverage: every glyph name of
  every Core 14 text face resolves in its Liberation face except
  `commaaccent` (listed by the Courier and Times AFMs; the glyph list
  maps it to the private-use U+F6C3, which no Liberation cmap carries),
  which per D5 draws nothing and advances by its AFM width — the test
  pins that as the one known gap — and every StandardEncoding name
  resolves in every TeX Gyre face. The Helvetica `H` outline is 558.6 units wide
  against the AFM box's 569, within the spec's two units at size 100;
  its height is 688 against 718, outside it, which is why the corpus
  scenario measures width.
- **`charpath` (D6) as built.** `FontKind::Resident(ResidentFace)`;
  `charpath` accepts a resident face when `has_outlines()`; per glyph
  the outline (already in glyph units, so `append_outline` gets scale 1)
  joins the path and the AFM width advances. A code whose encoding
  entry the metrics lack advances by 0 and appends nothing. The corpus
  files carry `% requires: resident-outlines`, which difftest reads
  (`ps_fonts::has_resident_outlines()` is a `const fn`; difftest now
  depends on `ps-fonts`) and reports as `skip … (requires the … feature,
  absent from this build)` with a separate count. The interpreter has no
  `pathforall`, and `pathbbox` counts the run's start point and the
  trailing point at the advance, which bracket every glyph's x extent;
  the "name lookup" scenario therefore shears the glyph by its own
  height through `makefont` (`[100 0 ±100 100 0 0]`) so H's top corners
  fall outside those points, and recovers the width from two boxes. The
  fallback and missing-glyph scenarios use `pathbbox` too (the path
  rises above the baseline; the path is only the start point).
- **The feature (D1, D6).** `resident-outlines` is a feature of
  `ps-fonts`, and every dependent crate (ps-vm, ps-graphics, remelt,
  efterscript-cli, platen, difftest, xtask) declares a `default` feature
  of the same name forwarding to it, with the workspace dependency table
  setting `default-features = false`; that is what makes `cargo build
  --workspace --no-default-features` an honest off build rather than
  one re-enabled by feature unification. With the feature off the extra
  faces are still resident with their metrics; `charpath` on any
  resident face is `invalidfont`; the extras are described to the
  backend as `FontSource::Resident` and reach the PDF as unembedded
  fonts named by their PostScript names (a viewer substitutes) — the
  natural degraded form, not covered by a golden since those files are
  skipped.
- **PDF output (D8) as built.** `FontSource::Resident` and
  `FontSpec::Resident` now carry a `ResidentFace` (the descriptor's serif
  flag comes from `Family::is_serif`); with the feature on, `show::describe`
  gives an extra face as `FontSource::Embedded { family: FID, kind:
  Type1, program: the face's parsed asset (shared `Rc`), font_matrix:
  [0.001 0 0 0.001 0 0], font_name: the file's own name }`, so the
  graphics layer interns it by snapshot and encoding and remelt embeds
  it at finish exactly like a job font: `pdffonts` lists
  `ZKEYPZ+TeXGyrePagella-Regular Type 1 … emb yes sub yes uni yes` beside
  `Helvetica … emb no`, `pdftotext` extracts `Pa`, and `pdfinfo` accepts
  every fonts golden. The PDF `Widths` come from the program's advances
  through the existing embedded width path (D8 said "from the AFM"): the
  two agree to the unit except for the `div` glyphs above, where the
  content writer's `TJ` adjustment absorbs the difference. Cost: a
  Pagella subset embeds about 100 KB because the 1762 `Subrs` are not
  pruned — the previous change's open question, now with a price; each
  extra face used adds that once per document.
- **Substitution (D7) as built.** `substitute` returns a `ResidentFace`;
  the alias table gained Palatino (`palatino`, `bookantiqua`,
  `palladio`), Bookman (`bookman`, `itcbookman`), AvantGarde
  (`avantgarde`, `itcavantgarde`, `avantgardegothic`,
  `itcavantgardegothic`, `gothic`), NewCenturySchlbk (`newcenturyschlbk`,
  `newcenturyschoolbook`, `centuryschoolbook`, `schoolbook`), ZapfChancery
  (`zapfchancery`, `itczapfchancery`, `chancery`), and Helvetica-Narrow
  (`helveticanarrow`, `arialnarrow`, and a Helvetica or Arial family whose
  style part starts with `narrow`, since `Helvetica-Narrow-Bold` splits
  at its first dash); spaces in a family name are ignored, so `Book
  Antiqua` and `ITC Avant Garde Gothic` resolve. Garamond, Optima, and
  Univers keep their old classes. Two pre-existing corpus files changed
  their expected output because the spec changed: `substitution-aliases`
  (ZapfChancery now resolves to itself) and `font-category-lists-resident`
  (thirty-five names); `resident-set-reported` gained the Palatino line.
  The text spec delta gained the "Encodings and resource categories"
  requirement with its `resourceforall` scenario at thirty-five, which
  the living spec still states at fourteen. Every pre-existing `.ir` and
  `.pdf` golden is byte-identical.
- **Corpus.** New: `text/extra-face-widths`, `text/laserwriter-aliases`;
  `fonts/resident-charpath-fill` (goldens), `fonts/resident-charpath-bbox`,
  `fonts/resident-charpath-unicode-fallback`,
  `fonts/resident-charpath-missing-glyph`, `fonts/resident-charpath-symbol`
  (no header: `invalidfont` in every build), `fonts/palatino-embeds-pagella`
  and `fonts/extra-face-with-helvetica` (goldens). Tests:
  `ps-fonts/tests/resident_assets.rs` (the parser and metrics over every
  asset file, the coverage test above), `tests/provenance.rs` (every
  asset hashed, every row resolved, nothing unlisted under `data/`, the
  licence checks), unit tests in `type1::file`, `outlines`, `resident`,
  `substitute`, and `xtask::fetch_fonts`.
- **Verification.** `cargo test --workspace`: 543 passed, 2 ignored with
  the feature on; 540 passed, 2 ignored with `--no-default-features`
  (the outline tests are behind `cfg(feature)`), 0 failed either way.
  `cargo clippy --workspace --all-targets` is clean in both states,
  `cargo fmt --check` clean, `difftest run` 115 of 115 with the feature
  on and 109 passed, 6 skipped with it off, `cargo xtask parse-survival`
  115 files with no failures, `cargo xtask fetch-fonts --check` all `ok`,
  `openspec validate resident-outlines` valid. `reuse lint` was not run
  (the tool is not installed here).
