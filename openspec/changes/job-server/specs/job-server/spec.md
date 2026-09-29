# job-server

## ADDED Requirements

### Requirement: Job encapsulation

The interpreter SHALL provide a job server: an embedder SHALL be able to
begin a job and end it, one job at a time, per the server steps of
PLRM3 §3.7.7. Beginning a job SHALL take an outermost save that
snapshots local VM, global VM, and the interpreter's state derived from
objects in them, and SHALL establish the initial state: empty operand
stack, the standard dictionary stack, an empty execution stack, local
allocation mode, packing off, no pending error, and fresh per-job
counters. Ending a job SHALL clear the stacks and, unless the job had
become unencapsulated, restore to the outermost save, so the next job
begins from the same state as this one did. System parameters, the
name table, and identifier counters SHALL NOT be reverted.

#### Scenario: A job's definitions do not reach the next job

- **GIVEN** one interpreter serving a job that runs `/x 1 def true setglobal globaldict /g 2 put false setglobal`
- **WHEN** a second job runs `/x where /g where`
- **THEN** both `where` results are `false`

#### Scenario: A job starts from the initial state

- **GIVEN** one interpreter serving a job that ends with `true setglobal 1 2 3 userdict begin`
- **WHEN** a second job runs `count currentglobal countdictstack`
- **THEN** the stack holds `0`, `false`, and `3`

#### Scenario: An error does not leak

- **GIVEN** one interpreter serving a job that ends with an uncaught `undefined`
- **WHEN** a second job runs `$error /newerror get`
- **THEN** the result is `false`

### Requirement: startjob

`startjob` SHALL be defined per its entry in PLRM3 §8.2. Under a job
server, with a password equal to `StartJobPassword` or
`SystemParamsPassword`, and with save nesting no deeper than at the
start of the current job, it SHALL end the current job (clearing the
stacks and restoring if the job was encapsulated), begin a new job that
is unencapsulated when its boolean operand is `true` and encapsulated
when `false`, continue reading the same input, and push `true`. When any
condition fails it SHALL push `false` and have no other effect. Outside
a job server it SHALL push `false`.

#### Scenario: An unencapsulated download persists

- **GIVEN** one interpreter serving a job that runs `true 0 startjob pop /resident (kept) def`
- **WHEN** a second job runs `resident =`
- **THEN** the output is `kept`

#### Scenario: Back to encapsulation within a file

- **GIVEN** one interpreter serving a job that runs `true 0 startjob pop /kept 1 def false 0 startjob pop /dropped 2 def`
- **WHEN** a second job runs `/kept where exch pop /dropped where`
- **THEN** the stack holds `true` then `false`

#### Scenario: Refused inside a nested save

- **GIVEN** a job running `save true 0 startjob`
- **THEN** the result is `false` and the job continues encapsulated

#### Scenario: Refused with a wrong password

- **GIVEN** a job running `true 1 startjob` on an interpreter with the default passwords
- **THEN** the result is `false`

#### Scenario: Outside a job server

- **GIVEN** a bare interpreter running `true 0 startjob =`
- **THEN** the output is `false`

### Requirement: exitserver under a job server

Under a job server, `exitserver` SHALL behave as `true password
startjob` does, raising `invalidaccess` when that would return `false`,
and on success SHALL write `%%[exitserver: permanent state may be
changed]%%` and a newline to standard output unless `binary` in
`$error` is `true`. Executing it more than once in one input SHALL be
equivalent to executing that many `startjob`s.

#### Scenario: The conventional download

- **GIVEN** one interpreter serving a job fed `serverdict begin 0 exitserver /persist 1 def`
- **THEN** that job's replies are the exitserver line, and a second job's `persist =` prints `1`

#### Scenario: A wrong password

- **GIVEN** a job fed `serverdict begin 1 exitserver`
- **THEN** the error is `invalidaccess` and a second job finds no change

### Requirement: Administrator jobs

A job started by `startjob` or `exitserver` with a password equal to
`SystemParamsPassword` SHALL be a system-administrator job, in which
`setsystemparams` and `setdevparams` are permitted without a
`Password` entry (PLRM3 §C.3.1). The prelude SHALL run as an
administrator job.

#### Scenario: No password needed after the administrator password

- **GIVEN** a job running `true 0 startjob pop << /MaxFontCache 123456 >> setsystemparams`
- **THEN** no error is raised and `currentsystemparams /MaxFontCache get` is `123456` in a later job

### Requirement: Parameters across jobs

User parameters SHALL follow the job's VM: a change made in an
encapsulated job SHALL be reverted at its end, and a change made in an
unencapsulated job SHALL become the default for later jobs. System
parameters SHALL persist across jobs whether or not the job that set
them was encapsulated. `PageCount` SHALL count the pages of every job
the interpreter has served.

#### Scenario: JobName does not outlive its job

- **GIVEN** one interpreter serving a job that runs `<< /JobName (first) >> setuserparams`
- **WHEN** a second job runs `currentuserparams /JobName get length =`
- **THEN** the output is `0`

#### Scenario: PageCount spans jobs

- **GIVEN** one interpreter serving two jobs that each show one page
- **WHEN** a third job runs `currentsystemparams /PageCount get =`
- **THEN** the output is `2`

### Requirement: Pending saves at the end of an unencapsulated job

When an unencapsulated job ends with saves still pending, the job
server SHALL restore to the outermost of them, and such a save SHALL
have snapshotted global VM as well as local, since it is at the outermost
level.

#### Scenario: An unencapsulated job's own save

- **GIVEN** one interpreter serving a job that runs `true 0 startjob pop /a 1 def save /b 2 def`
- **WHEN** a second job runs `/a where exch pop /b where`
- **THEN** the stack holds `true` then `false`

### Requirement: Per-job budget and reset

The execution budget SHALL apply to each job separately: the step
counter, the grace, and the budget outcome SHALL be reset when a job
begins. A job that executes `quit` SHALL end, and the interpreter SHALL
serve the next job.

#### Scenario: Budgets do not accumulate

- **GIVEN** one interpreter with a budget of 100000 serving three jobs that each execute 60000 objects
- **THEN** no job's outcome is the budget

#### Scenario: quit ends a job, not the server

- **GIVEN** one interpreter serving a job that runs `quit`
- **WHEN** a second job runs `(next) =`
- **THEN** the output is `next`

### Requirement: Bounded growth

Over any number of encapsulated jobs, the interpreter's VM and derived
state SHALL return to the size they had before each job. The name table
SHALL be the only structure that grows with the vocabulary of the jobs
served, and SHALL NOT be reclaimed.

#### Scenario: Repeated jobs

- **GIVEN** one interpreter serving a job that defines a thousand procedures and a font, repeated a hundred times
- **THEN** the counts of VM slots and derived-state entries after the hundredth job equal those after the first
