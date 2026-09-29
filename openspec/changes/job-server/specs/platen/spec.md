# platen

## REMOVED Requirements

### Requirement: Per-job instances

**Reason**: A printer keeps what an authorised job leaves behind; one
interpreter per job cannot. Replaced by `Printer sessions`, under which
jobs are still isolated from one another unless a job uses `startjob`
or `exitserver`.

**Migration**: `Job::new` and `platen_job_new` keep their behaviour — a
printer that serves one job — so existing hosts are unaffected. A host
that wants persistence creates a `Printer` (`platen_printer_new`) and
opens its jobs from it.

## ADDED Requirements

### Requirement: Printer sessions

A printer SHALL be created from the same configuration a job takes
(identity entries, prelude, server password, writer options, execution
budget); creating it SHALL build one interpreter, seed the identity,
and run the prelude as the first unencapsulated job. The printer SHALL
open one job at a time over that interpreter; opening a second while
one is open SHALL fail with a busy error. Each job SHALL be fed and
finished as a job created alone is, SHALL produce its own document,
and SHALL begin in the job server's initial state (see `job-server`),
so jobs are isolated unless one uses `startjob` or `exitserver`. The
budget SHALL apply to each job. `Finished` SHALL report whether the job
changed the printer's initial state. A job created alone SHALL be a
printer serving that one job.

#### Scenario: A download persists across jobs

- **GIVEN** a printer, a first job fed `serverdict begin 0 exitserver /x 1 def` and finished, and a second job fed `x =` and finished
- **THEN** the second job's replies are `1` and a newline, and the first job's `Finished` reports a changed initial state

#### Scenario: Encapsulated jobs are isolated

- **GIVEN** a printer, a first job fed `/x 1 def` and finished, and a second job fed `x`
- **THEN** the second job's outcome names `undefined`

#### Scenario: A persisted font is embedded in a later document

- **GIVEN** a printer, a first job that defines a Type 1 font after `exitserver`, and a second job that shows text in it
- **THEN** the second job's PDF embeds that font's program, subset to the glyphs shown

#### Scenario: One job at a time

- **GIVEN** a printer with a job open
- **WHEN** a second job is requested
- **THEN** the request fails with the busy error and the open job is unaffected

#### Scenario: The prelude runs once

- **GIVEN** a printer whose prelude runs `userdict /runs known { /runs runs 1 add def } { /runs 1 def } ifelse`
- **WHEN** three jobs each run `runs =`
- **THEN** each prints `1`

### Requirement: Abandoned jobs

A job dropped or freed before `finish` SHALL be abandoned: the job
server SHALL end it as at an end of data after an error — reverting it
if it was encapsulated, keeping what it had already made permanent if
not — and no document SHALL be produced. The printer SHALL then accept
a new job.

#### Scenario: A connection lost mid-job

- **GIVEN** a printer, a first job fed `/x 1 def` and dropped, and a second job fed `/x where =`
- **THEN** the second job prints `false`

### Requirement: Printer sessions through the C ABI

The C header SHALL declare `platen_printer_new` (taking the job
configuration struct), `platen_printer_job` (returning a job driven by
the existing `platen_job_*` functions, or NULL with `platen_last_error`
explaining when a job is open), and `platen_printer_free`.
`platen_job_free` on a printer's unfinished job SHALL abandon it. The
printer and its jobs SHALL be freeable in either order. The existing
functions and `PLATEN_ABI_VERSION` 1 SHALL be unchanged, so a host built
against the earlier header links and behaves as before.

#### Scenario: Two jobs through the ABI

- **GIVEN** the download scenario driven through the C functions from a test, freeing the printer before the second job
- **THEN** the second job's reply bytes are `1` and a newline, and freeing the job afterwards is valid
