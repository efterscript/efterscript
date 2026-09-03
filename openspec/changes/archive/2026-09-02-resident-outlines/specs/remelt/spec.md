# remelt

## ADDED Requirements

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
