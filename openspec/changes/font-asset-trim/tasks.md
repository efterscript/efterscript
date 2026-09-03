# Tasks: font-asset-trim

## 1. Metric tables

- [x] 1.1 Table format, parser, and generator (`xtask fetch-fonts` writes `.metrics` from each `.pfb`); verified by a ps-fonts test regenerating every table and comparing byte-for-byte
- [x] 1.2 `ResidentFace::metrics()` over Core 14 AFM or table; delete the TeX Gyre AFMs; update provenance and the provenance test; verified by the extra-face width scenario, the feature-off build, and `cargo test --workspace`

## 2. Subroutine pruning

- [x] 2.1 Charstring trace mode collecting reached subroutine indices; verified by unit tests including hint replacement and nested calls
- [x] 2.2 Writer keeps reached ∪ {0..3}, renumbered densely with every call rewritten (stubs, indices unchanged, when a call operand is not a literal); verified by a round-trip over every glyph of every extra face and by the Pagella subset size scenario

## 3. Verification

- [x] 3.1 Regenerate the affected `.pdf` goldens (embedded extras) and confirm every other golden is byte-identical; `cargo test --workspace`, feature-off tests, clippy clean, fmt, `difftest run`, `parse-survival`, `openspec validate font-asset-trim`; design.md gains "## Implementation notes"
