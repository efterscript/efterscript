## ADDED Requirements

### Requirement: The Emscripten archive is bound to an Emscripten version

A prebuilt session-library archive for the Emscripten target SHALL be
documented as usable only by a program linked with the Emscripten
version the archive names, and the embedding guide SHALL state the
rule and the current pin.

#### Scenario: A mismatched host

- **WHEN** a host pinned to a different Emscripten version fetches the archive
- **THEN** its fetch step can detect the mismatch from the file name before linking
