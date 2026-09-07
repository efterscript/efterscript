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

## Implementation notes

Recorded where the code departs from, or pins down, the decisions
above.

- **D1, the value type.** `MarkValue` gained a `Null` variant: the
  pdfmark reference's view arrays use `null` for a component the viewer
  keeps (`[/XYZ null null null]`), and a program that writes them must
  not be `typecheck`. Executable arrays (procedures), operators, marks,
  and the VM-only types are `typecheck`; a literal array or packed
  array converts recursively; a dictionary with a non-name key is
  `typecheck`; nesting beyond 32 levels is `limitcheck`. The kind is
  checked before the values, so `[ /Title (x) (OUT) pdfmark` fails on
  the kind. The operator lives in `ops/pdfmark.rs`, in the graphics
  group, with its table appended last so earlier operator indices stay
  put. The trait method `pdfmark(kind, entries)` defaults to `Ok(())`;
  the mock records calls as `Call::PdfMark`.
- **D2, no pending list and no `finish` on the sink.** A mark is
  resolved when made (the page under construction is `delivered + 1`,
  counting copied pages) and handed to the sink at once through
  `PageSink::document`. Page delivery happens only at `showpage` and
  `copypage`, so delivering marks immediately produces exactly the
  order batching at `showpage` would — every mark precedes the page it
  refers to, and a mark after the last page follows it — without the
  backend needing an end-of-run signal, which the trait does not have
  and `distill` cannot give it (the backend is a boxed trait object
  inside the interpreter). `PageSink::finish` was therefore not added:
  nothing waits for it, and `PdfSink::finish(self)` remains the single
  close. The only annotations that wait are links placed on a later
  page with `/SrcPg`; they are attached when that page is delivered.
  `DocMark::Dest` carries `page` and `view` directly rather than a
  `Target`, since a named destination never refers to another name.
- **D2, the tally.** `Graphics::ignored_marks()` keeps the count by
  kind for embedders, and each ignored mark also reaches the sink as
  `DocMark::Ignored { kind }`, which is how the count gets to `Report`:
  `distill` can reach the sink but not the backend. The dump prints it
  as `ignored /<kind>`, so the tolerance scenario has a golden. Keys are
  the kind (`NOSUCH`), `ANN/<Subtype>` for an annotation of another
  subtype (`Text` when the mark gives none, per the reference), and the
  kind again for a mark missing what its kind requires or a link placed
  on a page already delivered.
- **D2, what the parser accepts.** Pairs are a name followed by its
  value, later keys overriding earlier ones; a value that is not
  preceded by a name is skipped. `/Page` takes a positive integer,
  `/Next`, `/Prev`, or `0` (the null destination: a bookmark without
  a target); absent, it is the current page. `/View` honours `Fit`,
  `FitH top`, and `XYZ left top zoom` (a zoom of 0 counts as absent);
  other fit types degrade to `Fit`, and an absent `/View` is `Fit`. A
  bookmark whose `/Action` is anything but `GoTo` (a name or a
  dictionary) keeps its place in the tree with no destination, so the
  count nesting survives. A link's target is `/Dest` (name or string),
  a URI from an `/Action` dictionary with subtype `URI` or from
  `/Action /Launch` with a `/URI` beside it, else the page (`/Page` or
  the current one) with the view. `/Border` keeps its first three
  numbers (the dash array is dropped); `/Color` or `/C` needs three
  numbers. `DOCINFO` keeps string-valued entries only. `DOCVIEW`'s open
  target needs `/Dest` or `/Page`. `PAGE` names the current page only
  (the reference gives it no page key); `/Rotate` is accepted on `PAGE`
  and `PAGES` when a multiple of 90. `erasepage` keeps the page's
  annotations; a mark under the null device still resolves to the page
  that would be delivered next.
- **D3.** All four corners go through the CTM and the result is boxed
  and normalised, so a rotated user space gives the axis-aligned box
  of the rotated rectangle.
- **Dump.** `dump::document(pages, marks)` is `dump::pages` followed,
  only when there are marks, by a blank line and the `doc:` section;
  `dump::pages` is unchanged, and every pre-existing golden is
  byte-identical apart from the six charpath goldens D6 changes. A run
  with marks but no pages dumps as the version line and the section.
  `Collected` (pages plus marks with the number of pages delivered
  before each) is the collecting sink `difftest` and the CLI's `ir`
  mode use; `Collected::replay` feeds a `PdfSink` in the original
  order, so the corpus PDF goldens are what `distill` would write.
