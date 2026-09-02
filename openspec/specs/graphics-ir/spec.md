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
error for an unknown key.

#### Scenario: Unknown keys accepted

- **GIVEN** `<< /PageSize [612 792] /TraySwitch true >> setpagedevice`
- **THEN** no error is raised, the page media box is 612×792, and
  `currentpagedevice /TraySwitch get` is `true`

### Requirement: Deterministic IR dump

The IR SHALL have a versioned canonical text form; identical programs SHALL
dump identically; `difftest` SHALL compare dumps against sidecar goldens and
`efterscript ir` SHALL print them.

#### Scenario: Sidecar golden

- **GIVEN** a corpus graphics file with a committed `.ir` sidecar golden
- **WHEN** `difftest run` executes it
- **THEN** the dump is compared and matches
