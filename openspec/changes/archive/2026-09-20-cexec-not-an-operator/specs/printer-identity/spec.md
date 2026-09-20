## MODIFIED Requirements

### Requirement: Accepted device operators

`setscreen`, `setcolorscreen`, `settransfer`, `setcolortransfer` SHALL
accept their operands, record them in the graphics state, and apply
nothing; their `current…` counterparts SHALL return what was recorded
(the defaults before any set); `framedevice` SHALL accept its four
operands and do nothing. None of these SHALL affect the IR.

`cexec` SHALL NOT be defined: it is a printer extension that executes
native code for a particular printer's processor, PLRM3 has no such
operator, and a driver probing for it inside a guard depends on the
`undefined` error to discard the code string it pushed.

#### Scenario: Screen round-trips

- **GIVEN** `60 45 { pop } setscreen currentscreen pop pop =`
- **THEN** the output is `60.0` (the frequency comes back as a real)
  and the page's IR is unchanged

#### Scenario: cexec

- **GIVEN** `systemdict /cexec known =`
- **THEN** the output is `false`

#### Scenario: A driver's guarded probe

- **GIVEN** `{ (native code) cexec } stopped` with the guard's cleanup
  `{ dup type /stringtype eq { pop } if } if`
- **THEN** `stopped` returns `true`, the cleanup discards the code
  string, and the operand stack is left as the driver expects
