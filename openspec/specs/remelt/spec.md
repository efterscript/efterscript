# remelt Specification

## Purpose
Turns delivered page IR into a PDF document: the mapping of every IR
operation and resource to its content-stream and object form, the document
skeleton, determinism, and what a job that fails still leaves behind.

## Requirements

### Requirement: Page delivery becomes a PDF page

Each page delivered by the graphics layer SHALL become one PDF page whose
media box is the page's media box and whose content stream holds the page's
operations in order, written as the page arrives; the document SHALL be
closed once at the end of the job with all pages in delivery order.

#### Scenario: A stroked line distils

- **GIVEN** `2 setlinewidth 10 10 moveto 100 10 lineto stroke showpage`
- **WHEN** it is distilled
- **THEN** the PDF has one page with media box `[0 0 612 792]` whose content
  stream, in order, sets line width 2, moves to (10,10), lines to (100,10),
  and strokes

#### Scenario: Three pages in order

- **GIVEN** a program that fills a different rectangle before each of three
  `showpage`s
- **THEN** the page tree counts three pages and the first, second, and third
  content streams fill the first, second, and third rectangle respectively

#### Scenario: A job with no pages

- **GIVEN** `1 2 add =`
- **THEN** the result is a well-formed PDF with zero pages and the program
  output `3`

### Requirement: Operation mapping

Every IR operation SHALL map to the PDF content-stream operation with the
same meaning: line width, cap, join, mitre limit, dash array with phase,
flatness; fill and even-odd fill; stroke; clip and even-odd clip followed by
path end; save and restore. Paths SHALL be written as move, line, curve, and
close segments with coordinates in default user space, numbers in the
canonical form shared with `pdf-out`.

#### Scenario: Even-odd fill and clip

- **GIVEN** a self-intersecting path filled with `eofill` after `eoclip`
- **THEN** the content stream contains `W* n` for the clip and `f*` for the
  fill, each preceded by the path

#### Scenario: Dash and line parameters

- **GIVEN** `[3 1] 0 setdash 1 setlinecap 2 setlinejoin 4 setmiterlimit`
  before a stroke
- **THEN** the content stream sets the dash `[3 1] 0`, cap 1, join 2, and
  mitre limit 4 before the stroke, and nothing that was not set

### Requirement: Stroke geometry is measured in the program's space

A stroke whose recorded CTM is not the identity SHALL be written inside its
own save/restore with that matrix concatenated and the path taken back
through its inverse, so the line width and dash lengths are interpreted in
the same space the program set them in. A stroke whose CTM is singular
SHALL be written without a transform, with its path in default user space.

#### Scenario: Scaled stroke keeps its width

- **GIVEN** `2 2 scale 1 setlinewidth 5 5 moveto 50 5 lineto stroke`
- **THEN** the content stream wraps the stroke in `q 2 0 0 2 0 0 cm … Q`,
  the path inside is `5 5 m 50 5 l`, and the line width is 1

#### Scenario: Translated stroke needs no transform

- **GIVEN** `10 20 translate 0 0 moveto 30 0 lineto stroke`
- **THEN** the stroke is written with a translation-only transform or with
  the path at (10,20)–(40,20) and no transform; the line width is unchanged
  either way

### Requirement: Colour passes into PDF unconverted

Device spaces SHALL be written with the device colour operators. Every
other colour space in the page's resources SHALL become a colour-space
resource named in the page's resource dictionary and selected by name, with
components written as given; a Separation or DeviceN space SHALL carry its
colourant names, its alternate space, and its captured tint-transform
source as a calculator function; an Indexed space SHALL carry its base,
maximum index, and lookup table.

#### Scenario: Separation reaches the page resources

