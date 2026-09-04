# Tasks: oracle-testing

## 1. Harness (tools/difftest)

- [x] 1.1 Profile parsing and location (`EFTERSCRIPT_ORACLE_PROFILE`, `--profile` under the vault, refusal of repo-A paths), skip-with-message when absent; verified by unit tests with temporary profiles and the no-profile scenario
- [x] 1.2 Command runner with placeholders, timeouts, and output under `target/oracle/`; PNM reader; page/media-box/pixel/text comparison; stdout normalisation; verified by unit tests on synthetic PNMs and on a fake profile whose commands are shell scripts written by the test
- [x] 1.3 `% divergence:` header, registry resolution against the spec file, verdict summary and exit status, optional JSON report; verified by the divergence scenarios with a fake profile

## 2. Registry and corpus

- [x] 2.1 `expected-divergences` living spec created through this change; `% divergence: font-substitution` on the substitution corpus files; `corpus/README.md` documents verdicts and the header; verified by `difftest run` unchanged and the fake-profile oracle run resolving the slug

## 3. Lint

- [x] 3.1 `cargo xtask lint-strings` and its vault-gated test; verified against a scratch copy with a planted string and against the repository (clean)

## 4. Vault and triage (private)

- [ ] 4.1 Vault: `oracles/README.md`, `oracles/default.toml`, `oracles/denylist.txt`, `reference-outputs/oracle/` with provenance notes; verified by the lint passing on repo A and the profile loading
- [ ] 4.2 First triage over the full corpus with the private profile; findings recorded in the implementation notes in repo-A vocabulary; no corpus or golden changed to match

## 5. Verification

- [x] 5.1 `cargo test --workspace`, clippy clean, fmt, `difftest run`, `parse-survival`, `openspec validate oracle-testing`; every golden byte-identical; design.md gains "## Implementation notes"
