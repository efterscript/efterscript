# Tasks: pdf-out

## 1. Primitives

- [x] 1.1 `Writer` over `io::Write` with offset tracking; header + binary marker; `%%EOF`
- [x] 1.2 Canonical value serialization: integers, reals (one shared function, property-tested for round-trip and no-exponent), booleans, null
- [x] 1.3 Names with `#xx` escaping; strings with the literal/hex rule
- [x] 1.4 Arrays, dictionaries, indirect references via builder methods

## 2. Objects and streams

- [x] 2.1 `Document::alloc` / object writing / `finish` with dangling-id check
- [x] 2.2 Streams: buffered data, exact `Length`, `Filter::None`
- [x] 2.3 `flate.rs`: zlib container with stored blocks and Adler-32; `Filter::Flate`

## 3. File structure

- [x] 3.1 Classic xref section (20-byte entries, free-list head), trailer, `startxref`
- [x] 3.2 Test-only reader in `tests/`: tokenizer, xref walk, reference resolution, structural assertions

## 4. Document layer

- [x] 4.1 Catalog, flat page tree, `add_page` with MediaBox, Info dictionary
- [x] 4.2 One-page golden under `corpus/golden/pdf/` (uncompressed), byte-compared in tests

## 5. Verification

- [x] 5.1 Unit tests per spec scenario; property tests (determinism, real round-trip, flate round trip via test-only inflater or reference decode)
- [x] 5.2 Optional external check: pipe goldens through `$EFTERSCRIPT_PDF_CHECK` when set
- [x] 5.3 Resolve the crate doc comment's "decision open" note
