# Change: job-server — a long-lived interpreter serving encapsulated jobs

## Why

A printer is not a sequence of fresh interpreters; it is one interpreter
serving a sequence of jobs, each started from the same initial state and
reverted to it at the end, except that an authorised job may change that
initial state for every job after it (PLRM3 §3.7.7). Hosts that stand in
for a printer depend on the exception: a driver or utility downloads its
procedure sets and fonts once with `exitserver` or `startjob` and then
sends lean jobs that assume they are resident, and a system utility
changes the device's configuration for the jobs that follow.

`platen` today runs every job in its own interpreter. That model was
chosen deliberately, with a persistent-parent design deferred until a
host needed it; a host now does. Under it `exitserver` changes nothing
beyond the job that called it, `startjob` cannot be defined, and a host
has no way to keep what a real device would keep. Re-sending persistent
downloads from the host does not work either: the host would have to
recognise them in the byte stream and replay them, output and all, in
every later job.

It matters now because the parameter change just made (user parameters
in local VM, system parameters outside it) and the interpreter's
derived-state tables both need a defined place in a job's snapshot, and
the embedding API must not grow a second model later. Doing it once,
at the VM, keeps every host on one job model.

## What Changes

- **A job server in the interpreter** (`efterscript-vm`): `begin_job`
  and `end_job` implement the server steps of PLRM3 §3.7.7 — an
  outermost save of local *and* global VM and of the interpreter's
  derived-state tables; a per-job reset of stacks, allocation mode,
  counters, and error state; and at the end, a restore unless the job
  had become unencapsulated.
- **`startjob`** (Level 2) with its three conditions (under a job
  server, the right password, save nesting no deeper than at the job's
  start) and **`exitserver`** redefined through it, writing the
  conventional `%%[exitserver: permanent state may be changed]%%` line
  on success. Outside a job server `startjob` returns `false` and
  `exitserver` keeps its current behaviour, so the command-line tool
  and a bare interpreter are unchanged.
- **System-administrator jobs** (PLRM3 §C.3.1): a job started with the
  system-parameter password may change system parameters without
  presenting it again.
- **A document over a lent interpreter** (`efterscript-remelt`):
  `Distillation::over` builds a document around an existing interpreter,
  and `finish_keep` closes it and hands the interpreter back.
- **A printer session** (`efterscript-platen`): `Printer::new` builds the
  interpreter once (identity seeded, prelude run as the first
  unencapsulated job); `Printer::job` opens one job at a time over it;
  `Job::feed` and `Job::finish` are unchanged; dropping an unfinished job
  abandons it (the job's changes are reverted, no document). The
  existing `Job::new` remains and means a printer that serves one job.
  The execution budget applies per job.
- **The C ABI grows, compatibly**: `platen_printer_new`,
  `platen_printer_job`, `platen_printer_free`; a printer's job is driven
  by the existing `platen_job_*` functions. `PLATEN_ABI_VERSION` stays
  1, and a host built against it links unchanged.
- **Removed**: the `platen` requirement that nothing persists between
  jobs, replaced by the session requirements.
- Out of scope: several concurrent jobs or contexts; status queries
  answered between jobs (the host composes status); interpreter restart
  (a host frees the printer and builds another); reclaiming the name
  table (recorded as the one structure that only grows).

## Capabilities

### New Capabilities
- `job-server`: job encapsulation, `startjob`, `exitserver` under a job
  server, administrator jobs, parameters across jobs, the per-job
  budget, and what grows over a session.

### Modified Capabilities
- `platen`: REMOVED `Per-job instances`; ADDED the printer session in
  Rust and through the C ABI, job abandonment, and the one-job printer.
- `printer-identity`: MODIFIED `statusdict and serverdict` — what
  `exitserver` does under a job server.
- `remelt`: ADDED a document over a lent interpreter.
- `vm-object-model`: MODIFIED `Save and restore` — the outermost save of
  a job reverts global VM too.

## Impact

- Code: `crates/efterscript-vm` (`memory.rs` global snapshot; the
  derived-state tables grouped on `Interp`; a `job` module with the
  server steps, `startjob`, the new `exitserver`),
  `crates/efterscript-remelt` (`Distillation::over`, `finish_keep`,
  detaching the graphics backend), `crates/efterscript-platen`
  (`Printer`, the job's home slot, `ffi.rs`, `include/platen.h`).
- The platen embedding guide and the package README gain a section on
  printer sessions (documentation, edited directly outside this change).
- Depends on `interpreter-parameters` (the passwords and the parameter
  store) being archived first.
- No new dependencies. Existing C hosts are unaffected until they opt in.
