# Design: Oracle testing

See proposal.md for the isolation rule and its motivation; the specs
fix the contract. This document fixes the profile format, the
comparison mechanics, and where each piece lives.

## Context

- `difftest run` reads corpus headers, executes files through the
  interpreter with a collecting sink, and compares `.ir`/`.pdf`
  goldens; it already distils to PDF for the sidecar check and honours
  `EFTERSCRIPT_PDF_CHECK` as an opaque command.
- The vault is data-only and discovered through `EFTERSCRIPT_HELLBOX`;
  the public tier must be self-sufficient. The vault has no
  reference-converter material yet.
- Raster comparison needs no PDF reader; raw PNM (`P5`/`P6`) is a
  header and bytes.

## Goals / Non-Goals

**Goals:** a harness that says pass/fail/expected-divergence per corpus
file against any converter a profile describes; zero oracle identity in
repo A; a first honest inventory of mismatches.

**Non-Goals:** semantic PDF parsing; making the harness public-CI
mandatory; fixing what it finds.

## Decisions

**D1. Profile format.** A TOML-like flat text file parsed by hand (no
dependency): `name`, `version`, `ps2pdf = "<cmd> {in} {out}"`, `render
= "<cmd> {in} {out} {dpi}"` (must write one PNM per page with a `%d`
the harness substitutes), `run = "<cmd> {in}"` (executes the file and
prints its standard output), optional `text = "<cmd> {in}"`, `dpi`
(default 36), `threshold` (channel difference, default 48), `limit`
(differing-pixel fraction, default 0.005), `timeout_ms` (default
20000). Commands run through the shell so profiles can pass flags;
placeholders are the harness's only contract. Located by
`EFTERSCRIPT_ORACLE_PROFILE` (a path) or `--profile <name>` resolving to
`$EFTERSCRIPT_HELLBOX/oracles/<name>.toml`. *Alternative:* a fixed
command set — would encode one product's interface in repo A.

**D2. Comparison.** Both PDFs rendered by the profile's rasteriser,
never by two different ones, so rasterisation differences cancel;
anti-aliasing is the profile's concern. Pages compared pairwise after
size check (a size mismatch is a media-box failure); pixel diff on
grey (`P5`) or per channel (`P6`); fraction over the limit fails.
Media box read from our PDF (we wrote it) and from the rendering's
size at the profile's dpi for the converter's. Text comparison
collapses whitespace runs and trims. Standard-output comparison
normalises reals (`12.0` and `12` compare equal; trailing zeros
dropped) and line endings. *Alternative:* structural similarity
metrics — more forgiving of anti-aliasing but a dependency or a
substantial implementation; the tolerance plus shared rasteriser
suffices for a first inventory.

**D3. Isolation mechanics.** Repo A's strings for the oracle are
"reference converter", "reference interpreter", "profile". The
converter's output goes to `target/oracle/<corpus path>/` and is never
staged; the report is printed and optionally written as JSON under
`target/`. The vault gets `oracles/README.md` (install steps, product,
version pin), `oracles/default.toml`, `oracles/denylist.txt` (the
product name, its vendor, its binary name, and the names of any
third-party checker previously used, one per line), and
`reference-outputs/oracle/` for outputs the user chooses to persist.
The devcontainer in repo A stays as it is; the converter is installed
in the private environment by the vault's steps.

**D4. Divergence headers and registry.** `% divergence: <slug>` is a
corpus header like `% expect-output:`; `difftest run` ignores it,
`difftest oracle` resolves it against `openspec/specs/
expected-divergences/spec.md` by requirement name (a simple scan for
`### Requirement: <slug>`); unresolved slugs abort. The registry's first
entry restates the font-substitution decision by reference to the
`text` specification's requirement, which remains the normative
statement.

**D5. Denylist lint.** `cargo xtask lint-strings`: reads the vault's
denylist, walks `git ls-files`, skips binary files (NUL in the first
8 KB), matches case-insensitively, prints `path:line: <masked>` (the
match masked to its first letter so the lint's own output never
contains the string), exits non-zero on any hit; also wired as an
ignored-unless-vault test so `cargo test` runs it in the private tier.

**D6. First triage.** The implementer runs the full corpus with the
private profile and writes the findings into the change's
implementation notes using only repo-A vocabulary: per file, the
verdict, the differing fraction, and a one-line classification. No
corpus file or golden is altered to match the converter; declared
divergences are limited to the existing substitution decision.

