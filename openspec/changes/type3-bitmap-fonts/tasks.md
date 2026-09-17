# Tasks: type3-bitmap-fonts

## 1. Glyph metrics (ps-vm, ps-graphics)

- [x] 1.1 `glyph_matrix()` on the trait (default `None`) implemented by the backend from the capture's CTM at `begin_glyph`; the VM transforms the operands of `setcachedevice`, `setcachedevice2`, and `setcharwidth` from the CTM at the call into glyph space (delta transform for the width, envelope for the box), for `show` and `stringwidth` alike; verified by unit tests (scale, translate, rotate, identity, singular) and the three metric scenarios as corpus files with IR goldens, every existing golden byte-identical or listed with its reason

## 2. FontDirectory (ps-vm)

- [x] 2.1 `findfont` enters a resolved resident or substituted face in the current allocation mode's font directory under the requested key; verified by the `FontDirectory` scenario as a corpus file, a unit test that a program-defined font still wins, and the oracle on the scenario

## 3. Verification

- [x] 3.1 `cargo test --workspace`, clippy, fmt, `difftest run`, `parse-survival`, `fuzz-round`, `lint-strings`, `check-wasm`, `openspec validate type3-bitmap-fonts`; the oracle over `corpus/unit/text/`; the private tier: the captured bitmap-font job with the host prelude passes the oracle with matching text extraction and the checker accepts the font; the original captured job still passes; design.md gains "## Implementation notes" with provenance
