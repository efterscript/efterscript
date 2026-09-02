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
flatness, clip identifier, media box, and the current path) on a plain
stack: per PLRM3 §4.2 the current path is part of the graphics state and
`gsave`/`grestore` save and restore it like every other component, so the
backend's gsave copies the path (paths are small; sharing the segment
vector until mutation keeps the copy cheap). CTM math is `f32` with the same formatting rules as the
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

## 8. Implementation notes

Recorded where the code departs from, or pins down, the text above.

### Part 1

Covers the ps-vm side: the `GraphicsBackend` trait, the graphics operator
group, `image`/`imagemask`, `setpagedevice`/`currentpagedevice`, and the
save/restore plumbing. The graphics crate is untouched.

- **The operator table is a process-wide static chain, so "registered
  only with a backend" is realised through a third visibility.** Every
  `OpEntry` now carries `Visibility::{Public, Internal, Graphics}`;
  graphics entries have stable table indices from the start but are
  inserted into `systemdict` only by `Interp::set_graphics_backend`
  (through the raw dictionary storage, since `systemdict` is read-only by
  then; the entries are global and older than every save). Without a
  backend `moveto` is not a name in `systemdict`, so the spec scenario is
  met by the ordinary lookup failure, with the name as the offending
  command. Each operator additionally answers `undefined` if called with
  no backend, which is reachable only through an operator object held
  across a backend swap. `Interp::operator(name)` resolves graphics names
  only while a backend is installed, so the error machinery reports them
  correctly.
- **`setpagedevice` and `currentpagedevice` are always defined**, in a
  separate public group. They are device configuration, not marking; the
  page-device dictionary lives in the VM, and the "Unknown keys accepted"
  scenario has to run as a corpus file under `difftest`, which has no
  backend. With a backend installed, a `PageSize` request is forwarded as
  `set_media_box([0 0 w h])`; installing a backend forwards the current
  page size once so the two agree. The dictionary is global, read-only
  (`currentpagedevice` returns it directly, so a program cannot corrupt
  it), seeded with `/PageSize [612 792]`, and every request value is
  deep-copied into global VM (arrays, strings, dictionaries; other local
  composites are `typecheck`) so the global/local rule holds and the
  recorded values survive `restore`. A malformed `PageSize` (not two
  numbers) is `typecheck`/`rangecheck`; every other key is stored without
  inspection.
