# Design: Font programs

See proposal.md for motivation and the spec deltas for the contract. This
document fixes how a font program gets in (`eexec`), how its glyphs come
out (the glyph engine), how the program travels to the PDF, and how it
is subset.

## Context

- A Type 1 font program is PostScript: cleartext that builds the font
  dictionary, an `eexec` section that fills `Private` (with `Subrs`,
  `lenIV`, hinting values) and `CharStrings` (glyph name → encrypted
  charstring), then `definefont`. The interpreter already runs
  everything except `eexec`, which is registered and raises `undefined`.
  `RD`-style procedures read charstring bytes with `readstring` from
  `currentfile`, so the decrypting layer must be a real file.
- The file table holds entries with a one-byte pushback and a position;
  the scanner reads files through it, and `currentfile` finds the
  innermost file frame. A layered entry fits without changing the
  scanner.
- A Type 42 dictionary carries the TrueType program in `sfnts` (an array
  of strings, split at glyph boundaries) and `CharStrings` mapping glyph
  names to glyph indices.
- `text-core` fixed: the font in the graphics state as an instance id
  and matrix, `FontInfo`/`FontSource` describing a font to the backend
  once per instance, `Text` runs with glyph-space displacements, font
  resources interned per page, `PdfSink` writing font objects lazily,
  and `charpath` raising `invalidfont`.
- No outline files for the resident fonts ship yet; `ps-fonts` must not
  depend on `ps-vm`.

## Goals / Non-Goals

**Goals:**
- Jobs with embedded Type 1 and Type 42 fonts run, measure, draw, and
  distil with their own glyphs embedded.
- A glyph engine interface (`program + glyph name → outline + advance`)
  that the composite-font change reuses for CFF and CID fonts.
- Subsetting that is correct by construction: the regenerated Type 1
  program is checked by running it through this interpreter.

**Non-Goals:**
- Rendering-quality hinting (hints are dropped from the interpretation,
  kept in the embedded program).
- Font caching for speed beyond a per-font outline cache.
- Recovering the original program bytes; the PDF gets a regenerated
  program (D6).

## Decisions

**D1. `eexec` is a layered file-table entry.** `FileTable` gains an
entry kind that reads from a base entry, drops the first four bytes,
detects hexadecimal form (all of the first four bytes are hexadecimal
digits) versus binary, and decrypts with the standard key, byte by byte,
so the base's position stays exact: on `closefile` of the layer the base
has consumed precisely the bytes the layer decrypted (a pushed-back byte
is returned to the base). `eexec` on a file pushes a `SourceSlot::File`
frame for the layer with `systemdict` on the dictionary stack, and a
marker frame that pops `systemdict` and closes the layer when the frame
ends, whether by `closefile`, end of data, or an error unwinding through
it. `eexec` on a string decrypts it whole and executes it as a string
source. *Alternative:* decrypt the rest of the file into a buffer and run
it as a string — breaks `currentfile`, and the binary section's end is
only known by executing it.

**D2. Where the program lives.** The font dictionary is the source of
truth, as the reference has it: charstrings and subroutines are strings
in `CharStrings` and `Private/Subrs`. On the first glyph request for a
font instance family (`FID`), the VM builds a `ps_fonts::Program`
snapshot — Type 1: `lenIV`, the subroutines, and the charstrings by
name, decrypted once; Type 42: the concatenated `sfnts` bytes and the
`CharStrings` name → index map — wrapped in `Rc` and cached on the
interpreter by `FID`. A program that later mutates its font dictionary
is not followed, the same rule `text-core` set for encodings.
*Alternative:* re-read the dictionary per glyph — cheap for Type 1,
prohibitive for `sfnts` assembly.

**D3. The glyph engine.** `ps_fonts::Program` exposes `glyph(name) ->
Option<Glyph { advance: (f32, f32), outline: Outline }>` in the
program's glyph space (Type 1: charstring units under the font's
matrix; TrueType: font units, with the engine reporting `units_per_em`
so the VM scales to the 1000-unit convention the font matrix expects:
Type 42 fonts have `FontMatrix [1 0 0 1 0 0]` and glyph space is the
unit em, so outlines and advances are divided by the units per em).
`Outline` is a list of move/line/curve/close in `f32`. The Type 1
interpreter implements the charstring operators, `callsubr`,
`callothersubr` for flex (other-subroutines 0–2, reconstructing the two
curves from the collected points) and hint replacement (3, ignored),
`div`, `pop`, `seac` by looking up the two component names through
`StandardEncoding` and composing with the accent displacement, `sbw` and
`hsbw` for the advance. TrueType glyphs: `loca`/`glyf` with composite
glyphs resolved (offsets and scales), on/off-curve points to quadratic
segments to cubics (exact elevation). A per-family cache
`name → Rc<Glyph>` lives beside the program. *Alternative:* a trait
object per font type — the two programs are an enum with the same three
methods; a trait adds nothing until CFF, when the enum grows a variant.

**D4. Show and charpath use the engine.** For a Type 1 or Type 42 font
the show frame's resident branch generalises: per glyph, advance from
the engine (through the font matrix like a resident width), the run to
the backend as before. `charpath` runs the same frame in an outline
mode: for each glyph it appends the outline to the current path through
`backend.moveto/lineto/curveto/closepath` after mapping glyph space
through the font matrix and translating to the current point (the
backend applies the CTM as usual), then advances. The boolean is accepted
and ignored for `PaintType 0`; a `PaintType 2` (stroked) font is treated
as outline too, recorded. Resident and Type 3 fonts keep `invalidfont`.

