# Design: job-server

See proposal.md and the spec deltas. This document fixes where the job
server lives, what its snapshot covers, how `startjob` and `exitserver`
map onto it, and how `remelt` and `platen` expose it.

## Context

- PLRM3 §3.7.7 gives the job model: the server's steps for each job,
  encapsulation by an outermost save that covers global as well as
  local VM, `startjob`'s three conditions, `exitserver` as a spelling
  of `true password startjob`, and the automatic restore of pending
  saves at the end of an unencapsulated job. §C.3.1 adds the two
  passwords and the system-administrator job. The §8.2 entries for
  `startjob` (p. 709 of the PDF) and `save`/`restore` fix the details.
- `Memory` keeps local and global VM as persistent maps; `save` clones
  the local arena (constant time) and `restore` swaps it back while
  keeping the handle counter, so handles are never reused and a stale
  object fails to resolve rather than aliasing.
- `Interp` holds, beside `Memory`, tables derived from VM objects:
  font instances and their index, parsed font and CID programs, CMaps,
  pattern instances, CIE spaces, graphics procedures, the resident-font
  cache, defined matrices, the VM-side graphics state, loaded procsets.
  Some hold objects created lazily during a program (resident fonts are
  materialised in global VM on first use).
- `exitserver` sets a flag and trims the dictionary stack; nothing more
  is needed while every interpreter serves one program.
- `remelt::Distillation` owns its interpreter and drops it at `finish`;
  `set_graphics_backend` already supports replacing a backend (it
  clears the per-document font bookkeeping and the VM-side state stack).
- `platen::Job` wraps one `Distillation`; the C ABI (`platen.h`,
  version 1) exposes jobs only.
- `interpreter-parameters` put user parameters in local VM, system
  parameters and the passwords in a struct outside VM, and counts pages
  in the interpreter's life.

## Goals / Non-Goals

**Goals:** the §3.7.7 model exactly, for one channel; no leak between
encapsulated jobs; persistence after `startjob`/`exitserver`; constant-
time job boundaries; unchanged behaviour for every existing embedder.

**Non-Goals:** several contexts or channels; interpreter restart;
status between jobs; name-table reclamation.

## Decisions

**D1. The job server is part of the interpreter.** `Interp` gains
`begin_job(&mut self) -> Result<(), VmError>`, `end_job(&mut self) ->
JobEnd` (whether the job left permanent changes), and `in_job()`.
`begin_job` fails if a job is open. Encapsulation is language
semantics — `startjob` must end and begin jobs from inside the
execution loop — so it cannot live in a front-end. *Alternative:*
`platen` driving `save`/`restore` from outside — `startjob` could not
be defined, and the global-VM and derived-state snapshot would need
access the VM does not expose.

**D2. The outermost save covers both arenas.** `Memory` gains a
job-level save record holding clones of both arenas (each a root
pointer, so constant time) and the file-table watermark; restoring it
swaps both back and keeps both handle counters. The same record kind is
used for a save executed by an unencapsulated job with no save pending
(PLRM3 §3.7.7 note), so its `restore` reverts global VM too. Ordinary
saves are unchanged.

**D3. Derived state is snapshotted with the VM.** The tables listed in
Context move into one `Derived` struct on `Interp`, cloned into the job
save and put back by its restore. Its contents are ids and shared
pointers (`Rc` programs), so the clone is proportional to the entries,
not to what they describe. Outside `Derived`, and never reverted:
system parameters and passwords, the page count, the name table,
identifier counters (font ids, CMap ids) — counters stay monotonic for
the same reason handles do — the file capability, the clock, the I/O
streams, and the configured limits. *Alternative:* pruning dead entries
table by table after a restore — every new table would need its own
rule; one snapshot keeps the rule "derived state follows VM".

