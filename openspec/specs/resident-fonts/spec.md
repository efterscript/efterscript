# resident-fonts Specification

## Purpose
The outline and metric assets behind the resident font set: which free
releases back which faces, their licensing and provenance, how glyphs
are resolved, and what the fetch tool guarantees.

## Requirements

### Requirement: Assets with provenance and licences

The resident set SHALL be backed by Liberation 2.1.5 (SIL Open Font
License 1.1) for the Helvetica, Times, and Courier families and by TeX
Gyre 2.501 (GUST Font License) for the remaining twenty-one faces. Every
asset file SHALL be listed in the crate's provenance with its upstream
URL, release, retrieval date, and SHA-256; the licence text of each set
SHALL sit beside its files and under the repository's licence directory;
a test SHALL verify every file's checksum against the provenance.

#### Scenario: Provenance holds

- **WHEN** the provenance test runs
- **THEN** every outline and metric file's SHA-256 matches its entry and
  every entry names an existing file

#### Scenario: Fetch tool audits

- **WHEN** `cargo xtask fetch-fonts --check` runs with network access
- **THEN** it downloads the two releases, verifies their archive
  checksums, and reports every asset as matching, without writing

### Requirement: Glyph resolution

A resident face's outline for a glyph SHALL be found by glyph name: in a
Type 1 asset through its charstrings, in a TrueType asset through its
post-table names, falling back to the glyph name's Unicode value through
the asset's Unicode cmap. A glyph the asset lacks SHALL yield no outline
and the advance from the metrics. Advances SHALL always come from the
face's metric table — the Core 14 AFM for the fourteen, a table derived
from the outline program for the twenty-one — not from the outline asset
at lookup time.

#### Scenario: Name lookup in a TrueType asset

- **GIVEN** `/Helvetica findfont 100 scalefont setfont 0 0 moveto (H)
  false charpath pathbbox`
- **THEN** the bounding box's width is within 2 units of the Helvetica
  AFM's `H` bounding box width at that size

#### Scenario: Unicode fallback

- **GIVEN** a re-encoded Helvetica mapping a code to `/Euro` and a
  `charpath` of that code
- **THEN** an outline is produced (through the Unicode fallback) and
  the advance is the AFM's `Euro` width

#### Scenario: Missing glyph

- **GIVEN** a re-encoded Times mapping a code to a name no asset has
- **THEN** `charpath` appends nothing for it and advances by 0

#### Scenario: Derived table matches the program

- **WHEN** the metric-table test regenerates every extra face's table
  from its outline program
- **THEN** each regenerated table is byte-identical to the committed one

### Requirement: Feature-gated assets

The outline assets SHALL be included behind a default-on crate feature;
the metric tables SHALL be included unconditionally. With the feature
off, the fourteen keep their metrics and `charpath` on resident fonts
SHALL raise `invalidfont` as before, and the extra faces SHALL still be
resident with their metrics.

#### Scenario: Feature off

- **WHEN** the workspace builds with the feature disabled
- **THEN** it compiles, the metric tests pass, and the charpath corpus
  files for resident fonts are skipped by a header the harness honours

#### Scenario: No AFM for the extras

- **WHEN** the crate's data directory is listed
- **THEN** it holds no AFM file for the twenty-one extra faces, and the
  provenance lists their metric tables as derived data
