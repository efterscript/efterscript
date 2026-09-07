# Design: pdfmark

See proposal.md and the spec deltas. This document fixes the value
type across the boundary, where marks live in the IR, how the writer
assembles document objects, and the `moveto` rule.

## Context

- The boundary carries numbers and small value types; fonts cross as
  shared immutable snapshots. Pages are delivered one at a time at
  `showpage`; document-level state has no home yet.
- `pdf-out` writes objects in any order with ids allocated up front;
  the catalog is written at finish; `write_info` takes a closure.
- The pdfmark reference (in the vault) is Adobe's published technical
  note; implement from its semantics, cite it, never quote.
- `Path::move_to` pushes a `Move` unconditionally.

## Decisions

**D1. `MarkValue` is the boundary type.** `enum MarkValue { Name(Vec<u8>),
String(Vec<u8>), Int(i32), Real(f32), Bool(bool), Array(Vec<MarkValue>),
Dict(Vec<(Vec<u8>, MarkValue)>) }`, built by the operator from the
objects between the mark and the kind; procedures and other types are
`typecheck`. The trait gains `fn pdfmark(&mut self, kind: &[u8], entries:
&[MarkValue]) -> Result<(), VmError>` with a default that ignores
(`Ok`), so the mock and any backend without documents keep compiling.
Key/value pairing is the backend's job (kinds differ in shape).

**D2. Page versus document.** `Page` gains `annots: Vec<Annot>`; the
backend keeps a `page_index` (pages delivered + 1) and a pending list
of document marks; `PageSink` gains `fn document(&mut self, mark:
DocMark) {}`; marks referring to the current page are resolved to an
index when made, `/Page n` marks to `n`; delivered at `showpage`
before the page (so a sink sees the marks a page needs first) — or,
for marks made after the last page, at a new `finish` call on the sink
(`fn finish(&mut self) {}`) that `distill` invokes after the run;
record the exact order. `DocMark` variants: `Outline { title, count,
target }`, `Dest { name, target }`, `Info(entries)`, `View { page_mode,
page_layout, open }`, `PagesDefault(attrs)`, `PageAttr { page, attrs }`;
`Target = Named(name) | Page { index, view }`; `View = Fit | FitH(top) |
Xyz { left, top, zoom }` (options allowed to be absent as the mark
reference permits). *Alternative:* keep everything in the page —
outlines and named destinations are document-scoped and would need
merging in the writer anyway.

**D3. Rectangles through the CTM.** `/Rect` is transformed by the CTM
in effect at the mark (the mark reference's rule; implementer confirms
in the vault) to default user space, normalised to lower-left/upper-
right; the IR stores default-space rectangles as it stores paths.

**D4. Writer.** At finish: outlines built from the ordered `Outline`
marks with the count rule (`/Count n`: the next n items are children;
negative closes) into `/Outlines` with `First/Last/Next/Prev/Parent`
and item `/Count`; named destinations into the catalog's `/Dests`
dictionary (name → `[page /Fit]`-style arrays); page attributes applied
when the page is written (so `PAGE` marks must precede the page's
delivery — they do, being made before `showpage`); `/Annots` written
with the page (destinations by name resolve at view time through
`/Dests`, so no forward-reference problem); `/Info` merges `DOCINFO`
entries with the producer (job values win except `Producer`); catalog
gains `/PageMode`, `/PageLayout`, `/OpenAction`. Unknown info keys are
written as strings. Everything deterministic; no dates unless given.

**D5. Report.** `Report` gains `marks_written` and `marks_ignored` (a
small map kind → count); the CLI prints one line when any were ignored.

**D6. `moveto` rule.** `Path::move_to` replaces a trailing `Move`
segment instead of pushing; `start`/`current` update as before. The
IR and PDF of affected paths lose the stray one-point subpath.

**D7. Corpus.** `corpus/unit/pdfmark/`: nested bookmarks, cross-page
link by name, URI link, document info, view settings, crop box and
rotate, unknown kind tolerated, the guarded idiom, a `typecheck` case;
plus `graphics/moveto-replaces-moveto.ps`. `.ir` goldens show the new
sections; `.pdf` goldens are checked by the external checker for
outlines, page mode, and title where it reports them. The private
oracle run must keep every raster verdict (annotations do not paint).

## Risks / Trade-offs

- [Outline count semantics are subtle] → the count rule is tested with
  three nesting shapes and a closed branch.
- [A mark after the last `showpage`] → delivered through the sink's
  `finish`; the writer handles marks that name no page as
  document-level only.
- [Job-supplied dates break determinism goldens] → dates are copied
  only when the job gives them; corpus files give none.
