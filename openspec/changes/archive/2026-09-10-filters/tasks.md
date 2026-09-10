# Tasks: filters

## 1. Codec crate

- [x] 1.1 `crates/codec` with `inflate` (from the test inflater, `Result`-returning, streaming), `deflate`/`compress` (moved from pdf-out, byte-identical), and `lzw` encode/decode; `pdf-out` re-exports; verified by pdf-out's tests unchanged and every compressed golden byte-identical, plus LZW round-trip and known-vector tests

## 2. Filter operator and decode filters (ps-vm)

- [x] 2.1 `Kind::Decode` generalising the eexec layer; the `filter` operator; ASCIIHex, ASCII85, RunLength, Flate, LZW, SubFile decoders as pull functions; chaining; end-of-data; string/procedure/file sources; verified by the operator, hex, chain, unknown-filter, and SubFile scenarios as corpus files
- [x] 2.2 Parameter dictionaries: predictors (PNG 10–15, TIFF 2) after Flate/LZW, EarlyChange, EODCount/EODString, CloseSource; verified by the predictor scenarios and unit tests
- [x] 2.3 `Filter` category members; verified by the category scenario

## 3. Encode filters (ps-vm)

- [x] 3.1 `Kind::Encode` and the six encoders writing through to a target, flushing on close; verified by the encode-then-decode round-trip scenario for every filter

## 4. DCT passthrough (ps-vm, ps-graphics, remelt)

- [x] 4.1 `DCTDecode` recognised (raw read → undefined); image acquisition detecting a DCT source and keeping encoded bytes with a flag; `ImageSpec.encoded`, IR `Image` carrying it, dump note; the writer emitting `/DCTDecode`; a committed tiny-JPEG helper; verified by the passthrough scenario with a golden and the external checker

## 5. Verification

- [x] 5.1 `cargo test --workspace`, clippy, fmt, `difftest run` (pre-existing goldens byte-identical), `parse-survival`, `fuzz-round`, `lint-strings`, `openspec validate filters`; the captured driver job still passes in the private tier; design.md gains "## Implementation notes"
