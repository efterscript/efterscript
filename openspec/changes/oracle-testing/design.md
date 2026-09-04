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

### Harness amendments from the first run (private tier)

Three rules the four-file smoke test and the full run showed to be
missing; each is a comparison matter, not a behaviour of either
interpreter, and each has a fake-profile test:

- **`%stdin` on both sides.** The oracle run executes the corpus file
  through `execute_with_stdin`, which injects an empty, readable
  standard input, as a shell-run interpreter has one; `difftest run`
  keeps running without it, so `standard-files.ps` still pins the
  no-stdin behaviour there.
- **An implicit closing page.** A converter may close a job that showed
  no page with one empty page. When ours has no page and the converter's
  document renders to exactly one page that is entirely white and (with
  a text extractor) wordless, that page is taken as none: a note
  `theirs: one blank page where ours shows none, taken as no page` is
  recorded, `pages.theirs` keeps the rendered count, and the verdict is
  unaffected. A page with any mark or word, or two pages, still fails.
  Ours is never text-extracted when it has no page — a document without
  pages has no text, and an extractor may print a diagnostic for one.
- **Declared errors.** For a file carrying `% expect-error:`, the
  converter's abnormal end is agreement: a note records it and the
  document written up to the error is compared; its normal end is a
  reason (`reference converter ended normally where the file declares
  <error>`). This amends the earlier "non-zero exit is behaviour" bullet:
  it still is, but it is judged against the file's declaration. The
  output channel is untouched by this rule, so a reference interpreter
  that prints its error report on standard output shows as `output:
  differs` for such files.
- Reports gain `notes` (printed as `note: …`, in the JSON as `"notes"`).

### Triage (private tier)

Full corpus, profile `default`, 36 dpi, threshold 48, limit 0.005,
timeout 20 s. Before the amendments above: 138 files, 33 pass, 101 fail,
3 expected-divergence, 1 divergence-closed. After: **103 pass, 31 fail,
3 expected-divergence, 1 divergence-closed**; output channel: 83 same,
54 differs, 1 unavailable (unchanged by the amendments, which do not
touch it). The 31 remaining failures and the 54 output differences group
into the causes below; classification (a) probable EfterScript bug,
(b) probable reference deviation, (c) comparison noise, (d) declared
divergence. Reproductions are corpus paths; the harness leaves every
artefact under `target/oracle/<path>/`. D6 asked for a line per file;
the run made root causes the useful unit, and the JSON report holds the
per-file detail.

#### T1. FontSet `StartData` leaves the procedure set on the dictionary stack — (a), 13 files

`corpus/unit/fonts/{cff-charpath-bbox, cff-embedded-type1c, cff-width,
cidfont-fontset, cmap-ucs2-tounicode, composefont,
composite-charpath-fill, composite-dump, composite-mixed-lengths,
composite-partial-match, composite-two-byte-width,
composite-vertical-width, fontset-defines-fonts}.ps`. Every synthesised
FontSet in the corpus is written as `/FontSetInit /ProcSet findresource
begin … StartData <data> end`, and the reference interpreter stops at
that trailing `end` with a dictionary-stack underflow. Confirmed
black-box with two scratch copies of `cidfont-fontset.ps`, one with the
trailing `end` and one without: the reference refuses the first and runs
the second to completion with our expected output (status, size, and
CIDCount agree except for the size, see T9). So the canonical FontSet
form carries no trailing `end`: `StartData` itself ends the procedure
set's dictionary after reading the data, and our `StartData` does not.
The corpus generator (`ps-fonts/tests/corpus_fonts.rs`) then encodes the
same mistake. PLRM3: the FontSetInit procedure set and its `StartData`
(§5.11, CID-keyed fonts and the FontSetInit procedure set). Follow-up: **`fontset-startdata-
ends-dictionary`** — fix the loader, regenerate the 13 files, re-golden.

#### T2. CIDInit-form CIDFont refused by the reference — undetermined, 2 files

