# Change: Resident outlines — free outline fonts for the resident set

## Why

`charpath` on a resident font still raises `invalidfont`, and it is the
last error a common job hits: outlined or clipped text in Helvetica or
Times is everyday PostScript. The resident set also stops at the
fourteen standard fonts, while the jobs the project targets assume the
LaserWriter 35 (Palatino, Bookman, Avant Garde, New Century Schoolbook,
Zapf Chancery, Helvetica Narrow); today those names silently substitute
to Times or Helvetica, changing widths. This change ships freely
licensed outline programs for the resident set and grows it to
thirty-five names with correct metrics.

It is also the project's first intake of third-party binary assets into
the public repository, so it fixes the discipline that later assets
follow: exact release, exact licence text beside the files, checksums in
provenance, REUSE licence identifiers, and a fetch tool that audits the
files against their upstream. The licence choice is the decision that
matters most and is recorded here: Liberation 2.1.5 under the SIL Open
Font License 1.1 backs the Helvetica, Times, and Courier families; TeX
Gyre 2.501 under the GUST Font License (an LPPL-based free licence)
backs the other twenty-one faces. The only metric-compatible outlines
for Symbol and ZapfDingbats are in the current URW release, which is
AGPL with an embedding exemption only; an MIT library cannot carry them,
so those two stay metrics-only and `charpath` keeps raising
`invalidfont` for them, recorded as a gap with its trigger (a free
release of those two faces).

## What Changes

- **Assets** (`ps-fonts/data/outlines/`): twelve Liberation TrueType
  files (Sans, Serif, Mono in four styles) and twenty-one TeX Gyre Type 1
  files with their AFM metrics (Adventor, Bonum, Schola, Pagella, Chorus,
  Heros Condensed), each set with its licence file, in `PROVENANCE.md`
  with upstream URL, release, retrieval date, and SHA-256, and with REUSE
  licence texts under `LICENSES/`. Embedded into the crate behind a
  default-on feature so size-constrained builds can drop them.
- **A fetch and audit tool** (`cargo xtask fetch-fonts`): downloads the
  exact releases, verifies checksums, extracts the needed files, and
  refuses to overwrite files whose checksums differ from provenance
  without a flag.
- **A standalone Type 1 file parser** (`ps-fonts`): PFB and PFA forms,
  cleartext dictionary entries, the decrypted private section, subroutines
  and charstrings — producing the same program snapshot the interpreter
  builds from a job's font, so shipped fonts and embedded fonts share one
  engine and one writer.
- **Resident set of thirty-five**: the fourteen keep their AFM metrics
  and standard-14 status; the twenty-one extras get metrics from the TeX
  Gyre AFMs and their outlines from the Type 1 files. `findfont`,
  `resourcestatus`, and `resourceforall` know all thirty-five under their
  PostScript names; the alias table and heuristics resolve the common
  variants to the right family instead of Times or Helvetica.
- **`charpath` for resident fonts**: outlines looked up by glyph name
  (Type 1 charstrings; TrueType post names, with an Adobe Glyph List to
  Unicode to cmap fallback), advance still from the AFM, which remains
  the width authority.
- **PDF output**: the fourteen stay unembedded; an extra face is embedded
  as a subset Type 1 program through the existing embedding path, named
  by its TeX Gyre font name, so viewers render it as the job intended.
- **Out of scope, with triggers**: outlines for Symbol and ZapfDingbats
  (a free release); an "embed all" policy that embeds the fourteen too
  (the distillation parameter surface); host-font access (session work);
  kerning; loading font files at run time through a capability (when a
  WASM build needs the assets outside the binary).

## Capabilities

### New Capabilities
- `resident-fonts`: which outline and metric assets the resident set
  is built from, their licensing and provenance, how a glyph is
  resolved, and what the fetch tool guarantees.

### Modified Capabilities
- `text`: "Resident fonts with correct metrics" grows to thirty-five
  faces; "Font name substitution" resolves to the thirty-five and
  reports them as resident.
- `font-programs`: "charpath appends outlines" — resident fonts other
  than Symbol and ZapfDingbats now produce outlines.
- `remelt`: ADDED requirement for the extra resident faces embedded as
  subset programs.

## Impact

- Code: `crates/ps-fonts` (assets, Type 1 file parser, resident set of
  35, outline lookup), `crates/ps-vm` (resident materialisation for the
  extras, `charpath` resident branch, aliases), `crates/ps-graphics` and
  `crates/remelt` (extras as embedded resources), `xtask` (fetch-fonts),
  `LICENSES/`, corpus files under `corpus/unit/fonts/` with goldens.
- Repository grows by roughly seven megabytes of font files; the
  default binary by the same. No new Rust dependencies; the fetch tool
  uses the system's `curl` and `unzip`/`tar` through `std::process`
  and is a developer tool, not a build step.
- Depends on `font-programs` (archived).
