# Design: Triage fixes 2

See proposal.md. Four small fixes, two harness matters, one probe, one
record. Decisions below are the non-obvious ones.

## Context

- Resource status is computed per category from whether a name is in a
  defined dictionary (0) or the predefined table (2); nothing records
  that a predefined resource has been materialised into VM.
- `setpagedevice` checks only `PageSize`; every other key is recorded
  as given.
- `definefont` validation checks `FontType`, matrix, encoding, and the
  Type 3 procedure; the program's presence is checked at first glyph.
- A missing glyph name in a resident face yields width 0 because the
  Core 14 AFMs carry no `.notdef`; the derived tables for the
  twenty-one do; the outline assets have a `.notdef` glyph.
- The harness compares extracted text whenever the profile has an
  extractor; the reference prints its error report to standard output.

## Decisions

**D1. Loaded status is a per-name flag.** Each predefined table entry
(resident faces, the two CMaps, the procedure sets) gains a "loaded"
flag set when the resource is materialised; `resourcestatus` returns 1
when set. `restore` does not clear it (the reference leaves this
implementation-dependent, and the materialised object may be gone while
the status remains 1 — acceptable and recorded).

**D2. Page-device checks are a small table** of key → accepted types,
applied before recording; `PageSize` keeps its existing check. Unknown
keys are untouched. The corpus file `pagedevice-merges.ps` changes its
`InputAttributes` value to a dictionary.

**D3. Program presence at `definefont`**: Type 1 needs `CharStrings`
(a dictionary) and `Private` (a dictionary) unless the resident marker
is present; Type 42 needs `sfnts` (an array) and `CharStrings`; Type 2
is created only by `StartData` and keeps its existing path. The corpus
file `type1-without-program.ps` expects `invalidfont` from `definefont`.

**D4. `.notdef` width lookup order**: metric table (`w /.notdef`), else
the outline asset's `.notdef` advance scaled to 1000 units (feature
on), else 0. The Core 14 faces thus get their substitute outline's
notdef width, which differs from other interpreters' fonts; the corpus
file `resident-charpath-missing-glyph.ps` therefore declares
`resident-inventory`, and the new scenario uses a TeX Gyre face where
the table is authoritative.

**D5. Text comparability.** After distilling, the harness scans
EfterScript's PDF for `/ToUnicode` inside each page's font resources
(a cheap byte scan of the uncompressed goldens' form is not available
for the compressed output, so distil the oracle copy with `compress:
false`, which the harness controls); a page with a text-showing font
lacking it marks text "not comparable" (reported, never a failure).
*Alternative:* have `remelt` report the fact in `Report` — cleaner,
but it puts harness concerns into the product; revisit if the scan
proves fragile.

**D6. `error_marker`.** Optional profile string; when present, the
reference's stdout is truncated at its first occurrence for the
comparison and a flag "reference ended in error" is set; on
`% expect-error` files the flag counting as agreement replaces the
exit-status heuristic from the earlier amendment. The marker itself
lives in the vault's profile.

**D7. Vertical probe** (private tier): variants under `target/scratch/`
of the `composite-vertical-width` scenario — add `/WMode 1` to the
CIDFont dictionary; add a `W2`/`DW2`-style vertical metrics entry in
the CIDFont as the reference describes; use the `CIDFontType 2`
descendant; combine — run each through the reference interpreter alone
and read `currentpoint`. If the reference advances vertically once the
CIDFont declares `WMode 1`, our synthesis is at fault (the generator
adds it) and our loader should honour the CIDFont's `WMode` the same
way; if only explicit metrics do it, record `vertical-default-metrics`
as a divergence with the evidence; if nothing does, record the finding
as undetermined with the matrix.

**D8. `procedure-nesting-limit`** is recorded rather than the limit
raised: the limit exists to bound memory while scanning untrusted
input, which is the project's point.

## Risks / Trade-offs

- [Status 1 after `restore` frees the object] → documented; a job that
  cares can `findfont` again.
