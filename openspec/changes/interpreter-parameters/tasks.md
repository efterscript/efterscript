# Tasks: interpreter-parameters

Specification to read first (vault manuals): PLRM3 Appendix C,
§C.1–§C.4 (PDF pp. 759–774); the §8.2 entries for `currentdevparams`
(p. 569), `currentsystemparams` (p. 577), `currentuserparams` (p. 578),
`setdevparams` (p. 681), `setsystemparams` (p. 698), `setuserparams`
(p. 701), `cachestatus` (p. 554), `currentcacheparams` (p. 566),
`setcachelimit` (p. 674), `setcacheparams` (p. 675), `setvmthreshold`
(p. 702), `vmreclaim` (p. 730), `reversepath` (p. 663),
`writehexstring` (p. 734), `status` (p. 710), `echo` (p. 589).

## 1. Parameter store (efterscript-vm)

- [ ] 1.1 The user-parameter dictionary in local VM, allocated and filled at construction (D1, D2); `SystemParams` on `Interp` with the D2 keys and the passwords from `Config::server_password` (D3, D4); a `PageCount` counter advanced by `showpage` and `copypage`; verified by unit tests for the defaults and for `restore` reverting a user parameter and not a system parameter

## 2. Operators (efterscript-vm)

- [ ] 2.1 `ops/params.rs`: `setuserparams`, `currentuserparams` with validate-then-store; verified by the User parameters scenarios as `corpus/unit/params/` files
- [ ] 2.2 `setsystemparams`, `currentsystemparams`, the permission rules including the prelude (D4); verified by the System parameters and Passwords scenarios (the prelude one as a Rust test)
- [ ] 2.3 `setdevparams`, `currentdevparams` raising `undefined` after type checks (D5); verified by the Device parameters scenario
- [ ] 2.4 `cachestatus`, `setcachelimit`, `setcacheparams`, `currentcacheparams`, `setvmthreshold`, `vmreclaim` as views (D6); verified by the Font-cache and VM scenarios
- [ ] 2.5 `exitserver` accepting a string or integer and either password (D4); verified by the printer-identity scenarios, the existing `corpus/unit/identity/exitserver-*.ps` unchanged
- [ ] 2.6 `writehexstring`; `status` for file objects and names; `FileCapability::status` provided method and `FileStatus` (D8); verified by the interpreter-core scenarios and a Rust test with a capability that knows one file

## 3. Graphics (efterscript-graphics)

- [ ] 3.1 `GraphicsBackend::reverse_path` and its implementation; the `reversepath` operator (D7); verified by the graphics-ir scenarios, the twice-reversed IR comparison as a Rust test, and an IR golden for a reversed filled path

## 4. Verification

- [ ] 4.1 The public gate chain (`cargo fmt --check`, clippy with `-D warnings`, `cargo test --workspace`, `difftest run`, `parse-survival`, `fuzz-round`, the no-default-features build, `check-wasm`, `cargo doc`), `openspec validate interpreter-parameters`; no existing golden changed
- [ ] 4.2 Private tier: `lint-strings`; the oracle over `corpus/unit/params/`; the captured setup query and document job from the current-generation driver (vaulted with provenance before implementation starts) run through `platen` with the host prelude, the query answering without error and the document finishing with its pages
- [ ] 4.3 design.md gains "## Implementation notes" with the defaults chosen and the specification pages each operator was built from
