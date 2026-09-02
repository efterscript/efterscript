# Design: PDF writer

File syntax is ISO 32000-1 §7 (objects, filters, file structure) with
Annex C's implementation limits in view; this document records how
EfterScript writes it and why. The writer knows nothing about PostScript.

## 1. Goals and constraints

- **Byte-deterministic.** The same sequence of API calls produces identical
  bytes on every platform, every run. Golden tests compare bytes.
- **Zero dependencies.** Everything, including the Flate container, is
  hand-written from the specification.
- **Single pass to the sink.** Output streams to a `std::io::Write`; the
  writer buffers at most one object at a time. Documents of any page count
  write in bounded memory.
- **Primitives first.** Document structure (catalog, pages) is a thin layer
  that uses only the public primitives, proving the primitives are enough
  for any consumer.
- **Readable output when uncompressed.** Golden files double as
  documentation; a person can read them.

## 2. Structure

```
pdf-out
├── write.rs     Writer<W: io::Write>: byte sink, offset tracking, header/EOF
├── obj.rs       serialization of the object types; Name, PdfStr, Real newtypes
├── stream.rs    stream objects: dict + data, Length handling, filter plumbing
├── flate.rs     zlib container, stored blocks, Adler-32 (hand-written)
├── xref.rs      object table, classic xref section, trailer, startxref
└── doc.rs       Document: id allocation, deferred writes, catalog/pages helpers
```

## 3. Decisions

- **Object model is a writing API, not a data model.** Consumers do not
  build a tree of PDF values; they call typed builder methods
  (`dict().key("Type").name("Page") …`) that serialize immediately into the
  current object's buffer. This forbids unrepresentable states, keeps memory
  bounded, and makes determinism structural — there is no map ordering to
  stabilise because keys are written in call order.
- **One canonical form per value.**
  - Integers: decimal, no plus sign, no leading zeros.
  - Reals: no exponent notation (the syntax forbids it); fixed algorithm —
    shortest decimal representation within six significant digits that
    round-trips the `f32`, trailing zeros and trailing dot trimmed, `-0`
    normalised to `0`. One function, property-tested, shared by every path
    that writes a number.
  - Names: `#xx` escaping for delimiters, whitespace, `#`, and bytes outside
    `!`–`~`.
  - Strings: literal form when every byte is printable ASCII (with `\(`,
    `\)`, `\\` escapes); hexadecimal form otherwise. The rule is a property
    of the bytes, so the choice is deterministic.
  - Dictionaries and arrays: single space between tokens, no line breaking
    inside an object except streams. Objects end with a newline.
- **Object numbering is allocation order.** `Document::alloc()` hands out
  ids 1, 2, 3, …; generation is always 0 (no incremental update in scope).
  Objects may be *written* in any order but are typically written
  immediately; the xref section sorts by id. A `Ref` is a copyable token;
  writing a reference to a not-yet-written object is allowed and checked at
  end: an allocated-but-unwritten id is an error at `finish()`.
- **Streams buffer their data, once.** A stream's dictionary needs `Length`;
  the writer buffers the (possibly filtered) data, writes the dictionary
  with the known length, then the data. No indirect-Length trick, no
  second pass. Content streams are page-sized; buffering one is bounded.
- **Flate is a valid zlib container with stored blocks.** Two-byte header,
  stored-block framing at 65 535-byte chunks, Adler-32 — about fifty lines,
  no compression, fully valid `FlateDecode` input. This keeps the crate
  dependency-free and deterministic now. Real DEFLATE (fixed-Huffman first)
  is a follow-up change inside this crate, triggered when output size starts
  to matter; the filter API (`Filter::None | Flate`) will not change.
  Golden files use `Filter::None`.
