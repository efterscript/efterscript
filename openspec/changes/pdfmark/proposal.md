# Change: pdfmark — bookmarks, links, destinations, document info, and page attributes

## Why

Distilled output is only a page image until the document carries what
drivers and authoring tools put into `pdfmark`: bookmarks, links,
named destinations, document information, and viewer defaults. Today
the operator is undefined, so every job that guards it with `where`
silently loses that structure, and a job that does not guard it stops.
This is the first distillation feature beyond the page, and it fixes
the shape all later document-level features (forms, embedded files,
structure) reuse: a mark travels from the VM as small values, lands in
the IR as a page annotation or a document-level mark, and the writer
assembles the document objects at finish. A small graphics bug from
the generator round rides along: consecutive `moveto`s must replace,
not accumulate.

## What Changes

- **The `pdfmark` operator** (`ps-vm`): defined when a graphics backend
  is installed (undefined otherwise, like the painting operators);
  pops the objects down to the mark, takes the last as the mark kind,
  converts the rest to a value tree of names, strings, numbers,
  booleans, arrays, and dictionaries, and hands kind and values to the
  backend; unknown kinds are accepted and counted; malformed marks
  (no mark, kind not a name) raise the usual errors.
- **Marks in the IR** (`ps-graphics`): `ANN` marks with subtype `Link`
  become page annotations (rectangle transformed through the CTM in
  effect, destination by name or URI, border, colour, contents);
  `OUT`, `DEST`, `DOCINFO`, `DOCVIEW`, `PAGES`, and `PAGE` become
  document-level marks delivered through a new sink method, with the
  page they refer to resolved (`/Page` or the current page); the dump
  gains annotation lines inside a page and a `doc:` section after the
  pages, present only when marks exist, so existing goldens are
  unchanged. Other subtypes and kinds are counted, noted, and dropped.
- **Document objects in the PDF** (`remelt`): `/Annots` per page with
  link annotations (`/Dest` by name, or a URI action), an `/Outlines`
  tree from `OUT` marks with the count and nesting rule of the mark
  reference, named destinations in the catalog, `/Info` entries from
  `DOCINFO` (producer kept; no dates unless the job gives them),
  `/PageMode`, `/PageLayout`, and `/OpenAction` from `DOCVIEW`, and
  `CropBox`/`Rotate` on pages from `PAGE` and defaults from `PAGES`.
  Written at finish; deterministic.
- **`moveto` replaces a pending `moveto`**: a `moveto` following a
  `moveto` with no segment between them replaces the subpath start
  rather than leaving a one-point subpath, per the reference's path
  model.
- **Report**: counts of marks written and of kinds ignored, printed by
  the command-line tool with the substitution line.
- Out of scope, with triggers: form widgets and other annotation
  subtypes (when a corpus job carries them), embedded files, `BP`/`EP`/
  `SP` forms, `PS` passthrough, structure and tagging, `LNK`, page
  labels, the `/Dest` view arrays beyond `/Fit`, `/FitH`, `/XYZ`.

## Capabilities

### New Capabilities
- `pdfmark`: the operator's contract, the value conversion, which
  kinds are honoured and how, and tolerance for the rest.

### Modified Capabilities
- `graphics-ir`: ADDED requirements for page annotations, document
  marks and their dump, and the `moveto` replacement rule.
- `remelt`: ADDED requirement for the document objects.

## Impact

- Code: `crates/ps-vm` (`ops/pdfmark.rs`, a `MarkValue` type, an
  additive backend method), `crates/ps-graphics` (annotations on
  `Page`, `DocMark`, `PageSink::document`, dump), `crates/remelt`
  (outlines, dests, annots, info, catalog entries, page attributes),
  `crates/efterscript-cli` (report line), corpus files under
  `corpus/unit/pdfmark/` with goldens, difftest unchanged.
- Dependencies: none new.
- Depends on `remelt-minimal`, `graphics-ir`, `text-core` (archived).
