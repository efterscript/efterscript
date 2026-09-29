# expected-divergences

## MODIFIED Requirements

### Requirement: server-password-default

`exitserver` and `setsystemparams` SHALL accept the password 0 unless
the interpreter is configured with another, the customary default of
printers; the reference converter's passwords are not 0, so it raises
`invalidaccess`. Restored by configuring the server password.

#### Scenario: Declared

- **GIVEN** a corpus file presenting the password 0 to `exitserver` or
  `setsystemparams`
- **THEN** it carries `% divergence: server-password-default`
