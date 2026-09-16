## ADDED Requirements

### Requirement: Identity in systemdict and the font types

`systemdict` SHALL define `version`, `product`, `revision`, and
`serialnumber` with the configured identity's values (the same
`product`, `version`, and `revision` `statusdict` carries;
`serialnumber` an integer, default 0), and the `FontType` category
SHALL list 0, 1, 2, 3, 9, 11, and 42.

#### Scenario: The systemdict entries

- **WHEN** `systemdict /product get statusdict /product get eq serialnumber type` is executed
- **THEN** the stack holds `true` and `/integertype`

#### Scenario: CID font types are listed

- **WHEN** `9 /FontType resourcestatus` and `11 /FontType resourcestatus` and `10 /FontType resourcestatus` are executed
- **THEN** the first two leave `0 0 true` and the third `false`
