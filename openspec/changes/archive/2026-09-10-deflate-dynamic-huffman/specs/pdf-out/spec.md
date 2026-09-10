# pdf-out

## MODIFIED Requirements

### Requirement: Real Flate compression

`Filter::Flate` SHALL produce a deterministic DEFLATE stream with LZ77
matching over a 32 KB window and, per block, whichever of stored,
fixed-Huffman, or dynamic-Huffman coding costs the fewest bits, headers
included; dynamic codes SHALL be length-limited canonical codes built
from the block's symbol frequencies. Output SHALL decode to the input
under any conforming inflater and SHALL be byte-identical across runs
and platforms.

#### Scenario: Compresses and round-trips

- **GIVEN** a 100 KB content stream of repetitive path operators
- **THEN** the compressed stream is under a quarter of the size and the
  test inflater recovers the input exactly

#### Scenario: Varied numbers compress with a dynamic code

- **GIVEN** a 100 KB content stream whose numbers all differ
- **THEN** the compressed stream is under 30 percent of the size, its
  blocks are dynamic, and the test inflater recovers the input exactly

#### Scenario: Incompressible data stays stored

- **GIVEN** 64 KB of pseudo-random bytes
- **THEN** the stream is written as stored blocks and is at most 1 %
  larger than the input
