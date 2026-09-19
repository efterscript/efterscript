## MODIFIED Requirements

### Requirement: Deterministic output

Distilling the same program with the same options SHALL produce identical
bytes; the file SHALL carry a producer string naming the project and its
version and no timestamp. An embedder MAY ask for a producer naming the
project alone, so that output kept as a reference does not change from one
release to the next; the default names the version.

#### Scenario: Two runs agree

- **GIVEN** any corpus file distilled twice in one process
- **THEN** the two documents are byte-identical

#### Scenario: A golden does not name a version

- **GIVEN** a document distilled with the unversioned producer option
- **THEN** its `Producer` is the project name alone, and the document is
  byte-identical across releases that change nothing else
