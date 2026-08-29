# scanner

## ADDED Requirements

### Requirement: Token classes

The scanner SHALL produce `Object` values for integers, radix integers,
reals, executable names, literal names, immediate names, literal strings,
hexadecimal strings, ASCII85 strings, and executable arrays (procedures),
and SHALL treat `[`, `]`, `<<`, and `>>` as executable names.

#### Scenario: Numbers

- **GIVEN** the input `123 -7 16#ff 8#17 2#101 1.5 .5 1. -1e3 1E-3`
- **THEN** the tokens are integers 123, -7, 255, 15, 5 and reals 1.5, 0.5,
  1.0, -1000.0, 0.001

#### Scenario: Number-like names

- **GIVEN** the input `123abc - . 1e 16# 37#1`
- **THEN** every token is an executable name

#### Scenario: Integer overflow becomes real

- **GIVEN** the input `2147483648`
- **THEN** the token is a real equal to 2147483648.0

#### Scenario: Names and attributes

- **GIVEN** the input `abc /abc [ ] << >>`
- **THEN** the first token is the executable name `abc`, the second the
  literal name `abc`, and the remaining four are executable names

#### Scenario: Strings

- **GIVEN** the input `(a(b)c) (\101\n) <41 4> <~87cURD]i,"Ebo80~>`
- **THEN** the tokens are literal strings `a(b)c`, `A` followed by newline,
  `A@`, and `Hello World!`

#### Scenario: Procedures

- **GIVEN** the input `{1 {2} add}`
- **THEN** one executable array is produced whose second element is an
  executable array containing the integer 2

### Requirement: Exact position after a token

After returning a token ended by whitespace, the scanner SHALL have consumed
exactly that one whitespace byte; after a token ended by a delimiter it SHALL
have consumed nothing beyond the token.

#### Scenario: Binary data after an operator name

- **GIVEN** the input `eexec\r\n\x80\x01`
- **WHEN** one token is scanned
- **THEN** the token is the name `eexec` and the next unread byte is `\n`

#### Scenario: Delimiter left in place

- **GIVEN** the input `abc(x)`
- **WHEN** one token is scanned
- **THEN** the next unread byte is `(`

### Requirement: Incremental input

When input ends mid-token and the source reports that more may come, the
scanner SHALL return `NeedMore` and SHALL resume correctly when bytes are
appended; the token sequence SHALL be identical to scanning the whole input
at once, for every split point.

#### Scenario: Split inside a string

- **GIVEN** the input `(hello world)` delivered as `(hel` then `lo world)`
- **THEN** the first call returns `NeedMore` and the second returns the
  string `hello world`

### Requirement: Errors

Malformed input SHALL produce a `ScanError` carrying the PostScript error
name and the byte span; the scanner SHALL never panic on any byte sequence.

#### Scenario: Unmatched close brace

- **GIVEN** the input `}`
- **THEN** the result is `syntaxerror`

#### Scenario: Unterminated string at end of input

- **GIVEN** the input `(abc` with no more input to come
- **THEN** the result is `syntaxerror`

#### Scenario: Undefined immediate name

- **GIVEN** the input `//nosuchname` and a resolver that does not define it
- **THEN** the result is `undefined`

#### Scenario: Binary lead byte

- **GIVEN** the input `\x80`
- **THEN** the result is a distinct binary-encoding error, not a panic

### Requirement: Allocation through the VM

Strings and procedures SHALL be allocated in the arena selected by the VM's
current allocation mode, and an immediate-name substitution SHALL go through
the store-time global/local check.

#### Scenario: Global allocation mode

- **GIVEN** the VM in global allocation mode
- **WHEN** `{(a)}` is scanned
- **THEN** both the procedure and the string have the global space bit

### Requirement: DSC observer

Comment lines beginning `%%` or `%!` SHALL be offered to an observer with
their span; scanning SHALL be unaffected by the observer.

#### Scenario: Page comment

- **GIVEN** the input `%%Page: 1 1\n0`
- **THEN** the observer receives `%%Page: 1 1` and the token is the integer 0
