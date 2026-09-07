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
