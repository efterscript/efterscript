# Design: Distillation policy

See proposal.md and the spec deltas. This document fixes the parameter
model and its path from the job to the writer, the encoder, embed-all,
and downsampling.

## Context

- The writer takes `Options { compress }`; content and function streams
  use the Flate container when set; image data always does; the
  container is stored blocks only.
- The page device is the template for a tolerant, type-checked
  dictionary in the VM with a backend hook.
- Document-level information reaches the writer as `DocMark`s through
  `PageSink::document` since the pdfmark change.
- Resident faces reach the writer as `FontSource::Resident`; the
  twenty-one extras already embed from their assets through
  `FontSource::Embedded`; the fourteen do not.
- Images reach the writer as raw samples with a spec and a matrix.

## Decisions

**D1. One parameter model, `Params`, in `remelt`.** A struct with the
honoured keys typed, plus `others: BTreeMap<String, ParamValue>` for
recorded-but-not-honoured keys and `locked: BTreeSet<String>`.
`Options` becomes `Options { params: Params }` with `compress` kept as
a constructor convenience. The VM keeps its own dictionary object for
the operators (like the page device) and hands every change to the
backend as `MarkValue` entries through a defaulted trait method
`set_distiller_params(&[(name, MarkValue)])`; the backend turns it into
`DocMark::Params(entries)` delivered immediately; the sink merges into
its `Params` respecting locks. *Alternative:* make the VM's dictionary
authoritative and let the writer read it at finish — the writer starts
pages before finish and would honour the wrong value for early pages.

**D2. Precedence and timing.** The embedder's `Params` are the initial
state; each `setdistillerparams` merges over it except locked keys;
parameters take effect for pages delivered after the change (a page
already written keeps its settings), and document-wide ones
(`EmbedAllFonts`, `SubsetFonts`, `CompatibilityLevel`) take the value
in effect at finish, recorded. The header version is written at
`Document::new`, so `CompatibilityLevel` must be known before the first
byte: `distill` pre-scans? No — the header is rewritten at finish via
`pdf-out` support for a fixed-width version field (`%PDF-1.x` is fixed
width; `Document` gains `set_version` that patches the two bytes at
finish when the writer is seekable — it writes to a `Vec<u8>` or a
file; record: the header patch requires `W: Write + Seek`; when the
sink is not seekable the version stays 1.7 and is reported).
*Alternative:* buffer the whole document — defeats streaming.

**D3. DEFLATE encoder in `pdf-out`.** `flate::compress` implements
LZ77 with a 32 KB window, a hash chain on 3-byte prefixes with a
bounded chain length, lazy matching off (determinism and simplicity),
fixed Huffman block coding per RFC 1951 §3.2.6, blocks of at most
64 KB of input, and a stored-block fallback when the coded block would
not be smaller. Deterministic by construction (no threads, no
randomness). A test inflater (stored, fixed, and dynamic blocks) lives
in the crate's test support to verify round trips; proptests over
random and repetitive inputs. Dynamic Huffman is the recorded
follow-up. *Alternative:* a dependency — the writer's core path stays
zero-dependency by decision.

**D4. Embed-all.** When `EmbedAllFonts` is in effect at finish, the
writer treats each used `Resident` face with an outline asset as an
embedded TrueType font: it builds a `FontSpec::Embedded`-shaped entry
from the asset's parsed program (the `resident-fonts` outline lookup
already parses it), maps the used codes through the encoding to glyph
names to glyph ids, subsets with the existing TrueType subsetter, and
writes `FontFile2` with the symbolic cmap; widths stay the AFM's.
Symbol and ZapfDingbats are reported. The subset writer runs at finish
as for other embedded fonts, so the decision uses the final parameter
value. `SubsetFonts false` embeds whole programs (the subsetter with
all glyphs).

**D5. Downsampling.** For each image at page-write time: effective
resolution = samples per inch along each axis from the image matrix and
the page's 72 units per inch; factor = floor(resolution / target) per
axis, at least 1, applied equally on both axes (the smaller factor);
averaging over factor×factor blocks per component for 8-bit samples
(rounded), subsampling by taking the top-left sample; masks
subsampled; edge blocks averaged over what exists. The image's matrix
is unchanged (it maps the unit square). Indexed, 16-bit, and
sub-8-bit non-mask images are left and reported.

