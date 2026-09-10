# pdf-out Specification

## Purpose
Defines EfterScript's hand-written, zero-dependency PDF 1.7 writer: one
canonical serialization per value, deterministic byte output, streams with
exact lengths, the stored-block Flate container, classic cross-reference
structure, allocation discipline, and the minimal document layer. File
syntax authority is ISO 32000-1 §7; this spec records the behaviours the
tests pin down. The design rationale lives in the archived change
`2026-08-31-pdf-out`.

## Requirements

### Requirement: Deterministic serialization

The writer SHALL produce byte-identical output for the same sequence of API
calls, on every platform, with one canonical form per value: reals without
exponent notation, names with `#xx` escaping, strings in literal form iff
every byte is printable ASCII and hexadecimal form otherwise.

#### Scenario: Two runs, same bytes

- **GIVEN** any program against the pdf-out API executed twice
- **THEN** the two outputs are byte-identical

#### Scenario: Real formatting

- **GIVEN** the reals 1.0, 0.5, -0.0, 72.09, 0.0001
- **THEN** they serialize as `1`, `.5` or `0.5` (one form, fixed by the
  implementation), `0`, `72.09`, `.0001` or `0.0001` (same fixed form), and
  never with an exponent

#### Scenario: Name and string escaping

- **GIVEN** the name `A B#/x` and the strings `abc(1)` and `\xFF\x00`
- **THEN** the name serializes with `#20`, `#23`, and `#2F` escapes, the
  first string as a literal with balanced-safe escapes, and the second in
  hexadecimal form

### Requirement: File structure

Output SHALL begin with a `%PDF-1.7` header followed by a binary-marker
comment, contain each object as `N 0 obj … endobj`, and end with a classic
cross-reference section, trailer, `startxref`, and `%%EOF`, with every xref
offset pointing at the first byte of its object.

#### Scenario: Structural self-check

- **GIVEN** any document produced by the writer
- **WHEN** the test reader walks it
- **THEN** every xref entry resolves to its object, every indirect reference
  resolves, and the trailer's `Root` and `Size` are correct

### Requirement: Streams

A stream object's `Length` SHALL equal the exact byte count of its
(possibly filtered) data; `Filter::Flate` output SHALL be a valid zlib
container that standard inflaters decode to the original data.

#### Scenario: Flate round trip

- **GIVEN** arbitrary bytes written as a stream with `Filter::Flate`
- **THEN** the emitted data carries a correct Adler-32 and inflates to the
  original bytes

### Requirement: Allocation discipline

Object ids SHALL be allocated sequentially from 1 with generation 0; a
reference to an allocated id may be written before that object; finishing a
document with an allocated-but-unwritten object SHALL be an error.

#### Scenario: Forward reference

- **GIVEN** page objects that reference a Pages node allocated first but
  written last
- **THEN** the document finishes and self-checks

#### Scenario: Dangling allocation

- **GIVEN** an id allocated and never written
- **WHEN** `finish()` is called
- **THEN** it returns an error naming the id

### Requirement: Minimal document

The document layer SHALL produce, from catalog plus one page with a content
stream, a structurally valid PDF 1.7 file whose golden bytes are committed
and stable.

#### Scenario: One-page golden

- **GIVEN** a one-page document with an uncompressed content stream
- **THEN** its bytes equal the committed golden file

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
