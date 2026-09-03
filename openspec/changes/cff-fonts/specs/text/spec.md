# text

## MODIFIED Requirements

### Requirement: Encodings and resource categories

`StandardEncoding` and `ISOLatin1Encoding` SHALL be read-only 256-element
name arrays in `systemdict`. `findresource`, `resourcestatus`,
`defineresource`, `undefineresource`, and `resourceforall` SHALL operate
on the `Font` category (resident fonts and `definefont` results), the
`Encoding` category (the two encodings), the `ProcSet` category (the
built-in procedure sets, `FontSetInit` first), and the `FontSet`
category (FontSets loaded through `StartData`); an unknown category
SHALL raise `undefined`; a resource not found SHALL raise `undefined`
from `findresource` and leave `false` from `resourcestatus`.

#### Scenario: Encoding resource

- **GIVEN** `/ISOLatin1Encoding /Encoding findresource 233 get`
- **THEN** the name printed is `eacute`

#### Scenario: Font category lists the resident set

- **GIVEN** `(*) { == } 128 string /Font resourceforall`
- **THEN** the thirty-five resident names are printed in sorted order

#### Scenario: ProcSet category

- **GIVEN** `/FontSetInit /ProcSet resourcestatus` and `/NoSuchSet
  /ProcSet resourcestatus`
- **THEN** the first leaves `true` with status 2 and the second `false`
