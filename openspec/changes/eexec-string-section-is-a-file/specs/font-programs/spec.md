## MODIFIED Requirements

### Requirement: eexec decrypts and executes

`eexec` SHALL take a file or string, decrypt its contents with the
standard `eexec` key in hexadecimal or binary form (detected from the
first four bytes), discard the four leading bytes, and execute the
plaintext as a source with `systemdict` pushed on the dictionary stack
for its duration. The section SHALL be a file object whichever operand
form began it: inside it `currentfile` SHALL return that file,
`readstring` and `token` SHALL read decrypted bytes, and `closefile` on
it SHALL end the section, pop `systemdict`, and resume the enclosing
source — for a file operand, positioned after the last byte the layer
consumed.

#### Scenario: A hexadecimal eexec section

- **GIVEN** a program whose `eexec` section, when decrypted, is
  `userdict /x 42 put mark currentfile closefile`, followed by zeros and
  `cleartomark x =`
- **THEN** the output is `42`

#### Scenario: A binary eexec section

- **GIVEN** the same section in binary form
- **THEN** the output is `42`

#### Scenario: eexec on a string

- **GIVEN** a string holding an encrypted `(hi) print`
- **WHEN** `eexec` is applied to it
- **THEN** the output is `hi`

#### Scenario: A string section closes only itself

- **GIVEN** a string whose plaintext is
  `userdict /inside 1 put currentfile closefile`, executed by `eexec`
  between two statements of the job
- **THEN** the statement after the section runs and `inside` is `1` —
  the section's `closefile` ended the section, not the job
