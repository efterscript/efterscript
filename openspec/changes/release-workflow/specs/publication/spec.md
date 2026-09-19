## ADDED Requirements

### Requirement: Releases are tag-triggered and credential-free

A release SHALL be triggered only by pushing a tag of the form `v<X.Y.Z>`
whose version equals the workspace version, and the workflow SHALL
refuse any other tag. It SHALL run the public gate chain on the tagged
commit before publishing, SHALL publish only after a reviewer approves
the protected release environment, SHALL obtain its registry token
through trusted publishing for the duration of the job with no stored
registry credential, and SHALL publish the crates in dependency order,
skipping a crate whose version the registry already holds so a rerun
resumes.

#### Scenario: A tag that does not match the version

- **WHEN** a tag `v9.9.9` is pushed while the workspace version is `0.0.2`
- **THEN** the workflow fails at its version check and publishes nothing

#### Scenario: A rerun after a partial publish

- **WHEN** the publish job is rerun after three of nine crates reached the registry
- **THEN** those three are skipped as already published and the remaining six are published in order
