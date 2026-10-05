# Change: Stroke adjustment in the PDF

## Why

`setstrokeadjust` keeps its setting in the VM's graphics state and
nowhere else: the IR does not carry it and the PDF never states it. ISO
32000-1 §10.7.5 gives `SA` a default of `false`, but a renderer is free
to adjust strokes when a document is silent, and at least one widely used
rasteriser does. A program's thin rules then come out snapped and
thickened on screen when the program asked for no adjustment, or the
reverse.

The captured LaserWriter 8.7 job from the `masked-images` acceptance is
the case in hand. With masked images it completes, but the oracle still
reports 1.37% of pixels differing on its page, all of it on two hairline
double rules. The reference's PDF states `/SA false` at the start of the
page; ours states nothing. Writing `/SA false` into our PDF by hand brings
the difference to 0.007% (limit 0.5%), so the setting the program is
already in, stated, is the whole fix.

It matters now because every PDF the interpreter writes with a stroke is
affected, not only this job, and because the IR is the boundary between
the VM and every backend: a setting the IR does not carry cannot be
honoured by any of them. Retrofitting it later means touching the same
goldens again.

## What Changes

- **The backend hears stroke adjustment** (`efterscript-vm`): the
  graphics-backend boundary gains a `set_stroke_adjust` call, made on
  every `setstrokeadjust`, on the reset a glyph procedure starts with,
  and on a restoration that changes the value, as overprint is today. A
  backend that keeps nothing ignores it.
- **The IR carries it** (`efterscript-graphics`): a stroke is preceded by
  the stroke adjustment setting in effect wherever the IR has not yet
  stated it in that content (a page, a glyph procedure, a pattern cell,
  a form body), and wherever it has changed since. The dump prints it as
  a state line, `sa true|false`.
- **The PDF states it** (`efterscript-remelt`): the setting becomes an
  extended graphics state resource carrying `SA`, one per distinct value
  used, selected with `gs` where the IR sets it (ISO 32000-1 §8.4.5).
  Overprint keeps its own resources, and their names, unchanged.
- **Goldens**: every IR and PDF golden with a stroke gains the stated
  setting (one `sa false` line, one `ExtGState` resource and one `gs`).
  Goldens without strokes do not change.
- **Corpus**: unit programs for the stated default, a toggle across
  `gsave`/`grestore`, and a stroking Type 3 glyph procedure, which starts
  without stroke adjustment.

Out of scope: applying stroke adjustment to the geometry the interpreter
computes itself (`strokepath`, `ustrokepath`, and stroke outlines
painted as fills); the PDF keeps the program's path exact and leaves
adjustment to the renderer. The default stays `false`.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `graphics-ir`: the stroke adjustment setting is carried in the IR and
  printed by the dump, alongside overprint.
- `remelt`: stroke adjustment is written as an extended graphics state.

## Impact

- `efterscript-vm`: `graphics.rs` (`GraphicsBackend::set_stroke_adjust`,
  default no-op), `interp/mod.rs` (`set_stroke_adjust` tells the backend;
  `restore_vm_gstate` repeats a changed value).
- `efterscript-graphics`: `state.rs` (the backend's graphics state keeps
  the value), `backend.rs` (the emitter's record of what the IR set, and
  the flush before a stroke), `ir.rs` (`IrOp::StrokeAdjust`), `dump.rs`.
- `efterscript-remelt`: `resources.rs` (extended graphics states keyed by
  parameter and value), `content.rs` (the `gs`), `fonts.rs` (what a
  glyph procedure's resources list).
- `efterscript-platen`, `efterscript-cli`, the browser package: no API
  change; their PDFs state stroke adjustment.
- Corpus: new unit files; the existing goldens with strokes change as
  above, and no other golden changes.