**D6. Reporting.** `Report.params: Params` (in effect at finish),
`Report.not_honoured: Vec<(String, String)>` (key, value or reason),
`Report.downsampled: usize`; the CLI prints one line for not-honoured
keys. The `% expect-output:` corpus files observe `currentdistillerparams`.

**D7. Goldens.** Every `.pdf` golden with a compressed stream changes
bytes once because the encoder changes; corpus goldens are written
uncompressed by design, so only pdf-out's own golden and any test that
pins compressed bytes move. The oracle harness is unaffected (it
distils uncompressed).

## Risks / Trade-offs

- [Encoder bugs corrupt documents] → round-trip proptests through the
  test inflater, and the external checker over every golden.
- [Downsampling changes rasters] → the corpus scenario compares within
  tolerance; the harness's own limit applies to generated images at
  the default (downsampling off).
- [Header patching needs a seekable sink] → `distill` uses `Vec<u8>`
  or a file, both seekable; the library API documents the fallback.

## Implementation notes

### Part 1 (tasks 1.1, 2.1, 3.1)

- **D3, the encoder** (`pdf-out/src/flate.rs`). Greedy LZ77 over a
  32 KB window with a hash chain on three-byte prefixes (a 15-bit
  multiplicative hash, chain limit 64, early stop at a match of 128),
  minimum match 3, maximum 258; blocks cover at most 65 535 input bytes
  and a match never crosses a block boundary, so the stored form is
  always available; per block the fixed-Huffman size is counted exactly
  (padding included) against the stored size and the smaller written.
  The zlib header stays `78 01`. Empty input is one final fixed block.
  Measured: the spec's repetitive stream (100 KB of grid strokes, the
  numbers repeating) 1.7 %; the same length with every number different
  37.8 % — the independent deflater's own fixed-Huffman mode gives
  37.9 % there and its dynamic mode 25.3 %, which is what the recorded
  dynamic-Huffman follow-up is worth; 64 KB pseudo-random +13 bytes
  (the first block stored, the one-byte remainder coded because 18 bits
  beat the 48 a stored block costs after alignment); from the corpus, a
  2 135-byte text content stream 54 %, a 7 949-byte eexec-encrypted
  Type 1 program 95 %, a whole 13 KB document 75 %. The "under a
  quarter" scenario therefore holds for repetitive content, not for a
  stream whose numbers all differ; the test fixture is the former.