## Risks / Trade-offs

- [Raster diffs at 36 dpi miss thin-line differences] → the profile's
  dpi is tunable per run; the output-channel comparison catches VM-level
  differences exactly.
- [Font substitution differs between the two renderings of our
  unembedded standard-14 output] → both PDFs are rendered by the same
  rasteriser with the same substitutes, so the same glyphs appear on
  both sides.
- [A profile author leaks the name into repo A through a corpus header
  or note] → the lint in the private tier, and review.
- [Shell-run profile commands are a code-execution surface] →
  profiles come from the private vault only; the harness refuses a
  profile path inside repo A.

## Open Questions

- Whether persisted oracle outputs should be kept at all; the vault
  directory exists for it, the harness does not need them.

## Implementation notes

The public tier (tasks 1.1–1.3, 2.1, 3.1, 5.1) is built; 4.1 and 4.2
await the private environment. What the code pins down:

- **Profile grammar (D1).** `tools/difftest/src/profile.rs`. One
  `key = value` per line; blank lines and `#` lines ignored; a `"…"`
  string honours `\"` and `\\` and may be followed by a `#` comment; a
  bare value is a number. Strings: `name`, `version`, `ps2pdf`,
  `render`, `run` (required) and `text` (optional). Numbers: `dpi`
  (integer ≥ 1, default 36), `threshold` (0–255, default 48), `limit`
  (0–1, default 0.005), `timeout_ms` (default 20000). Unknown keys,
  duplicates, wrong kinds, and missing placeholders are errors naming
  the line or key (`ps2pdf` needs `{in}` and `{out}`, `render` needs
  `{in}` and `{out}`, `run` and `text` need `{in}`). Placeholders are
  filled with single-quoted paths; `{dpi}` is bare. Location:
  `--profile <name>` (a single path component, not starting with `.`)
  resolves to `$EFTERSCRIPT_HELLBOX/oracles/<name>.toml` and is an
  error without the vault; otherwise `EFTERSCRIPT_ORACLE_PROFILE` is a
  path; the flag wins over the variable when both are given (an
  explicit request beats the environment — D1 lists them without a
  precedence). Neither: `oracle tier skipped: no profile`, exit 0. A
  named profile that does not exist is an error, not a skip. A profile
  whose canonical path lies under the repository root is refused.
- **Render contract.** `{out}` of `render` is the pattern
  `<dir>/<side>-%d.pnm`; the rasteriser expands `%d` with the 1-based
  page number and the harness collects `<side>-1.pnm`, `<side>-2.pnm`,
  … up to the first missing file. A rasteriser with another numbering
  is wrapped in the profile's own shell (commands run through `sh -c`
  from the file's output directory, standard streams captured to
  `<step>.stdout`/`<step>.stderr` there). One deadline per file covers
  all its commands; on expiry the shell is killed (children of a
  pipeline may outlive it) and the file fails as an error.
- **Documents.** Ours is written by `remelt::PdfSink`, uncompressed,
  from the pages `difftest run` already collects — a zero-page document
  when the program showed none, in which case rendering ours is skipped
  and only the converter's document is rendered, so its rasteriser must
  exit 0 on a zero-page document (or the profile wraps it).
- **Comparison (D2).** Reasons accumulate per file: a non-zero
  converter exit (`reference converter failed (…)`); page counts
  (`pages: ours N, theirs M`, the common pages still compared); per
  page, our declared media box against the converter's rendering
  converted at 72/dpi points per pixel; the two renderings' pixel sizes
  (a mismatch counts as fraction 1); the differing fraction, a pixel
  differing when any channel differs by more than `threshold`
  (grey is expanded to colour when the two PNM kinds differ); and,
  with `text`, the extracted texts after whitespace collapse and trim.
  *Amendment to D2's 0.5-unit media box:* the converter's box is known
  only through whole pixels, so the tolerance is 0.5 units plus one
  pixel's worth (72/dpi) — 2.5 units at 36 dpi; without it any
  non-integer pixel size (A4 at 36 dpi) would misfire.
- **Error against mismatch.** A converter's non-zero exit is
  behaviour and takes part in the verdict; a spawn failure, a timeout,
  a failing or unsupported rendering (`P4`, maxval ≠ 255, truncated),
  or an unreadable file is an error: always `fail`, never
  `expected-divergence`.
