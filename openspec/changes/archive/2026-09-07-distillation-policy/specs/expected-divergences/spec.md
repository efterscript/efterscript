# expected-divergences

## ADDED Requirements

### Requirement: distiller-params-unknown-keys

`setdistillerparams` and `currentdistillerparams` are defined whether or
not the job writes PDF, and a key the writer does not recognise is kept
and read back by `currentdistillerparams`; other converters define the
two operators only while writing PDF and drop the keys they do not know.
Chosen so a job's own settings survive the round trip the parameters
reference describes and the writer can report them as not honoured.

#### Scenario: Declared

- **GIVEN** the corpus file reading an unknown key back
- **THEN** it carries `% divergence: distiller-params-unknown-keys`

### Requirement: distiller-params-typecheck

`setdistillerparams` given a recognised key with a value of the wrong
type raises `typecheck` with the dictionary still on the operand stack
and no parameter changed; other converters raise a stack fault having
consumed the dictionary. Chosen because the reference's type table for
the recognised keys is the operator's contract.

#### Scenario: Declared

- **GIVEN** the corpus file offering ill-typed recognised keys
- **THEN** it carries `% divergence: distiller-params-typecheck`
