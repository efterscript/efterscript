# Design: Minimal distillation engine

See proposal.md for motivation. This document fixes how `remelt` consumes
the IR and drives `pdf-out`; the spec fixes what the output must contain.

## Context

- The IR (`ps_graphics::Page`) is PDF-shaped by design: paths already in
  default user space, paint operations carrying their path, state settings
  emitted lazily, `Save`/`Restore` only around clips, colour spaces and
  images interned per page. Two things are not directly PDF: a `Stroke`
  carries the CTM it was measured in, and a Separation/DeviceN tint
  transform is captured PostScript source, not a function.
- `pdf-out` serialises immediately: allocate ids, write objects in any
  order through closure builders, `PageTree::add_page` writes a content
  stream and page dictionary, `Document::finish` closes. It knows nothing
  about PostScript and must stay that way.
- The interpreter owns its backend (`Box<dyn GraphicsBackend>`), the
  backend owns its sink, and `PageSink::page` returns nothing. Delivery
  happens inside `showpage`; the document must be finished after the run.
- Determinism is already a `pdf-out` requirement; `remelt` must not spend
  it (no timestamps, no addresses, no hash-ordered iteration).

## Goals / Non-Goals

**Goals:**
- One faithful mapping per IR operation and resource, testable without an
  interpreter by feeding `Page` values to the sink.
- Streaming: a page is fully written when `showpage` returns; memory is
  bounded by the largest page, not the document.
- The public surface later features grow on: a sink type, an options
  struct, and a driver function.

**Non-Goals:**
- Any interpretation or validation of tint-transform source; any policy;
  any PDF reading. A content-stream parser for semantic comparison is a
  separate tool change.
- Compression better than the stored-block container `pdf-out` has.

## Decisions

**D1. Two layers: a sink and a driver.** `remelt::PdfSink<W: Write>`
implements `ps_graphics::PageSink` and writes one page per call into a
`pdf_out::Document<W>`; `remelt::distill(program, config, options, out)`
builds the interpreter, installs `Graphics::new(shared sink)`, runs, drops
the interpreter, recovers the sink, and finishes the document. The sink is
the unit under test (pages built by hand, as `ps-graphics` tests its
backend); the driver is a few lines. *Alternative:* a single function that
hides the sink — loses the interpreter-free tests and forces every
embedder through `Interp::run`.

**D2. Write errors are latched, not lost.** `PageSink::page` cannot return
an error, so the sink keeps the first `pdf_out::Error` (or I/O error) and
ignores later pages; `finish` returns it. A page that fails to write is
never half-written: the content stream is built into a `Vec<u8>` first and
`add_page` is called once. *Alternative:* panic in the sink — an
embedder's disk-full becomes a crash inside `showpage`.

**D3. Sink ownership after the run.** The driver holds
`Rc<RefCell<Option<PdfSink<W>>>>`, hands a clone to `Graphics`, and after
dropping the interpreter takes the sink out of the cell. The `Option` is
what makes `finish(self)` possible through shared ownership. The `Rc`
never escapes the driver.

**D4. Stroke CTM as `q cm … S Q` with the inverse-mapped path.** The IR
path is in default space; PDF measures the width in the space current at
the stroke. Wrapping the stroke in the recorded matrix and mapping the
path back through its inverse makes PDF's arithmetic reproduce the
program's exactly, for any matrix including skew and non-uniform scale.
The identity emits no wrapper. A singular matrix has no inverse: the
stroke is written unwrapped with the width as recorded, because emitting
nothing would drop a mark and a singular `cm` would collapse the page's
own transform. *Alternative:* scale the line width and dash by the
matrix's expansion factor — exact only for uniform scale and rotation, and
it changes the numbers the program set.