- **GIVEN** a fill in `[/Separation /Spot /DeviceCMYK {…}]` with tint 0.6
- **THEN** the page's resources contain a colour space `[/Separation /Spot
  /DeviceCMYK <function>]` whose function is a calculator function with a
  one-component domain and four-component range, and the content stream
  selects it by name and sets colour `0.6`

#### Scenario: Device colour is direct

- **GIVEN** `0.2 0.4 0.6 setrgbcolor` before a fill
- **THEN** the content stream sets `0.2 0.4 0.6 rg` and the page declares no
  colour-space resource

### Requirement: Images become image XObjects

Each image in a page's resources SHALL become an image XObject with its
width, height, bits per component, colour space (or the image-mask flag
for a mask), decode array when it differs from the default, and
interpolation flag, with its sample data in a Flate container; the paint
SHALL be written inside a save/restore that concatenates the image's matrix
and invokes the XObject.

#### Scenario: A 2×2 gray image

- **GIVEN** a 2×2 8-bit DeviceGray image drawn at `100 100 translate 50 50
  scale`
- **THEN** the page has one image XObject with width 2, height 2, 8 bits,
  DeviceGray, whose decoded data is the four sample bytes, and the content
  stream is `q 50 0 0 50 100 100 cm /Im0 Do Q`

#### Scenario: An image mask paints the current colour

- **GIVEN** an `imagemask` in a Separation colour
- **THEN** the XObject is flagged as an image mask with no colour space, and
  the Separation is selected in the content stream before the mask is drawn

### Requirement: Deterministic output

Distilling the same program with the same options SHALL produce identical
bytes; the file SHALL carry a producer string naming the project and its
version and no timestamp.

#### Scenario: Two runs agree

- **GIVEN** any corpus file distilled twice in one process
- **THEN** the two outputs are byte-identical

### Requirement: A failed job still leaves a document

When the program ends in an uncaught error, every page delivered before the
error SHALL be in the output, the document SHALL be closed well-formed, and
the error SHALL be reported to the caller with the document.

#### Scenario: Error after the first page

- **GIVEN** a program that fills and `showpage`s once and then executes an
  undefined name
- **THEN** the output is a one-page PDF and the reported outcome is the
  `undefined` error

### Requirement: Command-line distillation

`efterscript pdf <in.ps> [<out.pdf>]` SHALL distil the input to the named
file, defaulting to the input's path with a `.pdf` extension; `-` as the
output SHALL write the PDF to standard output and move the program's own
output to standard error. The exit status SHALL be 0 when the job ended
normally and 1 when it ended in an error, in both cases after the PDF is
written.

#### Scenario: Default output name

- **WHEN** `efterscript pdf corpus/unit/graphics/stroked-line.ps` runs in a
  writable directory copy
- **THEN** `stroked-line.pdf` exists beside the input and the exit status is 0

#### Scenario: Corpus sidecar goldens

- **GIVEN** a corpus file with a sidecar under `corpus/golden/pdf/`
- **WHEN** `difftest run` executes
- **THEN** the distilled bytes are compared exactly with the golden, a
  mismatch is reported as a diff of the text lines, and `--update-pdf`
  rewrites the golden with uncompressed streams and a generator comment

### Requirement: Text becomes PDF text

Each text operation SHALL be written as a text object that selects the
font resource at size 1 with the text matrix derived from the IR's glyph
matrix, shows the glyph codes, and expresses displacements that differ
from the glyph width through positioning adjustments or explicit moves. A
resident font SHALL become a Type 1 font dictionary naming the standard
font, unembedded, with encoding differences, first and last code,
widths, and a ToUnicode CMap derived from the encoding's glyph names
through the Adobe Glyph List (names it cannot map are omitted). A Type 3
font SHALL become a Type 3 font dictionary with the font matrix,
bounding box, encoding, widths, and one CharProc per captured glyph
written through the content writer, opening with the width operator. Font
dictionaries SHALL be written once per document and shared by every page
whose resource is structurally equal.

#### Scenario: Standard font text

- **GIVEN** the "Hi" scenario of the IR
- **THEN** the content stream contains `BT`, `/F0 1 Tf`, `12 0 0 12 100
  700 Tm`, `(Hi) Tj`, `ET`; the page's font resource `/F0` is
  `/Type1 /BaseFont /Helvetica` with `/Widths` giving 722 for code 72,
  and its ToUnicode maps code 72 to U+0048

#### Scenario: Type 3 CharProcs

- **GIVEN** the square-glyph scenario
- **THEN** the page's font is `/Subtype /Type3` with `/FontMatrix [0.001
  0 0 0.001 0 0]`, `/CharProcs` holding one stream beginning `1000 0 d0`
  followed by the square's fill, and `/Encoding` mapping the code to the
  glyph name

#### Scenario: Fonts shared across pages

- **GIVEN** Helvetica text on each of two pages with the same encoding
- **THEN** the document contains one Helvetica font dictionary
  referenced from both pages' resources

#### Scenario: A checker accepts text output

- **WHEN** `EFTERSCRIPT_PDF_CHECK` names a checker and `difftest run`
  executes
- **THEN** every text corpus golden is accepted and text extraction of
  the "Hi" golden yields `Hi`

### Requirement: Embedded fonts in the PDF

An embedded-font resource SHALL become a font dictionary (`Type1` or
`TrueType`) whose descriptor embeds the subset program, with widths for
the used codes, encoding differences (Type 1) or a symbolic flag with the
synthesised cmap (TrueType), and a ToUnicode CMap from the program's
glyph names. The subset SHALL be computed over the whole document, so
font objects for embedded fonts SHALL be written when the document
finishes; determinism SHALL hold.

#### Scenario: Type 1 embedded

- **GIVEN** the synthesised Type 1 font shown on two pages with different
  glyphs
- **THEN** the document holds one font dictionary with a descriptor
  whose `FontFile` stream has `Length1`, `Length2`, `Length3`, contains
  exactly the glyphs used on both pages plus `.notdef`, and both pages
  reference it

#### Scenario: TrueType embedded

- **GIVEN** the synthesised TrueType font shown once
- **THEN** the font dictionary is `/TrueType` with a descriptor whose
  `FontFile2` is the subset program and flags marking it symbolic, and
  the content stream shows the code

### Requirement: Extra resident faces are embedded

Text in one of the twenty-one extra resident faces SHALL be written as
an embedded Type 1 font subset from the face's outline asset, named by
the asset's own font name with a subset tag, with widths from the face's
metrics and ToUnicode from its glyph names; the fourteen standard fonts
SHALL remain unembedded.

#### Scenario: Palatino text embeds Pagella

- **GIVEN** `/Palatino-Roman findfont 12 scalefont setfont 100 700
  moveto (Pa) show showpage`
- **THEN** the document holds one embedded Type 1 font whose base name
  ends in the TeX Gyre Pagella font name, subset to `P`, `a`, and
  `.notdef`, and the page's Helvetica-free resources reference it

#### Scenario: Helvetica stays unembedded

- **GIVEN** Helvetica text on the same page
- **THEN** the Helvetica font dictionary has no font file

### Requirement: CFF fonts in the PDF

An embedded-font resource whose program is CFF SHALL become a `Type1`
font dictionary with encoding differences, widths for the used codes,
ToUnicode from glyph names, and a descriptor whose `FontFile3` stream
has subtype `Type1C` and holds the subset program.

#### Scenario: Type1C embedded

- **GIVEN** the synthesised CFF font shown once
- **THEN** the font dictionary is `/Type1`, its descriptor's `FontFile3`
  has `/Subtype /Type1C`, the stream parses as CFF with the used glyph
  and `.notdef`, and a checker accepts the file and extracts the text

### Requirement: Composite fonts in the PDF

Text in a composite font SHALL be written through a Type 0 font
dictionary with `Identity-H` (or `Identity-V` for writing mode 1) whose
content-stream strings are two-byte CIDs, a `CIDSystemInfo`, a `DW` and
`W` array from the used glyphs' advances, and one descendant: a
`CIDFontType0` with a `FontFile3` of subtype `CIDFontType0C` holding a
CID-keyed CFF subset, or a `CIDFontType2` with a `FontFile2` subset and
a `CIDToGIDMap` stream. ToUnicode SHALL map CIDs through the job's CMap
when the CMap is Unicode-based, else through the TrueType cmap, else be
omitted. A CID-keyed font with Type 1 charstrings SHALL be written as a
Type 3 font whose CharProcs are its used glyphs' outlines.

#### Scenario: CIDFontType0C embedded

- **GIVEN** the two-byte show scenario
- **THEN** the document holds a Type 0 font with `Identity-H`, a
  `CIDFontType0` descendant whose `FontFile3` has `/Subtype
  /CIDFontType0C` and parses as CID-keyed CFF with CIDs 1 and 2, a `W`
  array giving 500 and 700, and the content stream shows `<00010002>`

#### Scenario: CIDFontType2 with a CID map

- **GIVEN** the `CIDFontType 2` scenario
- **THEN** the descendant is `CIDFontType2` with a `FontFile2` subset and
  a `CIDToGIDMap` stream mapping CID 3 to the subset's glyph index

#### Scenario: Unicode-based CMap gives ToUnicode

- **GIVEN** a corpus CMap named `Syn-UCS2-H` mapping `<0041>` to CID 1
- **THEN** the Type 0 font's ToUnicode maps CID 1 to U+0041

#### Scenario: Type 1 charstring CID font falls back to Type 3

- **GIVEN** the Type 1 charstring CIDFont scenario shown once
- **THEN** the page's font is a Type 3 font with one CharProc holding
  the glyph's outline, and a checker accepts the file

### Requirement: Stroke colour follows the current colour

Every colour-space and colour setting written to a content stream SHALL
be written for both painting operations: the non-stroking operators for
fills, text, and image masks, and the stroking operators for strokes,
so a stroke is painted in the colour current when it was painted.

#### Scenario: A red stroke

- **GIVEN** `1 0 0 setrgbcolor 4 setlinewidth 100 100 moveto 300 300
  lineto stroke showpage`
- **THEN** the content stream sets the stroking colour to red before the
  stroke, and the rendered page shows a red line

#### Scenario: A Separation stroke

- **GIVEN** a stroke in a Separation colour space
- **THEN** the stroking colour space and colour are set from the same
  resource as the non-stroking ones

### Requirement: Document objects from marks

The output SHALL contain, from the IR's marks: an `/Outlines` tree
whose items follow the mark order with the count-and-sign nesting rule
and reference their destinations; named destinations in the catalog;
per-page `/Annots` with link annotations (`/Dest` by name or a URI
action, border and colour when given); `/Info` entries from document
information alongside the producer; `/PageMode`, `/PageLayout`, and
`/OpenAction`; and `CropBox` and `Rotate` on pages. Output SHALL stay
deterministic and a document without marks SHALL be byte-identical to
today's.

#### Scenario: Bookmarks and links in the PDF

- **GIVEN** the nested-bookmark and cross-page link scenarios
- **THEN** the document's catalog references an outlines tree with
  three items, page 2 has one link annotation whose destination
  resolves to page 1, and a checker accepts the file

#### Scenario: No marks, no change

- **GIVEN** the stroked-line corpus file
- **THEN** its PDF golden is byte-identical
