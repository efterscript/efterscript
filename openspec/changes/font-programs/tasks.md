# Tasks: font-programs

## 1. eexec (ps-vm)

- [x] 1.1 Layered decrypting file entry in the file table (hex/binary detection, four-byte skip, exact base position on close, pushback returned); verified by unit tests reading through the layer with `read`, `readstring`, and `token` and checking the base position afterwards
- [x] 1.2 `eexec` operator on files (source frame, `systemdict` pushed, marker frame closing the layer on end/close/error) and on strings; verified by the three eexec scenarios as corpus files plus an error-unwinding test

## 2. Glyph engine (ps-fonts)

- [x] 2.1 `Outline` type, `Program` enum, per-family glyph cache; verified by unit tests on hand-built programs
- [x] 2.2 Type 1 charstring decryption and interpreter: operator set, `Subrs`, `div`/`pop`, flex via other-subroutines, hint operators ignored, `seac` through `StandardEncoding`, `hsbw`/`sbw` advance, malformed → error; verified by unit tests per operator group and a flex glyph
- [x] 2.3 TrueType parser: table directory, `head`/`hhea`/`hmtx`/`maxp`/`loca`/`glyf`/`post`/`cmap`, composite glyphs, quadratic → cubic; verified by unit tests on a synthesised font and, when present, the host's DejaVu Sans (skipping otherwise)
- [x] 2.4 `testing` module: charstring encoder, Type 1 program builder, TrueType program builder; verified by round-tripping built programs through the parsers

## 3. Fonts in the VM (ps-vm)

- [x] 3.1 Program snapshot from Type 1 and Type 42 dictionaries on first use (`FID`-cached, `Rc`), `FontSource::Embedded`; verified by tests that `definefont` then `stringwidth` answers from the program
- [x] 3.2 Show family and `stringwidth` over embedded fonts through the engine; verified by the Type 1 and Type 42 width scenarios as corpus files
- [x] 3.3 `charpath` in outline mode for Type 1 and Type 42 (font matrix, current point, backend path calls, advance), `invalidfont` for resident and Type 3; verified by the charpath scenarios through the mock backend and as corpus files with goldens

## 4. IR (ps-graphics)

- [x] 4.1 Embedded font resource interned by family and encoding, dump line without program bytes; verified by backend tests and the dump scenario; pre-existing goldens byte-identical

## 5. Subsetting and embedding (ps-fonts, remelt)

- [x] 5.1 Type 1 writer (D6): cleartext, private section printed back through the VM's syntactic printer, re-encryption, trailer, three lengths; subset to used glyphs plus `.notdef` and `seac` components; verified by the round-trip scenario feeding the output back through the interpreter
- [x] 5.2 TrueType subsetter and table writer (D7) with synthesised (3,0) cmap; verified by re-parsing the subset and by the cmap scenario
- [x] 5.3 Glyph usage per family across pages; embedded font dictionaries, descriptors (D9), `FontFile`/`FontFile2` streams, widths, ToUnicode from glyph names, written at `finish` (D8); verified by sink tests on hand-built pages and the two embedding scenarios
- [x] 5.4 Subset tag prefix on `BaseFont` and `FontName`; determinism test covering embedded fonts

## 6. Corpus and verification

- [x] 6.1 Generated corpus fonts under `corpus/unit/fonts/` (ignored generator test), corpus files for every scenario with `.ir` and `.pdf` goldens; `difftest run` green; with a checker available, every fonts golden accepted and text extracts
- [x] 6.2 `cargo test --workspace`, `cargo clippy --workspace --all-targets` clean, `cargo fmt --check`, `cargo xtask parse-survival`, `openspec validate font-programs`; design.md gains "## Implementation notes"