**D5. Colour.** Device spaces use `g`/`rg`/`k` (PDF's direct operators;
no resource, and they read as the program wrote them). Every other space
becomes `/CSn` in the page's `ColorSpace` resources, selected with `cs`
and coloured with `scn`, indexed by the IR's `SpaceRef` so the mapping is
`n = SpaceRef.0`. A Separation or DeviceN tint transform is written as a
Type 4 (calculator) function stream whose body is the captured source
bytes verbatim, `Domain [0 1]` per component, `Range [0 1]` per component
of the alternate; the alternate space is written recursively (an
alternate is a device space in every case the graphics layer accepts
today). The captured source is not checked against the calculator
subset — that is the deferred validator. Indexed writes
`[/Indexed <base> <hival> <hex lookup>]` inline. *Alternative:* convert
Separation to its alternate at distillation — exactly the loss the
project exists to avoid.

**D6. Images.** One image XObject per `Resources::images` entry, named
`/Imn` by `ImageRef`, samples in the Flate stored-block container (the
only filter `pdf-out` has; recompression is policy). `Decode` is written
only when it differs from the default for the space and depth, since the
IR fills in the default when the program gave none. Masks write
`/ImageMask true` and no `ColorSpace`. The paint is `q <matrix> cm /Imn
Do Q`; the IR's matrix already maps the unit square in PDF's row order.

**D7. Content-stream text.** Numbers go through `pdf_out::fmt_real`; one
operation per line, operands before the operator, so an uncompressed
stream diffs line by line and matches the IR dump one-to-one. The
`Options` struct has one field, `compress: bool` (default true); the
corpus goldens are written with `compress: false`. Content streams are
`Filter::Flate` when compressing, `Filter::None` otherwise; image data is
always in the Flate container.

**D8. Document skeleton.** Header comment, catalog, flat page tree, Info
with `Producer (EfterScript <version>)` and nothing else. No dates:
determinism is worth more than metadata, and `pdfmark`/DocInfo is a
later change. Media box comes from `Page::media_box`, so `setpagedevice`
page sizes flow through unchanged.

**D9. A failed job finishes the file.** `distill` returns
`Result<Report, Error>` where `Report { outcome: Outcome, pages: usize }`:
the interpreter's outcome is data, not an error; only a write failure or
a `pdf-out` error is `Err`. The CLI exits 1 on an error outcome after
writing the file, so a partially distilled job is inspectable.

**D10. Corpus PDFs are byte goldens of our own output, not oracle
comparisons.** `difftest` distils every file that delivered pages and,
when `corpus/golden/pdf/<path>.pdf` exists, compares bytes and reports a
line diff (the goldens are uncompressed so the diff is legible). The
goldens carry `% GENERATED-BY: difftest --update-pdf` as a comment
between objects, via `Document::comment`. The external checker hook
reuses `EFTERSCRIPT_PDF_CHECK` exactly as `pdf-out`'s golden test does.
*Alternative:* parse-and-compare against another interpreter's PDF — the
right long-term oracle test, but it needs a content-stream reader and an
oracle in CI, neither of which this change should carry.

**D11. The CLI's program output.** `efterscript pdf` keeps the program's
standard output on the host's standard output unless the PDF itself goes
there (`-`), in which case program output moves to standard error, the
same rule `efterscript ir` follows for the dump.

## Risks / Trade-offs

- [Captured tint transforms may use operators outside the calculator
  subset, giving a PDF whose function is invalid] → recorded as the
  deferred validator; the corpus separation file uses a calculator-safe
  procedure so its golden passes an external checker.
- [Byte goldens churn whenever `pdf-out` or number formatting changes] →
  `--update-pdf` regenerates them in one command and the diff is
  reviewable text; the number of PDF goldens is kept to the files whose
  `.ir` golden exists.
- [Inverse-mapping a stroke path introduces rounding not present in the
  IR] → the round trip is `f32` through a well-conditioned matrix; a
  property test checks that mapping through the matrix and back stays
  within tolerance, and the corpus scaled-stroke golden pins the printed
  numbers.
- [`Rc<RefCell<Option<_>>>` in the driver looks like a smell] → confined
  to `distill`; documented as the consequence of the trait's ownership.

## Open Questions

