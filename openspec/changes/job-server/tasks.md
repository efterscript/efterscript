# Tasks: job-server

Specification to read first (vault manuals): PLRM3 §3.7.3 (save and
restore) and §3.7.7 (job execution environment), PDF pp. 75–76 and
82–86; Appendix C §C.3.1 (passwords), PDF p. 768; the §8.2 entries for
`startjob` (p. 709), `save` (p. 667), `restore` (p. 662).

## 1. Memory (efterscript-vm)

- [ ] 1.1 The job-level save record over both arenas, its restore keeping both handle counters, and its use for an unencapsulated job's outermost `save` (D2); verified by unit tests in `memory.rs` and the vm-object-model scenario

## 2. Job server (efterscript-vm)

- [ ] 2.1 Group the derived-state tables into `Derived` with no behaviour change (D3); verified by the full test suite and `difftest run` with no golden changed
- [ ] 2.2 `begin_job`, `end_job`, `in_job`, the per-job reset, the pending-save rule, `Derived` in the job save (D1, D3, D4); verified by the Job encapsulation, Pending saves, Per-job budget, and Bounded growth scenarios as Rust tests, and a debug assertion that an empty encapsulated job leaves every table's size unchanged
- [ ] 2.3 `startjob`, `exitserver` under a job server, the exitserver line, administrator jobs, the prelude as an administrator job, both operators' behaviour outside a job server unchanged (D5); verified by the startjob, exitserver, Administrator jobs, and Parameters across jobs scenarios, and the existing `corpus/unit/identity/` files unchanged

## 3. Document over a lent interpreter (efterscript-remelt)

- [ ] 3.1 `Interp::take_graphics_backend`; `Distillation::over`, `finish_keep`; `new` and `finish` rebuilt on them (D6); verified by the remelt scenario and every existing remelt test unchanged

## 4. Printer (efterscript-platen)

- [ ] 4.1 `Printer`, the shared slot, `Printer::job`, `JobError::Busy`, `Finished::permanent`, abandonment on drop, `Job::new` as a one-job printer (D7); verified by the Printer sessions and Abandoned jobs scenarios as Rust tests, the persisted-font scenario checking the second PDF's embedded font, and the existing scenarios unchanged apart from the exitserver line
- [ ] 4.2 `platen_printer_new`, `platen_printer_job`, `platen_printer_free`, abandonment in `platen_job_free`, the header (D8); verified by the ABI scenario, a test freeing in both orders, and the proptest over the one-job path unchanged

## 5. Verification

- [ ] 5.1 The public gate chain (`cargo fmt --check`, clippy with `-D warnings`, `cargo test --workspace`, `difftest run`, `parse-survival`, `fuzz-round`, the no-default-features build, `check-wasm`, `cargo doc`), `openspec validate job-server`; no existing golden changed; `libplaten.a` exports the three new symbols and every old one
- [ ] 5.2 Private tier: `lint-strings`; the captured driver jobs run as a sequence on one printer with the host prelude — a download made permanent in one job used by the next — and the snapshot cost measured
- [ ] 5.3 design.md gains "## Implementation notes" with the tables placed in `Derived`, the measured cost, and the specification pages each part was built from
