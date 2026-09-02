# Design: Graphics state and vector IR

Graphics semantics are the PostScript Language Reference's (PLRM3 §4
graphics, §7.1 device setup as far as v1 honours it). This document records
the layer boundary, the IR shape, and the decisions.

## 1. Goals and constraints

- **Vector-preserving.** No rasterization anywhere; the IR records what was
  painted, resolution-independent.
- **N-channel colour end to end.** A colour is a colour-space reference plus
  a variable-length component vector; DeviceN and Separation pass through.
- **PDF-shaped.** IR operations correspond closely to PDF content-stream
  operations, so serialization is mechanical and semantic comparison is
  natural.
- **Narrow boundary.** The VM knows a trait, not a graphics library; the
  graphics layer knows objects and numbers, not the interpreter.
- **Deterministic dumps.** The IR has one canonical text form; goldens diff.

## 2. The boundary: `GraphicsBackend` in ps-vm

```
trait GraphicsBackend {
    // state:   gsave, grestore, grestoreall, initgraphics, set/current for
    //          line width/cap/join/miter/dash/flat, matrix ops (concat,
    //          setmatrix, currentmatrix, transform/itransform/dtransform/…)
    // colour:  setcolorspace(SpaceSpec), setcolor(&[f32]), current…
    // path:    newpath, moveto/lineto/curveto (+relative), arc family,
    //          closepath, currentpoint, pathbbox, rect helpers
    // paint:   fill, eofill, stroke; clip, eoclip, initclip, clippath
    // images:  image(ImageSpec, data: &[u8]), imagemask(…)
    // pages:   set_media_box, showpage, copypage, erasepage
}
```

Decisions:

- **The trait speaks numbers and small enums, never `Object` and never
  `Interp`.** Operators in `ps-vm` pop and type-check operands, then call
  the backend; backend errors are `VmError`s. This keeps the trait
  implementable by any consumer and testable without an interpreter.
- **Procedure-driven work stays in the VM.** `image` data acquisition from
  procedures is run by the operator using loop frames (like `forall`),
  accumulating bytes; the backend receives complete data. Nothing in the
  backend ever executes PostScript.
- **Queries go through the trait too** (`currentpoint`, `currentmatrix`, …):
  the backend owns the graphics state, the VM owns nothing graphical.
- **No backend installed → graphics names are simply not defined.** The
  operator group registers only when a backend is; a pure scripting embedder
  sees `undefined` for `moveto`, which is accurate.
- **`gsave`/`grestore` versus `save`/`restore`:** `save` calls the backend's
  gsave and records the depth (already plumbed); `restore` pops to that
  depth. `grestore` below the bottom of the current save's region clamps,
  per the spec's nesting rules.

## 3. Graphics state (in ps-graphics)

A `GState` value (CTM, colour space + components, line parameters, dash,
flatness, clip identifier, media box) on a plain stack. The current path is
not part of the gstate (per the spec it survives gsave/grestore); it lives
beside the stack. CTM math is `f32` with the same formatting rules as the
rest of the project. Default user space is PDF's: origin bottom-left, 72
units per inch; there is no device resolution anywhere.

The clip is represented as a stack of path+rule entries referenced by id;
the IR records clip changes as operations, and the dump names clips by
content, not id, so dumps stay stable.

## 4. The IR

```
Page { media_box, ops: Vec<IrOp>, resources: Resources }

IrOp
├── Save / Restore                        (q / Q)
├── Concat(Matrix)                        (cm)
├── LineWidth(f32) | LineCap | LineJoin | MiterLimit | Dash | Flatness
├── SetColorSpace(SpaceRef) | SetColor(components)
├── Path(Vec<Seg>) + Paint(Fill|EoFill|Stroke)   one op: path then paint
├── Clip(rule, Vec<Seg>)
├── Image(ImageRef)
└── Group hooks reserved (BeginGroup/EndGroup — unused in v1)

Seg = Move | Line | Curve | Close      (arcs are emitted as curves)
Resources: colour spaces, images (fonts later), interned per page
```

Decisions:

- **Paths are flattened to the four PDF segment kinds at emission**; arcs
  become Bézier curves in the backend (the standard four-arc construction),
  so the IR needs no arc segment and the serializer none either.
- **A paint operation carries its path.** PostScript's current path is
  builder state; the IR records only what was painted (or clipped). This is
  the single biggest semantic compression and makes dumps read like the
  page.
- **State-setting ops are recorded lazily** — only the settings in effect
  at a paint are emitted, deduplicated against the previously emitted state,
  so trivial gsave/grestore churn in the program does not bloat the IR.
  `Save`/`Restore` pairs appear only around clips (matching PDF's model
  where clip is part of the saved state).
- **Colour spaces are resources.** `SpaceSpec` covers DeviceGray/RGB/CMYK,
  Separation, DeviceN, Indexed, and ICC-carrying arrays as data (names +
  parameters + alternate space + tint-transform *as a captured procedure
  rendered to a PostScript-calculator function later*; in this change the
  tint transform is stored as opaque source bytes with its span). Nothing
  converts between spaces.
- **Images carry their spec** (dimensions, bits, colour space ref, decode,
  matrix) **and raw data**; no recompression, no decoding beyond what
  acquisition required.
- **Marks-to-source**: every `IrOp` carries the `Span` of the token that
  triggered the paint (the scanner already provides spans), enabling
  click-a-mark tracing later at near-zero cost now.

## 5. Page delivery and the dump

`PageSink` receives completed `Page`s at `showpage` (`copypage` delivers a
clone without resetting, per spec semantics). The dump format is versioned
(`ir/1`), one op per line, canonical number formatting shared with the rest
of the project, resources listed first. `efterscript ir <file>` prints it;
`difftest run` compares it when a sidecar golden `corpus/golden/ir/<same
path>.ir` exists — corpus graphics files get both expectation headers and
sidecar goldens.

## 6. setpagedevice

Reduces to: `/MediaBox`-equivalent extraction (`/PageSize` → media box),
everything else accepted and recorded into a page-device dictionary
readable back via `currentpagedevice`. Unknown keys never error. This is
the tolerant-acceptance policy; per-key mappings to viewer preferences are
a later, distillation-side concern.

## 7. Alternatives considered

- **Retained scene graph with object identity** — richer, but PDF-shaped
  linear ops serialize directly and compare naturally; a scene graph would
  be rebuilt from them anyway.
- **Recording every state change eagerly** — simpler emitter, noisy IR,
  noisy goldens; lazy emission keeps dumps semantic.
- **Three-component colour with tagged extensions** — rejected outright;
  variable-length components from day one.
- **Backend executing tint/image procedures via a callback into the
  interpreter** — inverts the dependency and reentrancy is poison; the VM
  drives all procedure execution.
