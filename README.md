# EfterScript

A memory-safe, embeddable interpreter for the PostScript® language, written in
Rust, whose primary output is PDF. It executes a PostScript program, captures
the marking operations it performs, and serialises them as archival-quality
PDF — vectors stay vectors, text stays text, colour spaces are preserved.
Nothing is rasterised.

**Status:** pre-implementation. The architecture, scope, and process are
decided; the first crates are being built.

## What it is for

- Converting untrusted PostScript documents to PDF safely — in servers,
  serverless functions, sandboxes, and browsers (WebAssembly is a supported
  target).
- Embedding a PostScript-language engine in other software through a clean
  library API, with a command-line tool as a thin wrapper.

## Key values

**Security by construction.** The interpreter holds no ambient authority.
File access, pipes, devices, and host-font enumeration exist only as
capabilities the embedder injects; a document cannot reach anything it was not
explicitly handed. This replaces the traditional model of a fully capable
interpreter with dangerous operators filtered out afterwards. Memory safety
comes from Rust throughout the core; image and font decoders — historically
the riskiest surface — run behind sandbox boundaries as replaceable
dependencies rather than in-house code.

**A strict layered architecture.** PostScript VM → PDF-shaped intermediate
representation → pluggable backends. Each layer is independently usable and
published as its own crate: the VM is a PostScript scripting engine on its
own, the font layer serves any document tooling, the PDF writer knows nothing
about PostScript. Raster, SVG, and GPU sinks are reserved slots, not v1 code.

**Vector-preserving distillation as the product.** PDF's imaging model is
close to a superset of PostScript's, so distillation is capture-and-serialise,
not render-and-embed. Fonts are embedded and subsetted; ToUnicode maps are
derived from real encodings and CMaps rather than glyph-name guessing, so
text extraction and accessibility work; DeviceN, Separation, and ICC colour
pass through untouched; overprint is recorded, not simulated. Parameter
compatibility with the established distillation configuration surface
(`setdistillerparams`) is a goal.

**Library first, everywhere.** A clean embedding API, deterministic output
for golden testing, pure-function parsers built for fuzzing, and marks-to-
source traceability ("which tokens drew this?") for debugging and text
extraction. The CLI comes second; WASM is first-class.

**Test-driven at scale from day one.** A hand-written corpus authored from the
published specification, a property-based PostScript program generator,
semantic (never byte-exact) differential comparison against golden outputs,
and a formal **expected divergence** verdict for deliberate compatibility
decisions — so compatibility with decades of driver-generated PostScript is a
measured, incremental property rather than a promise.

**Permissive licensing and rigorous IP hygiene.** MIT, from the first commit.
Every file carries an SPDX header; the language is implemented from the
published specification and from black-box behavioural observation only.
Reference material and any encumbered test inputs live outside this
repository, which is treated as public even while private.

## Explicit non-goals for v1

Rasterisation of any kind, JIT compilation, transparency compositing,
print-production colour (separations, trapping, screening), and a
general-purpose multi-client print spooler. Each is a reserved slot revisited
only by an explicit proposal; none is rejected outright.

## Repository layout

```
crates/ps-vm           scanner, object model, stacks, save/restore, resources, errors
crates/ps-graphics     graphics state and the PDF-shaped vector IR; sink traits
crates/ps-fonts        Type 1 / CFF / TrueType parsing, metrics, subsetting, ToUnicode
crates/pdf-out         low-level PDF serialisation, zero PostScript knowledge
crates/remelt          the distillation engine: policies, pdfmark, PDF writing
crates/platen          session front-end: a job fed in pieces, replies and error reports
                       read back, a PDF at the end; C ABI and Emscripten build
                       (see crates/platen/docs/embedding.md)
crates/efterscript-cli PostScript-to-PDF command-line tool
tools/psgen            property-based PostScript program generator
tools/difftest         differential test harness
corpus/                unit inputs, generator seeds, golden outputs (text, diffable)
openspec/              decision registry (see below)
xtask/                 workspace automation (`cargo xtask`)
```

Internal crate names come from letterpress vocabulary: *remelt* (recasting old
type into new) and *platen* (the plate that presses paper against type).

## Development process: OpenSpec

This project is spec-driven and uses [OpenSpec](https://github.com/Fission-AI/OpenSpec)
as its decision registry. Architecture, policies, scope decisions, and every
recorded divergence from reference behaviour live in `openspec/` as
proposals and delta specs. One governing rule: the published PostScript
Language Reference owns language semantics and is *cited*, never paraphrased;
the corpus owns behavioural truth; OpenSpec owns what neither covers.

Significant changes start as an OpenSpec proposal. Trivial fixes may be
committed directly.

## Testing tiers

The standard pipeline runs entirely on the public corpus in this repository
and must stay self-sufficient. An additional private tier is activated by
setting `EFTERSCRIPT_HELLBOX` to a local checkout and skips cleanly with a
message when unset.

That private tier exists because much of the reference material this project
depends on is freely *available* but not freely *redistributable*. The
PostScript Language Reference, Adobe's font and technical notes, printer
vendors' developer documentation, and the PDF specifications can all be
downloaded from their publishers and are legitimate to read and implement
from — but they remain copyrighted, and committing copies here would be
republishing them. The same applies to test inputs: a PostScript job captured
from a real printer driver embeds the driver vendor's own prologue code,
licensed conformance suites come with their own terms, and reference outputs
from commercial converters or hardware are fine to keep privately but not to
publish. All of that lives in a separate vault, one directory per source with
its provenance recorded, and only material we hold redistribution rights to —
hand-written test files, generator seeds, our own outputs, and clean-room
reproductions of behaviours the private material revealed — ever graduates
into this repository. The rule is applied per file at commit time: if the
answer to "may we redistribute this?" needs a lawyer, it stays in the vault.

## AI notice

This project contains AI-generated code. Large parts of the codebase,
documentation, and test corpus are produced with AI coding assistants under
human direction and review. Every commit to this repository is made by a
human maintainer; agents may prepare and stage changes but cannot commit or
push (enforced by the repository's hooks and tool settings). Contributors
should assume any file may have been machine-authored and review accordingly.
The same IP-hygiene rules apply to AI-produced content as to human-written
content: implemented from the published specification, no copied code, no
reproduced manual text.

## License and trademarks

MIT — see [LICENSE](LICENSE).

PostScript is a registered trademark of Adobe. EfterScript is an independent
implementation of the PostScript language and is not affiliated with or
endorsed by Adobe. Other product and typeface names are trademarks of their respective
owners and are used nominatively.
