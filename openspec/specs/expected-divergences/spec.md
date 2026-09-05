# expected-divergences Specification

## Purpose
The registry of reviewed decisions to behave differently from the
behaviour observed in other interpreters or specified by the reference,
one requirement per divergence, each tied to the corpus files that
declare it.

## Requirements

### Requirement: Registry discipline

Every expected divergence SHALL be a requirement in this specification
whose name is the slug corpus files declare with `% divergence:`, stating
the behaviour chosen, the behaviour it departs from without naming any
implementation, the reason, and the configuration that restores the
reference behaviour when one exists. A divergence SHALL be added only
through a change proposal.

#### Scenario: Slug resolution

- **GIVEN** a corpus file declaring a slug
- **THEN** a requirement with that name exists here and the oracle
  harness accepts the file

### Requirement: font-substitution

`findfont` of a name not defined in the job resolves to a resident face
instead of raising `invalidfont` as the reference specifies; chosen so
jobs that reference fonts they do not embed keep running; restored by
disabling substitution in the interpreter configuration.

#### Scenario: Declared on the substitution corpus files

- **GIVEN** the corpus files exercising alias and heuristic substitution
- **THEN** each carries `% divergence: font-substitution`

### Requirement: fmaptype-cmap-only

Type 0 fonts with a map type other than the CMap-driven one raise
`invalidfont`, where the reference defines map types 2 through 9;
chosen because the older map types have no consumer in the corpus and
no PDF form, and adding them is a recorded trigger.

#### Scenario: Declared

- **GIVEN** the corpus file exercising an older map type
- **THEN** it carries `% divergence: fmaptype-cmap-only`

### Requirement: resident-metrics-only

`charpath` on the two resident faces without a free outline asset
raises `invalidfont`, where other interpreters ship outlines; chosen
because no metric-compatible outlines under a compatible licence exist
for those faces.

#### Scenario: Declared

- **GIVEN** the corpus file outlining text in such a face
- **THEN** it carries `% divergence: resident-metrics-only`

### Requirement: pagedevice-records-unknown-keys

`setpagedevice` records keys it does not recognise and
`currentpagedevice` reads them back, where other interpreters ignore
them; chosen so jobs can round-trip device hints for a later
distillation policy; the reference leaves the recognised set to the
device.

#### Scenario: Declared

- **GIVEN** the corpus file setting an unknown page-device key
- **THEN** it carries `% divergence: pagedevice-records-unknown-keys`

### Requirement: integer-range

Integers are 32 bits and overflow promotes to reals, where other
interpreters carry wider integers; the reference makes the range
implementation-dependent.

#### Scenario: Declared

- **GIVEN** the corpus files exercising overflow and hexadecimal
  radix values beyond 32 bits
- **THEN** each carries `% divergence: integer-range`

### Requirement: job-server-save-level

`vmstatus` reports save level 0 for a job run directly, where an
interpreter running jobs under a job server's encapsulating save
reports 1; the session work will introduce the encapsulating level.

#### Scenario: Declared

- **GIVEN** the corpus file printing the save level
- **THEN** it carries `% divergence: job-server-save-level`

### Requirement: resource-size-unknown

`resourcestatus` reports size 0 for a resource whose size is unknown,
where other interpreters report −1; the reference allows either.

#### Scenario: Declared

- **GIVEN** the corpus files printing a resource status size
- **THEN** each carries `% divergence: resource-size-unknown`

### Requirement: unspecified-forall-order

Dictionary `forall` visits entries in insertion order, which the
reference leaves unspecified; chosen for deterministic output.

#### Scenario: Declared

- **GIVEN** the corpus file enumerating a dictionary
- **THEN** it carries `% divergence: unspecified-forall-order`

### Requirement: file-access-policy

Opening a host file without a file capability raises
`undefinedfilename`, where an interpreter with a restricted file policy
raises `invalidfileaccess`; chosen because the capability model has no
ambient file system, so the name is undefined rather than forbidden.

#### Scenario: Declared

- **GIVEN** the corpus file opening a host file without a capability
- **THEN** it carries `% divergence: file-access-policy`

### Requirement: malformed-font-invalidfont

A malformed font program (a charstring ending mid-operator, a FontSet
declaring more data than the file holds) raises `invalidfont`, where
other interpreters draw what they can; chosen because silent partial
fonts hide corruption; the reference leaves the treatment to the
implementation.

#### Scenario: Declared

- **GIVEN** the corpus files with a malformed charstring and short
  FontSet data
- **THEN** each carries `% divergence: malformed-font-invalidfont`

### Requirement: resident-inventory

The resident font, CMap, and procedure-set inventories and the resident
faces' metrics are this implementation's own (thirty-five faces from
the Core 14 and TeX Gyre metrics, two Identity CMaps, two procedure
sets), so enumerations and third-decimal advances differ from other
interpreters'.

#### Scenario: Declared

- **GIVEN** the corpus files enumerating resources or printing resident
  advances
- **THEN** each carries `% divergence: resident-inventory`

### Requirement: cvrs-negative-unsigned

`cvrs` of a negative integer in a radix other than 10 yields the
unsigned two's-complement digits, per the reference's description,
where an interpreter with wider integers raises `rangecheck`.

#### Scenario: Declared

- **GIVEN** the corpus file converting a negative integer in radix 16
- **THEN** it carries `% divergence: cvrs-negative-unsigned`

### Requirement: radix-without-digits

A radix mark with no digits after it scans as a name, per the
reference's number syntax, where other interpreters scan it as an
integer.

#### Scenario: Declared

- **GIVEN** the corpus file scanning number-like names
- **THEN** it carries `% divergence: radix-without-digits`

### Requirement: unshown-marks-not-flushed

A job that paints and ends without `showpage` shows nothing, per the
reference, where other interpreters flush the unshown marks as a final
page.

#### Scenario: Declared

- **GIVEN** a corpus file that paints and deliberately ends without
  `showpage`
- **THEN** it carries `% divergence: unshown-marks-not-flushed`

### Requirement: procedure-nesting-limit

Procedure bodies nested deeper than the scanner's limit raise
`limitcheck` while scanning, where other interpreters scan arbitrarily
deep nesting; the reference makes such limits implementation-dependent,
and the limit bounds memory during scanning of untrusted input.

#### Scenario: Declared

- **GIVEN** the corpus file nesting procedures beyond the limit
- **THEN** it carries `% divergence: procedure-nesting-limit`

### Requirement: vertical-default-metrics

Through a vertical CMap, a CIDFont carrying no vertical metrics advances
by the default vertical metrics of PLRM3 §5.11 — one em downward, the
glyph placed at its vertical origin — where the reference interpreter
advances by the glyph's horizontal width and moves nothing vertically,
whatever the CIDFont declares (`WMode` 1, `W2`/`DW2` entries, a
`CIDFontType 2` descendant, an explicit Type 0 font with `WMode` 1);
what input would make the reference advance vertically is not known,
and ours follows the specification's default.

#### Scenario: Declared

- **GIVEN** the corpus file showing through `Identity-V` a synthesised
  CIDFont without vertical metrics
- **THEN** it carries `% divergence: vertical-default-metrics`
