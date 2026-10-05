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
matches PDF's `Q`.

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
