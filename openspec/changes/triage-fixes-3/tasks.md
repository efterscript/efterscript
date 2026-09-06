# Tasks: triage-fixes-3

## 1. Output and graphics

- [ ] 1.1 Stroke colour pairs in the content writer; corpus file for the red stroke and the Separation stroke with goldens; existing goldens with non-black strokes re-pinned and listed; verified by the scenarios, a rendered check of the red stroke, and `difftest run`
- [ ] 1.2 Empty clip recorded, emitted, dumped, and written; corpus file with golden; verified by the blank-page scenario rendered
- [ ] 1.3 Operand-form image samples in DeviceGray; corpus file with golden; verified by the IR scenario
- [ ] 1.4 Arc piece stability and `arcto` in double precision; unit tests for the rounding case; the two shrunk `arcto` findings as corpus files if they reproduce

## 2. Interpreter

- [ ] 2.1 `sin`/`cos` argument reduction; unit tests; corpus file; verified by the large-angle scenario
- [ ] 2.2 `bitshift` decision per D6 with the reference text; corpus file with the chosen behaviour and, if kept, the divergence header; registry requirement kept or removed accordingly

## 3. Harness

- [ ] 3.1 Invisible-text rule; fake-profile test; verified by the scenario

## 4. Verification

- [ ] 4.1 Private oracle round: the corpus (totals recorded) and a regenerated 2 000-per-profile generated round with totals and remaining root causes recorded in the notes; `cargo test --workspace`, clippy, fmt, `difftest run`, `parse-survival`, `fuzz-round`, `lint-strings`, `openspec validate triage-fixes-3`; design.md gains "## Implementation notes"
