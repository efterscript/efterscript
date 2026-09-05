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
