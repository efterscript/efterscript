# Tasks: stroke-adjust-in-pdf

## 1. The boundary and the VM (efterscript-vm)

- [x] 1.1 `GraphicsBackend::set_stroke_adjust` with a default no-op, called from `Interp::set_stroke_adjust` and from `restore_vm_gstate` when the value changes (D4); verified by a VM test with the recording backend that sees `setstrokeadjust`, the glyph procedure's reset, and a `grestore` that changes the value, but not one that leaves it unchanged; every golden stays byte-identical

## 2. IR and dump (efterscript-graphics)

- [x] 2.1 The backend's graphics state keeps the value; `IrOp::StrokeAdjust`; the emitter's record is `Option<bool>`, `None` at page start and at the start of every capture, and the setting is flushed before a stroke only (D1–D3); the dump prints `sa true|false` and its format header documents it (D6); verified by backend unit tests for the default stated once, a change, a restore, a stroking glyph procedure, a pattern cell and a form body, and a fill-only page with no setting
- [x] 2.2 Corpus files for the `graphics-ir` scenarios (`corpus/unit/graphics/stroke-adjust-*.ps`) with IR goldens; the existing IR goldens regenerated, and the diff reviewed to add only `sa` lines, in contents with a stroke; `difftest run` passes

## 3. PDF output (efterscript-remelt)

- [x] 3.1 Extended graphics states keyed by parameter and value (D5): `/GS0` and `/GS1` for overprint unchanged, `/SA0` and `/SA1` with `/SA`, gathered from every content and listed by each content that selects them; the content writer's `gs`; verified by unit tests on names and dictionaries, and on a glyph procedure's resources
- [x] 3.2 PDF goldens for the new corpus files and the `remelt` scenarios; the existing PDF goldens regenerated, and the diff reviewed to add only the `SA` resource, its listing, and the `gs`; the external checker accepts the new and changed PDFs

## 4. Acceptance

- [x] 4.1 The oracle tier over `corpus/unit/graphics/stroke-adjust-*.ps` gives pass for every file, or an expected divergence registered with its reason
- [x] 4.2 Private tier: the vaulted job `corpora/realworld-drivers/os904-lw87-finder-window/job-print-window.ps`, run through `difftest oracle --profile default` with the hosting application's prelude, passes (1.373% before)
- [x] 4.3 Gates: `cargo test --workspace`, clippy, fmt, `difftest run`, `parse-survival`, `fuzz-round`, `lint-strings` (with the vault path), `check-wasm`, `cargo doc` with warnings denied, `openspec validate stroke-adjust-in-pdf`; design.md gains "## Implementation notes" with the measured results
