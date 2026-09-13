# Change: Patterns, forms, and user paths

## Why

Driver output of the LaserWriter-8 era and the page-description
libraries behind it lean on three Level 2 constructs the interpreter
does not have: tiling patterns (`makepattern`/`setpattern` and the
`Pattern` colour space) for fills and screens of repeated artwork,
forms (`execform`) for artwork drawn many times from one definition,
and user paths (`ufill`, `ustroke`, `uappend`, the encoded form) which
drivers emit for nearly every path once they detect a Level 2 device.
Today `makepattern`, `execform`, and the user-path operators are
undefined, so a job stops at its first use, and the `Pattern` and
`Form` categories cannot be populated. All three are vector constructs
with exact PDF counterparts (tiling pattern objects, form XObjects,
plain paths), so a distiller preserves them rather than flattening
them. The hard-to-retrofit part is the boundary shape: a pattern cell
and a form body are procedures whose marks must be captured into a
reusable resource instead of the page, the mechanism Type 3 glyph
capture already established, and a pattern is a *colour* that the
colour model, the IR, and the writer's colour-space resources must be
able to carry. Fixing that shape now, before CIE colour and shadings,
means those later changes extend a resource-carrying colour model
rather than rebuild it.

## What Changes

- **Tiling patterns** (`ps-vm`, `ps-graphics`, `remelt`):
  `makepattern` builds a pattern instance from a pattern dictionary
  (`PatternType` 1, `PaintType` 1 coloured or 2 uncoloured,
  `TilingType`, `BBox`, `XStep`, `YStep`, `PaintProc`, optional
  `Implementation`) and the matrix, capturing the CTM; `setpattern`
  and `[/Pattern] setcolorspace … setcolor` with a pattern operand
  (with an underlying space for uncoloured patterns) make it the
  current colour; a paint with a pattern colour runs the `PaintProc`
  once per page with emission redirected into a pattern resource (in
  pattern space, clipped to the cell), and the paint operation
  references it; `currentcolor`/`currentcolorspace` report it. The
  writer emits a tiling pattern object with its matrix, cell, steps,
  paint type, resources, and content, and paints with the `Pattern`
  colour space. The `Pattern` regular category accepts instances.
- **Forms** (`ps-vm`, `ps-graphics`, `remelt`): `execform` executes a
  form dictionary (`FormType` 1, `BBox`, `Matrix`, `PaintProc`) with
  the graphics-state rules of PLRM3 §4.7; the first execution on a
  page captures the body into a form resource (in form space, clipped
  to the bounding box) and every execution emits one placement
  operation carrying the current CTM; the writer emits a form XObject
  and places it with `Do`. The `Form` regular category accepts
  instances.
- **User paths** (`ps-vm`): `upath`, `uappend`, `ufill`, `ueofill`,
  `ustroke` (with and without a matrix), `ucache`, `ucachestatus`,
  `setucacheparams`, and `setbbox`; both the literal array form and
  the encoded (binary number string) form of PLRM3 §4.6; interpreted
  into the existing path-construction and painting entry points, so
  the IR and PDF are unchanged. `ucache` is accepted and ignored.
- **Colour model**: a pattern is a colour; the boundary, IR dump, and
  writer carry a pattern reference plus any underlying components.
- Out of scope, with triggers: shading patterns (`PatternType` 2) and
  `shfill` — their own change with the `Shading` category, when a
  corpus job or the CIE change needs them; `ustrokepath` and
  `strokepath` (stroke outlining) — when a job relies on the outline
  as a path; `Implementation`-keyed pattern reuse across pages beyond
  the per-page resource; pattern fills of text (a pattern as text
  colour is carried like any colour, but a Type 3 glyph painting a
  pattern is deferred); form `Matrix`/`BBox` validation beyond type
  and arity checks.

## Capabilities

### New Capabilities
- `patterns`: pattern instances, the pattern colour, paint-procedure
  capture, and the `Pattern` category.
- `forms`: form execution, capture, placement, and the `Form`
  category.
- `user-paths`: the user-path operators, both encodings, and their
  errors.

### Modified Capabilities
- `graphics-ir`: ADDED requirements for pattern and form resources in
  the IR, a pattern as paint colour, form placement, and their dump.
- `remelt`: ADDED requirements for tiling pattern objects, the
  `Pattern` colour space, and form XObjects in the PDF.

## Impact

- Code: `crates/ps-vm` (`ops/pattern.rs`, `ops/form.rs`,
  `ops/upath.rs`, the pattern colour in `graphics.rs`, loop frames for
  running a paint procedure under capture, `Pattern`/`Form` category
  entries in `ops/resource.rs`), `crates/ps-graphics` (pattern and
  form resources on `Page`, emission redirection generalised from the
  Type 3 glyph capture, a `Form` placement operation, dump lines),
  `crates/remelt` (pattern objects, form XObjects, the `Pattern`
  colour-space resource, content-stream operators), `pdf-out` only if
  a stream helper is missing, corpus files under
  `corpus/unit/patterns/`, `corpus/unit/forms/`, and
  `corpus/unit/upath/` with goldens; `psgen` may gain user-path
  generation later.
- No new dependencies.
- Depends on `text-core` (Type 3 capture), `graphics-ir`,
  `remelt-minimal`, `printer-identity-mechanism` (resource
  categories), all archived.
