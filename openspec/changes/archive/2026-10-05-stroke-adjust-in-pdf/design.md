# Design: Stroke adjustment in the PDF

See proposal.md for the motivation and the spec deltas for the behaviour.
This document fixes how the setting travels from the VM to the PDF, and
where it is stated.

## Context

- The VM keeps stroke adjustment in `VmGState` (`interp/mod.rs`), next
  to overprint. Overprint is also handed to the backend
  (`GraphicsBackend::set_overprint`, default no-op), on every
  `setoverprint` and from `restore_vm_gstate` when a restoration changes
  it. Stroke adjustment never leaves the VM. `show.rs` resets it to
  `false` inside the `gsave` that brackets a glyph procedure.
- The graphics backend emits lazily (`backend.rs`): `Emitted` records
  what the IR last set, one entry per open `Save`, and `flush(needs)`
  records the differences before a paint. Overprint is flushed before
  every paint, and stated only once a program has set it. A capture (a
  Type 3 glyph, a pattern cell, a form body) starts its record either
  from the state it inherits (`Emitted::of`) or from the initial state.
- remelt writes overprint as `/GS0` and `/GS1` extended graphics states
  (`/OP` and `/op`), gathered per page from every content
  (`overprints_used`, `Refs::overprints`) and listed by every content
  that selects one.
- ISO 32000-1 §10.7.5 gives `SA` and its default; §8.4.5 the extended
  graphics state. PLRM3 §6.5.2 and the `setstrokeadjust` entry in §8.2
  give the PostScript side.

## Goals / Non-Goals

**Goals:** a stroke in any content stream is drawn under the setting
the program had at that stroke, whichever default a renderer applies to
a silent document, and the dump shows the setting.

**Non-Goals:** adjusting the interpreter's own stroke geometry
(`strokepath`, stroke outlines), or the clip and fill paths; changing
the default.

## Decisions

**D1. State it at the first stroke, not only when it changes.**
Overprint is recorded only once a program sets it, because PDF's
default (`false`) is what every renderer applies. For `SA` the
renderers disagree, and the job in hand shows the effect. The emitter's
record therefore holds `Option<bool>`: `None` until the IR has stated
the value in the content, then the value stated. A stroke is preceded by
`StrokeAdjust(v)` when the record is not `Some(v)`.
*Alternative:* state it at the start of every page. Rejected: a page
without strokes would change for nothing, and a capture would still
need its own statement. *Alternative:* record it only when the program
sets it, as overprint is. Rejected: it does not fix the case in hand,
where the program is in the default.

**D2. Every capture starts unstated.** A glyph procedure and a form body
are written once and used in many contexts, and a pattern cell starts
from the initial state (ISO 32000-1 §8.7.3.1). So none of them may rely
on what the enclosing content stated: `Emitted::of` and the initial
record both start at `None`, and each content states the value before
its own first stroke. A `Restore` pops the record with the rest, which
matches PDF's `Q`. A `gsave`/`grestore` pair without a clip leaves no
`Save` in the IR, so a value changed inside it and restored by its
`grestore` is stated again at the next stroke.

**D3. Only strokes trigger it.** The setting is flushed before an
`IrOp::Stroke` and nothing else. In particular it is not flushed by the
other users of `Needs::Stroke` (a Type 3 `show` and a form placement,
which flush the line parameters for what the capture inherits): by D2
the capture states its own setting.

**D4. The backend hears every change, as with overprint.**
`GraphicsBackend::set_stroke_adjust(on)`, with a default no-op, is
called from `Interp::set_stroke_adjust` and from `restore_vm_gstate`
when the value differs. Through that path the reset in `show.rs` reaches
the backend, inside the glyph's `gsave`, and its `grestore` brings the
value back. The backend's `GState` keeps the value; `initgraphics`
leaves it alone in both layers.

**D5. Separate resources from overprint.** The extended graphics states
become keyed by parameter and value (`Overprint(bool)`,
`StrokeAdjust(bool)`). Overprint keeps `/GS0` and `/GS1` with `/OP` and
`/op` only, so no golden that uses overprint changes for that reason;
stroke adjustment is `/SA0` and `/SA1` with `/SA` only. A `gs` changes
only the keys its dictionary holds (§8.4.5), so the two never interfere.
`Refs::overprints` generalises to the set of these keys, and the page,
cell, body, and glyph-procedure resource dictionaries list what they
select, as now.
*Alternative:* one combined dictionary per (overprint, stroke adjust)
pair. Rejected: it couples two settings that change independently, and
renames the overprint resources.

