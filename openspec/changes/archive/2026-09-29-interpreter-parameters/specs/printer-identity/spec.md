# printer-identity

## MODIFIED Requirements

### Requirement: statusdict and serverdict

`systemdict` SHALL hold a writable `statusdict` containing `product`,
`version`, and `revision` describing this interpreter and nothing else
by default, and a writable `serverdict` containing `exitserver`.
`exitserver` SHALL take a password, a string or an integer converted as
`cvs` converts it, compare it with the `StartJobPassword` and
`SystemParamsPassword` system parameters (both the configured server
password by default, 0 unless configured), raise `invalidaccess` when it
matches neither, and on a match continue execution at the server level
so that subsequent definitions persist for the interpreter's life.

#### Scenario: Default identity

- **GIVEN** `statusdict /product get = statusdict length =`
- **THEN** the output is the interpreter's own product string followed
  by `3`

#### Scenario: exitserver with the right password

- **GIVEN** `serverdict begin 0 exitserver /persist 1 def` and later
  `persist =`
- **THEN** the output is `1`: the dictionary stack is back at the
  permanent dictionaries and the definition landed in `userdict`

#### Scenario: exitserver with the password as a string

- **GIVEN** `serverdict begin (0) exitserver /persist 1 def persist =`
- **THEN** the output is `1`

#### Scenario: exitserver with the wrong password

- **GIVEN** `serverdict begin 1 exitserver`
- **THEN** the error is `invalidaccess`