- **Classic xref table, PDF 1.7 header.** `%PDF-1.7`, then the binary-marker
  comment line the spec recommends so transfer tools treat files as binary.
  Cross-reference entries are the fixed 20-byte format; a single section;
  free-list head entry only. Xref streams, object streams, encryption,
  linearization, and incremental updates are out of scope — each is listed
  with its trigger (2.0 features, size, DRM never, web view, editing) so
  scope creep needs a proposal.
- **The document layer is convenience, not privilege.** `Document` offers
  `catalog()`, `add_page()`, `finish()` building the page tree (flat, one
  `Pages` node — balanced trees are an optimisation with a recorded
  trigger), Info dictionary, and MediaBox plumbing — all implemented with
  the public primitives. `remelt` may use it or bypass it.
- **Verification is self-contained.** Tests parse the writer's own output
  with a minimal test-only reader (tokenizer + xref walker, a few hundred
  lines in `tests/`) that checks structural invariants: every xref offset
  points at `N 0 obj`, every `Length` is exact, the trailer resolves, every
  reference resolves. This reader is test code, not product code, and is
  the seed of the parse-and-compare layer the differential harness will
  need anyway. Optionally, if the environment defines
  `EFTERSCRIPT_PDF_CHECK` as a command, golden files are additionally piped
  through it (any installed third-party PDF checker) — skipped silently
  when unset.

## 4. Alternatives considered

- **Reusing an existing writer crate** (the `pdf-writer` crate was the
  candidate) — mature and license-compatible, but adds a dependency to the
  product's core path; self-sufficiency was chosen deliberately, with the
  full file-syntax specification in hand making the cost bounded.
- **A value-tree model serialized at the end** — simpler to think about,
  unbounded memory, ordering-dependent determinism problems; rejected.
- **Indirect Length objects for single-pass streams** — avoids buffering
  but scatters stream metadata and complicates offset bookkeeping for no
  benefit at page-sized buffers.
- **Compression via an external encoder** — rejected with the rest of the
  dependencies; stored blocks are correct today and the upgrade path stays
  internal.

## 5. Implementation notes

- **Builder closing is structural.** Containers are written through
  closure-scoped builders: `Val::dict(f)` emits `<<`, runs `f` against a
  borrowed `DictBuilder`, then emits `>>` itself (arrays likewise), so an
  unclosed container is unrepresentable. A `Val` is a one-shot value sink —
  every writing method consumes it — and one dropped unused writes `null`,
  so a dangling key or empty object body degrades to valid output rather
  than a panic or a malformed file.
- **Canonical-form choices** left open by the spec delta, fixed here: reals
  carry a leading zero (`0.5`, `0.0001`); `#xx` name escapes and
  hexadecimal strings use uppercase digits; when six significant digits
  cannot represent a real exactly, decimal-string rounding ties away from
  zero. Non-finite reals have no PDF syntax: NaN serializes as `0` and
  infinities clamp to the extreme finite `f32` values.
- **Module layout deviation:** the catalog/page-tree convenience lives in
  `pages.rs` (`PageTree`, `write_info`) rather than inside `doc.rs`. As a
  sibling module it cannot see `Document`'s private internals, so the
  compiler itself enforces "implemented only with the public primitives".
- **Added primitive:** `Document::comment` writes a `%` comment line
  between objects (printable ASCII only); the golden file uses it for its
  self-describing `GENERATED-BY` marker.
- **Assumptions verified against ISO 32000-1** (reviewed with the spec text
  in hand): a NUL byte can never occur in a name (§7.3.5's definition
  excludes character code 0), so the writer rejects it with
  `Error::NulInName` rather than escaping it; cross-reference
  entries use the CR LF two-byte terminator of §7.5.4; the binary-marker
  comment uses the four bytes `E5 E6 F4 F2` (all ≥ 0x80, per the §7.5.2
  recommendation); `Length` excludes the end-of-line bytes framing stream
  data (§7.3.8).
- **`$EFTERSCRIPT_PDF_CHECK`** is treated as a program path invoked with
  the golden file as its single argument (exit 0 = pass), not as a shell
  command line.