- **Trait shape.** All arguments are numbers, slices, and the value types
  in `ps_vm::graphics` (`Point`, `Rect` as origin+extent for the `rect…`
  operators, `Bounds` as corners for `pathbbox` and the media box,
  `Matrix([a b c d tx ty])`, `LineCap`, `LineJoin`, `SpaceSpec`,
  `ImageSpec`, `Seg`). Queries that cannot fail return plain values;
  everything else returns `Result<_, VmError>`. Beyond §2's list the trait
  has `gstate_depth`, `grestore_to(depth)`, `default_matrix` (what
  `defaultmatrix`/`initmatrix` read), `rectfill`/`rectstroke`/`rectclip`
  (explicit rather than composed, because they must leave the current path
  alone and the path is the backend's), `nulldevice`, and `clippath`
  returning the clip's segments. `ImageSpec.matrix` is a `Matrix`, not a
  bare `[f32; 6]`.
- **What the operator layer computes versus leaves to the backend.**
  Computed here: matrix arithmetic for every form with a matrix operand
  (`translate`/`scale`/`rotate` with a matrix, `concatmatrix`,
  `invertmatrix`, `…transform` with a matrix); `transform`/`itransform`/
  `dtransform`/`idtransform` against the CTM obtained from
  `current_matrix()` (a singular CTM is `undefinedresult`); relative path
  operators as `current_point()` plus the delta (so the trait has no
  relative methods); HSB↔RGB and the gray/RGB/CMYK conversions of the
  `current…color` queries (a Separation, DeviceN, or Indexed current
  colour reads as black through those queries, since evaluating a tint
  transform is not this layer's business); clamping of the device-space
  convenience operators' components to [0, 1] (`setcolor` passes raw
  values); `setlinecap`/`setlinejoin` range (`rangecheck`),
  `setmiterlimit` below 1 and a `setdash` array that is negative or all
  zero (`rangecheck`). Left to the backend: the CTM itself, the Bézier
  work of the `arc` family (raw arguments are passed), `arcto`'s tangent
  points, `currentpoint` in user space, the bounding box, the meaning of
  `nulldevice` (marks are discarded until the state that installed it is
  restored; the reference also resets the CTM, which is the backend's to
  honour), and every parameter range the trait does not check. Failing
  operators leave their operands on the stack.
- **Tint transforms are captured as re-scannable source text.** Arrays
  carry no span (the scanner reports spans per token and the array object
  does not retain one), so the procedure is serialised with
  `ops::output::source`: the `==` form with operators printed as bare
  names, so a `bind`-ed procedure round-trips, and objects without a
  syntax printed as `null`. `currentcolorspace` rebuilds the array from
  the `SpaceSpec`, re-scanning the tint source into a procedure; nested
  device alternates come back as bare names, the top level always as an
  array. `SpaceSpec::Indexed` accepts a string lookup table only; a
  procedure lookup is `typecheck`. `hival` is limited to 4095. Families
  outside DeviceGray/RGB/CMYK, Separation, DeviceN, and Indexed are
  `undefined`; a malformed array of a known family is `rangecheck`/
  `typecheck`. Nesting deeper than eight levels is `limitcheck`.
- **Image data acquisition.** A string source is taken as is; a file is
  read until the required count or end of file; a procedure runs as a
  `LoopFrame::ImageData` frame (`LoopFrame` is now `Clone`, not `Copy`)
  that the loop stepper drives like `forall`, taking one string off the
  operand stack per iteration (anything else is `typecheck`, an empty
  stack `stackunderflow`, with `image`/`imagemask` as the offending
  command) until the byte count is reached or a chunk is empty. The count
  is `height × ceil(width × bits × components / 8)`. A source that runs
  dry does not pad: the data is trimmed to whole rows and
  `ImageSpec.height` becomes the number of rows delivered, so the backend
  always sees exactly `height` complete rows. `image` samples are in the
  current colour space (`color_space: Some(…)`, components from it);
  `imagemask` has `color_space: None`. Default `Decode` is `[0 1]` per
  component, `[0 2^bits−1]` for Indexed, and the Level 1 `imagemask`
  polarity maps to `[1 0]`/`[0 1]`. `ImageType` other than 1 is
  `rangecheck`, `MultipleDataSources true` is `typecheck`, bits outside
  {1, 2, 4, 8, 12} (masks: 1) are `rangecheck`, and `colorimage` is not
  registered.
- **save/restore.** `save` asks the backend for `gstate_depth()`, calls
  `gsave`, and records the pre-gsave depth in the save record (undoing
  the gsave if the save itself fails); `restore` calls
  `grestore_to(depth)` with what the record returns, so the state at
  `save` time is current again. The clamp is operator-layer bookkeeping:
  `Interp` keeps one floor per live save (the depth just after `save`'s
  gsave, aligned with the save stack and truncated with it). `grestore` at
  the floor restores the state without popping it — a `grestore` followed
  by a `gsave` on the backend — and is a no-op on an empty stack;
  `grestoreall` pops to the floor and then does the same. Saves made
  before a backend is installed record depth 0 and a floor of 0.
- **`nocurrentpoint` is a new `VmError`** with the usual default handler.
- **Caution for part 2, §3.** The claim that the current path is not part
  of the graphics state and survives `gsave`/`grestore` should be checked
  against PLRM3 §4.2 and §4.4 before the gstate stack is built; the
  language reference lists the path among the graphics-state parameters.
  Nothing in part 1 depends on either reading.

- **Resolved (reviewer, PLRM3 §4.2 checked in the vault):** the current
  path *is* part of the graphics state and is saved/restored by
  `gsave`/`grestore`; §3 above has been corrected before part 2 builds the
  gstate stack. `save`/`restore` likewise include it (§3.7.7 lists the
  current path among the saved elements).
