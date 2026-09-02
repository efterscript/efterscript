# remelt

## ADDED Requirements

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
