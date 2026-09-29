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
