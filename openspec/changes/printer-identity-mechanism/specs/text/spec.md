# text

## MODIFIED Requirements

### Requirement: Encodings and resource categories

`StandardEncoding` and `ISOLatin1Encoding` SHALL be read-only 256-element
name arrays in `systemdict`. `findresource`, `resourcestatus`,
`defineresource`, `undefineresource`, and `resourceforall` SHALL operate
on the `Font` category (resident fonts and `definefont` results), the
`Encoding` category (the two encodings), the `ProcSet` category (the
built-in procedure sets, `FontSetInit` first), the `FontSet` category
(FontSets loaded through `StartData`), and the implicit categories
`FontType`, `FMapType`, `Filter`, `ColorSpaceFamily`, `Category`, and
`Generic`, whose members describe this interpreter's own capabilities:
`resourcestatus` SHALL answer status 0 and size 0 for a member,
`findresource` SHALL return the key, `resourceforall` SHALL enumerate
the members, and `defineresource` SHALL raise `invalidaccess` on them;
an unknown category SHALL raise `undefined`; a resource not found SHALL
raise `undefined` from `findresource` and leave `false` from
`resourcestatus`.

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

#### Scenario: Implicit categories answer for the interpreter

- **GIVEN** `42 /FontType resourcestatus`, `7 /FontType
  resourcestatus`, and `(*) { == } 32 string /Category resourceforall`
- **THEN** the first leaves `0 0 true`, the second `false`, and the
  listing includes `Font`, `FontType`, and `Category`
