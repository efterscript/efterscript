## MODIFIED Requirements

### Requirement: The Emscripten archive is bound to an Emscripten version

A prebuilt session-library archive for the Emscripten target SHALL be
documented as usable only by a program linked with the Emscripten
version the archive names and with WebAssembly exception handling
enabled at the link (`-fwasm-exceptions`, the form the Rust target
uses), and the embedding guide SHALL state both rules and the current
pin.

#### Scenario: A mismatched host

- **WHEN** a host pinned to a different Emscripten version fetches the archive
- **THEN** its fetch step can detect the mismatch from the file name before linking

#### Scenario: A link without exception handling

- **WHEN** a host links the archive without `-fwasm-exceptions`
- **THEN** the link fails on the undefined exception tag, and the
  embedding guide names the flag as the remedy
