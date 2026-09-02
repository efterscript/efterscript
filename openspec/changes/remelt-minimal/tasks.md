# Tasks: remelt-minimal

## 1. Sink (remelt)

- [ ] 1.1 `PdfSink<W>`: `new(out, options)`, `PageSink` impl writing one page per call (media box, content stream, page dictionary), `finish` → `Result<W, Error>`; latched first error; verified by a test building a `Page` by hand and checking the finished bytes parse as one page
- [ ] 1.2 Content-stream writer for state ops, paths, fill/eofill/stroke, clip/eoclip, save/restore with `fmt_real` numbers, one op per line; verified by tests per spec scenario "Operation mapping" comparing the uncompressed stream text
- [ ] 1.3 Stroke CTM wrapping (`q cm … Q`, inverse-mapped path; singular → unwrapped); verified by the scaled-stroke and translated-stroke scenarios and a property test that the inverse round trip stays within tolerance
- [ ] 1.4 Colour: device operators for device spaces; `ColorSpace` resources for Separation/DeviceN (calculator function stream from captured source, domain/range by arity) and Indexed; `cs`/`scn` selection; verified by the Separation and device-colour scenarios
- [ ] 1.5 Images: XObject per resource with Flate data, `Decode` only when non-default, mask flag; `q cm /Imn Do Q` paint; verified by the 2×2 gray and image-mask scenarios
- [ ] 1.6 Document skeleton: catalog, flat page tree, Info with producer only, `Options { compress }`; verified by the zero-page scenario and a determinism test distilling twice

## 2. Driver (remelt)

- [ ] 2.1 `distill(program, config, options, out) -> Result<Report, Error>` building the interpreter, installing the backend with the shared sink, running, recovering and finishing the sink; verified by the stroked-line and three-page scenarios through the interpreter
- [ ] 2.2 Failed job behaviour: pages before the error are written, outcome reported; verified by the error-after-first-page scenario

## 3. Command line (efterscript-cli)

- [ ] 3.1 `efterscript pdf <in.ps> [<out.pdf>|-]` with default output name, program output routing, exit codes; verified by running it on a corpus file and checking the file exists and the status

## 4. Corpus tooling (difftest, corpus)

- [ ] 4.1 `difftest run` distils each file that delivered pages and compares with `corpus/golden/pdf/<path>.pdf` when present, line-diff on mismatch; `--update-pdf` writes uncompressed goldens with the generator comment; verified by the difftest unit tests and a full `difftest run`
- [ ] 4.2 External checker: run `EFTERSCRIPT_PDF_CHECK` over each produced file when set; verified manually with the variable set and unset
- [ ] 4.3 New corpus files for the scaled stroke, even-odd fill and clip, dash parameters, 2×2 image, image mask, three pages, and error-after-page scenarios, with `.ir` and `.pdf` sidecar goldens; existing graphics corpus files gain `.pdf` goldens; verified by `difftest run` green

## 5. Verification

- [ ] 5.1 `cargo test --workspace`, `cargo clippy --workspace --all-targets` clean, `cargo fmt --check`, `difftest run`, `cargo xtask parse-survival` all green; `openspec validate remelt-minimal` passes; design.md gains an "Implementation notes" section recording deviations
