# interpreter-core

## ADDED Requirements

### Requirement: type yields an executable name

`type` SHALL return the type name as an executable name object.

#### Scenario: Executable attribute

- **GIVEN** `1 type xcheck =` and `1 type ==`
- **THEN** the output is `true` then `integertype`

### Requirement: == writes null in its syntactic form

`==` SHALL write a null object as `null`; `=` SHALL be unchanged.

#### Scenario: null through both writers

- **GIVEN** `null == null =`
- **THEN** the output is `null` then `--nostringval--`