- **The test inflater** (`pdf-out/tests/common/inflate.rs`) decodes
  stored, fixed, and dynamic blocks and checks the Adler-32; it is one
  file included by `#[path]` from the crate's unit tests and from
  `tests/common/mod.rs` (and through it by `remelt`'s test support), so
  `inflate_stored` is gone. Every stream the encoder produced was also
  decoded from the shell by an independent inflater; the structural
  reader now accepts any `%PDF-1.x` header.
- **D2, the header patch.** `Document::new_seekable(sink)` (for
  `W: Write + Seek`) keeps a function pointer that rewrites bytes at an
  offset and returns to the end; `Document::new` keeps none.
  `set_version(major, minor)` takes one digit each (`Error::
  InvalidVersion` otherwise) and `version_patchable()` says whether
  `finish` will honour it: the three bytes at offset 5 are rewritten
  after `%%EOF`, so a plain sink names 1.7 whatever was set. `PdfSink::
  new_seekable`, `distill_seekable`, and `distill_into` (a sink already
  started) carry the choice up; `distill` itself is unchanged and
  reports a level other than 1.7 as not honoured. The CLI writes files
  through a seekable sink and standard output through a plain one.
- **D7, amended.** The image XObjects always use the Flate container,
  so the corpus goldens that hold one changed bytes with the encoder —
  `graphics/gray-image`, `graphics/imagemask-in-separation`,
  `graphics/clip-and-pages`, `graphics/image-operand-form-gray` (each
  image stream a few bytes shorter) — and `pdf-out`'s own `one-page.pdf`
  did *not*, since it is written with `Filter::None`. The four were
  re-pinned with `--update-pdf`; the external checker accepts every
  regenerated golden (converts and renders without a warning).
- **D1, the operators** (`ps-vm/src/ops/distiller.rs`, public
  visibility, appended last in the table). The dictionary lives in
  global VM beside the page device, is not subject to `restore` (the
  reference ties the values to the save level; not followed, like the
  page device), and starts with the writer's built-in defaults — the
  parameters reference documents no defaults for these keys, so they
  are ours: `CompressPages true`, `EmbedAllFonts false`, `SubsetFonts
  true`, `CompatibilityLevel 1.7`, the three `Downsample…Images false`,
  `ColorImageResolution 150`, `GrayImageResolution 150`,
  `MonoImageResolution 300`, the three `…DownsampleType /Average`,
  `ColorConversionStrategy /LeaveColorUnchanged`, `AutoRotatePages
  /None`. The type table: booleans for the six flags, numbers (integer
  or real) for `CompatibilityLevel` and the three resolutions, names for
  the three types, the strategy, and `AutoRotatePages`; checked over the
  whole request before anything changes, so a `typecheck` leaves the
  dictionary and the operand as they were. Unknown keys take any value;
  a value without a boundary form (a procedure) is recorded but left
  out of what the backend receives. `currentdistillerparams` returns a
  fresh copy each call, as the reference describes, so the
  `currentdistillerparams dup … put setdistillerparams` idiom works.
  `Interp::set_distiller_params` seeds values without a backend call;
  `distill_into` uses it with `Params::entries()` so the job reads the
  embedder's options (a locked key still reads the job's own later
  value; the writer's report shows the override). The trait method
  `set_distiller_params` defaults to `Ok(())`, `Graphics` turns it into
  `DocMark::Params(entries)` at once, the mock records
  `Call::DistillerParams`, and the dump prints `param /Key value` per
  entry (`dump::mark_value` is public and the writer's report reuses it).
- **D1, the writer** (`remelt/src/params.rs`). `Params` has typed
  fields for the fourteen honoured keys (`compatibility_level` as
  `(major, minor)`, `Downsample::{Average, Subsample}`, the strategy as
  the name given), `others: BTreeMap<String, MarkValue>` — `MarkValue`
  is reused rather than a separate `ParamValue`, one value type across
  the boundary — and `locked`. Merge rules: a locked key is refused as
  `value (locked at current)`; an unsupported value of a honoured key
  (`CompatibilityLevel` outside 1.3–1.7, a resolution outside 9–2400,
  a type other than `Average`, `Bicubic`, `Subsample`) is refused with
  the reason and the old value kept; a strategy other than
  `LeaveColorUnchanged` is stored *and* reported; any other key goes to
  `others` and is reported with its value. `Report.not_honoured` lists
  these in order, plus at finish `EmbedAllFonts=true (not honoured
  yet)` and `CompatibilityLevel=1.x (the output cannot be rewound; 1.7
  written)` when they apply. The embedder's own `others` are not
  reported against the job; the CLI reports its own line. `Options`
  is `{ params }` with `Options::compress(bool)` and `Options::lock(key)`.
- **D2, timing, as built.** `CompressPages` is read when each page is
  written; `SubsetFonts` and `CompatibilityLevel` at `finish`.
  `SubsetFonts false` embeds a simple font's whole program (every
  charstring, every glyph, every CFF name) under its untagged name;
  composite fonts stay subset, as the reference says of CID fonts.
  `EmbedAllFonts` is parsed, kept, and reported — part 2 builds it and
  removes the report line in `PdfSink::not_honoured`. The downsampling
  fields are parsed and kept; `Report.downsampled` is 0 until part 2.
- **D6, the CLI.** `pdf [options] <in> [<out> | -]` with `--param
  Key=Value` (repeatable; `true`/`false`, an integer, a real, else a
  name with or without its slash), `--no-compress`, `--embed-all`,
  `--no-subset`, `--lock Key` (repeatable; locks apply after every
  parameter whatever their order). One line, `efterscript: N
  parameter(s) not honoured: Key=value, …`, joins the line's refused
  parameters with the report's.
- **Harnesses.** `difftest` distils goldens through a seekable cursor
  with `Options::compress(false).lock("CompressPages")`, so a golden
  stays readable whatever the job asks and `policy/compat-header.pdf`
  begins `%PDF-1.4`; the oracle copy and `psgen` lock the key the same
  way and stay uncompressed.
- **Corpus.** `corpus/unit/policy/`: `params-round-trip`,
  `params-typecheck`, `compress-off`, `compat-header`,
  `not-honoured-reported`, with `.ir` goldens showing the `param` lines
  and `.pdf` goldens (page-less for the two output scenarios). The
  oracle run over them is part 2's 4.1.
- **Gates.** `cargo test --workspace` 842 passed, 0 failed, 2 ignored
  (from 804); clippy and fmt clean; `difftest run` 170/170;
  `parse-survival` over the corpus; `fuzz-round` 2 600 programs, 0
  failed; `lint-strings` clean; `openspec validate distillation-policy`
  valid. The private oracle run is left to part 2.

### Part 2 (tasks 3.2, 3.3, 4.1, 4.2)

- **D4, embed-all, as built** (`remelt/src/embed_all.rs`). A
  `FontSpec::Resident` whose face `has_outlines()` is routed to the
  embedded table when `EmbedAllFonts` is in force *as the page is
  written*, so its object is allocated then and written at finish with
  the codes of every page; at finish the value then in force decides
  the form — embedded from the asset, or unembedded through the usual
  resident writer when the job switched the key off again. D4's "the
  decision uses the final parameter value" therefore holds for the form
  but not for the routing: a face written unembedded before the request
  cannot be recalled (its object is already in the file, and deferring
  every resident font would move every text golden's objects), so it
  stays unembedded and is reported. The embedded form: the face's
  parsed asset (`ResidentFace::outlines()`, the same per-thread parse
  `charpath` uses); each used code through the encoding to a glyph name
  to a glyph index by `ResidentOutlines::glyph_index` — the `post` name,
  else the name's single Unicode value through the `(3,1)` cmap, the
  lookup `outline` already made and now public; a name the asset lacks
  draws glyph 0 and is left out of the cmap — then the existing TrueType
  subsetter with the symbolic `(3,0)` cmap, `FontFile2` with `Length1`,
  `/TrueType` with no `Encoding`. `BaseFont` and the descriptor's
  `FontName` are the asset's own name (`LiberationSans-Regular`) behind
  the six-letter tag over the kept glyph indices; with `SubsetFonts
  false` every glyph is kept and the name is untagged. Widths are the
  AFM's over the *used* codes (the unembedded form lists every encoded
  code; an embedded font lists what it shows, as the other embedded
  kinds do), and the asset's advances agree with them to the unit for
  the tested glyphs. Descriptor: `Flags` symbolic plus fixed pitch (the
  program's or the AFM's) plus serif from the family, italic when the
  program's angle is not zero; `FontBBox`, `Ascent`, `Descent`,
  `ItalicAngle` from the program (they describe the embedded outlines);
  `CapHeight` and `StemV` from the AFM, 80 without one. ToUnicode over
  the used codes through the glyph list, as for resident fonts. A face
  whose asset is a charstring program (none in a build with the
  feature; the branch exists for completeness) is embedded through the
  Type 1 path by building the `FontSpec::Embedded` the VM would have
  built. `embedded.rs` gained the shared pieces (`SimpleFont`,
  `write_simple_font`, `write_truetype_stream`, `truetype_bbox`,
  `SERIF`); the job-font paths are unchanged in output (every
  pre-existing golden is byte-identical).
- **Report.** `PdfSink::not_honoured` lists, when `EmbedAllFonts` is
  true, one `EmbedAllFonts` entry per reason naming the faces written
  unembedded in order of first use: `true (Symbol, ZapfDingbats: no
  outline asset)`, `true (Helvetica: written before the request)`, and
  with the feature off `true (Helvetica: outline assets absent from
  this build)` — the whole set stays unembedded there, as the change
  says, since `has_outlines()` is false for every face. The "not
  honoured yet" line is gone. The CLI prints them on its existing
  parameters line; `--embed-all` needed no change.
- **D5, downsampling, as built** (`remelt/src/downsample.rs`, applied
  in `resources::Objects::write`). Per image of a page: the class is
  by depth and component count, not by family name — Indexed is
  unsupported; 1-bit one-component (masks and gray) is mono; 8-bit
  one-component is gray, which takes a one-colourant Separation or
  DeviceN too; 8-bit with two or more components is colour, so RGB,
  CMYK, and any DeviceN; 2-, 4-, and 16-bit are unsupported. The
  matrices the page paints the image with are gathered from its own
  operations (`downsample::painted`); resolution per axis is the sample
  count over the length of the unit square's edge (`hypot` of the
  matrix's column) times 72, so rotation does not change it; several
  paints reduce by the least resolution among them; a matrix that
  collapses an axis leaves the image alone. `factor = floor(min(res_x,
  res_y) / target)`, at least 1; 8-bit samples are averaged per
  component with half rounding up or subsampled from each block's
  top-left sample, edge blocks over what exists; mono is subsampled
  whatever the type says, and an `/Average` request on a one-bit image
  is reported once per document as `MonoImageDownsampleType=/Average
  (one-bit images are subsampled)`. `/Bicubic` is averaging by part 1's
  parse. The written XObject has the new width and height and the same
  matrix; `Report.downsampled` counts images reduced (`PdfSink::
  downsampled`). Unsupported images are noted once each, `page N: image
  K (Indexed) is not downsampled`, only while some class is enabled, so
  the default configuration adds no notes; an image painted only inside
  a Type 3 glyph procedure (its matrix is in glyph space) is left alone
  with the same kind of note. Data shorter than the spec says is left
  alone. The corpus scenario: 300×300 8-bit gray over one inch is 300
  per inch, factor 4 at 72, written 75×75 — 5 625 samples, a 137-byte
  Flate stream in the golden against the 90 000 raw bytes the IR golden
  still records.
- **Verification of the two scenarios.** The external checker lists the
  embed-all golden's font as `TrueType … emb yes sub yes uni yes` and
  its text extraction gives `Hi`; the oracle harness renders the
  downsampled golden pixel-identical to the reference at 36 dpi
  (differing fraction 0) and the embed-all golden at 0.02 % (limit
  0.5 %). Tests: `downsample` unit tests (resolution and factor,
  averaging with edge blocks and rounding, subsampling of bytes and
  bits, class routing, the mask case); `remelt/tests/sink.rs` (the
  Helvetica scenario by hand with the subset parsed back and the
  advances checked against the AFM, Symbol and an early-written face
  reported, the whole asset under `SubsetFonts false`, the Unicode
  fallback for `Euro`, colour averaging and mask subsampling beside an
  Indexed image left with a note); `remelt/tests/params.rs` (embed-all
  through the operators with Symbol beside Helvetica, the job switching
  it off, gray averaging and `/Subsample` through the operators, the
  2-bit note); the CLI's `--embed-all` and `--no-subset` end to end.
  The feature-off build runs the same tests down their other branch.
- **Corpus.** `policy/embed-all-helvetica` (with `% requires:
  resident-outlines`, since the golden holds the subset) and
  `policy/downsample-gray-300-to-72` (the ramp built by a `for` loop
  and delivered by a procedure returning the same row), both with `.ir`
  and `.pdf` goldens. No pre-existing golden changed.
- **4.1, the oracle run.** Before this part, over the whole corpus:
  170 files, 129 pass, 2 fail, 33 expected-divergence, 1
  divergence-closed, 5 skipped; output 131 same, 34 differs. The two
  fails were part 1's `params-round-trip` and `params-typecheck`: the
  reference interpreter run for output has no `setdistillerparams` at
  all, and its converter drops the unknown key (`/Foo get` fails) and
  answers an ill-typed value with a stack fault having consumed the
  dictionary. Both are reviewed decisions here, so this change adds an
  `expected-divergences` delta with `distiller-params-unknown-keys` and
  `distiller-params-typecheck`, the two files carry the headers, and
  `difftest`'s pinned slug counts gained the two rows — the one change
  outside `remelt` and `ps-fonts` besides the tests. After: 172 files,
  131 pass, 0 fail, 35 expected-divergence, 1 divergence-closed, 5
  skipped; output 133 same, 34 differs; every other verdict unchanged.
  The policy directory alone: 5 pass, 2 expected-divergence.
- **Gates.** `cargo test --workspace` 855 passed, 0 failed, 2 ignored
  (from 842); `cargo test -p remelt --no-default-features` 120 passed;
  clippy clean on all targets with and without the feature; fmt clean;
  `difftest run` 172/172; `parse-survival` 172 files, no failures;
  `fuzz-round` 2 600 programs, 0 failed; `lint-strings` clean;
  `openspec validate distillation-policy` valid.