- **D4, pdf-out.** `PageTree` gained `add_page_with` (extra page
  entries after `Contents`), `finish_with` (extra catalog entries after
  `Pages`), and `pages()` (the ids so far); `add_page` and `finish`
  delegate with empty closures, so a document without marks is
  byte-identical (checked against the stroked-line golden). Annotation
  objects are allocated when their page is written and written at
  finish, since a link may lead to a page not yet written; outline
  items and the destinations dictionary follow the embedded fonts, then
  the page tree and catalog. Object order: annotations in page order,
  the outline root, items in mark order, `Dests`.
- **D4, the writer's rules.** Outline item `/Count` is the number of
  descendants a viewer shows when the item is open, negative when the
  mark's count was negative (ISO 32000-1 §12.3.3); the root's is the
  visible total. A count that names more children than the document
  has ends the branch early. Named destinations are written sorted by
  name, the later definition of a name winning, and a destination or
  link naming a page the document lacks is left without one. `/Border`
  defaults to `[0 0 0]` as decided (the reference's own default is a
  one-unit border; drivers set it explicitly). Info entries are
  written in first-seen order with later values replacing earlier
  ones, then `Producer`, which a job cannot override. `PAGES` defaults
  apply to pages written after the mark, so a job must place it before
  its first page, as the reference recommends. `OpenAction` by name is
  the name itself; by page it is the destination array.
- **D5.** `Report { marks_written, marks_ignored }`; `marks_written`
  counts honoured document marks and link annotations. The CLI prints
  `efterscript: N pdfmark(s) ignored: kind×n, …` on standard error.
- **D6.** `Path::move_to` replaces a trailing `Move` in place (through
  `Rc::make_mut`, so a saved state's copy is untouched). Six corpus
  goldens changed, each a charpath whose program `moveto` preceded the
  outline's own first move — `fonts/cff-charpath-bbox`,
  `fonts/charpath-fill`, `fonts/composite-charpath-fill`,
  `fonts/resident-charpath-fill`, `fonts/type1-seac-glyphshow`,
  `fonts/type42-charpath-bbox` — and the last one's expected `pathbbox`
  output moved from `0.0` to `0.977` (the outline's own start), which
  is what the reference interpreter reports too (`0.984`, the residual
  being the recorded control-box difference). Its generator template in
  `ps-fonts` was updated with it.
- **D7, corpus and oracle.** Eleven files under `corpus/unit/pdfmark/`
  (`guarded-idiom`, `guarded-idiom-no-backend`, `kind-typecheck`,
  `nested-bookmarks`, `closed-bookmarks`, `cross-page-link`,
  `uri-link`, `document-info`, `view-settings`, `crop-and-rotate`,
  `unknown-kind`) and `graphics/moveto-replaces-moveto`, with `.ir` and
  `.pdf` goldens for every file that delivers pages. The external
  checker (the private tier's reference interpreter, run from the
  shell) re-distils every marked golden without a warning and keeps
  the outline titles and counts, the named destination, the link and
  URI action, `PageMode`/`PageLayout`/`OpenAction`, the crop boxes,
  and the information entries; a second checker reports the title,
  author, subject, keywords, and creator of `document-info`, the
  destination `top` on page 1 of `cross-page-link`, the URI of
  `uri-link`, and the crop box and 90° rotation of `crop-and-rotate`.
  The oracle harness compares a page's media box with the rendered
  size, which a quarter-turn rotation swaps; it now reads the
  rotation from the marks delivered before the page. Oracle totals
  after the change: 165 files, 126 pass, 0 fail, 33 expected-
  divergence, 1 divergence-closed, 5 skipped; output 128 same, 32
  differs (before: 153 files, 115/0/33/1/4; output 117/32 — every
  pre-existing verdict unchanged, the twelve new files all pass, and
  `type42-charpath-bbox` remains an output difference for the reasons
  already recorded). Gates: 804 tests pass (2 ignored), clippy and fmt
  clean, `difftest run` 165/165, `parse-survival` 165 files,
  `fuzz-round` green, `lint-strings` clean, `openspec validate
  pdfmark` valid.
