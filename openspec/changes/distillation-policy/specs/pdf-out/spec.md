# pdf-out

## ADDED Requirements

### Requirement: Real Flate compression

`Filter::Flate` SHALL produce a deterministic DEFLATE stream with LZ77
matching over a 32 KB window and fixed Huffman coding, falling back to
stored blocks for data that would not shrink; output SHALL decode to
the input under any conforming inflater and SHALL be byte-identical
across runs and platforms.

#### Scenario: Compresses and round-trips

- **GIVEN** a 100 KB content stream of repetitive path operators
- **THEN** the compressed stream is under a quarter of the size and the
  test inflater recovers the input exactly

#### Scenario: Incompressible data stays stored

- **GIVEN** 64 KB of pseudo-random bytes
- **THEN** the stream is written as stored blocks and is at most 1 %
  larger than the input