`corpus/unit/fonts/{cidfont-type1-charstrings, cidfont-type1-fallback}.ps`.
The reference stops inside its own `StartData` for the CIDInit form
(`/CIDInit /ProcSet findresource begin 20 dict begin … (Binary) n
StartData`) with an access error while it defines the glyph data; the
operand stack shows our font dictionary already popped, so at that point
its current dictionary is the read-only procedure set. Black-box the
same for the file as is, with the trailing `end` removed, and truncated
right after the binary data (the canonical resource-file form), so
unlike T1 the trailing `end` is not the cause. Either the synthesised
form omits something the reference's `StartData` needs (its font
dictionary reached it with nine entries) or the reference expects a
different dictionary arrangement around `StartData`. Not classified
until probed further; follow-up folded into
`fontset-startdata-ends-dictionary` as a probe task.

#### T3. `type` returns a literal name — (a), 7 files, output channel only

`corpus/unit/interp/{array-and-string-operators, type-and-conversion}.ps`,
`corpus/unit/scanner/{integer-overflow-real, number-like-names,
procedures}.ps`, `corpus/unit/vm/integer-overflow-promotes.ps`. Ours
prints `/integertype`; the reference prints `integertype` and its
result answers `xcheck` with true (probe: `1 type xcheck =`). PLRM3
§8.2 `type` describes the result as an executable name. Reproduction:
`1 type xcheck =` — ours false. Every golden with a `type ==` line
changes with the fix. Follow-up: **`type-name-executable`**.

#### T4. `==` prints null as `-null-` — (a), 3 files, output channel only

`corpus/unit/interp/{captured-output, array-and-string-operators,
standard-files}.ps`. The reference prints `null` (and agrees with ours on
`-mark-`, `-dict-`, `-file-`, `--add--`). PLRM3 §8.2 `==` writes objects
in their syntactic form where one exists, and `null` has one.
Reproduction: `null ==`. Follow-up: fold into `type-name-executable`
(both are `==` output forms).

#### T5. `resourcestatus` after loading an external resource — (a), 1 file, output only

`corpus/unit/fonts/cmap-identity-predefined.ps`: after `findresource`
loads a resource held outside VM, ours still reports status 2; the
reference reports 1 (probe: `/Identity-H /CMap resourcestatus` before
and after `findresource`, and the same for `/Helvetica /Font`). PLRM3
§3.9.2 and the `resourcestatus` entry in §8.2 give status 1 to a resource loaded from external storage into
VM. Follow-up: **`resourcestatus-loaded-status`**.

#### T6. Unshown marks flushed as a final page by the converter — (b), 7 files

`corpus/unit/text/{derived-fonts, kshow-runs-between-glyphs,
show-advances, type3-glyph-widths, widthshow-adds-to-spaces,
xshow-positions}.ps`, `corpus/unit/fonts/type1-malformed-charstring.ps`
(also T11). These programs paint text and never invoke `showpage`; ours
emits no page, the converter emits one carrying the marks (its closing
page, which the amendment above accepts only when blank). PLRM3 §8.2
`showpage`: a page is transmitted by `showpage` (and `copypage`); a job
that ends without one shows nothing. Comparison noise as far as the
verdict goes but the converter's behaviour, so (b). Follow-up: none in
the interpreter; **`corpus-showpage-endings`** may add `showpage` to the
six text files so their pages are compared instead of skipped, which
would turn them into raster comparisons of text output.

#### T7. Declared limitations without a registry entry — (d) candidates, 6 files

- `corpus/unit/fonts/type0-older-map-types.ps`: ours refuses `FMapType`
  2 with `invalidfont` (only the CMap-driven type 9 is accepted); the
  reference defines the font. PLRM3 §5.10 lists types 2–9.
- `corpus/unit/fonts/resident-charpath-symbol.ps`: ours has Symbol and
  ZapfDingbats as metrics-only resident faces and raises `invalidfont`
  from `charpath`; the reference has their outlines.
- `corpus/unit/graphics/{no-backend-names-unknown,
  no-backend-moveto-undefined}.ps`, `corpus/unit/text/no-backend-fonts.ps`:
  scenarios of a build without the graphics backend; the reference
  always has one.
- `corpus/unit/text/derived-fonts.ps`: after `undefinefont`, `findfont`
  substitutes — Helvetica in ours, the reference's own default face —
  which is the existing `font-substitution` divergence, undeclared on
  this file.