**D6. Dump line.** `sa true|false`, documented in the dump's format
header next to `op`.

## Risks / Trade-offs

- [About 44 IR goldens and their PDF goldens change] → The change is one
  `sa false` line per content with a stroke, plus one resource and one
  `gs` in the PDF. The goldens are regenerated with `difftest run
  --update-ir --update-pdf`, and the diff is reviewed to contain nothing
  else.
- [The reference may treat the setting differently inside a glyph
  procedure or a pattern cell] → The oracle tier runs over the new
  corpus files; any disagreement that follows from the manual's rule
  is registered as an expected divergence, not papered over.
- [Renderers that ignore `SA`] → Nothing changes for them. The setting is
  stated, not simulated.

## Acceptance

The vaulted job `corpora/realworld-drivers/os904-lw87-finder-window/
job-print-window.ps`, run through `difftest oracle --profile default`
with the hosting application's prelude, passes. It reports 1.373% of
pixels differing before the change, and 0.007% with `/SA false` written
by hand.

## Implementation notes

**As designed**, with one correction to a scenario. In the `graphics-ir`
delta, the restore scenario first said that the stroke after `grestore`
needs no further setting. That holds only when the pair holds a clip. A
`gsave`/`grestore` without a clip leaves no `Save` in the IR (lazy
emission), so nothing in the output undoes the `true` set inside it, and
the stroke after it states `false` again. The scenario, its corpus file,
and D2 say so now.

**Where the setting is flushed.** In `paint`, after the other settings,
when the operation being recorded is a stroke. A Type 3 `show` and a
form placement flush the line parameters with `Needs::Stroke` but never
stroke adjustment (D3).

**Tests.** The VM's recording backend sees every `setstrokeadjust`, the
reset at the start of a glyph procedure, and a restoration only when the
value changes. The backend tests cover the default stated once, a change
and its restore through a clip's `Q`, a stroking glyph procedure, a
pattern cell, a form body, and a fill-only page. remelt tests cover the
names and dictionaries (each holding its own keys only) and a form
body's own resources. Tests that pinned an exact operation list or
content stream with a stroke now include the stated setting.

**Goldens.** 45 IR goldens and their 45 PDF goldens changed. The IR
diff adds `sa false` lines and nothing else. The PDF content changes are
the added `/SA0 gs` lines and `<< /Type /ExtGState /SA false >>`
objects, with the page resources listing them; the other differing
lines are the object numbers, offsets, and stream lengths they shift.
Five new corpus files: `stroke-adjust-default`, `-restore`, `-glyph`,
`-with-overprint`, and `-fill-only`. `qpdf --check`, run as the external
checker, accepts every document `difftest run` writes.

**Oracle tier** (profile `default`). All five new files pass. On
`stroke-adjust-restore.ps` the program's output differs: after the
`grestore` the reference answers `true` to `currentstrokeadjust` when it
runs a program for its output, so its default is on there. Its PDF
output states `/SA false` for the same kind of job, so its own default
depends on the output device; the manual leaves the default to the
device, and ours stays `false` (proposal, out of scope). Output
differences are reported, not counted.

**Acceptance.** The vaulted job `job-print-window.ps`, run with the
hosting application's prelude, passes: 0.007% of pixels differ (limit
0.5%), against 1.373% before.

**Gates.** `cargo test --workspace` 1299 passed (1296 before); clippy
and `cargo fmt --check` clean; `difftest run` 390 files, 390 passed;
`parse-survival` no errors; `fuzz-round` 1300 + 1300 programs, 0
failed; `lint-strings` with the vault 1416 files clean; `check-wasm`
passes; `cargo doc` with warnings denied clean; the build without
default features, the browser package's tests (10 passed), and the site
build pass; `openspec validate stroke-adjust-in-pdf` valid. One full
test run showed a failure in `efterscript-platen`'s FFI test
`a_rejected_identity_and_a_failing_prelude_return_null`, which read
another test's error message. It passed in eight runs of its own and in
the full runs before and after. This change does not touch that code.