**D5. Font info to the backend.** `FontSource` gains `Embedded {
family, kind: Type1 | TrueType, program: Rc<Program>, font_matrix }`.
The program is immutable and shared, which is the value-type rule's
intent; a backend that does not embed can ignore it. `ps-graphics`
interns the resource per page by `family` and encoding as for Type 3.
The dump prints kind, `FontName`, and glyph count only.

**D6. Type 1 embedding regenerates the program.** `ps_fonts::type1::write`
produces a complete Type 1 program from the snapshot plus the entries
the PDF consumer needs: cleartext `FontName`, `FontType`, `FontMatrix`,
`FontBBox`, `PaintType`, `Encoding` (the font's own array, written as
`dup <code> /<name> put` lines), `FontInfo` when present; the encrypted
section with the `Private` numeric and array entries written back
through the VM's syntactic printer (`BlueValues`, `StdHW`, `StdVW`,
`StemSnap*`, `BlueScale`, `ForceBold`, `LanguageGroup`, `lenIV`),
`OtherSubrs` printed back from the dictionary the same way (the
procedures the font brought in, unchanged), `Subrs` re-encrypted, and
the kept `CharStrings` re-encrypted with `lenIV 4`; then the 512-zero
trailer and `cleartomark`. The three lengths are the cleartext,
binary-encrypted, and trailer sizes. Correctness check: the test suite
feeds the regenerated program back into the interpreter and compares
glyph advances and outlines with the original. *Alternative:* capture
the original bytes as they pass through `eexec` — the cleartext before
`eexec` is already consumed by the scanner and its start is unknown, and
a job that alters the dictionary would embed a program that disagrees
with what it showed.

**D7. TrueType subsetting rewrites tables.** Keep glyph 0 and the used
glyphs (and their composite components), renumber densely, rewrite
`glyf`/`loca`/`hmtx`/`hhea`/`maxp`/`head`/`post` (format 3, no names),
synthesise a `cmap` with one (3,0) subtable mapping each used code to
its new index, and copy `cvt`/`fpgm`/`prep` when present so hinted
programs stay valid. The font dictionary is `/TrueType` with the
symbolic flag and no `Encoding`, so viewers use the cmap; widths are
listed per used code. The writer is shared with the synthesised test
fonts. *Alternative:* embed the whole program — correct, wasteful, and
still needs the cmap synthesis because Type 42 addresses glyphs by
name, not by the program's own cmap.

**D8. Glyph usage and finish-time fonts.** `PdfSink` records, per
embedded family, the set of codes and glyph names shown across pages
and allocates the font object id at first use; the dictionary,
descriptor, and program stream are written in `finish`. Resident and
Type 3 fonts keep their lazy path. The document stays deterministic
(sets are ordered).

**D9. Descriptors.** `FontBBox` from the dictionary (Type 1) or `head`
(TrueType), `ItalicAngle` from `FontInfo` or `post`, `StemV` from
`Private/StdVW` or a fixed default for TrueType, `Ascent`/`Descent`
from the bounding box, `Flags`: symbolic unless the encoding is the
standard one; fixed pitch from `FontInfo/isFixedPitch` or `post`.
`CapHeight` when `FontInfo` has it.

**D10. Synthesised test fonts.** `ps_fonts::testing` (a `cfg(test)`-
free module used by tests of several crates) builds a Type 1 program
from named outlines (a charstring encoder for the few operators the
synthesised glyphs need) and a TrueType program from quadratic
contours through the D7 writer. The corpus fonts under
`corpus/unit/fonts/` are generated once by an ignored test and
committed as hexadecimal `eexec` programs and `sfnts` arrays; they are
the project's own work. A test embeds the host's DejaVu Sans when
present, checking parse, subset, and round-trip, skipping otherwise.

**D11. Errors.** A charstring that runs past its end, an unknown
operator, a `seac` component missing, a `Subrs` index out of range, a
`sfnts` too short for its tables, or a glyph index out of range raises
`invalidfont` from the operator that needed the glyph; `eexec` on a
closed file raises `ioerror`.

## Risks / Trade-offs

- [Regenerated Type 1 programs differ from the originals in byte layout;
  a consumer that fingerprints fonts sees a new font] → the `FontName`
  and `UniqueID` are dropped or tagged with the subset prefix, which is
  what subsetting implies anyway.
- [Flex and `seac` corner cases in real fonts] → the corpus covers the
  documented forms; a hellbox-tier test over harvested jobs is the
  signal for more, with clean-room reductions added to the corpus.
- [Layered file semantics around `closefile` and pushback] → pinned by
  the `eexec` scenarios including one that reads with `readstring`
  across the boundary and one that ends by end of data.
- [The `Rc<Program>` in `FontSource` crosses the value-type boundary]
  → the program is immutable and the backend only reads it; the
  alternative, an opaque handle, would need a lookup callback the trait
  does not have.

## Open Questions

- Whether `Subrs` should be pruned to those reachable from kept
  charstrings. Not needed for correctness; can follow if embedded sizes
  matter.