**D4. The per-job reset.** `begin_job` (and `startjob`'s new job):
operand, dictionary, and execution stacks to their initial contents;
local allocation mode; packing off; `$error` `newerror` false and no
pending error; the step counter, grace, and budget flag reset (the
budget is per job); `quit` cleared; the font-substitution list and
declared bounding boxes cleared; the job's input reset. The graphics
state is the backend's: a new document installs a new backend (D6).

**D5. `startjob` and `exitserver`.** `startjob` checks, in order: a job
is open (else `false`); the password — a string, or an integer
converted as `cvs` converts it — equals `SystemParamsPassword` or
`StartJobPassword` (else `false`); the save depth equals the job's
starting depth (else `false`). On success it performs `end_job`'s
steps without closing the input, begins a new job with or without the
job save as its boolean says, records an administrator job when the
system-parameter password matched (the permission `interpreter-
parameters` reserved as case (e)), and pushes `true`. `exitserver`
under a job server runs the same path with `true`, raises
`invalidaccess` on `false`, and on success writes the conventional
line to standard output unless `$error /binary` is true. Outside a job
server both keep their prior behaviour (`startjob` pushes `false`;
`exitserver` checks the password and trims the dictionary stack),
which keeps the command-line tool, the corpus, and bare embedders
unchanged. The prelude runs under a job server as an unencapsulated
administrator job whose output is discarded.

**D6. `remelt` lends a document an interpreter.** `Distillation::over
(interp, sink)` does what `new` does after building the interpreter:
seeds the sink's parameters, installs a `Graphics` backend over the
sink. `finish_keep(self)` finishes as `finish` does, then detaches the
backend (`Interp::take_graphics_backend`, new) and returns `(Report, W,
Interp)`. `new` and `finish` become thin wrappers. The graphics
operators stay defined in `systemdict` after the first backend; without
a backend they raise `undefined`, as today.

**D7. `platen::Printer`.** `Printer` holds `Rc<RefCell<Slot>>`, where
the slot holds the interpreter when idle and records an open job
otherwise, plus the writer options and budget. `Printer::new(
JobConfig)` builds the interpreter, runs the prelude (D5), and leaves
it idle. `Printer::job()` takes the interpreter out of the slot
(`JobError::Busy` if absent), calls `begin_job`, lends it to
`Distillation::over` with a new sink, and returns a `Job` that keeps a
handle to the slot. `Job::finish` finishes keeping the interpreter,
calls `end_job`, reports `permanent` in `Finished`, and returns the
interpreter to the slot. Dropping an unfinished `Job` abandons it:
the document is discarded, `end_job` runs, the interpreter goes home.
If the `Printer` has been dropped, the slot's last handle is the job's
and the interpreter is freed with it. `Job::new(config)` is
`Printer::new(config)?.job()`, finishing into a dropped printer.

**D8. The C ABI.** `platen_printer` wraps the `Printer`; `platen_job`
gains an internal variant for a printer's job. New entries:
`platen_printer_new(const platen_config *)` (the same struct; its
`step_budget` is per job), `platen_printer_job(platen_printer *)`
(NULL with `platen_last_error` "a job is open"), `platen_printer_free`.
All existing entries keep their signatures and meaning; `platen_job_free`
on a printer's unfinished job abandons it. Freeing order is free
because both handles share the slot (D7). The version stays 1: the
additions are new symbols, and the configuration struct is unchanged.

**D9. Growth.** VM and `Derived` return to their size at each
encapsulated job's end (restore). The name table only grows: names are
interned forever and shared by every job; the vocabulary of drivers and
applications is small and repetitive, so it is recorded here and not
reclaimed. The scenario in the spec measures the rest.

**D10. Status and stdout.** The exitserver line is the job's standard
output like any reply; a host passes it on. No other message is added;
status text remains the host's.

## Risks / Trade-offs

- [A derived table added later is left out of `Derived`] → the growth
  scenario and a debug assertion comparing table sizes across an empty
  encapsulated job catch it.
- [An object cached in a system-level structure outlives its restore]
  → handles are never reused, so it fails to resolve (`invalidaccess`
  where a pattern already behaves so), never aliases.
- [Snapshot cost of `Derived`] → proportional to entries, which are
  few; measured in the implementation notes on the captured driver jobs.
- [Hosts expecting `exitserver` to print nothing] → only under a
  printer, where the conventional line is what drivers expect.

## Migration Plan

Additive for every existing embedder, with one visible difference: a
job created alone is now served by a job server, so a successful
`exitserver` in it writes the conventional line among its replies, as a
device does; definitions after it still last for the rest of that job.
Hosts opt in to persistence by creating a printer.
`interpreter-parameters` must be archived first.

## Open Questions

- None blocking.

## Implementation notes

Built from PLRM3 §3.7.3 and §3.7.7 (PDF pp. 75–76, 82–86), §C.3.1
(p. 768), and the §8.2 entries for `startjob` (p. 709), `save`
(p. 667), and `restore` (p. 662).

- **Memory (D2).** `SaveRecord` gains an optional global arena;
  `Memory::save_with(depth, global)` takes it and `restore` swaps it
  back keeping the global handle counter. `outlived_by` checks global
  handles too when the record covers global, so a global composite made
  after such a save and left on a stack is `invalidrestore`.
- **Derived state (D3), as built.** The tables stay where they were on
  `Interp`; `interp/job.rs` defines `Derived`, which captures them into
  one struct at a save covering global VM and reinstates them at its
  restore, instead of moving the fields into a nested struct — the same
  rule with no churn in the code that reads them. Captured: the VM-side
  graphics state and its stack, font instances and their index, defined
  matrices, font and CID programs, CMaps, predefined CMaps, pattern
  instances, graphics procedures and their index, CIE spaces and their
  index, the resident-font cache, loaded procsets, the default colour
  rendering, and the no-backend font, screens, transfers, and colour
  rendering. Every live save covering global VM keeps its `Derived` in
  `Interp::derived_saves` by serial; a restore reinstates the matching
  one and drops the ones nested inside. A debug assertion checks that
  an encapsulated job leaves every table's size as it found it; the
  growth test (1000 procedures, a resident font, a global array, 100
  jobs) passes under it. Identifier counters (`next_fid`,
  `next_cmap_id`) are not captured and only grow. Table indices (font
  instance, pattern, procedure, and CIE ids) can recur after a job
  reverts its entries, which is safe because each job has its own
  document and graphics state.
- **Save and restore (D1).** `save` and `restore` now go through
  `Interp::vm_save` and `vm_restore`, which the job server shares. A
  `save` covers global VM when `save_covers_global` holds (an
  unencapsulated job with no save pending).
- **A defect found and fixed during implementation:** a restore covering
  global VM took back the graphics operators the backend had entered in
  `systemdict` during the job, so drawing after an `exitserver` in the
  same job raised `undefined` for `moveto`. `vm_restore` re-enters them
  when such a restore happens with a backend installed, and
  `set_graphics_backend` enters them on every installation, not only
  the first. A `platen` test covers drawing after `exitserver`.
- **The reset (D4)** also clears the execution stack when a job begins
  and when it ends (an abandoned job leaves suspended frames), and sets
  `$error /newerror` false.
- **`startjob` (D5).** The operator pops its operands before deciding,
  then pushes the result. Besides the three conditions, a job whose
  restore something on the execution stack outlived (a procedure made
  during the job) returns `false`, since the restore would be
  `invalidrestore`. The execution stack is otherwise left as it is: the
  input continues from where it was. `exitserver` reads `$error
  /binary` before ending the job, because ending it restores `$error`.
- **Administrator jobs.** `params::permitted` asks `Interp::admin_job`,
  which is true while the prelude runs or in a job started with the
  system-parameter password. With the default configuration both
  passwords are the same, so `exitserver 0` starts an administrator
  job.
- **`remelt` (D6).** `Distillation::over`, `finish_keep`, and a new
  `abandon` (the document discarded, the interpreter returned);
  `Interp::take_graphics_backend` removes the backend.
- **`platen` (D7, D8).** `Printer` and `Job` share a slot
  (`Rc<RefCell<Slot>>`) holding the interpreter while idle, the writer
  options, and the capture streams; `Drop` on an unfinished `Job`
  abandons it. `Finished::permanent` reports a permanent job. A job
  whose document cannot be closed takes the interpreter with it (writing
  to memory does not fail). `ffi.rs` adds `platen_printer_new`,
  `platen_printer_job`, and `platen_printer_free`; the header documents
  them; the library exports the three new symbols and every earlier
  one.
- **Tests adjusted.** The split-feeding property test now compares
  against a whole-program distillation over an interpreter serving one
  job, since jobs report the encapsulating save level; two one-shot
  scenarios expect the exitserver line. The `job-server-save-level`
  divergence is reworded: printers' jobs report the level, direct runs
  do not.
- **Private tier.** The captured driver jobs (a current-generation
  driver's setup query, print query, and two-page document job, kept in
  the private vault) run in sequence on one printer through the C ABI in
  578-byte pieces with the host's stock prelude, twice over and then
  after a job that makes a procedure resident with `exitserver` and a
  job that uses it: every job ends `ok` and each document job has its
  two pages. Opening a job costs 0.03–0.16 ms; finishing and ending one
  costs 0.02–2.8 ms, most of it closing the document.
