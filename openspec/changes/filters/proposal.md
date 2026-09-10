# Change: Filters — the filter operator and the standard decode/encode filters

## Why

The `Filter` implicit resource category is empty and the `filter`
operator does not exist, so a job that wraps its data in a filter stops
at the first `filter` call and the `Filter` category answers `false` to
every probe. Driver output from the LaserWriter 8 era and nearly every
job carrying an image or a compressed font uses filters:
`ASCII85Decode` and `ASCIIHexDecode` to make binary data
seven-bit-safe over a serial or PAP link, `RunLengthDecode` and
`FlateDecode` and `LZWDecode` to compress it, `DCTDecode` for JPEG
images, and `SubFileDecode` to bound a job's data. Without them the
whole class of real jobs is unreachable, and the harvested LaserWriter 8
milestone cannot begin. The interpreter already has the mechanism: an
`eexec` layer is a decoding stream over a base file, and a filter is
the same idea with a codec in place of the cipher. This change adds the
operator, the decode filters end to end, the encode filters for
completeness, and the `Filter` category's members, and it carries an
image's filter through to the PDF so a DCT image passes through
unrecompressed.

## What Changes

- **The `filter` operator** (`ps-vm`): `source /Name [params] filter`
  returns a file that reads the named filter's output over the source
  (a file or a procedure-or-string data source, like `image`), or, for
  an encode filter over a target file, writes encoded bytes through to
  it. Filters chain: a filter's source may be another filter.
- **Decode filters**: `ASCIIHexDecode`, `ASCII85Decode`,
  `RunLengthDecode`, `FlateDecode` (a real inflater, stored, fixed, and
  dynamic blocks, with the `Predictor` parameters), `LZWDecode` (the
  variable-width PostScript/PDF variant with `EarlyChange` and the
  `Predictor` parameters), and `SubFileDecode` (`EODCount` and
  `EODString`). Each honours its parameter dictionary and ends cleanly
  at its end-of-data marker.
- **Encode filters**: `ASCIIHexEncode`, `ASCII85Encode`,
  `RunLengthEncode`, `FlateEncode`, `LZWEncode`, `NullEncode`, reusing
  the writer's DEFLATE encoder for Flate; used far less by jobs but
  part of the operator's contract and cheap once the framing exists.
- **`DCTDecode`**: recognised, its parameters parsed, and its data
  passed through opaquely. The interpreter does not decode JPEG (a
  decoder is out of scope); an image whose data is DCT-encoded is
  carried to the PDF as a `DCTDecode` stream so the viewer decodes it,
  and `filter`ing a DCT source for the program to read raw samples
  raises `undefined` filter behaviour recorded as a limit.
- **`Filter` category members**: the decode and encode filter names,
  answered by `resourcestatus`/`findresource`/`resourceforall`.
- **Images carry their filter** (`ps-graphics`, `remelt`): the image
  spec records whether its samples are already DCT-encoded (from an
  `image` dictionary whose `DataSource` is a `DCTDecode` filter); the
  IR keeps the encoded bytes and the flag; the writer emits `DCTDecode`
  for such an image and Flate for raw samples as today.
- **`currentfile` filters**: the common `currentfile /ASCII85Decode
  filter … image` and `currentfile /FlateDecode filter cvx exec`
  idioms work, the filter reading the job stream and stopping at
  end-of-data so the scanner resumes after it.
- Out of scope, with triggers: a JPEG decoder (needed only if a job
  reads DCT samples back, or for a raster backend); `CCITTFaxDecode`
  and `JBIG2Decode` (fax and bilevel image jobs — their own change,
  behind a codec boundary); `ReusableStreamDecode`; DCT *encoding* (a
  policy that recompresses images).

## Capabilities

### New Capabilities
- `filters`: the `filter` operator, each standard filter's decoding and
  encoding behaviour and parameters, chaining, end-of-data, and the
  `Filter` category.

### Modified Capabilities
- `remelt`: ADDED requirement for passing a DCT-encoded image through
  to a `DCTDecode` PDF stream.

## Impact

- Code: `crates/ps-vm` (`ops/filter.rs`, the codecs as decoding streams
  reusing the layer mechanism in `files.rs`, the `Filter` category
  members in `ops/resource.rs`, image `DataSource` recognising a filter
  chain), `crates/pdf-out` (a shared inflater and LZW promoted from the
  test inflater, or a new `codec` module used by both the writer and
  ps-vm — decided in design), `crates/ps-graphics` (the image spec's
  encoded-filter flag and dump), `crates/remelt` (`DCTDecode`
  passthrough), corpus files under `corpus/unit/filters/` with goldens.
- No new dependencies; the inflater and LZW are hand-written.
- Depends on `printer-identity-mechanism` and `deflate-dynamic-huffman`
  (archived).
