# Change: Distillation policy — parameters, real compression, embed-all, image downsampling

## Why

The writer has one option, `compress`, and it does not compress: the
Flate container writes stored blocks, so a "compressed" document is
larger than an uncompressed one. Jobs and drivers set distillation
parameters through `setdistillerparams`, which is undefined here, and
the output policy the project promises — preserve by default, convert
by choice — has no surface. This change gives the writer its parameter
model, the operators that feed it from the job, a real DEFLATE encoder
so compression means something, the embed-all mode that modern
consumers and archival profiles expect, and image downsampling, the
one image policy that changes file size by an order of magnitude. The
parameter model is the shape every later policy (colour conversion,
image recompression with a lossy codec, PDF/A) plugs into.

## What Changes

- **Parameters** (`ps-vm`): `setdistillerparams` and
  `currentdistillerparams` with a tolerant dictionary, like the page
  device: recognised keys are type-checked, unknown keys accepted and
  readable back; the dictionary is handed to the backend as values on
  every change and reaches the writer as a document mark.
- **The parameter set honoured in v1**: `CompressPages`,
  `EmbedAllFonts`, `SubsetFonts`, `CompatibilityLevel` (header version
  1.3 to 1.7, features unchanged), `DownsampleColorImages`,
  `DownsampleGrayImages`, `DownsampleMonoImages` with their
  `…ImageResolution` and `…ImageDownsampleType` (`/Average` and
  `/Subsample`; `/Bicubic` treated as average), and
  `ColorConversionStrategy` with only `/LeaveColorUnchanged` honoured
  (others recorded). Every other documented key is accepted and
  recorded as not honoured.
- **Precedence**: writer options given by the embedder or the
  command line are the defaults; a job's `setdistillerparams` overrides
  them unless the embedder locks a key. The command line gains
  `--param Key=Value` (repeatable) and short forms `--no-compress`,
  `--embed-all`, `--no-subset`, `--lock`.
- **Real DEFLATE** (`pdf-out`): a dependency-free encoder with LZ77
  matching over a 32 KB window and fixed Huffman coding (dynamic
  Huffman as a recorded follow-up), deterministic, behind the existing
  `Filter::Flate`; stored blocks remain for incompressible data. Every
  compressed golden changes bytes once and is re-pinned.
- **Embed-all** (`remelt`): with `EmbedAllFonts`, the fourteen standard
  faces are embedded from their outline assets as subset programs
  through the existing embedding path (TrueType for the Liberation
  faces), named by the asset's font name; Symbol and ZapfDingbats stay
  unembedded and are reported.
- **Image downsampling** (`remelt`): images above the target
  resolution (computed from the image matrix and the page) are reduced
  by an integer factor, averaging or subsampling, for 8-bit gray, RGB,
  and CMYK samples and 1-bit masks (subsample only); Indexed and other
  depths are left unchanged and reported.
- **Report and notes**: parameters in effect, keys not honoured, and
  images downsampled.
- Out of scope, with triggers: lossy image codecs (a JPEG encoder or a
  sandboxed codec capability), colour conversion strategies (the
  colour policy change), PDF/A and output intents, `AutoRotatePages`
  (accepted, recorded), object streams and cross-reference streams
  for versions above 1.4.

## Capabilities

### New Capabilities
- `distillation-policy`: the parameter operators, the honoured set,
  precedence, and how policies reach the writer.

### Modified Capabilities
- `remelt`: ADDED requirements for embed-all and downsampling, and for
  the compatibility header.
- `pdf-out`: ADDED requirement for real Flate compression.

## Impact

- Code: `crates/ps-vm` (`ops/distiller.rs`, a parameter dictionary,
  an additive backend method), `crates/ps-graphics` (a `Params` document
  mark), `crates/remelt` (`Params` model, precedence, embed-all,
  downsampling, header version, report), `crates/pdf-out` (`flate.rs`
  encoder), `crates/efterscript-cli` (flags), corpus files under
  `corpus/unit/policy/` with goldens; every compressed `.pdf` golden
  re-pinned once for the new encoder.
- Dependencies: none new.
- Depends on `pdfmark`, `resident-outlines`, `font-programs`
  (archived).