- Whether the corpus PDF goldens should be run through the external
  checker in CI by default once one is available there. Does not change
  this design; the hook exists either way.

## Implementation notes

Recorded where the code departs from, or pins down, the decisions above.

- **D1/D3, the writer's lifetime.** The interpreter holds its backend as
  a `Box<dyn GraphicsBackend>`, which is `'static`, and the backend owns
  the sink, so `distill` requires `W: Write + 'static`: a `&mut Vec<u8>`
  cannot be the writer. `distill` therefore hands the writer back with
  the report — `Result<(Report, W), Error>` — the way
  `pdf_out::Document::finish` returns its sink, so a caller writing to
  memory keeps its bytes and one writing to a file can close it. `Report`
  stays plain data. The shared cell is wrapped in a private `Shared<W>`
  newtype implementing `PageSink`, so `ps-graphics` did not need a
  `PageSink for Option<S>`.
- **D2.** `PdfSink::new` can fail (the header is written at once).
  `finish` returns the latched error without attempting to close the
  document: an object that failed mid-write has already broken the file.
  `pages()` counts pages actually written.
- **D5, device spaces.** The emitter records `cs n` without a following
  `sc` when only the space changed, relying on `setcolorspace` selecting
  the space's initial colour; PDF's `cs` does the same, so a device-space
  `SetColorSpace` is written as `/DeviceGray cs` (`/DeviceRGB`,
  `/DeviceCMYK`), which needs no resource, and `SetColor` in a device
  space as `g`/`rg`/`k`. The content stream thus mirrors the dump line
  for line. Non-device spaces are inline arrays both in the page's
  `ColorSpace` dictionary and in an image dictionary that uses them; the
  function streams behind them are written once and shared by reference.
  Function streams follow `Options::compress` like content streams. The
  Indexed lookup is always hexadecimal: `pdf-out` gained
  `Val::hex_string`/`ArrayBuilder::hex_string` (additive) so binary data
  does not switch form when its bytes happen to be printable.
- **D6.** A mask is an image whose `color_space` resource is `None`.
  `BitsPerComponent` is written for masks too (always 1). The default
  `Decode` compared against is `[0 1]` per component, `[0 2^bits−1]` for
  Indexed, `[0 1]` for a mask. `Interpolate` appears only when true. The
  paint is one line, `q a b c d e f cm /Imn Do Q`, mirroring the dump's
  `Do img` line; the stroke wrapper is the one place the stream has a
  line the dump lacks (the closing `Q`).
- **D7.** Dash is written `[a b] phase d`. The scaled-stroke scenario
  sets a width of 1, which is PDF's initial value as well, so the emitter
  records nothing and the stream carries no `w`; the scenario's "the line
  width is 1" holds by default.
- **D8.** Per page the objects are written in the order functions, image
  XObjects, content stream, page dictionary; the page tree, catalog, and
  Info close the file.
- **D10.** `difftest` already collects the delivered pages, so it feeds
  them into a `PdfSink` instead of running the program a second time
  through `distill`; the sink is deterministic per page, so the bytes are
  those `distill` would write. The produced document is part of `Actual`
  and is built for every run. PDF goldens carry the two SPDX comments
  before the `GENERATED-BY` comment, as the `.ir` goldens do. Command-line
  paths are made absolute so goldens are found for a relative
  `corpus/unit/...`. The checker's output is captured into the failure
  report; the files it is run on stay under `target/difftest/` for
  inspection. The external PDF checker's validation and text extraction accept every
  golden with no warning; nothing in CI runs a checker yet.
- **D11.** Host failures — unreadable input, uncreatable output, a write
  error — exit 2, as `run` and `ir` already do for an unreadable file; 0
  and 1 are reserved for the job's outcome. Output goes through a
  `BufWriter`, which `Document::finish` flushes.
- **Tests.** `remelt`'s tests include `pdf-out`'s test-side reader by
  `#[path]` rather than copying it. `ps-graphics` is unchanged.
