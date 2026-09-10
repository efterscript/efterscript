# Change: Dynamic Huffman coding in the DEFLATE encoder

## Why

The encoder codes every block with the fixed Huffman tables, which
leaves typical content streams at about half their size where a
per-block code tree brings them to a third or less: the measured gap
on a 100 KB stream of varied numbers was 38 percent fixed against 25
percent dynamic. Distilled files are mostly content and font streams,
so this is the single largest remaining size factor, and it is
self-contained: RFC 1951's dynamic block format, a length-limited
Huffman code builder, and a per-block choice among stored, fixed, and
dynamic by exact bit count. The test inflater already decodes dynamic
blocks, so the round-trip guard exists.

## What Changes

- **Dynamic blocks**: per block, symbol frequencies over the LZ77
  output build length-limited (15-bit) literal/length and distance
  codes, the code-length alphabet is coded with the run-length symbols
  and its own 7-bit-limited code, and the block header carries the
  three counts; canonical code assignment per the standard, so output
  is deterministic.
- **Per-block choice**: the block is emitted in whichever of stored,
  fixed, or dynamic form costs the fewest bits, headers included.
- **Determinism and portability** unchanged: integer arithmetic only,
  a deterministic tie-break in the code builder.
- Out of scope: lazy matching, larger hash chains, and other
  match-finding improvements (a later size pass if measurements
  warrant); any change to the container API.

## Capabilities

### New Capabilities
- none.

### Modified Capabilities
- `pdf-out`: "Real Flate compression" gains dynamic blocks and the
  three-way per-block choice, with a tighter size scenario.

## Impact

- Code: `crates/pdf-out/src/flate.rs` and its tests. Every compressed
  golden changes bytes once and is re-pinned (the four image-bearing
  corpus goldens and any test pinning compressed bytes); uncompressed
  goldens and the oracle tier are unaffected.
- Dependencies: none.
- Depends on `distillation-policy` (archived).
