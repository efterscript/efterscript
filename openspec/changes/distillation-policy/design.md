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