Follow-up: **`divergence-registry-2`** — requirements `fmaptype-cmap-only`,
`resident-metrics-only`, and `no-backend-build` (or a `% oracle: skip`
header for build-specific scenarios), plus the header on
`derived-fonts.ps`.

#### T8. Page-device parameters — (a) and (c), 2 files

- `corpus/unit/graphics/pagedevice-merges.ps`: `<< /InputAttributes
  (tray) >> setpagedevice` is a type error in the reference (probe with
  that line alone), and PLRM3 §6.2 defines `InputAttributes` as a
  dictionary; ours accepts any value. (a), low: `setpagedevice` should
  type-check the parameters it knows, and the corpus file should use a
  well-formed value. Follow-up: **`pagedevice-typecheck`**.
- `corpus/unit/graphics/setpagedevice-unknown-keys.ps`: ours records an
  unknown key (`TraySwitch`) and reads it back; the reference ignores
  it (probe: `currentpagedevice /TraySwitch known` is false). PLRM3
  §6.1.1 leaves the recognised set to the device. (c), device-dependent;
  a candidate registry entry (`pagedevice-records-unknown-keys`).

#### T9. Implementation-dependent variation — (c), 12 files, output channel only

- Integer range: the reference has wider integers, so `2147483648` and
  `2147483647 1 add` stay integers there and `16#ffffffff` is
  4294967295 (`scanner/integer-overflow-real.ps`,
  `vm/integer-overflow-promotes.ps`); PLRM3 Appendix B makes the range
  implementation-dependent. In `scanner/number-like-names.ps` the
  reference also scans `16#` as an integer where §3.2.2 requires digits
  after the radix mark — (b), one token.
- Job encapsulation: `vmstatus` reports save level 1 in the reference,
  whose jobs run under a job server's `save` (PLRM3 §3.7.7); ours 0
  (`interp/save-restore-exec-stack.ps`).
- Limits: `scanner/procedure-depth-limit.ps` — the reference has no
  procedure-nesting limit near ours; `interp/deep-recursion.ps` — the
  reference reaches no execution-stack limit within 180 s of CPU (the
  file is the one timeout, on both channels). PLRM3 Appendix B.
- Dictionary `forall` order (`vm/forall-insertion-order.ps`): ours is
  insertion order, the reference's is its own; PLRM3 §8.2 `forall`
  leaves it unspecified.
- File access policy (`vm/file-without-capability.ps`): opening a host
  file is `undefinedfilename` in ours (no file capability) and
  `invalidfileaccess` under the profile's restricted mode.
- `resourcestatus` size: ours reports 0, the reference −1, for
  resources whose size it does not know (`fonts/cmap-embedded.ps`,
  `text/procset-status.ps`, `text/resident-set-reported.ps`); PLRM3
  §8.2 `resourcestatus` allows −1.
- `cvrs` of a negative integer in radix 16 (`interp/type-and-conversion.ps`,
  also the converter failure there): ours gives `(FFFFFFFF)`, the
  reference a `rangecheck` (probe: `-1 16 10 string cvrs`); PLRM3 §8.2
  `cvrs` treats a negative integer in a radix other than 10 as its
  unsigned two's-complement value. (b), with the caveat that the
  reference's wider integers make the case ambiguous.

Follow-up: none for the interpreter; `divergence-registry-2` may record
`unspecified-order` and `file-access-policy` if these should stop
counting as output differences.

#### T10. Resident inventory and metrics — (c), 9 files, output channel only

`corpus/unit/text/{font-category-lists-resident, procset-status,
resident-set-reported}.ps`, `corpus/unit/fonts/cmap-category-listing.ps`:
the reference enumerates its own resident fonts, CMaps, and procedure
sets. `corpus/unit/text/{helvetica-widths, extra-face-widths,
reencoding-widths}.ps`, `corpus/unit/fonts/{resident-charpath-fill,
resident-charpath-unicode-fallback}.ps`: advances differ in the third
decimal (4.44 against 4.438, 27.336 against 27.324) — the reference's
resident faces carry slightly different metrics; the rasters agree
within tolerance. No follow-up; a registry entry `resident-inventory`
would silence the enumeration files.

#### T11. Error tolerance of the reference — (c), 2 files

