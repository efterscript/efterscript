## ADDED Requirements

### Requirement: Releases publish the npm package

Every tagged release SHALL build the npm package from the tagged
commit with the workspace version, run its tests under Node.js, and
publish it to the npm registry from the protected `release`
environment through trusted publishing with a provenance statement,
with no stored registry credential; a version the registry already
holds SHALL be skipped so a rerun resumes. Continuous integration
SHALL build the package and run its tests on every push.

#### Scenario: A release reaches npm

- **WHEN** a tag `v<X.Y.Z>` naming the workspace version is pushed and the release environment is approved
- **THEN** `efterscript@<X.Y.Z>` is published with the module built from that commit

#### Scenario: A rerun after the package was published

- **WHEN** the npm job reruns for a version the registry already holds
- **THEN** it reports the version as published and succeeds without publishing

### Requirement: Releases deploy the try-it page

Every tagged release SHALL assemble the try-it page with the package
built from the tagged commit and deploy it to the repository's GitHub
Pages site once the release has published that package to npm; every
crate manifest's `homepage` and the npm manifest's `homepage` SHALL
name the page.

#### Scenario: The page follows the release

- **WHEN** a release's jobs have finished
- **THEN** the page's footer names the released version and its engine is that version's module

#### Scenario: The package publish fails

- **WHEN** the npm job fails for a tagged release
- **THEN** the page is not deployed, and the previous deployment stays live
