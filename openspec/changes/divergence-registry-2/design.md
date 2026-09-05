# Design: Divergence registry 2

See proposal.md. This is corpus and harness bookkeeping; the decisions
are about how headers are assigned and what the private run must show.

## Context

- The harness resolves `% divergence:` slugs against the registry, and
  `difftest run` ignores the header. There is no way to exclude a file
  from the oracle tier; build-specific scenarios (`% backend: none`) and
  the one timeout therefore count as failures on every run.
- Six text files paint text and end without `showpage`; ours emits no
  page, the reference flushes one, so the harness compares nothing for
  them. With a `showpage` they become raster comparisons of text.
- Registry entries are requirements; the harness only needs the name.

## Decisions

**D1. One slug per cause, not per file.** Slugs name the decision;
files declare the slug that explains their difference. A file with two
causes declares the one that produces the mismatch on the channel that
fails; the notes list any file where two would apply.

**D2. `% oracle: skip <reason>`** is parsed by the expectation reader
alongside the other headers; `difftest run` ignores it; the oracle
subcommand reports `skipped: <reason>` without running any command.
Reserved for scenarios that cannot be compared in principle (a build
without a backend, a limit test that never terminates in the
reference), never for inconvenient mismatches — those get a divergence
slug or stay failures. *Alternative:* a `% requires:` feature — that
header means "feature off in this build", a different thing.

**D3. Showpage endings.** Each of the six files gains `showpage` as its
last operator and, where its expected output is a `currentpoint` or
width, keeps that output; `--update-ir`/`--update-pdf` create their
goldens, which are reviewed once by eye (rasterised) before committing.
`type1-malformed-charstring.ps` keeps its error ending and gets the
malformed-font slug instead.

**D4. The private run's acceptance.** After headers: zero `fail` on the
document channel except the open probable bugs (T8a, T12, T13, T5) and
the two probes, which are listed by name in the notes as known open
items; every output-channel `differs` is either covered by a slug or
listed the same way. The notes record the totals.

## Risks / Trade-offs

- [Slugs become a way to silence real bugs] → every entry cites the
  reference's position and is reviewed as a change; the harness reports
  `divergence-closed` when a declared file starts matching, so a stale
  record is visible.
- [Rasterised text comparisons at 36 dpi are coarse] → they catch
  missing or misplaced runs, which is what the six files test.

## Implementation notes

All four tasks are built. What the harness, the headers, and the
private run pinned down:

- **D2, the skip header** (`tools/difftest/src/main.rs`,
  `oracle.rs`). `expectation` reads `% oracle: skip <reason>` into
  `Expectation::oracle_skip`; only the word `skip` followed by
  whitespace or the end of the line is recognised, any other `% oracle:`
  value being ignored like an unknown header, and `difftest run` never
  looks at it. `check_file` returns `Verdict::Skipped` with the reason
  in `FileReport::skip` before the program is executed or any command
  runs, and clears the file's `target/oracle/<path>/` so no artefact of
  an earlier run lingers. The printed line is `skipped <path> skip:
  <reason>`; the JSON record gains `"skip"` (null elsewhere) and its
  `"output"` is null for a skipped file; the summary's `skipped` total
  counts the skipped verdicts together with the files left out for a
  build feature, which it already counted. Fake-profile test
  `a_skipped_scenario_runs_nothing_and_is_counted`: a converter set to
  refuse is never invoked, a stale artefact directory is gone, the
  summary and JSON carry the count and the reason, and a `% divergence:`
  beside a skip is recorded but not judged.
- **Registry lookup** (`Registry::paths`). It returned the living
  specification alone once that existed, so the thirteen slugs of this
  change's delta resolved nowhere after the previous archive created
  `openspec/specs/expected-divergences/spec.md`. The rule is now the
  union: the living specification when present, then the delta
  `specs/expected-divergences/spec.md` of every directory under
  `openspec/changes/` other than `archive/`, in name order — a slug
  resolves from the moment its change is proposed until the change is
  archived into the living text. Tests: a synthetic root (living
  `living`, open change `proposed`, archived `archived`: the first two
  resolve, the third does not; without the living file the delta alone
  is read) and, in this workspace, every slug in every source resolves.
  The unknown-slug abort is unchanged.
