# Tasks: masked-images

## 1. The boundary and the VM (efterscript-vm)

- [x] 1.1 `ImageSpec` gains `mask: Option<ImageMask>` (D1), set to `None` at every existing constructor across the workspace, with the key-to-stencil function of D6; verified by unit tests of the conversion (exact values, ranges, 1/4/8/12-bit, clamping, min > max) and by every existing golden staying byte-identical
- [x] 1.2 Type 3 and type 4 dictionaries are parsed and validated before any data is read (D4, D5, D9): unknown image and interleave types give `rangecheck`, structural inconsistencies `typecheck`, `MaskColor` length `rangecheck` and non-integers `typecheck`, and `imagemask` still accepts type 1 only; verified by the error scenarios of the `images` delta as `% expect-error:` corpus files and unit tests for each consistency rule
- [x] 1.3 Acquisition of interleave types 1, 2, and 3 from strings, files, decode filters, and procedures, including the two-stage loop frame, cutting short data to the rows both parts cover, mask normalisation, and alignment by reversal (D2–D4); verified by unit tests of the block layouts in both height ratios, short deliveries, and reversed matrices, and by a black-box check of the interleave 3 order against the reference with the outcome recorded in the implementation notes
- [x] 1.4 The CIE conversion job carries the mask, turning a key into a stencil first (D6); `limitcheck` for a `DCTDecode` data source under interleave 1 or 2, with the `masked-dct-interleaved` corpus file; verified by unit tests and `difftest run`

## 2. IR and dump (efterscript-graphics)

- [x] 2.1 The IR image resource keeps the mask and the dump prints it (D8); verified by the `graphics-ir` delta's scenarios as IR goldens for the new `masked-*.ps` files and every existing IR golden byte-identical

## 3. PDF output (efterscript-remelt)

- [x] 3.1 The stencil mask XObject and `/Mask` reference, and the colour key array (D7); verified by unit tests on the written dictionaries, PDF goldens for the new corpus files, and the external checker accepting them
- [x] 3.2 Downsampling keeps a stencil mask at its resolution and turns a key into a stencil before averaging (D6); verified by unit tests and the colour-keyed downsampling scenario as a corpus file

## 4. Acceptance

- [x] 4.1 The driver-construction corpus file (D10) and the remaining scenarios of the `images` and `remelt` deltas exist as corpus files with goldens; the oracle tier over `corpus/unit/graphics/masked-*` gives pass or the registered divergence for every file
- [ ] 4.2 Private tier: the real job re-captured from the emulator pairing against this branch's library, vaulted under `corpora/realworld-drivers/` with its `PROVENANCE.md` and checksums, distils with the hosting application's prelude to outcome `ok`, and passes the oracle
- [ ] 4.3 Gates: `cargo test --workspace`, clippy, fmt, `difftest run`, `parse-survival`, `fuzz-round`, `lint-strings` (with the vault path), `check-wasm`, `openspec validate masked-images`; design.md gains "## Implementation notes" with provenance (manual sections and black-box observations)