- [Page-device type table drifts from the reference's key list] → it is
  a small allow-list of types for keys we already recognise; unknown
  keys are still accepted.
- [The ToUnicode byte scan misreads a PDF] → it only downgrades a
  failure to "not comparable", never the reverse.

## Implementation notes

All eight tasks are built. What the code, the probe, and the private
run pinned down:

- **D1, the loaded flag** (`ops/resource.rs`, `interp/mod.rs`). For
  the resident faces and the predefined CMaps the materialised object is
  the flag: `resident_fonts[face]` is set by `findfont`/`findresource`
  (through an alias too — `/BookAntiqua findfont` loads Palatino-Roman),
  and `predefined_cmaps` gains an entry when `findresource`, `usecmap`,
  `composefont`, or `definefont` finishes a load. The procedure sets are
  built at start-up, so they get a two-entry `loaded_procsets` array
  set when `findresource` returns one (`resourcestatus` alone does not
  load). `resourcestatus` answers 1 when the flag is set, 2 otherwise;
  `restore` leaves all three alone, as D1 records. Encodings stay 0.
  Corpus: `fonts/cmap-identity-predefined.ps` now expects 1, 1 after its
  two `findresource`s; `text/resident-set-reported.ps` gained a
  `resourcestatus` after its `findfont` (status 1, under its existing
  `resource-size-unknown` header); the two scenario files are new —
  `fonts/cmap-loaded-status.ps` (2 then 1) and
  `text/resident-loaded-status.ps` (2, 1 for Helvetica and for
  Palatino-Roman loaded by name). The reference agrees on all of them;
  it does not load Palatino-Roman through the BookAntiqua alias (the
  alias table is ours, `font-substitution`), so the new file loads the
  face by name. Two existing tests flipped from 2 to 1 where the program
  had already loaded the resource (`composite.rs` after `findresource`;
  `cff.rs`, whose FontSet prologue finds `FontSetInit`).
- **D2, the type table** (`ops/pagedevice.rs`): eleven keys, checked
  over the whole request before anything is recorded, so a refused
  request leaves the page device untouched and its operand on the
  stack. `pagedevice-merges.ps` uses `<< /Priority [0] >>`. New
  `graphics/pagedevice-typecheck.ps` (string, integer, real for a
  dictionary, boolean, integer key; `null` accepted for `ImagingBBox`)
  agrees with the reference line for line.
- **D3** (`ops/font.rs::validate`): Type 1 without the resident marker
  needs `CharStrings` and `Private`, both dictionaries; Type 42 needs
  `sfnts` (array) and `CharStrings`. Every corpus Type 1 program defines
  those before its `definefont` (the eexec ones inside the encrypted
  section), so no golden moved. `text/type1-without-program.ps` now
  catches `invalidfont` at `definefont` and shows nothing registered
  under the key; the reference prints the same two lines. A Type 42
  whose `sfnts` is present but unreadable is still found out at the
  first glyph (test kept).
- **D4** (`ps-fonts/src/outlines.rs::{notdef_advance, notdef_width}`,
  `ops/show.rs::simple_glyph`): table `.notdef` (the twenty-one carry
  one: 500 for Palatino and AvantGarde, 280 for the rest), else the
  asset's — glyph 0 of a TrueType asset, the `.notdef` charstring of a
  Type 1 one — scaled to 1000 units (Times 777.832, Helvetica 750,
  Courier 600.098), else 0 (Symbol, ZapfDingbats, and every Core 14 face
  in a build without the assets). The same lookup serves a code the
  encoding leaves unassigned, since that code's name *is* `.notdef`:
  the reference advances such a code by its `.notdef` width too (2.78
  for Helvetica, 2.5 for Times, Palatino, and Symbol at size 10), so
  the earlier zero was wrong for both cases, and the `text.rs` test
  that pinned zero for `(\001)` now expects the face's width. No
  outline is drawn for a missing name (the assets' `.notdef` is a box
  the reference's fonts do not draw). `resident-charpath-missing-glyph.ps`
  reads 7.778 and, since the current point moves, its `pathbbox` check
  is now `false` — which is also what the reference prints (2.5 there);
  it carries `resident-inventory`. The new `fonts/resident-notdef-width.ps`
  (Palatino-Roman, 5.0) also carries `resident-inventory`: the
  reference's substitute answers 2.5, so the table value is
  authoritative for us, not agreed.
- **D5, the ToUnicode scan** (`oracle.rs::pages_without_unicode`). A
  line-based byte scan of the uncompressed document the harness already
  writes as `ours.pdf`: objects are the lines between `N G obj` and
  `endobj`; a page is an object containing `/Type /Page` not followed
  by a letter; its fonts are the `/Name N 0 R` triples inside
  `/Font << … >>`; a font lacks a mapping when its object has no
  `/ToUnicode` (a reference to a missing object counts as lacking).
  Pages are numbered in file order. Limits, recorded here: it reads
  only the layout our writer produces (one object per `obj`/`endobj`
  pair on lines of their own, inline page resources, no object streams,
  no nested dictionaries inside `/Font`), a content stream carrying a
  line that is exactly `endobj` would confuse it, and it never inspects
  the reference's document. Any misreading either downgrades a text
  failure to a note or leaves the comparison as it was. When a page is
  found, both texts are still extracted (so `ours.txt` is there to
  read) and the comparison is replaced by one note per page naming the
  fonts. In the private run seven files are marked — the three that
  failed on text alone (`composite-mixed-lengths`,
  `composite-partial-match`, `cidfont-type1-fallback`, now `pass`) and
  four whose Identity-CMap composites never had a mapping
  (`cidfont-type2-embedded`, `composite-dump`,
  `composite-two-byte-width`, `composite-vertical-width`), whose rasters
  had always agreed. The oracle copy was already distilled with
  `compress: false`; nothing changed there.
- **D6, `error_marker`** (`profile.rs`, `oracle.rs`). Optional string
  key, refused when empty. The reference interpreter now runs *before*
  the converter (both within the file's one deadline) so its output is
  known when the converter's exit status is judged; on a hanging
  converter the output channel therefore reads `same` where it read
  `unavailable`, and that test was updated. The reference's output is
  cut at the marker's first occurrence for the comparison
  (`theirs.stdout` keeps the full text) and the note "reference
  interpreter ended in error; its output is compared up to the marker"
  is recorded. On a `% expect-error:` file the marker decides:
  present → note "as the file declares …", absent → reason "ended
  normally where the file declares …"; the converter's abnormal exit is
  then a note, never a reason. A file declaring no error keeps the old
  rule (converter failure is a reason). Fake-profile tests cover both
  the scan (`pages_whose_fonts_lack_a_unicode_mapping_are_found`,
  `text_without_a_unicode_mapping_is_not_comparable`, using a Type 3
  font whose glyph names the glyph list lacks) and the marker
  (`an_error_marker_cuts_the_reference_output_and_judges_declared_errors`).
  The vault profile gained the key (left unstaged); the marker itself
  appears nowhere in this repository. In the private run six files hit
  it — the four T15 files, now `output: same`, plus two under slugs
  where the reference stops in an error.
- **D7, the vertical probe.** Twenty variants under
  `target/scratch/vertical/`, each showing one glyph through
  `Identity-V` at size 10 from (0, 100) and printing `currentpoint`,
  run through the reference interpreter alone:

  | descendant | change to the CIDFont | reference | ours |
  |---|---|---|---|
  | CFF (FontSet), as in the corpus | none | (5, 100) | (0, 90) |
  | CFF, copied and redefined | none (the copy alone) | (5, 100) | (0, 90) |
  | CFF, copied | `/WMode 1` | (5, 100) | (0, 90) |
  | CFF, copied | `/DW2 [880 -1000]`, `/W2 [1 [-1000 250 880]]` | (5, 100) | (0, 90) |
  | CFF, copied | `WMode` and `DW2`/`W2` | (5, 100) | (0, 90) |
  | CFF | explicit Type 0 with `/WMode 1` (no `composefont`) | (5, 100) | (0, 90) |
  | CFF, copied | `WMode 1` in the CIDFont *and* the explicit Type 0 | (5, 100) | (0, 90) |
  | CFF, copied | `CDevProc` in the CIDFont | unrecoverable failure | — |
  | Type 1 charstrings (CIDInit form) | none | (5, 100) | (0, 90) |
  | Type 1 charstrings | `/WMode 1` in the dictionary | (5, 100) | (0, 90) |
  | Type 1 charstrings | `DW2`/`W2` | (5, 100) | (0, 90) |
  | Type 1 charstrings | `WMode` and `DW2`/`W2` | (5, 100) | (0, 90) |
  | Type 1 charstrings | `CDevProc` in each FDArray font | (5, 100) | — |
  | Type 1 charstrings | `CDevProc` in the CIDFont | unrecoverable failure | — |
  | `CIDFontType 2` (the TrueType wrapper) | none | (5, 100) | (0, 90) |
  | `CIDFontType 2` | `/WMode 1` | (5, 100) | (0, 90) |
  | `CIDFontType 2` | `DW2`/`W2` | (5, 100) | (0, 90) |
  | `CIDFontType 2` | `WMode` and `DW2`/`W2` | (5, 100) | (0, 90) |
  | `CIDFontType 2` | `CDevProc` in the CIDFont | unrecoverable failure | — |
  | query | `Identity-V /WMode`, composed font's `/WMode` | 1, 1 | 1, 1 |

  The `DW2`/`W2` values are the PDF-style pair for a 1000-unit em: a
  vertical origin at (w/2, 880) and a downward advance of one em (the
  defaults PLRM3 §5.11 describes for a CIDFont without vertical
  metrics), given as the entries the task named. So the reference loads
  `Identity-V` with writing mode 1, composes a font that reports mode 1,
  and still advances by the horizontal width with no vertical movement,
  for every descendant type and every entry tried; nothing on our side
  is at fault (a CIDFont `WMode` we could add is ignored there), and
  nothing was found that makes it advance vertically. **Amendment to
  D7's third branch:** that finding is recorded as the expected
  divergence `vertical-default-metrics` in this change's delta rather
  than left as an undetermined open item — the matrix determines *that*
  the reference differs from PLRM3 §5.11's default and that ours follows
  it; only the input that would change the reference's mind is unknown,
  and that is stated in the requirement. The header went into the
  generator's scenario literal and the file was regenerated (the one
  line differs; goldens unchanged). No loader or generator fix was made.
- **D8**: `% divergence: procedure-nesting-limit` on
  `scanner/procedure-depth-limit.ps`; the slug resolves from the delta
  while the change is open. The registry-count test now pins
  `procedure-nesting-limit` 1, `vertical-default-metrics` 1,
  `resident-inventory` 9.
- **A finding outside this change**, from `pagedevice-merges.ps` once
  its request was well typed: the reference *reverts* the page device at
  `restore` — `PageSize` back to letter, `Duplex` no longer known —
  where ours, per the graphics-ir scenario "values survive in global VM
  across restore", keeps them. To keep the file from ending in an
  uncaught error there (its `/Duplex get`), it now prints `/Duplex
  known` (true for us, false there); the file reads `pass` on the
  document channel and `output: differs`, uncovered, and is named
  below. Whether the page device is subject to `restore` is a design
  question for its own change, not decided here.
- **Timeouts** (`oracle.rs::run_command`, added on review): the
  harness killed only the `sh -c` shell when a file's deadline passed,
  so the converter or interpreter the shell had started lived on — three
  such processes were found still running an earlier run's
  recursion-limit file. The shell now leads its own process group and a
  timeout kills the group before reaping the shell (`kill -KILL --
  -<pid>`, no new dependency). The stalled-converter test's fake now
  forks a 30-second child and records its pid; the test asserts the
  child is gone after the timeout.
- **Goldens**: none changed and none added; `difftest run` 145/145 with
  every existing golden byte-identical (the new files show no page).
- **Gates**: `cargo test --workspace` 694 passed, 0 failed, 2 ignored
  (from 687); `cargo clippy --workspace --all-targets` 0 warnings;
  `cargo fmt`; `parse-survival` 145 files, 0 failed; `lint-strings`
  clean; `openspec validate triage-fixes-2` valid.
- **Private run**, profile `default`, whole corpus, 36 dpi, threshold
  48, limit 0.005, 20 s, `--json target/oracle/report.json`. Before
  (divergence-registry-2): 141 files, 103 pass, 5 fail, 28
  expected-divergence, 1 divergence-closed, 4 skipped; output 101 same,
  36 differs. After: **145 files, 108 pass, 0 fail, 32
  expected-divergence, 1 divergence-closed, 4 skipped; output 110 same,
  31 differs, 0 unavailable.** Of the 31, 29 are covered by the file's
  slug (`resident-inventory` 9, `resource-size-unknown` 5,
  `font-substitution` 4, `integer-range` 2, and one each of the other
  nine, `procedure-nesting-limit` and `vertical-default-metrics` among
  them). The two uncovered, both named items:
  - `fonts/type42-charpath-bbox.ps` — the triage's T14, `pathbbox` of a
    converted TrueType outline, still undetermined; rasters agree.
  - `graphics/pagedevice-merges.ps` — the page device across `restore`,
    above.

  So the tier has no unexplained document failure and every output
  difference is covered by a slug or named here.
