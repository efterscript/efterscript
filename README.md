# EfterScript

A memory-safe, embeddable interpreter compatible with the PostScript
language, written in Rust, whose primary output is PDF. It executes a
program, captures the marking operations it performs, and serialises them
as archival-quality PDF: vectors stay vectors, text stays text, colour
spaces are preserved. Nothing is rasterised.

**Status:** working, pre-1.0. The language surface is complete through
LanguageLevel 3 for the constructs that distillation needs: the full
operator set for objects, control, files, and resources; filters; user
paths, forms, and tiling patterns; device, Separation, DeviceN, Indexed,
and CIE-based colour; shadings and function dictionaries; Type 1, Type 3,
TrueType, CFF, and CID-keyed fonts with embedding, subsetting, and
ToUnicode; `pdfmark`; the distillation parameter surface; and a printer
identity layer. The distillation path produces PDF 1.7 with a hand-written
writer and no external dependencies. A session library runs one job at a
time from bytes fed in pieces, with a C interface and a WebAssembly
build. Compatibility is measured, not promised: every behaviour is pinned
by a corpus of hand-written programs with golden outputs, checked by a
differential harness, and every deliberate departure from reference
behaviour is recorded in a public registry. Not yet done: rasterisation
(by design), a few LanguageLevel 3 features listed in the specs, and
publication to the crate registry.

## Try it

```
cargo build --release
cargo run --release -p efterscript-cli -- pdf input.ps output.pdf
cargo test --workspace
cargo run -q -p difftest -- run
```

The command-line tool has three modes: `run` executes a program and prints
its output, `ir` dumps the captured vector operations as text, and `pdf`
distils. `--prelude` runs a program once at the server level before the
job, and `--identity` seeds the printer identity.

## What it is for

- Converting untrusted documents in the PostScript language to PDF safely,
  in servers, serverless functions, sandboxes, and browsers.
- Embedding the engine in other software through a library API, with the
  command-line tool as a thin wrapper.
- Standing in for a network printer: the session library accepts a print
  job as it arrives, answers the job's queries, and yields a PDF.

## Key values

**Security by construction.** The interpreter holds no ambient authority.
Files, the clock, and the output sink exist only as capabilities the
embedder injects; a document cannot reach anything it was not handed. The
library crates contain no unsafe code and no external dependencies: the
decoders for the filter chain, the image and font formats, and the PDF
writer are all in-house safe Rust, so there is no third-party parser
behind the boundary to sandbox. Unsafe code exists only in the C
interface of the session library.

**A strict layered architecture.** The language VM dispatches graphics
through a trait to a graphics layer that produces a PDF-shaped
intermediate representation, which pluggable backends consume. Each layer
is a crate on its own: the VM is a scripting engine by itself, the font
layer serves any document tooling, the PDF writer knows nothing about the
PostScript language. Raster and SVG sinks are reserved slots, not v1 code.

**Vector-preserving distillation as the product.** PDF's imaging model is
close to a superset of the language's, so distillation is capture and
serialise, not render and embed. Fonts are embedded and subsetted;
ToUnicode maps come from real encodings and CMaps rather than glyph-name
guessing, so text extraction works; colour passes through unconverted,
CIE-based colour as calibrated PDF colour; patterns, forms, and shadings
become their PDF counterparts; overprint is recorded. The
`setdistillerparams` parameter surface is honoured.

**Library first, everywhere.** A small embedding API, deterministic output
for golden testing, parsers built for fuzzing, and a text dump of the
intermediate representation for debugging. The command line comes second;
WebAssembly is a checked build target.

**Test-driven at scale.** A hand-written corpus authored from the
published language reference, a property-based program generator, a
differential harness comparing our output with a reference converter's
rendering, and a registry of expected divergences for deliberate
compatibility decisions, so compatibility with decades of driver-generated
programs is a measured, incremental property.

**Permissive licensing and strict IP hygiene.** MIT from the first commit.
Every file carries an SPDX header; the language is implemented from the
published reference and from black-box observation only, never from
another implementation's code; reference material and encumbered test
inputs live outside this repository, which has been treated as public
since day one.

## Explicit non-goals for v1

Rasterisation of any kind, JIT compilation, transparency compositing,
print-production colour management, and a multi-client print spooler.
Each is a reserved slot revisited only by an explicit proposal.

## Repository layout

