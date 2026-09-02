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
