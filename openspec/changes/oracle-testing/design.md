# Design: Oracle testing

See proposal.md for the isolation rule and its motivation; the specs
fix the contract. This document fixes the profile format, the
comparison mechanics, and where each piece lives.

## Context

- `difftest run` reads corpus headers, executes files through the
  interpreter with a collecting sink, and compares `.ir`/`.pdf`
  goldens; it already distils to PDF for the sidecar check and honours
  `EFTERSCRIPT_PDF_CHECK` as an opaque command.
- The vault is data-only and discovered through `EFTERSCRIPT_HELLBOX`;
  the public tier must be self-sufficient. The vault has no
  reference-converter material yet.
- Raster comparison needs no PDF reader; raw PNM (`P5`/`P6`) is a
  header and bytes.

## Goals / Non-Goals

**Goals:** a harness that says pass/fail/expected-divergence per corpus
file against any converter a profile describes; zero oracle identity in
repo A; a first honest inventory of mismatches.

**Non-Goals:** semantic PDF parsing; making the harness public-CI
mandatory; fixing what it finds.

## Decisions

**D1. Profile format.** A TOML-like flat text file parsed by hand (no
dependency): `name`, `version`, `ps2pdf = "<cmd> {in} {out}"`, `render
= "<cmd> {in} {out} {dpi}"` (must write one PNM per page with a `%d`
the harness substitutes), `run = "<cmd> {in}"` (executes the file and
prints its standard output), optional `text = "<cmd> {in}"`, `dpi`
(default 36), `threshold` (channel difference, default 48), `limit`
(differing-pixel fraction, default 0.005), `timeout_ms` (default
20000). Commands run through the shell so profiles can pass flags;
placeholders are the harness's only contract. Located by
`EFTERSCRIPT_ORACLE_PROFILE` (a path) or `--profile <name>` resolving to
`$EFTERSCRIPT_HELLBOX/oracles/<name>.toml`. *Alternative:* a fixed
command set — would encode one product's interface in repo A.

**D2. Comparison.** Both PDFs rendered by the profile's rasteriser,
never by two different ones, so rasterisation differences cancel;
anti-aliasing is the profile's concern. Pages compared pairwise after
size check (a size mismatch is a media-box failure); pixel diff on
grey (`P5`) or per channel (`P6`); fraction over the limit fails.
Media box read from our PDF (we wrote it) and from the rendering's
size at the profile's dpi for the converter's. Text comparison
collapses whitespace runs and trims. Standard-output comparison
normalises reals (`12.0` and `12` compare equal; trailing zeros
dropped) and line endings. *Alternative:* structural similarity
metrics — more forgiving of anti-aliasing but a dependency or a
substantial implementation; the tolerance plus shared rasteriser
suffices for a first inventory.

**D3. Isolation mechanics.** Repo A's strings for the oracle are
"reference converter", "reference interpreter", "profile". The
converter's output goes to `target/oracle/<corpus path>/` and is never
staged; the report is printed and optionally written as JSON under
`target/`. The vault gets `oracles/README.md` (install steps, product,
version pin), `oracles/default.toml`, `oracles/denylist.txt` (the
product name, its vendor, its binary name, and the names of any
third-party checker previously used, one per line), and
`reference-outputs/oracle/` for outputs the user chooses to persist.
The devcontainer in repo A stays as it is; the converter is installed
in the private environment by the vault's steps.

**D4. Divergence headers and registry.** `% divergence: <slug>` is a
corpus header like `% expect-output:`; `difftest run` ignores it,
`difftest oracle` resolves it against `openspec/specs/
expected-divergences/spec.md` by requirement name (a simple scan for
`### Requirement: <slug>`); unresolved slugs abort. The registry's first
entry restates the font-substitution decision by reference to the
`text` specification's requirement, which remains the normative
statement.

**D5. Denylist lint.** `cargo xtask lint-strings`: reads the vault's
denylist, walks `git ls-files`, skips binary files (NUL in the first
8 KB), matches case-insensitively, prints `path:line: <masked>` (the
match masked to its first letter so the lint's own output never
contains the string), exits non-zero on any hit; also wired as an
ignored-unless-vault test so `cargo test` runs it in the private tier.

**D6. First triage.** The implementer runs the full corpus with the
private profile and writes the findings into the change's
implementation notes using only repo-A vocabulary: per file, the
verdict, the differing fraction, and a one-line classification. No
corpus file or golden is altered to match the converter; declared
divergences are limited to the existing substitution decision.

## Risks / Trade-offs

- [Raster diffs at 36 dpi miss thin-line differences] → the profile's
  dpi is tunable per run; the output-channel comparison catches VM-level
  differences exactly.
- [Font substitution differs between the two renderings of our
  unembedded standard-14 output] → both PDFs are rendered by the same
  rasteriser with the same substitutes, so the same glyphs appear on
  both sides.
- [A profile author leaks the name into repo A through a corpus header
  or note] → the lint in the private tier, and review.
- [Shell-run profile commands are a code-execution surface] →
  profiles come from the private vault only; the harness refuses a
  profile path inside repo A.

## Open Questions

- Whether persisted oracle outputs should be kept at all; the vault
  directory exists for it, the harness does not need them.
