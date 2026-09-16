# reusable-streams Specification

## Purpose
Reusable streams: the `ReusableStreamDecode` filter that turns any
data source into a positionable in-memory file, and the file
positioning operators that work on such files.

## Requirements

### Requirement: The ReusableStreamDecode filter

`source [dict] /ReusableStreamDecode filter` SHALL read the whole
source — through the decoding filters named by the dictionary's
`Filter` entry (a name or an array) with their `DecodeParms` — into a
file that can be read repeatedly and positioned, per PLRM3 §3.13.3.
The file SHALL NOT close itself at end of file; `closefile` SHALL
close it and, with `CloseSource` true, the original source. `AsyncRead`
and `Intent` SHALL be accepted (type-checked) and MAY be ignored. The
`Filter` category SHALL list the name.

#### Scenario: A reusable stream is read twice

- **WHEN** `(abc) /ReusableStreamDecode filter` is read to end, `resetfile` is applied, and it is read again
- **THEN** both reads yield `abc` and the file is still open after the first end of file

#### Scenario: Pre-filters decode the source

- **WHEN** an ASCIIHex-encoded string is given with `<< /Filter /ASCIIHexDecode >>`
- **THEN** the stream holds the decoded bytes

### Requirement: File positioning

`fileposition` SHALL return the position of a positionable file,
`setfileposition` SHALL set it within 0 to the length (`rangecheck`
beyond), `resetfile` SHALL set it to 0, `bytesavailable` SHALL return
the length minus the position, and `flushfile` on a reusable stream
SHALL move to the end. On a file that is not positionable,
`fileposition` and `setfileposition` SHALL raise `ioerror`, and
`bytesavailable` SHALL return −1 when the count is unknown.

#### Scenario: Positioning within a stream

- **WHEN** `(hello) /ReusableStreamDecode filter dup 3 setfileposition dup bytesavailable exch 10 string readstring` is executed
- **THEN** the stack holds `2`, `lo`, and `false`

#### Scenario: Positioning an ordinary file

- **WHEN** `currentfile 0 setfileposition` is executed on the job's source
- **THEN** the error is `ioerror`
