# interpreter-parameters

## ADDED Requirements

### Requirement: User parameters

`setuserparams` and `currentuserparams` SHALL be defined per their
entries in PLRM3 §8.2 and §C.1.1. The interpreter SHALL define every key
of Table C.1 with the defaults recorded in the design.
`currentuserparams` SHALL return a new dictionary holding every defined
key each time it is executed. `setuserparams` SHALL apply each defined
key present in its operand, leave the others unchanged, ignore keys it
does not define, and raise `typecheck` for a value of the wrong type
without changing any parameter. `JobName` SHALL be stored, truncated to
100 bytes. `MaxOpStack`, `MaxDictStack`, `MaxExecStack`, and
`MaxLocalVM` SHALL report the interpreter's configured limits, and
setting them SHALL leave them there. `IdiomRecognition` SHALL stay
`false`. Every other key SHALL store the value given, with a negative
integer stored as 0, and SHALL have no other effect. User parameters
SHALL live in local VM, so `restore` reverts them to their values at
the matching `save`.

#### Scenario: The defined keys

- **WHEN** `currentuserparams dup /JobName known exch /MaxOpStack get =` is executed
- **THEN** the output is `500` and the stack holds `true`

#### Scenario: JobName is stored and restored

- **WHEN** `save << /JobName (report) >> setuserparams currentuserparams /JobName get = restore currentuserparams /JobName get length =` is executed
- **THEN** the output is `report` then `0`

#### Scenario: Unknown keys are ignored and limits are kept

- **WHEN** `<< /NoSuchKey 1 /MaxOpStack 10 >> setuserparams currentuserparams /MaxOpStack get =` is executed
- **THEN** the output is `500` and no error is raised

#### Scenario: A value of the wrong type

- **WHEN** `<< /JobName 7 >> setuserparams` is executed
- **THEN** the error is `typecheck` and `JobName` is unchanged

### Requirement: System parameters

`setsystemparams` and `currentsystemparams` SHALL be defined per their
entries in PLRM3 §8.2 and §C.1.2. The interpreter SHALL define the
system parameters listed in the design, with deterministic values:
`Revision` equal to the `revision` in `systemdict`, `PageCount` the
number of pages shown in the interpreter's life, `PrinterName` the
`product` in `systemdict` until set, `GenericResourceDir` and
`FontResourceDir` `(%null)`. `currentsystemparams` SHALL return a new
dictionary holding every defined key except `SystemParamsPassword` and
`StartJobPassword`. `setsystemparams` SHALL raise `invalidaccess` and
change nothing when permission is not granted (next requirement); when
granted, it SHALL apply each defined, writable key present, ignore
read-only and unknown keys, and raise `typecheck` for a value of the
wrong type without changing any parameter. Setting `PrinterName` to the
empty string SHALL set it to `product`. System parameters SHALL be held
outside VM, so no `restore` reverts them.

#### Scenario: Reading the system parameters

- **WHEN** `currentsystemparams dup /Revision get revision eq exch /SystemParamsPassword known =` is executed
- **THEN** the output is `false` and the stack holds `true`

#### Scenario: A system parameter survives restore

- **WHEN** `save << /Password 0 /MaxFontCache 200000 >> setsystemparams restore currentsystemparams /MaxFontCache get =` is executed
- **THEN** the output is `200000`

#### Scenario: Read-only keys are ignored

- **WHEN** `<< /Password 0 /Revision 99 >> setsystemparams currentsystemparams /Revision get revision eq =` is executed
- **THEN** the output is `true`

### Requirement: Passwords

`SystemParamsPassword` and `StartJobPassword` SHALL be write-only system
parameters holding strings, both set at construction to the configured
server password converted as `cvs` converts an integer. A password
operand SHALL be accepted as a string or an integer, an integer being
converted the same way, and compared byte for byte. `setsystemparams`
and `setdevparams` SHALL be permitted when the operand's `Password`
equals `SystemParamsPassword`, when `SystemParamsPassword` is empty,
when `FactoryDefaults` is the only entry besides `Password`, or while
the prelude runs; otherwise they SHALL raise `invalidaccess`.

#### Scenario: The default password

- **WHEN** `<< /Password (0) /MaxFontCache 300000 >> setsystemparams currentsystemparams /MaxFontCache get =` is executed on an interpreter with the default server password
- **THEN** the output is `300000`

#### Scenario: A wrong password

- **WHEN** `<< /Password 1 /MaxFontCache 300000 >> setsystemparams` is executed on an interpreter with the default server password
- **THEN** the error is `invalidaccess` and `MaxFontCache` is unchanged

#### Scenario: Changing the password

- **WHEN** `<< /Password 0 /SystemParamsPassword (s3) >> setsystemparams` is executed, then `<< /Password 0 /MaxFontCache 1 >> setsystemparams`
- **THEN** the second raises `invalidaccess`, and `<< /Password (s3) /MaxFontCache 1 >> setsystemparams` then succeeds

#### Scenario: The prelude needs no password

- **GIVEN** an interpreter whose prelude is `<< /PrinterName (Front Office) >> setsystemparams`
- **WHEN** `currentsystemparams /PrinterName get =` is executed
- **THEN** the output is `Front Office`

### Requirement: Device parameters

`setdevparams` and `currentdevparams` SHALL be defined per their entries
in PLRM3 §8.2 and §C.4. The interpreter SHALL define no parameter set,
so both SHALL raise `undefined` for every device name, after checking
their operands' types (`typecheck`).

#### Scenario: A probe inside stopped

- **WHEN** `mark { (%Serial%) currentdevparams } stopped { cleartomark 0 dict } { exch pop } ifelse length =` is executed
- **THEN** the output is `0`

### Requirement: Font-cache and VM operators

`cachestatus`, `setcachelimit`, `setcacheparams`, `currentcacheparams`,
`setvmthreshold`, and `vmreclaim` SHALL be defined per their entries in
PLRM3 §8.2 and §C.3.2, §C.3.5, as views of the parameters above:
`cachestatus` SHALL leave seven integers, the current consumptions zero,
`bmax` equal to `MaxFontCache`, and `blimit` equal to `MaxFontItem`;
`setcachelimit` SHALL set `MaxFontItem`; `setcacheparams` SHALL set
`MinFontCompress` and `MaxFontItem` from its two topmost operands and
leave `MaxFontCache` unchanged; `currentcacheparams` SHALL leave a mark
followed by `MaxFontCache`, `MinFontCompress`, and `MaxFontItem`;
`setvmthreshold` SHALL set `VMThreshold`; `vmreclaim` SHALL set
`VMReclaim` for −2, −1, and 0, do nothing for 1 and 2, and raise
`rangecheck` otherwise.

#### Scenario: cachestatus

- **WHEN** `cachestatus count =` is executed on an empty stack, followed by `currentuserparams /MaxFontItem get eq =`
- **THEN** the output is `7` then `true`

#### Scenario: The cache operators agree with the parameters

- **WHEN** `mark 1 2 3 setcacheparams currentuserparams dup /MinFontCompress get = /MaxFontItem get = 900 setcachelimit currentcacheparams = pop pop pop` is executed
- **THEN** the output is `2`, `3`, then `900`