```
crates/efterscript-vm        scanner, object model, stacks, save/restore, resources, operators
crates/efterscript-graphics  graphics state, capture, and the PDF-shaped vector IR
crates/efterscript-fonts     Type 1 / CFF / TrueType parsing, metrics, subsetting, ToUnicode
crates/efterscript-codec     inflate, deflate, LZW, predictors
crates/efterscript-pdf       low-level PDF serialisation, zero PostScript knowledge
crates/efterscript-remelt    the distillation engine: policies, pdfmark, PDF writing
crates/efterscript-platen    session front-end: a job fed in pieces, replies and errors
                             read back, a PDF at the end; C ABI and WebAssembly build
                             (see crates/efterscript-platen/docs/embedding.md)
crates/efterscript           the facade: distillation and session entry points in one crate
crates/efterscript-cli       the command-line tool
tools/psgen                  property-based program generator
tools/difftest               corpus runner and differential harness
corpus/                      unit inputs, generator seeds, golden outputs (text, diffable)
openspec/                    decision registry (see below)
xtask/                       workspace automation (`cargo xtask`)
```

Internal codenames come from letterpress vocabulary: *remelt* (recasting
old type into new) and *platen* (the plate that presses paper against
type).

## Development process: OpenSpec

The project is spec-driven and uses [OpenSpec](https://github.com/Fission-AI/OpenSpec)
as its decision registry. Architecture, policies, scope, and every
recorded divergence from reference behaviour live in `openspec/` as
proposals, designs, and specs; the archived changes carry implementation
notes explaining what the code does and why. One governing rule: the
published language reference owns semantics and is cited by section,
never paraphrased; the corpus owns behavioural truth; OpenSpec owns what
neither covers.

Contributions of behaviour start as an OpenSpec proposal, which is how a
change is discussed before code exists. Documentation and trivial fixes
are committed directly.

## Testing tiers

The standard pipeline runs entirely on the public corpus in this
repository and must stay self-sufficient. A private tier is activated by
setting `EFTERSCRIPT_HELLBOX` to a local checkout of the reference vault
and skips cleanly with a message when unset.

The private tier exists because much of the reference material this
project depends on is freely available but not freely redistributable.
The language reference, the font and technical notes, printer vendors'
developer documentation, and the PDF specifications can be downloaded
from their publishers and are legitimate to read and implement from, but
they remain copyrighted, and committing copies here would be republishing
them. The same applies to test inputs: a job captured from a real printer
driver embeds the driver vendor's own prologue, licensed conformance
suites have their own terms, and reference outputs from commercial
converters or hardware are fine to keep privately but not to publish. All
of that lives in a separate vault, one directory per source with its
provenance recorded, and only material we hold redistribution rights to,
hand-written test files, generator seeds, our own outputs, and clean-room
reproductions of behaviours the private material revealed, ever graduates
into this repository. The rule is applied per file at commit time: if the
answer to "may we redistribute this?" needs a lawyer, it stays in the
vault.

## AI notice

This project contains AI-generated code. Large parts of the codebase,
documentation, and test corpus are produced with AI coding assistants
under human direction and review. Every commit is made by a human
maintainer; agents may prepare and stage changes but cannot commit or
push, enforced by the repository's hooks and tool settings. Contributors
should assume any file may have been machine-authored and review
accordingly. The same IP-hygiene rules apply to AI-produced content as to
human-written content: implemented from the published reference, no
copied code, no reproduced manual text.

## License and trademarks

MIT, see [LICENSE](LICENSE), for the project's own code and data. The
fonts crate bundles third-party data under its own terms, listed in
[REUSE.toml](REUSE.toml) with the licence texts under `LICENSES/`: the
resident outlines (OFL-1.1 and LPPL-1.3c; disabled by turning off the
`resident-outlines` feature), the Core 14 metrics (Adobe's AFM notice),
and the glyph list and predefined CMaps (BSD-3-Clause). PDFs produced with
these fonts carry no obligation. One duty does reach embedders: the
BSD-3-Clause terms of the glyph list and CMaps require a binary that links
the fonts crate to reproduce their copyright notice and conditions in its
documentation or accompanying materials; the notice text is in
`crates/efterscript-fonts/data/PROVENANCE.md`.

PostScript is a registered trademark of Adobe. EfterScript is an
independent interpreter compatible with the PostScript language and is
not affiliated with or endorsed by Adobe. Other product and typeface
names are trademarks of their respective owners and are used
nominatively.