`corpus/unit/fonts/type1-malformed-charstring.ps` (a charstring ending
after a number) and `corpus/unit/fonts/fontset-short-data.ps`
(`StartData` declaring more bytes than the file holds): ours raises
`invalidfont` as the files declare; the reference ends normally and
draws what it can. PLRM3 leaves the treatment of malformed font
programs to the implementation. Candidate registry entries
(`malformed-font-invalidfont`); no interpreter change.

#### T12. `definefont` accepts a Type 1 dictionary without a program — (a), 1 file

`corpus/unit/text/type1-without-program.ps`: ours defines the font and
raises `invalidfont` at `show`; the reference raises it at `definefont`
(probe: the dictionary from the file alone). PLRM3 §5.2 lists
`CharStrings` and `Private` among the required entries of a Type 1
font. Low priority; fold into `pagedevice-typecheck` as a validation
change or its own `definefont-required-entries`.

#### T13. `.notdef` advance for a missing glyph name — (a), 1 file, output only

`corpus/unit/fonts/resident-charpath-missing-glyph.ps`: for a code
re-encoded to a name the font lacks, the reference advances by its
`.notdef` width (2.5 at size 10 in Times-Roman) and ours by 0, since
the resident metrics carry no `.notdef`. PLRM3 §5.3: a name absent
from the glyph set selects `.notdef`. Low priority; follow-up
`resident-notdef-width`.

#### T14. `pathbbox` of a converted TrueType outline — undetermined, 1 file, output only

`corpus/unit/fonts/type42-charpath-bbox.ps`: ours `0 -3.372 11.719
3.138`, the reference `0.984 -3.668 10.75 3.141`; the rasters agree.
The x extent differs by the start point and the trailing point, which
ours includes and the reference does not; the y extent by the
quadratic-to-cubic control points. PLRM3 §8.2 `pathbbox` (whether a
trailing `moveto` counts) and §4.6 (control points bound the path).
Probe as part of `type-name-executable`'s output pass or leave.

#### T15. Converter error reports on the output channel — (c), 5 files

`corpus/unit/graphics/error-after-page.ps`,
`corpus/unit/interp/{operand-stack-overflow, uncaught-error-report}.ps`,
`corpus/unit/text/invalid-font-dict.ps`, and the ended-normally cases:
the reference prints its error report on standard output, ours on
standard error, so every declared-error file is `output: differs` even
when both ended with the same error. Harness follow-up, if wanted:
stop comparing the output channel after the declared error's first
line — needs the reference's report shape, which the profile could
describe (`error_marker = "…"`); not done here.

#### T16. Declared divergences — (d), 4 files

`corpus/unit/text/{laserwriter-aliases, substitution-aliases,
substitution-heuristics}.ps` are `expected-divergence` on the output
channel only (the reference substitutes its own faces; the rasters
agree, both sides having painted with substitutes the rasteriser then
renders alike). `corpus/unit/text/substitution-arial.ps` is
`divergence-closed`: the reference maps Arial to the same face and
prints the same name. The record stays — the divergence is from the
PLRM's `invalidfont`, not from this converter.

#### Output channel

83 same, 54 differs, 1 unavailable. The 54: T1/T2 converter refusals
(15), T3 `type` names (7), T4 `null` (3), T15 error reports (5), T9
variation (12), T10 inventory and metrics (9), T16 substitution (3),
T5/T7/T8/T13/T14 (one or two each). No output difference is unexplained.

#### Recommended follow-ups, in order

1. `fontset-startdata-ends-dictionary` (T1, with the T2 probe) — 15
   files, the whole composite-font corpus is invisible to the oracle
   until then.
2. `type-name-executable` (T3, T4; T14 probe) — 10 files, every
   `type ==` golden.
3. `corpus-showpage-endings` (T6) — turns 6 text files into raster
   comparisons.
4. `divergence-registry-2` (T7, T8b, T9, T10, T11 entries; header on
   `derived-fonts.ps`).
5. `resourcestatus-loaded-status` (T5), `pagedevice-typecheck` (T8a,
   T12), `resident-notdef-width` (T13) — small, low.
6. Harness: T15's error marker, if the output channel's noise on
   declared-error files matters.
