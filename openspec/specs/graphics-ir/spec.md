# graphics-ir Specification

## Purpose
Defines the boundary between the PostScript VM and graphics: a backend
trait fed numbers and small value types, the graphics-state stack with
its current path, the PDF-shaped page IR that painting produces, colour
passed through as spaces plus components without conversion, the
coordinate model, save/restore integration, tolerant page-device
handling, and the versioned canonical `ir/1` dump used for sidecar
goldens. Semantics authority is the PostScript Language Reference §4
and §8; this spec records the behaviours the implementation commits to.

## Requirements

### Requirement: Layer boundary

Graphics operators SHALL dispatch through a backend trait that receives
numbers and small value types, never interpreter state; with no backend
installed, graphics operator names SHALL be undefined.

#### Scenario: Scripting embedder pays nothing

- **GIVEN** an interpreter constructed without a graphics backend
- **WHEN** `0 0 moveto` is executed
- **THEN** the error is `undefined` for `moveto`

### Requirement: Painting produces IR

`fill`, `eofill`, and `stroke` SHALL emit one IR operation carrying the
painted path (arcs as Bézier curves) and the graphics settings in effect,
emitted lazily and deduplicated; `showpage` SHALL deliver the completed page
to the sink and reset for the next page.

#### Scenario: A stroked line

- **GIVEN** `2 setlinewidth 10 10 moveto 100 10 lineto stroke showpage`
- **THEN** the delivered page contains a line-width setting of 2 and one
  stroke of Move(10,10) Line(100,10)

#### Scenario: Unpainted paths leave no trace

- **GIVEN** `0 0 moveto 5 5 lineto newpath 1 1 moveto 2 2 lineto stroke showpage`
- **THEN** the page contains exactly one stroke, of the second path

#### Scenario: gsave/grestore does not bloat the IR

- **GIVEN** fifty `gsave grestore` pairs followed by one `fill` of a path
- **THEN** the page contains exactly one paint operation and no save/restore
  churn

### Requirement: N-channel colour pass-through

Colour SHALL be a colour-space resource plus a component vector of that
space's arity; Separation and DeviceN specifications SHALL be captured with
their names, alternate space, and tint-transform source, and SHALL never be
converted.

#### Scenario: Separation survives

- **GIVEN** a program selecting `[/Separation /Spot /DeviceCMYK {…}]` and
  filling with tint 0.6
- **THEN** the IR resources contain a Separation space named `Spot` with a
  CMYK alternate and captured transform, and the paint carries the single
  component 0.6

### Requirement: Coordinate model

Default user space SHALL be origin bottom-left at 72 units per inch with no
device resolution; the CTM SHALL be applied by the backend so IR coordinates
are in default user space; `currentpoint` SHALL answer in the current user
space.

#### Scenario: Translate then draw

- **GIVEN** `72 72 translate 0 0 moveto 72 0 lineto stroke showpage`
- **THEN** the stroke is Move(72,72) Line(144,72)

### Requirement: save/restore integration

`save` SHALL perform the implicit graphics save and `restore` SHALL return
the graphics state to it; `grestore` SHALL clamp at the current save's
graphics region.

#### Scenario: restore restores line width

- **GIVEN** `1 setlinewidth save 5 setlinewidth restore`
- **THEN** the current line width is 1

### Requirement: Page device tolerance

`setpagedevice` SHALL extract the media box from a page-size entry, record
all other entries retrievably via `currentpagedevice`, and never raise an
error for an unknown key. For the keys it recognises it SHALL raise
`typecheck` when the value has the wrong type: dictionaries for
`InputAttributes`, `OutputAttributes`, and `Policies`; booleans for
`Duplex`, `Collate`, and `Tumble`; integers for `NumCopies` and
`Orientation`; arrays or null for `ImagingBBox`, `HWResolution`, and
`PageOffset`.

#### Scenario: Unknown keys accepted

- **GIVEN** `<< /PageSize [612 792] /TraySwitch true >> setpagedevice`
- **THEN** no error is raised, the page media box is 612×792, and
  `currentpagedevice /TraySwitch get` is `true`

#### Scenario: Ill-typed known key

- **GIVEN** `<< /InputAttributes (tray) >> setpagedevice`
- **THEN** the error is `typecheck`

