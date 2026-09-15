# Tasks: cie-color-spaces

## 1. Boundary (ps-vm, ps-graphics)

- [ ] 1.1 `SpaceSpec::{CalGray, CalRGB, Lab}` with arity, family, initial colour, interning, and the dump lines; every exhaustive match in the workspace updated; verified by ps-vm and ps-graphics unit tests and every existing IR golden byte-identical

## 2. CIE spaces in the VM (ps-vm)

- [ ] 2.1 `ops/cie.rs`: parsing and validation of the four dictionaries (defaults, errors), the VM-side `CieSpace` beside the boundary spec, `setcolorspace` acceptance with the initial colour, range clamping in `setcolor`, `currentcolorspace`/`currentcolor` reporting the original, the device getters returning initial device values; verified by the acceptance, white-point, clamping, and getter scenarios as `% backend: none` corpus files and unit tests for every error
- [ ] 2.2 The collapse test (design D2) producing `CalGray`/`CalRGB` with components passing through; verified by the gamma-gray, LMN-stage, and XYZ scenarios with IR goldens and unit tests on the procedure-shape check (bound and unbound, non-gamma rejected)
- [ ] 2.3 `LoopFrame::CieDecode` and the conversion (matrices, `RangeLMN` clamp, table interpolation for DEF/DEFG, XYZ→L*a*b*) at `setcolor` and `setcolorspace`; verified by the Lab round-trip and table-driven scenarios with IR goldens, a double-precision unit test of the inverse transformation against hand-computed values, and a test that a decode procedure returning a non-number is `typecheck`
- [ ] 2.4 Image sample conversion with per-component caches and eight-bit Lab output with its decode array; pass-through for collapsed spaces; verified by the converted-image and pass-through-image scenarios with IR goldens and a unit test on the cache bound
- [ ] 2.5 `setcolorrendering`, `currentcolorrendering`, `findcolorrendering`, the `ColorRendering` and `ColorSpace` categories, the four family names in `ColorSpaceFamily`, `UseCIEColor` recorded; verified by the rendering and families scenarios as corpus files and the sorted-table tests

## 3. Writer (remelt)

- [ ] 3.1 `CalGray`/`CalRGB`/`Lab` colour-space arrays as `/CSn` resources with the omission rules, selection and colour setting, images naming the space with the supplied decode; verified by the Lab-fill scenario as a PDF golden, a sink test reading back the arrays, and every existing PDF golden byte-identical

## 4. Corpus and verification

- [ ] 4.1 The remaining D7 corpus files (non-collapsing A, sRGB-like, DEFG, save/restore, errors) with goldens; verified by `difftest run`
- [ ] 4.2 `cargo test --workspace`, clippy, fmt, `difftest run` (pre-existing goldens byte-identical), `parse-survival`, `fuzz-round`, `lint-strings`, `check-wasm`, `openspec validate cie-color-spaces`; the oracle tier over `corpus/unit/cie/` with each verdict inspected and recorded (tolerance or registered divergence only with a stated reason); the external checker on the PDFs; the captured driver job still passes; design.md gains "## Implementation notes" with provenance and the `findcolorrendering` decision
