# Change: Graphics state and vector IR

## Why

This change is the middle of the pipeline: the graphics operators, the
graphics-state machine, and the page IR that everything downstream
consumes. Two decisions here are nearly impossible to retrofit. First, the
IR must carry N-channel colour (separations, DeviceN, ICC) end to end from
day one — a three-component IR can never grow back the information. Second,
the boundary between the VM and the graphics layer must be a narrow trait,
so the VM stays publishable alone and future backends (raster, SVG) plug in
without touching the interpreter. Making the IR semantically congruent with
PDF content-stream operations is what later reduces the PDF backend to
nearly a serializer.

## What changes

- `ps-vm` gains the graphics operator set and a `GraphicsBackend` trait the
  operators dispatch to; a VM with no backend installed treats graphics
  operators as `undefined` (a scripting-engine embedder never pays for them).
- `ps-graphics` becomes real: graphics-state stack (CTM, colour, line
  parameters, clip), path construction, and per-page IR emission delivered
  to a `PageSink`.
- `save`/`restore` integration: `save` performs the implicit graphics save;
  `restore` pops graphics states to the saved depth (the interpreter already
  passes the depth through).
- A deterministic, versioned text dump of the IR; `difftest` compares it
  against sidecar goldens; `efterscript ir <file>` prints it.
- `setpagedevice` reduces to MediaBox extraction plus tolerant
  accept-and-record of other keys.
- Out of scope, recorded with triggers: text operators (the font change),
  `charpath`, flattening, transparency semantics (the IR reserves group
  hooks), halftone/screen/transfer beyond accept-and-record, and any
  rendering.

## Impact

- New capability spec: `graphics-ir`.
- Code: `crates/ps-vm` (`ops/graphics.rs`, the trait), `crates/ps-graphics`
  (the implementation), `efterscript-cli` (`ir` mode), `tools/difftest`
  (`--ir` sidecar comparison), corpus files under `corpus/unit/graphics/`
  with goldens under `corpus/golden/ir/`.
- Depends on `interpreter-core`; `pdf-out` is not consumed yet — the
  IR-to-PDF serializer is the following change (`remelt` minimal).
