# Change: Triage fixes 2 — resource status, page-device checks, font validation, notdef width, two probes

## Why

After the registry batch the oracle tier reports five document
failures and eleven uncovered output differences, all named. Four are
small probable bugs with reference support; two are probes whose answer
decides whether a fix or a record follows; one is a harness gap that
hides three text comparisons; one is an implementation-limit decision
left open. Closing them together brings the private tier to zero
unexplained items, which is the state in which every later change can
use it as a regression net.

## What Changes

- **`resourcestatus` reports 1 for a loaded resource.** A predefined
  resident font, CMap, or procedure set reports status 2 until it has
  been loaded into VM (`findfont`, `findresource`), then 1; defined
  resources stay 0.
- **`setpagedevice` type-checks the entries it knows.** Dictionary
  entries (`InputAttributes`, `OutputAttributes`, `Policies`), boolean
  entries (`Duplex`, `Collate`, `Tumble`), integer entries (`NumCopies`,
  `Orientation`), and array-or-null entries (`ImagingBBox`,
  `HWResolution`, `PageOffset`) SHALL raise `typecheck` when given
  another type; unknown keys stay accepted and recorded. The one corpus
  file with an ill-typed value is corrected.
- **`definefont` requires a program for Type 1 and Type 42.** A Type 1
  dictionary without `CharStrings` and `Private` (and without the
  resident marker), or a Type 42 dictionary without `sfnts` and
  `CharStrings`, is `invalidfont` at `definefont` rather than at the
  first glyph.
- **A missing glyph name advances by `.notdef`.** For a resident face,
  a code re-encoded to a name the face lacks advances by the face's
  `.notdef` width: from the derived metric table for the twenty-one,
  from the outline asset for the fourteen when outlines are present,
  else 0.
- **Vertical-writing probe.** Determine, black-box and in the private
  tier, what makes the reference advance vertically through `Identity-V`
  (the CIDFont's own `WMode`, explicit vertical metrics, the descendant
  type); fix the corpus synthesis or our loader if the cause is ours,
  else record a divergence with the evidence.
- **Text comparison without a Unicode mapping.** When EfterScript's PDF
  carries no ToUnicode for the fonts on a page, the text comparison is
  reported as not comparable rather than as a failure. A profile key
  `error_marker` lets the output-channel comparison stop at the
  reference's error report, so declared-error files compare their
  program output only.
- **`procedure-nesting-limit`** recorded as an expected divergence
  (implementation-dependent limit), with the header on its file.

## Capabilities

### New Capabilities
- none.

### Modified Capabilities
- `text`: "Font dictionaries and the font directory" (program required),
  "Font name substitution" (status 2 before loading, 1 after), "CMap and
  CIDFont categories" (same for CMaps), and "Resident fonts with correct
  metrics" is unchanged in text but gains a scenario for `.notdef`
  through the ADDED requirement below.
- `graphics-ir`: "Page device tolerance" (type checks on known keys).
- `oracle-testing`: "Comparison" (text not comparable without Unicode;
  `error_marker`).
- `expected-divergences`: ADDED `procedure-nesting-limit`; possibly a
  vertical-writing entry depending on the probe.

## Impact

- Code: `crates/ps-vm` (`ops/resource.rs`, `ops/pagedevice.rs`,
  `ops/font.rs`, `ops/show.rs`), `crates/ps-fonts` (notdef width
  lookup), `tools/difftest` (text comparability, `error_marker`),
  corpus expectation updates and headers; the vault profile gains
  `error_marker` (private).
- Depends on `divergence-registry-2` (archived).
