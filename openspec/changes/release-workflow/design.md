# Design: Tag-triggered releases with trusted publishing

See proposal.md and the spec delta.

## Context

- `ci.yml` runs the public tier on push and pull request; the gate
  chain is the project's local command list. Tags are human-only by
  the repository hooks.
- The registry's trusted publishing lets a GitHub Actions job exchange
  GitHub's OIDC identity token for a short-lived registry token through
  `rust-lang/crates-io-auth-action`, which exposes it as an output and
  revokes it when the job ends; the crate owner registers the
  repository, workflow file, and optional environment on each crate's
  settings page. The job needs `id-token: write`.
- New crate names are rate-limited (one per ten minutes after a burst);
  new versions of existing crates are not, so a release of nine
  existing crates runs without waiting.

## Decisions

**D1. One workflow, two jobs.** `verify` (no secrets, no identity token)
checks the tag against `Cargo.toml`'s workspace version and runs the
same gate steps as `ci.yml`; `publish` needs `verify`, runs in the
`release` environment (configured on GitHub with a required reviewer),
has `id-token: write` and `contents: read`, authenticates with the
action, and publishes. *Alternative:* publish from `ci.yml` on tags —
mixing an unprivileged workflow with a privileged job widens what a
pull request could touch.

**D2. Dependency order with resume.** A shell loop over the nine
names in order; before each `cargo publish -p`, the registry's API is
queried for the version, and an existing version is skipped with a
message; after each publish, the loop waits until the API reports the
version so the next crate's verification finds it. The token is
passed as `CARGO_REGISTRY_TOKEN` from the action's output, never
written to a file.

**D3. The version check is the tag's only input.** `v${version}` must
equal the tag name exactly; the workflow reads `version` from the
workspace table with a small `sed`, as `ci.yml` reads `rust-version`.

**D4. Owner-side setup, once per crate.** On crates.io, each crate's
Settings → Trusted Publishing: publisher GitHub, repository owner
`efterscript`, repository name `efterscript`, workflow filename
`release.yml`, environment `release`. On GitHub: an environment named
`release` with required reviewers. Recorded in the notes; nothing in
the repository holds a credential.

## Risks / Trade-offs

- [The action or the registry's OIDC changes] → pinned to the major
  version; a failure shows at the auth step before any upload.
- [A partial publish] → the resume rule; the crates already up are
  immutable anyway.
- [Approval fatigue] → one click per release is the intended cost.

## Implementation notes

- **As built.** `.github/workflows/release.yml`, two jobs. `verify`
  (permissions `contents: read` only) checks `v${version}` against the
  tag from `GITHUB_REF_NAME`, then runs the same steps as `ci.yml` plus
  `cargo package --workspace`, so a release is refused unless every
  crate packages from the tagged tree. `publish` needs `verify`, runs in
  the `release` environment with `contents: read` and `id-token: write`,
  authenticates with `rust-lang/crates-io-auth-action@v1` and passes its
  `token` output as `CARGO_REGISTRY_TOKEN` to the publish step only; the
  action revokes the token when the job ends. The loop publishes the
  nine crates in dependency order, skipping a crate whose version the
  registry API already reports and waiting (up to five minutes) for
  each new version to appear before the next crate verifies against it.
- **Checked here.** The workflow parses; the skip logic run against the
  live registry with the workspace at 0.0.1 reports all nine as already
  published; the tag check passes for `v0.0.1`; no denylisted string in
  the file. The publish job itself cannot run here (it needs the
  environment and the identity token); its first real run is the first
  tag after 0.0.1.
- **Owner setup (D4), once.** GitHub: Settings → Environments → new
  environment `release`, required reviewers = the maintainer(s), and
  optionally restrict its deployment branches/tags to `v*`. crates.io,
  for each of the nine crates: Settings → Trusted Publishing → add a
  GitHub publisher with repository owner `efterscript`, repository name
  `efterscript`, workflow filename `release.yml`, environment `release`
  (the registry requires the crate's first version to have been
  published manually, which 0.0.1 was). Until every crate has its
  publisher, the loop fails at that crate with an authentication error
  and can be rerun after the setup; crates already up are skipped.
- **Deviation.** None from D1–D4.
