# Change: Text core — fonts in the VM, Type 3 glyphs, resident metrics, text in the IR and PDF

## Why

Every real job draws text, and none of the pipeline can carry it yet:
`show` is undefined, `stringwidth` cannot answer, and the IR has no text
operation. This change is the first half of the font milestone and the
one whose shapes are hard to retrofit. The current font must live in the
graphics state so `gsave`/`grestore` and `save`/`restore` handle it like
line width, which fixes where the font sits at the VM/graphics boundary
(an opaque id, not a dictionary). Text must reach the IR as glyph runs
with a per-glyph displacement, or `ashow`, `widthshow`, and `xshow` can
never be represented. Type 3 glyphs must be captured by executing them
through the interpreter into per-glyph procedures, because no library
can, and the capture path is the same one later transparency groups
need. And ToUnicode must be derived from the real encoding vector from
day one, which is the content-extraction quality the project promises.

Font-program parsing (Type 1, CFF, TrueType), embedding, subsetting,
outline extraction for `charpath`, composite fonts, and host-font access
are the following change, `font-programs`; they land on the font
dictionary semantics, glyph-run IR, and PDF font objects fixed here.

## What Changes

- **VM font semantics** (`ps-vm`): font dictionaries per the reference's
  model (FontType, FontMatrix, Encoding, FontBBox, and for Type 3
  `BuildGlyph`/`BuildChar`); `definefont`, `undefinefont`, `findfont`,
  `scalefont`, `makefont`, `setfont`, `selectfont`, `currentfont`,
  `FontDirectory`, `GlobalFontDirectory`; `show`, `ashow`, `widthshow`,
  `awidthshow`, `kshow`, `xshow`, `yshow`, `xyshow`, `glyphshow`,
  `stringwidth`; `setcachedevice`, `setcachedevice2`, `setcharwidth`;
  the `StandardEncoding` and `ISOLatin1Encoding` arrays; and the Level 2
  resource operators (`findresource`, `resourcestatus`, `defineresource`,
  `undefineresource`, `resourceforall`) over the `Font` and `Encoding`
  categories.
- **Type 3 fonts executed natively**: `show` runs the glyph procedure
  through the interpreter as a nested frame, with the graphics layer
  capturing the glyph's marks into a per-glyph procedure that the IR
  references; `setcachedevice` and `setcharwidth` supply the width.
- **Resident metrics** (`ps-fonts`): the fourteen standard fonts' metrics
  from the Core 14 AFM files, promoted from the vault under their
  redistribution grant with the licence file alongside; widths, font
  bounding box, and built-in encodings for Symbol and ZapfDingbats.
  `stringwidth` is correct for these fonts.
- **Name-level substitution that never errors**: `findfont` of a name not
  defined in the job resolves through an alias table (the metric-pair
  families and their PostScript-name variants) and a style heuristic to
  one of the fourteen; the resource operators report exactly the fourteen
  names. The reference makes an unknown font an `invalidfont` error;
  substituting instead is a recorded expected divergence, switchable off
  in the interpreter configuration.
- **Text in the IR** (`ps-graphics`): the graphics state gains the
  current font (an id and matrix); a `Text` operation carries a font
  resource, the text matrix, and glyphs each with their displacement; font
  resources are either a resident font with its encoding differences or a
  Type 3 font with captured glyph procedures. The `ir/1` dump grows
  `font` resource lines and `text` operation lines; existing goldens are
  unchanged.
- **Text in PDF** (`remelt`): text objects with the fourteen standard
  fonts referenced unembedded, encoding differences and widths from the
  IR, a ToUnicode CMap derived from the encoding's glyph names through
  the Adobe Glyph List (promoted from the vault under its grant), and
  Type 3 fonts as PDF Type 3 fonts whose CharProcs are the captured
  glyph procedures written through the existing content writer. Font
  objects are shared across pages when structurally equal.
- **Out of scope, with triggers**: font-program parsing and embedding
  (`font-programs`, next); `charpath` (registered, raises `invalidfont`
  until outlines exist); composite and CID-keyed fonts, CMaps (`Type 0`
  fonts, after `font-programs`); host-font access (a capability, with the
  session work); kerning data (present in the AFMs, unused until a
  consumer needs it); `eexec` stays `undefined` (it is the Type 1 loader).

## Capabilities

### New Capabilities
- `text`: font dictionaries and the font operators in the VM, resident
  metrics and substitution, Type 3 glyph execution, and the observable
  results of `stringwidth` and `show`.

### Modified Capabilities
- `graphics-ir`: ADDED requirements for the current font in the graphics
  state, the `Text` operation with per-glyph displacement, font resources,
  Type 3 glyph capture, and the dump's text lines. No existing requirement
  changes.
- `remelt`: ADDED requirement for text output — standard fonts unembedded
  with encoding, widths and ToUnicode; Type 3 fonts as CharProcs; font
  objects shared across pages.

## Impact

- Code: `crates/ps-vm` (`ops/font.rs`, `ops/resource.rs`, font state in
  the interpreter, a `Show` loop frame, additive `GraphicsBackend` methods
  for font, text, and glyph capture — the mock backend updated),
  `crates/ps-fonts` (AFM parsing, the resident set, substitution, the
  glyph list; first real code in the crate), `crates/ps-graphics` (font
  in `GState`, `IrOp::Text`, `FontSpec` resources, glyph capture, dump
  lines), `crates/remelt` (font objects, text in the content writer,
  ToUnicode), `tools/difftest` and `corpus/unit/text/` with `.ir` and
  `.pdf` goldens.
- Data promoted into repo A: `crates/ps-fonts/data/core14/` (14 AFM files
  and Adobe's licence file, unmodified) and `crates/ps-fonts/data/glyphlist.txt`
  (with its header), each with a provenance note recording source, commit,
  and grant. Binary size grows by the embedded metrics (well under a
  megabyte; kerning pairs are retained for now).
- Dependencies: none new.
- Depends on `interpreter-core`, `graphics-ir`, and `remelt-minimal` (all
  archived).
