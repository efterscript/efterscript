# expected-divergences

## ADDED Requirements

### Requirement: cexec-defined

`cexec` is defined in `systemdict` and executes its operand as `exec`
does; the reference converter has no such operator and raises
`undefined`. Chosen because printer setup code probes for it before
downloading, and the download it guards is native code for a printer's
own processor, which `exec` of a literal string leaves untouched.

#### Scenario: Declared

- **GIVEN** the corpus file executing a procedure through `cexec`
- **THEN** it carries `% divergence: cexec-defined`

### Requirement: server-password-default

`exitserver` accepts the password 0 unless the interpreter is
configured with another, the customary default of printers; the
reference converter's server password is not 0, so it raises
`invalidaccess`. Restored by configuring the server password.

#### Scenario: Declared

- **GIVEN** the corpus file leaving the server loop with password 0
- **THEN** it carries `% divergence: server-password-default`
