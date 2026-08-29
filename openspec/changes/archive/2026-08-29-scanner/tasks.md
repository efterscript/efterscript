# Tasks: scanner

## 1. Sources

- [x] 1.1 `Source` trait (`peek`, `advance`, `position`, `more_may_come`) and `Span`
- [x] 1.2 `SliceSource`, `StringSource` (over a string `Object`), `ChunkSource` with compaction
- [x] 1.3 `FileSource` over a file-table entry sharing its position

## 2. Core scanner

- [x] 2.1 Whitespace, delimiters, comment skipping, terminator rule
- [x] 2.2 Numbers: integer, radix, real, overflow-to-real, number-like names
- [x] 2.3 Names: executable, literal, immediate via `Resolver`
- [x] 2.4 Strings: literal with escapes and nesting, hex, ASCII85
- [x] 2.5 Procedures: nesting stack, allocation on close, depth limit
- [x] 2.6 `ScanError` with PostScript error names and spans; binary lead-byte error
- [x] 2.7 `NeedMore` and partial-state resumption

## 3. Hooks

- [x] 3.1 DSC observer callback
- [x] 3.2 Allocation-mode and global/local behaviour for immediate names

## 4. Verification

- [x] 4.1 Unit tests per scenario; `corpus/unit/scanner/*.ps` mirrors
- [x] 4.2 Property test: every split point yields the one-shot token sequence
- [x] 4.3 Property test: serialize → scan round trip over generated tokens
- [x] 4.4 `cargo fuzz` target: no panics, bounded allocation
- [x] 4.5 `xtask parse-survival` over the corpus and the private tier when configured
