# Design: Dynamic Huffman coding

See proposal.md. One module changes; the decisions are the code
builder and the block choice.

## Context

- `flate::compress` produces LZ77 symbols per block of at most 65 535
  input bytes and emits each block as fixed-Huffman or stored by exact
  bit count; a bit writer and the fixed tables exist.
- The test inflater decodes dynamic blocks, so round trips are checked
  without new test code.

## Decisions

**D1. Length-limited canonical codes by the package-merge method.**
Frequencies → code lengths limited to 15 (7 for the code-length
alphabet) with package-merge, which is exact and deterministic;
canonical code assignment per RFC 1951 §3.2.2; symbols with zero
frequency get no code; a block with a single distinct distance or
literal symbol still gets a valid code (the standard's one-code case is
handled by assigning length 1). *Alternative:* a heuristic length
limiter over a plain Huffman tree — simpler but produces slightly
longer codes and needs its own correctness argument.

**D2. Code-length alphabet coding.** The two code-length sequences are
concatenated, run-length coded with symbols 16, 17, and 18 as the
standard defines (greedy longest runs), the code-length code built the
same way with the 7-bit limit, and the trailing zero counts trimmed
(`HLIT`, `HDIST`, `HCLEN` minima respected). Deterministic by
construction.

**D3. Three-way choice.** For each block compute the stored size, the
fixed size, and the dynamic size (header bits + coded symbols) exactly,
emit the smallest; ties prefer stored, then fixed (fewer bits to decode
and simpler output). The encoder keeps its greedy matcher.

**D4. Goldens.** Compressed goldens re-pinned once; the four corpus
image goldens and no others expected; listed in the notes.

## Risks / Trade-offs

- [Package-merge complexity] → about 100 lines; proptests over random
  frequency tables check the Kraft inequality and the length limit, and
  round trips check the codes decode.
- [A bug in header trimming produces streams some inflaters reject] →
  the test inflater is strict, and an independent inflater on this host
  is run over every regenerated golden from the shell.

## Implementation notes

- **D1, the code builder as built** (`code_lengths`, `Code` in
  `pdf-out/src/flate.rs`). Package-merge over the symbols with a
  nonzero frequency in a node arena: `limit − 1` rounds of pairwise
  packaging merged with the leaves, then the 2n − 2 cheapest items of
  the last level counted per leaf, so a symbol's length is the number
  of chosen items containing it. Ties: leaves sorted by weight with the
  lower symbol index favoured for the shorter code, and a leaf before
  an equal-weight package. A zero frequency gets no code; a lone symbol
  gets one code of length 1; an empty alphabet is all zeros, which the
  distance side declares as `HDIST` 1 with one zero length (the
  standard's "no distance codes" form). `Code::from_lengths` is the
  canonical assignment and also builds the fixed tables from their
  length runs; a test checks the result against the standard's fixed
  code words and its worked example. Proptests over flat, sparse, and
  power-of-two frequency tables at limits 7–15: the Kraft sum is
  exactly 1 for two or more symbols and ½ for one, every length is
  within the limit, the output is deterministic, and every code word
  decodes through the test inflater's table (`decode_symbols`). A
  Fibonacci-weight table forces the limit at both 15 and 7.
- **D2 and D3 as built.** `Dynamic::plan` builds the two codes from
  the block's frequencies (end-of-block counted once), trims `HLIT` and
  `HDIST` to the last used symbol with the minima 257 and 1, run-length
  codes the concatenated sequence greedily (18 before 17 before 16; a
  run may cross the literal/distance boundary as the standard allows),
  builds the 7-bit code-length code, trims `HCLEN` over the permutation
  order to a minimum of 4, and counts the block's bits. `Emitter::block`
  costs the stored form (alignment padding at the current bit position
  included), the fixed form, and the dynamic form exactly and writes the
  cheapest; ties go stored, then fixed. The block's symbols are resolved
  once to code indices and extra bits (`Coded`) so the three costings
  and the writing share one lookup each, and a test checks that each
  form's written bit count equals its costed count. The test inflater
  gains `block_kinds` (every block's type, after full verification) and
  `decode_symbols`; nothing else in the test code changed.
- **Measured, before → after** (before: the fixed-or-stored encoder).
  - Repetitive 100 KB grid: 1.7 % → 1.1 % (1 706 → 1 102 bytes), both
    blocks dynamic.
  - Varied 100 KB: the fixture behind the archived 38 %/25 % figures
    was not recorded and could not be recovered. Of the shapes tried —
    pseudo-random integers, pseudo-random two-decimal reals, sequential
    integers, and grids whose counter never wraps — only the last
    reaches under 30 % here, and an independent deflater at its best
    setting also stays above 33 % on the others. The test therefore
    uses a staircase of 40-unit strokes, each 5 right and 6 up from the
    last, so every coordinate is new: 40.6 % → 26.9 % (40 620 → 26 887
    bytes), both blocks dynamic. The scenario holds for content whose
    numbers all differ but share digits with their neighbours, not for
    numbers with unrelated digits, which settle near 37–40 % either way.
  - 64 KB pseudo-random: +13 bytes, unchanged (one stored block, then
    a fixed block for the last byte).
  - A corpus content stream (`text/derived-fonts`, 2 135 bytes):
    53.6 % → 38.4 %; that whole document distilled compressed: 2 947 →
    2 621 bytes.
  - A whole document with an embedded font program
    (`fonts/palatino-embeds-pagella`, compressed): 9 093 → 8 788 bytes;
    the 7 949-byte eexec-encrypted subset 95.0 % → 91.5 %; its 344-byte
    content stream 74.7 % → 66.9 %.
  - `fonts/type1-embedded-two-pages`: 2 600 → 2 574 bytes; its
    1 476-byte content stream stays at 57.4 % — at that size the fixed
    form still wins.
  - `graphics/clip-and-pages` compressed: 1 735 → 1 732 bytes.
  - The image goldens: unchanged, see D4.
- **D4, amended: no golden changed bytes.** The five goldens holding a
  compressed stream (the four image goldens and
  `policy/downsample-gray-300-to-72`) carry images of 4, 4, 2, 2 + 2,
  and 5 625 raw bytes, and for each the fixed block is still the
  cheapest form, so `--update-pdf` over exactly those five files
  rewrote identical bytes. All five, the measured documents above, and
  the encoder's adversarial streams (one byte throughout, two bytes
  alternating, a literal-only block with no distance code, empty input,
  a mixed stream) inflate correctly under an independent inflater run
  from the shell.
- **Gates.** `cargo test --workspace` 866 passed, 0 failed, 2 ignored
  (from 855); clippy and fmt clean; `difftest run` 172/172;
  `parse-survival` 172 files, 0 failed; `fuzz-round` 2 600 programs,
  0 failed; `lint-strings` clean; `openspec validate
  deflate-dynamic-huffman` valid.
