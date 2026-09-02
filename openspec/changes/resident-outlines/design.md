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
