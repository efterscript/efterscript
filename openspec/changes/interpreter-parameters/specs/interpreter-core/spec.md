# interpreter-core

## ADDED Requirements

### Requirement: writehexstring

`writehexstring` SHALL write each byte of its string operand to the file
as two lowercase hexadecimal digits, with no separators, per its entry
in PLRM3 §8.2, raising `invalidaccess` for a file not open for writing.

#### Scenario: Hex digits on standard output

- **WHEN** `(%stdout) (w) file <41ff0a> writehexstring` is executed
- **THEN** the output is `41ff0a`

### Requirement: status

`status` SHALL be defined per its entry in PLRM3 §8.2. For a file
object it SHALL return whether the file is open. For a string it SHALL
ask the embedder's file capability for the named file's pages, bytes,
reference time, and creation time and return them followed by `true`,
or `false` when the capability knows no such file; without a file
capability, or with one that does not answer, it SHALL return `false`.

#### Scenario: A file object

- **WHEN** `(%stdout) (w) file dup status = closefile` is executed
- **THEN** the output is `true`

#### Scenario: A file name without a file capability

- **WHEN** `(fonts/Anything) status =` is executed with no file capability installed
- **THEN** the output is `false`
