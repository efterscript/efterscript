# Change: Oracle testing — differential comparison against a reference converter

## Why

The corpus is now 138 files and every one of them is checked only
against our own goldens: a golden pins what the implementation does,
not whether it is right. The project's correctness engine was always
meant to be differential testing against an existing interpreter, and
with fonts complete the output is rich enough for that comparison to
say something. This change builds the harness under a strict isolation
rule the project adopts now: repo A never names the reference
converter, never lists its package, and never holds its outputs. The
harness knows only an abstract "reference converter" described by a
profile it finds through the environment, and the profile, its product
name, version, flags, and any persisted outputs live in the private
vault. The shapes fixed here — the profile contract, the verdict
categories with expected-divergence records, and the string denylist
lint — govern every later oracle, including proprietary ones.

## What Changes

- **A `difftest oracle` subcommand.** For each corpus file (or the
  paths given): distil with EfterScript; convert with the reference
  converter; render both PDFs to raw PNM at low resolution with
  anti-aliasing through one rasteriser (the profile's); compare page
  count, media box, pixels within a tolerance, and, when the profile
  provides a text extractor, extracted text; separately run the file
  through the reference interpreter and compare its standard output with
  ours after normalisation. Timeouts per file; a summary report;
  optional JSON output. Everything the converter produces stays under
  the build directory.
- **Profiles.** A small text file (command templates for
  PostScript-to-PDF, PDF-to-PNM, optional text extraction, a version
  string, tolerances) located through `EFTERSCRIPT_ORACLE_PROFILE` or,
  when `EFTERSCRIPT_HELLBOX` is set, under its `oracles/` directory by
  name. No profile: the subcommand skips with a clear message and
  succeeds, so public CI never depends on an oracle.
- **Verdicts.** Pass, fail, and expected divergence. A corpus file
  declares `% divergence: <slug>` naming a requirement in the
  `expected-divergences` registry; the harness then reports a mismatch
  as expected and a match as a note that the divergence may have closed.
  The registry starts with the existing font-substitution divergence
  moved into it by reference.
- **The denylist lint.** A test in `xtask` that, when the vault is
  present, reads `oracles/denylist.txt` from it and scans every text
  file in repo A for the listed strings, case-insensitively; it skips
  with a message otherwise. The vault gains the denylist and the first
  profile; repo A's devcontainer does not install any converter.
- **A first triage run.** The implementer runs the harness over the
  whole corpus with the private profile and reports every mismatch,
  classified as probable bug, probable oracle deviation, or comparison
  noise, with reproduction commands. Fixes are separate changes; nothing
  in repo A is adjusted to match the oracle.
- **Out of scope, with triggers**: parse-and-compare of PDF content
  (needs a content-stream reader; when raster diffs prove too coarse
  for a class of bugs); `psgen` (its own change); rerunning the oracle
  tier in public CI (never, by policy); comparing against proprietary
  converters (the same profile contract; when one is licensed).

## Capabilities

### New Capabilities
- `oracle-testing`: the profile contract, the comparison, the verdicts,
  the isolation rules, and the denylist lint.
- `expected-divergences`: the registry of reviewed compatibility
  decisions, one requirement per divergence with its corpus scenario.

### Modified Capabilities
- none.

## Impact

- Code: `tools/difftest` (the subcommand, profile parsing, PNM
  comparison, output normalisation, report), `xtask` (the denylist
  lint), `corpus/README.md` (verdicts and the divergence header),
  `corpus/unit/` (the `% divergence:` header on the substitution files).
  The vault: `oracles/<name>.toml`, `oracles/denylist.txt`,
  `oracles/README.md` with install steps, `reference-outputs/oracle/`.
- Dependencies: none new; the harness invokes profile commands through
  the standard process API and reads raw PNM itself.
- Depends on the existing corpus and `remelt`.
