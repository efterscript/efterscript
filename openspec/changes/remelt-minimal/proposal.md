# Change: Minimal distillation engine

## Why

The pipeline now ends at a page IR nobody consumes. This change closes the
loop: `remelt` serialises the IR through `pdf-out`, and `efterscript pdf`
turns a PostScript program into a PDF file for the first time. Doing it
before fonts, `pdfmark`, or any policy exists is deliberate — the IR was
designed to be "nearly a serializer" away from PDF, and the only way to
know the shapes are right (the stroke CTM, the clip entries, N-channel
colour, image matrices) is to serialise them and look at the result. Every
later distillation feature (text, embedding, policies) lands on top of the
document and page skeleton this change fixes, so its shape is what is hard
to retrofit: streaming one page at a time into the writer rather than
building a document in memory, colour spaces and images as per-page
resources, and a deterministic file for the same input.

## What Changes

- `remelt` becomes real: a `PageSink` that writes each delivered page into
  a `pdf-out` document as it arrives (content stream, page dictionary,
  per-page resources) and closes the document at the end of the job, plus
  a one-call driver that builds the interpreter, installs the graphics
  backend, runs a program, and finishes the PDF.
- IR-to-content-stream mapping for everything the IR carries today: line
  parameters, colour in device spaces and in Separation/DeviceN/Indexed
  spaces (as colour-space resources with the captured tint transform
  written as a calculator function), fills and strokes with their rule,
  clips, saves/restores, and images as image XObjects with raw sample data
  in a Flate container.
- A stroke recorded with a non-identity CTM is serialised inside its own
  transform so PDF measures the line width and dash in the same space the
  program did.
- A job that ends in an error still yields a well-formed PDF of the pages
  delivered before the error; the error is reported alongside.
- `efterscript pdf <in.ps> [<out.pdf>]` in the command-line tool.
- `difftest run` distills every corpus file that produces pages and
  compares the bytes with a sidecar golden under `corpus/golden/pdf/`
  when one exists (`--update-pdf` regenerates them, uncompressed so diffs
  read as text); an external checker named by `EFTERSCRIPT_PDF_CHECK` is
  run over each produced file when set.
- Out of scope, each with its trigger: text and fonts (the fonts change),
  `pdfmark` and document info beyond a producer string (needs a job-level
  dictionary model), distillation parameters and any policy
  (`setdistillerparams` compatibility comes with the policy surface),
  image recompression (policy), transparency and groups (IR hooks
  reserved, unused), semantic PDF comparison against an oracle (the first
  differential run against another interpreter), and any validation of
  captured tint transforms beyond emitting them (a calculator-function
  checker when a corpus file needs it).

## Capabilities

### New Capabilities
- `remelt`: turning delivered page IR into a PDF document — the mapping of
  every IR operation and resource to its content-stream and object form,
  the document skeleton, determinism, and behaviour on a failed job.

### Modified Capabilities
- none. `graphics-ir` and `pdf-out` are consumed as they are; if serialising
  shows an IR shape is wrong, that is a follow-up delta to `graphics-ir`,
  not a silent change here.

## Impact

- Code: `crates/remelt` (the engine and its tests), `crates/efterscript-cli`
  (`pdf` mode), `tools/difftest` (PDF sidecar goldens, `--update-pdf`,
  external checker), goldens under `corpus/golden/pdf/`. `crates/pdf-out`
  and `crates/ps-graphics` gain at most small additive exports.
- Dependencies: none new. `remelt` already depends on `ps-vm`,
  `ps-graphics`, `pdf-out` (and `ps-fonts`, unused until the fonts change).
- Depends on `graphics-ir` and `pdf-out` (both archived).
