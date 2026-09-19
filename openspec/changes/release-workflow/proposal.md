# Change: Tag-triggered releases with trusted publishing

## Why

The first release went to the registry by hand: a personal token
created for the occasion, exported in a terminal, nine `cargo publish`
commands typed in dependency order, the token revoked afterwards. That
worked once, but it makes every future release depend on a credential
that must be created, kept away from every agent-operated environment,
and destroyed again, and on a person typing the order correctly. The
registry now supports trusted publishing: a workflow run in this
repository proves its identity to the registry through GitHub's
short-lived identity token and receives a publishing token that lives
for the job and is revoked when it ends. No long-lived credential
exists anywhere. The decision that matters is the shape of the human
act that releases: pushing a version tag, which the repository's hooks
already reserve for people, followed by an approval on a protected
environment before anything is uploaded. Encoding that now, right
after the first release, sets the release path before there is a
second version to ship.

## What Changes

- **A release workflow** (`.github/workflows/release.yml`) triggered
  only by tags `v*`: it checks that the tag names the workspace version
  exactly, runs the public gate chain on the tagged commit, and then,
  in a `release` environment that requires a reviewer's approval,
  obtains a registry token through trusted publishing and publishes the
  nine crates in dependency order, skipping any crate whose version is
  already on the registry so a rerun after a partial failure resumes.
- **The `publication` capability** gains the requirement that releases
  are tag-triggered, version-checked, approval-gated, and use no stored
  registry credential.
- **Registry setup, by the owner**: each crate's trusted publisher on
  crates.io names this repository, the workflow file, and the
  environment; recorded in the design as the one-time setup.
- Out of scope: automated version bumps and changelogs; publishing
  from anything but a tag; the typo-redirect crate.

## Capabilities

### New Capabilities
- none.

### Modified Capabilities
- `publication`: ADDED requirement for tag-triggered, approval-gated
  releases through trusted publishing.

## Impact

- Code: `.github/workflows/release.yml`; no crate changes.
- No new dependencies.
- Depends on `publication-hardening` (archived) and the crates being
  on the registry (they are, at 0.0.1).
