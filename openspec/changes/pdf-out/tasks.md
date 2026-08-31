# Tasks: pdf-out

## 1. Primitives

- [ ] 1.1 `Writer` over `io::Write` with offset tracking; header + binary marker; `%%EOF`
- [ ] 1.2 Canonical value serialization: integers, reals (one shared function, property-tested for round-trip and no-exponent), booleans, null
- [ ] 1.3 Names with `#xx` escaping; strings with the literal/hex rule
- [ ] 1.4 Arrays, dictionaries, indirect references via builder methods

## 2. Objects and streams

- [ ] 2.1 `Document::alloc` / object writing / `finish` with dangling-id check
- [ ] 2.2 Streams: buffered data, exact `Length`, `Filter::None`
- [ ] 2.3 `flate.rs`: zlib container with stored blocks and Adler-32; `Filter::Flate`

## 3. File structure

- [ ] 3.1 Classic xref section (20-byte entries, free-list head), trailer, `startxref`
- [ ] 3.2 Test-only reader in `tests/`: tokenizer, xref walk, reference resolution, structural assertions

## 4. Document layer

- [ ] 4.1 Catalog, flat page tree, `add_page` with MediaBox, Info dictionary
- [ ] 4.2 One-page golden under `corpus/golden/pdf/` (uncompressed), byte-compared in tests

## 5. Verification

- [ ] 5.1 Unit tests per spec scenario; property tests (determinism, real round-trip, flate round trip via test-only inflater or reference decode)
- [ ] 5.2 Optional external check: pipe goldens through `$EFTERSCRIPT_PDF_CHECK` when set
- [ ] 5.3 Resolve the crate doc comment's "decision open" note