### Requirement: Deterministic IR dump

The IR SHALL have a versioned canonical text form; identical programs SHALL
dump identically; `difftest` SHALL compare dumps against sidecar goldens and
`efterscript ir` SHALL print them.

#### Scenario: Sidecar golden

- **GIVEN** a corpus graphics file with a committed `.ir` sidecar golden
- **WHEN** `difftest run` executes it
- **THEN** the dump is compared and matches

### Requirement: Text produces IR

The graphics state SHALL hold the current font as an opaque font
reference and matrix that `gsave`/`grestore` save and restore. Showing
text SHALL emit one text operation carrying a font resource, the matrix
mapping glyph space to default user space at the start of the run, and
the glyphs shown, each with the displacement applied after it; the
graphics settings a text paint depends on (colour) SHALL be emitted
lazily as for fills. Font resources SHALL be either a resident standard
font with its encoding, or a Type 3 font with its font matrix, encoding,
and the captured procedure of every glyph shown on the page, and SHALL be
interned per page.

#### Scenario: Text operation shape

- **GIVEN** `/Helvetica findfont 12 scalefont setfont 100 700 moveto
  (Hi) show showpage`
- **THEN** the page holds one font resource for Helvetica with the
  standard encoding, and one text operation with matrix
  `[0.012 0 0 0.012 100 700]`, glyph codes 72 and 105 with displacements
  722 and 222 in glyph space

#### Scenario: Type 3 glyphs are captured in glyph space

- **GIVEN** a Type 3 font with `FontMatrix [0.01 0 0 0.01 0 0]` whose
  glyph fills `0 0 50 50` in glyph units, shown at `2 2 scale`
- **THEN** the captured glyph procedure's fill is the rectangle 0 0 50 50
  in glyph space, and the text operation's matrix is `[0.02 0 0 0.02 tx ty]`

#### Scenario: Text under a clip and colour

- **GIVEN** a clip, `1 0 0 setrgbcolor`, then `show`
- **THEN** the dump shows `q`, the clip, the colour setting, then the
  text operation, and nothing else

### Requirement: Text in the dump

The `ir/1` dump SHALL list font resources as `font <n> …` lines (a
resident font by name with its encoding differences from the built-in
encoding; a Type 3 font by matrix followed by one `glyph` block per
captured glyph with its width and procedure lines indented) and text
operations as `text <font> <a> <b> <c> <d> <tx> <ty> (<bytes>) <dx> <dy>…`
lines; dumps of pages without text SHALL be unchanged.

#### Scenario: Existing goldens unchanged

- **WHEN** `difftest run` executes after this change
- **THEN** every pre-existing `.ir` and `.pdf` golden still matches

### Requirement: Embedded-font resources

A font resource SHALL also be an embedded font: the program snapshot
(Type 1 or TrueType) with its encoding, interned per page by the font's
identity and encoding; text operations over it SHALL carry glyph codes
and displacements as for resident fonts. The dump SHALL list such a
resource as `font <n> embedded <kind> <FontName> glyphs=<count>` with
encoding differences, and SHALL never print program bytes.

#### Scenario: Embedded font in the dump

- **GIVEN** a synthesised Type 1 font `Syn` shown once
- **THEN** the dump's resources contain `font 0 embedded type1 Syn`
  followed by the glyph count, and the text line references font 0

### Requirement: Composite glyph runs

Glyphs in a text operation SHALL carry a code of one to four bytes with
its byte length and a CID (equal to the code for simple fonts), and a
font resource SHALL also be a composite font: the CMap's name, writing
mode, whether the CMap is Unicode-based, and the descendant's program
snapshot with its kind. The dump SHALL write a run's codes in
hexadecimal when any code is longer than one byte and SHALL list a
composite resource as `font <n> composite <CMapName> wmode=<m>
<descendant kind> <FontName> glyphs=<count>`; dumps of pages without
composite text SHALL be unchanged.

#### Scenario: Composite run in the dump

- **GIVEN** the two-byte show scenario
- **THEN** the dump's text line writes `<00010002>` and the displacements
  500 and 700, and the resource line names `Identity-H`

#### Scenario: Existing goldens unchanged

- **WHEN** `difftest run` executes after this change
- **THEN** every pre-existing `.ir` and `.pdf` golden still matches
