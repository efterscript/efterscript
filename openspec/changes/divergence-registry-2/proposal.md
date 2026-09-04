# Change: Divergence registry 2 — recorded decisions, oracle skips, and showpage endings

## Why

After the first fixes the oracle harness still reports 19 document
failures and 38 output differences, and the triage classified most of
them as decisions rather than bugs: limits the reference leaves
implementation-dependent, a resident inventory that is ours, tolerance
choices for malformed fonts, and scenarios that exist only for a build
without a graphics backend. Until each is recorded, every run re-raises
them and real regressions hide among them. This change turns the
triage's (b), (c), and (d) groups into registry requirements and corpus
headers, adds the one header the harness lacks (a skip for scenarios
that cannot be compared), and gives the six page-less text files a
`showpage` so their text output is compared as rasters instead of
being skipped. No interpreter behaviour changes.

## What Changes

- **Registry entries** in `expected-divergences`, each with the chosen
  behaviour, the departure, the reason, and the restoring
  configuration where one exists: `fmaptype-cmap-only`,
  `resident-metrics-only`, `pagedevice-records-unknown-keys`,
  `integer-range`, `job-server-save-level`, `resource-size-unknown`,
  `unspecified-forall-order`, `file-access-policy`,
  `malformed-font-invalidfont`, `resident-inventory`,
  `cvrs-negative-unsigned`, `radix-without-digits`,
  `unshown-marks-not-flushed`.
- **`% oracle: skip <reason>`** header: the harness reports the file as
  `skipped` with the reason and compares nothing; used for the three
  build-specific `% backend: none` scenarios and the deep-recursion
  file that times out on both channels. The verdict requirement in
  `oracle-testing` gains the skip.
- **Headers placed** on every file the triage listed under those
  groups, including `derived-fonts.ps` for the existing
  `font-substitution` record.
- **Showpage endings**: the six text files that paint and never show a
  page gain a `showpage` and `.ir`/`.pdf` goldens, so the oracle
  compares their rendered text.
- **A private-tier run** records the new totals; the target is zero
  unexplained document failures and every remaining output difference
  covered by a header or listed as a known open item.
- Out of scope: the probable bugs still open (T8a, T12, T13, T5) and
  the two probes (vertical writing, text extraction without ToUnicode).

## Capabilities

### New Capabilities
- none.

### Modified Capabilities
- `expected-divergences`: ADDED requirements, one per entry above.
- `oracle-testing`: "Verdicts and divergence headers" gains the
  `skipped` verdict and the `% oracle: skip` header.

## Impact

- Code: `tools/difftest` (the skip header and verdict); corpus headers
  on about twenty files; six text files gain `showpage` and goldens.
- No crate outside the tools changes; every existing golden stays
  byte-identical.
- Depends on `oracle-testing` and `triage-fixes-1` (archived).