- **Output normalisation.** CRLF to LF; each line's trailing whitespace
  dropped; trailing blank lines dropped; each space-separated token that
  is a number is made canonical: trailing fraction zeros and a bare
  point removed (`12.0` → `12`, `2.50` → `2.5`), a leading `+` dropped,
  `.5` → `0.5`, `-0`/`-0.0` → `0`; exponents kept as written. Internal
  spacing is preserved. The result is `output: same | differs |
  unavailable` (the last when the reference run could not complete,
  which is an error).
- **Verdicts (D4).** No divergence declared: `pass` when no reason
  accumulated, else `fail`; the output column does not enter this
  verdict. Divergence declared: `divergence-closed` only when there is
  no reason *and* the output is the same — a divergence whose only
  remaining trace is in the program's output is still open — else
  `expected-divergence`. Errors are `fail` regardless. Exit 1 only
  when some file is `fail`; 2 for a usage, profile, registry, or
  report-path problem. The summary line counts every verdict and every
  output state; `--json <path>` writes the run (profile settings, one
  record per file with verdict, output, slug, page counts, per-page
  fractions, reasons; the summary) and accepts only a path under
  `target/`; `--dpi` overrides the profile's.
- **Registry lookup.** `openspec/specs/expected-divergences/spec.md`
  when it exists (after this change is archived); until then every
  `openspec/changes/<change>/specs/expected-divergences/spec.md` except
  under `archive/`, in path order, their `### Requirement:` names
  unioned. The registry is read only when some file declares a slug;
  an unknown slug aborts before any comparison, naming file, slug, and
  the registry files consulted. Living specs are written only by
  archiving, so 2.1's "living spec created through this change" is the
  delta here plus this lookup rule.
- **Corpus.** `% divergence: font-substitution` on
  `substitution-aliases`, `substitution-heuristics`, `laserwriter-aliases`,
  and `substitution-arial`; not on `resident-set-reported`, whose
  `resourcestatus` answers agree with the reference. `difftest run`
  parses the header and ignores it; every golden is byte-identical.
- **Lint (D5).** `cargo xtask lint-strings` in
  `xtask/src/lint_strings.rs`: the denylist is one string per line,
  `#` comments and blank lines ignored; files come from `git ls-files`
  (tracked and staged — an unstaged new file is not covered until
  added); a NUL in the first 8 KB marks a file binary; matching is
  case-insensitive on the whole line; the printed mask is the listed
  spelling's first character plus asterisks. Without the vault:
  `lint-strings skipped: no vault`; without the denylist: a line naming
  the missing path; both exit 0. The vault-gated test follows
  `ps-fonts/tests/provenance.rs` — it skips with a message rather than
  `#[ignore]`, as this repository gates on the vault everywhere — and a
  second test plants a string in a temporary tree with a temporary
  denylist, so nothing depends on the real vault.
- **Fake-profile tests.** `oracle.rs`'s tests write a profile and four
  shell scripts into a temporary directory outside the repository: the
  "converter" copies our own `ours.pdf` and appends `mark` lines from a
  control file (or exits 3 on `reject`, or sleeps on `hang`); the
  "rasteriser" writes one white `P5` page per `/MediaBox` in its input,
  sized from the box at the given dpi, honouring `%fake-fill <value>
  <count>`, `%fake-extra-pages <n>`, and `%fake-size <w> <h>` marks;
  the "interpreter" prints the file's own `% expect-output:` lines plus
  the control's `output` lines; the "extractor" prints a constant plus
  a `%fake-text` mark. Each scenario of the specs (identical, one pixel
  under and over the limit, missing page, media box, text, output-only
  difference, refusing converter with and without the header, closed
  divergence, unknown slug, timeout, JSON) runs through the real
  pipeline with a distinct output root under the fixture.
- **What 4.1–4.2 need.** In the vault: `oracles/<name>.toml` with the
  keys above (`default` is the name the spec scenario uses),
  `oracles/denylist.txt`, `oracles/README.md` with the install steps,
  and `reference-outputs/oracle/` if outputs are to be kept; then
  `cargo xtask lint-strings` must pass on the repository and
  `difftest oracle --profile default --json target/oracle/report.json`
  runs the triage whose findings go here in repo-A vocabulary.