- **Headers placed**: 29, each inserted after the file's last directive
  line and before its `% Scenario:` prose, where the four existing
  `font-substitution` headers sit. Per slug: `fmaptype-cmap-only` 1,
  `resident-metrics-only` 1, `pagedevice-records-unknown-keys` 1,
  `integer-range` 2, `job-server-save-level` 1, `resource-size-unknown`
  5 (`cmap-embedded`, `procset-status`, `resident-set-reported`, and the
  two CID files whose only output difference the report showed to be
  the size line, `cidfont-fontset` and `cidfont-type1-charstrings`),
  `unspecified-forall-order` 1, `file-access-policy` 1,
  `malformed-font-invalidfont` 2, `resident-inventory` 7,
  `cvrs-negative-unsigned` 1, `radix-without-digits` 1, and
  `font-substitution` on `derived-fonts.ps` (5 in all); `% oracle:
  skip` on the three `% backend: none` files ("build without a graphics
  backend") and `interp/deep-recursion.ps` ("no execution-stack limit
  reached by the reference within the timeout"). The test
  `every_declared_divergence_in_the_corpus_resolves` pins these counts
  and the four skip files. D1 choices where two causes apply:
  - `text/procset-status.ps` and `text/resident-set-reported.ps` differ
    on the size line (0 against −1) and on inventory (the reference
    lists more procedure sets, and resident alias names that ours
    reports absent). They carry `resource-size-unknown`, the cause the
    triage filed them under; `resident-inventory` would apply as well.
  - `interp/type-and-conversion.ps` carries `cvrs-negative-unsigned`:
    the reference stops at `-1 16 10 string cvrs` on both channels, so
    nothing after it is compared. `integer-range` is latent there
    (`1e10 cvi` is a `rangecheck` only with 32-bit integers) and would
    surface if the `cvrs` difference were ever closed.
  - `unshown-marks-not-flushed` has no declaring file after the
    showpage endings: `type1-malformed-charstring.ps` carries the
    malformed-font slug, its page-count difference being a consequence
    of the reference drawing on past the malformed glyph.
  - **Deviation from the proposal's Impact.** Five of the files
    (`cmap-embedded`, `cidfont-fontset`, `cidfont-type1-charstrings`,
    `type1-malformed-charstring`, `fontset-short-data`) are written by
    the corpus generator in `crates/ps-fonts/tests/corpus_fonts.rs`,
    whose drift check compares them byte for byte, so a hand-placed
    header fails `cargo test --workspace`. The five scenario literals in
    the generator gained the header line and the ignored generator test
    was run; the regenerated files are byte-identical to the hand-placed
    ones. That test file is the one change outside `tools/` and
    `corpus/`; no library code changed.
- **D3, showpage endings.** `showpage` is the last line of the six
  files; their `% expect-output:` lines are unchanged and still hold.
  Goldens were created with `--update-ir --update-pdf` restricted to
  the six paths; no other golden changed. Reviewed rasterised at 36 and
  144 dpi through the profile's render command under `target/review/`:
  - `derived-fonts`: Courier "ab" at the origin with a second "a"
    overprinted 3 pt to the right (the translated `makefont`).
  - `kshow-runs-between-glyphs`: "abc" as three one-glyph text
    operations at x 0, 6, 12, at the page corner.
  - `show-advances`: "abc" at (100, 100).
  - `type3-glyph-widths`: two adjacent filled 20 pt squares from
    (10, 10) to (50, 30); the `BuildChar` font is measured only.
  - `widthshow-adds-to-spaces`: "a b" with the space widened by 5 pt.
  - `xshow-positions`: "abc" at 0, 10, 30 pt with the `xyshow` "ab"
    overprinted on the first glyph at its (1, 2) and (2, 3) offsets.

  Nothing is misplaced. The only oddities are by construction: four
  files paint on the page edge with the baseline at y = 0, and two
  overprint runs, so at 36 dpi a page carries 15–32 inked pixels —
  enough for the comparison to catch a missing run, not a glyph shifted
  within a pixel. All six render with differing fraction 0 against the
  reference: five pass, `derived-fonts` is expected-divergence on the
  output channel alone (the substituted face's name).
- **D4, the private run.** Profile `default`, whole corpus, 36 dpi,
  threshold 48, limit 0.005, 20 s per file, `--json
  target/oracle/report.json`. Before (triage-fixes-1): 141 files, 118
  pass, 19 fail, 3 expected-divergence, 1 divergence-closed; output 102
  same, 38 differs, 1 unavailable. After: **141 files, 103 pass, 5 fail,
  28 expected-divergence, 1 divergence-closed, 4 skipped; output 101
  same, 36 differs, 0 unavailable** (a skipped file has no output
  verdict; the one `unavailable` was the deep-recursion timeout).

  The five `fail`, every one a named open item:
  - `graphics/pagedevice-merges.ps` — T8a `pagedevice-typecheck`: the
    converter refuses `/InputAttributes (tray)`.
  - `text/type1-without-program.ps` — T12
    `definefont-without-program`: the reference errors at `definefont`.
  - `fonts/composite-mixed-lengths.ps`, `fonts/composite-partial-match.ps`,
    `fonts/cidfont-type1-fallback.ps` — the text-extraction-without-
    ToUnicode probe: rasters agree (fraction 0), extracted text differs.

  The 36 output `differs`: 25 are covered by the file's slug —
  `resource-size-unknown` 5, `resident-inventory` 7,
  `font-substitution` 4 (`substitution-arial` reads `same`),
  `integer-range` 2, and one each of `resident-metrics-only`,
  `pagedevice-records-unknown-keys`, `job-server-save-level`,
  `unspecified-forall-order`, `file-access-policy`,
  `cvrs-negative-unsigned`, and `radix-without-digits`
  (`fmaptype-cmap-only` and the two `malformed-font-invalidfont` files
  read `same`). The 11 uncovered, each a known item:
  - `fonts/cmap-identity-predefined.ps` — T5
    `resourcestatus-loaded-status` (status 2 against 1 after loading).
  - `fonts/resident-charpath-missing-glyph.ps` — T13
    `resident-notdef-width` (advance 0 against 2.5).
  - `fonts/composite-vertical-width.ps` — the vertical-writing probe
    (current point (0, 90) against (5, 100)).
  - `graphics/pagedevice-merges.ps`, `text/type1-without-program.ps` —
    the T8a and T12 failures' error reports on the output channel.
  - `graphics/error-after-page.ps`, `interp/operand-stack-overflow.ps`,
    `interp/uncaught-error-report.ps`, `text/invalid-font-dict.ps` — the
    reference prints its error report on standard output where ours
    goes to standard error (the triage's T15 comparison noise; the
    harness follow-up is an error marker in the profile).
  - `fonts/type42-charpath-bbox.ps` — the triage's T14, `pathbbox` of a
    converted TrueType outline, still undetermined; rasters agree.
  - `scanner/procedure-depth-limit.ps` — not on this change's list: the
    triage's T9 limits group. A 1001-deep `{` nesting is a `limitcheck`
    in ours and scans in the reference, whose `$error /errorname` is
    then `null`. Implementation-dependent limit (PLRM3 Appendix B);
    classified (c), a candidate registry entry (`procedure-nesting-
    limit`) or a raised limit, not decided here.

  So D4 holds: no unexplained document failure, every output difference
  covered by a slug or named above.
- **Gates**: `cargo test --workspace` 687 passed, 0 failed, 2 ignored (from 685; the two new harness tests); `cargo clippy
  --workspace --all-targets` 0 warnings; `cargo fmt`; `difftest run`
  141/141 with every existing golden byte-identical (only the twelve new
  goldens are new files); `parse-survival` 141 files, 0 failed;
  `lint-strings` clean; `openspec validate divergence-registry-2` valid.
