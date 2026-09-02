# Change: Font programs — eexec, Type 1 and Type 42 glyphs, charpath, subsetting and embedding

## Why

Real jobs carry their own fonts: Type 1 programs behind `eexec` and
TrueType programs as Type 42 `sfnts` arrays. Today the interpreter stops
at `eexec` with `undefined`, so most driver-produced jobs cannot run past
their prologue, and a font the job defines cannot be measured, shown, or
outlined. This change makes embedded fonts first-class: their programs
execute, their glyphs measure and draw, `charpath` yields outlines, and
the PDF embeds a subset of the program so the output looks like the
input. Two shapes are fixed here that later work depends on: the glyph
engine's interface (a font program becomes outlines and advances on
demand, cached by glyph name), and how a font program travels from the VM
to the PDF (as an immutable program snapshot the graphics layer carries
as a resource and the PDF writer subsets at the end of the document).
Composite fonts, CID-keyed fonts, and CFF programs arrive in a following
change on top of these; host-font access is a capability for the session
work; shipping outline files for the resident fonts is a separate asset
change.

## What Changes

- **`eexec`** (`ps-vm`): a decrypting layer over the current file (or a
  string), in hexadecimal or binary form detected from the first bytes,
  executed as a source with `systemdict` pushed for its duration;
  `currentfile` inside it is the layer, `closefile` on the layer ends it
  and leaves the underlying file positioned after the consumed bytes, so
  the usual `cleartomark` trailer runs in the clear.
- **Type 1 glyphs** (`ps-fonts`, `ps-vm`): a charstring interpreter
  (Type 1 charstring decryption with `lenIV`, the full operator set,
  `Subrs`, `seac` composition, flex through the other-subroutine
  convention, hint operators accepted and ignored) yielding an outline and
  advance in glyph space. After `definefont` of a Type 1 dictionary, the
  VM measures and shows with it: `stringwidth` and the `show` family use
  the charstring advances.
- **Type 42 glyphs**: the `sfnts` array is assembled into a TrueType
  program; `CharStrings` maps names to glyph indices; outlines come from
  the glyph table (quadratic contours converted to cubics), advances from
  the horizontal metrics scaled by the units per em.
- **`charpath`** appends a glyph's outline to the current path through
  the font matrix and the CTM for Type 1 and Type 42 fonts and advances
  the current point; for resident fonts (no outlines shipped yet) and
  Type 3 fonts it still raises `invalidfont`, each recorded with its
  trigger.
- **Embedded fonts in the IR** (`ps-graphics`): a font resource kind
  carrying the program snapshot, and `Text` runs over it as for resident
  fonts.
- **Embedding and subsetting** (`ps-fonts`, `remelt`): glyph usage per
  font across the document; Type 1 programs regenerated from the font
  dictionary with only the used charstrings (plus `.notdef` and `seac`
  components) and re-encrypted, embedded as `FontFile`; TrueType programs
  subset to the used glyphs with a synthesised (3,0) cmap from code to
  glyph, embedded as `FontFile2`; a `FontDescriptor` from the program;
  widths from the program; ToUnicode from glyph names. Font objects are
  written when the document finishes, when the glyph set is known.
- **Synthesised test fonts**: a small Type 1 writer (also the subsetter's
  output path) and a small TrueType table writer (also the subsetter's)
  build fonts with a few glyphs for the corpus; the corpus font programs
  are the project's own. An optional test uses a permissively licensed
  TrueType file found on the host when present.
- **Out of scope, with triggers**: CFF/Type 2 charstrings and FontType 2
  (with CID fonts, where they appear); composite fonts, CMaps, `cshow`;
  `charpath` for Type 3 (needs painting-as-path capture) and for resident
  fonts (needs shipped outlines — the resident-outlines asset change);
  Type 1 to CFF conversion on embedding; hint replacement and counter
  control; `GlyphDirectory` and incremental Type 42 downloads; `eexec`
  with a non-standard key; host fonts.

## Capabilities

### New Capabilities
- `font-programs`: `eexec`, the interpretation of Type 1 and Type 42 font
  programs into advances and outlines, `charpath`, and the subsetting and
  embedding of those programs.

### Modified Capabilities
- `text`: the requirement "The show family and glyph names" changes —
  `charpath` now produces outlines for Type 1 and Type 42 fonts; `show`
  and `stringwidth` with a Type 1 or Type 42 dictionary no longer raise
  `invalidfont`.
- `graphics-ir`: ADDED requirement for embedded-font resources.
- `remelt`: ADDED requirement for embedded font programs in the PDF.

## Impact

- Code: `crates/ps-vm` (`eexec`, a layered file entry, font-program
  extraction from dictionaries, glyph cache per font, `charpath`),
  `crates/ps-fonts` (`type1` charstrings, writer, and subsetter;
  `truetype` parser, subsetter, and table writer; outline type),
  `crates/ps-graphics` (embedded font resource, dump lines),
  `crates/remelt` (font embedding at finish, descriptors, ToUnicode from
  program glyph names), `tools/difftest`, `corpus/unit/fonts/` with
  goldens.
- Dependencies: none new. The Type 1 encryption is a published algorithm
  implemented in a few lines.
- Depends on `text-core` (archived). Enables the composite-font change
  and the resident-outlines asset change.
