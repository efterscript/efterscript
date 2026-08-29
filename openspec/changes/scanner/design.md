# Design: Scanner

Token syntax is the PostScript Language Reference's (PLRM3 §3.2 syntax,
§3.3 object attributes as produced by the scanner, §3.8 `token`/`exec`
interaction with files). This document records how EfterScript scans and why.

## 1. Goals and constraints

- **Exact position semantics.** After a token is returned, the source is
  positioned so that an operator can read raw bytes from the same stream:
  `currentfile eexec`, `currentfile 100 string readline`, and inline image
  data after `image` all rely on this. The scanner may hold at most one byte
  of lookahead, and only where the syntax requires it.
- **One scanner, every source.** `exec` on a string, `token` on a string or
  file, `run`, and a session's job stream all use the same core.
- **Resumable.** A session delivers bytes in chunks; the scanner must be able
  to report "need more input" mid-token and continue later without
  re-scanning.
- **Pure core.** No I/O, no host access; bytes come from a `Source`,
  allocations go to `Memory`. Safe to fuzz, deterministic by construction.
- **Traceable.** Every token carries its byte span so marks can later be
  traced to the tokens that drew them.

## 2. Structure

```
Source (trait)     peek() / advance() / position() / more_may_come()
  ├── SliceSource       &[u8]                       tests, cvx/exec on literals
  ├── StringSource      a string Object via Memory  exec / token on strings
  ├── FileSource        a FileTable entry           run, currentfile, token
  └── ChunkSource       append-only buffer          session jobs (platen)

Scanner            state machine + procedure-nesting stack
  next(&mut Source, &mut Memory, &mut dyn Resolver) -> Result<Scan, ScanError>

Scan = Token { object: Object, span: Span } | End | NeedMore
```

`Resolver` answers `//immediate` name lookups (the interpreter supplies the
dictionary stack); it is the scanner's only view of interpreter state.

## 3. Decisions

- **Hand-written byte cursor, no generator or combinator library.**
  PostScript has no grammar above tokens; `{ }` produce objects inside the
  scanner and everything else is executed as read. The exact-position rule
  rules out any tool that tokenizes ahead, and the context-sensitive lexemes
  (nested parentheses, ASCII85, binary lead bytes) would each need escape
  hatches. The scanner is a few hundred lines; the tooling budget goes to
  fuzzing and property tests instead.
- **Terminator handling.** A token ended by whitespace consumes that single
  whitespace byte; a token ended by a delimiter leaves the delimiter in the
  source. This is what makes the byte after `eexec` or `image` the first
  byte of the binary data. Whitespace is space, tab, CR, LF, FF, and NUL;
  CR LF counts as one line end for line counting but as two bytes for
  consumption.
- **Procedures are built inside the scanner.** `{` pushes an accumulator;
  `}` pops it, allocates an executable array in the arena selected by
  `setglobal`, and either returns it (outermost) or appends it to the
  enclosing accumulator. `[`, `]`, `<<`, `>>` are ordinary executable names
  — the interpreter's operators handle them. Nesting depth is bounded by a
  limit that maps to `limitcheck`; an unmatched `}` is `syntaxerror`.
- **Numbers are tried before names.** A token that scans completely as an
  integer, radix integer (`16#ff`, base 2–36), or real (with optional
  exponent) is a number; anything else that starts like a number is a name
  (`123abc`, `-`, `.`, `1e` are names). Integers that overflow `i32` become
  reals; radix integers are unsigned 32-bit patterns reinterpreted as `i32`.
  Reals parse directly to `f32` (correctly rounded), never via `f64`.
- **Strings allocate as they close.** `( )` strings accumulate into a
  scratch buffer with balanced-parenthesis counting and the escape set
  (`\n \r \t \b \f \\ \( \)`, `\ddd` octal of one to three digits, `\` +
  line end as continuation, unknown `\x` yielding `x`); `< >` hex ignores
  whitespace and pads an odd final digit with zero; `<~ ~>` ASCII85 handles
  `z` and partial final groups. Each allocates one string in `Memory` when
  closed; the length limit maps to `limitcheck`.
- **Names are interned at scan time**; executable by default, literal with
  `/`, and `//name` is looked up through the `Resolver` and its value
  substituted (`undefined` if absent). The name-length limit comes from the
  name table.
- **Comments.** `%` to end of line is skipped. Lines beginning `%%` or `%!`
  are additionally offered to an optional DSC observer with their span, so
  page boundaries and resource comments can be tracked without a second
  pass; the observer cannot affect scanning.
- **Spans, not line/column, are the primary position.** A `Span` is a byte
  range in the source; line numbers are derived on demand for diagnostics.
- **Incremental scanning.** Mid-token at end of input, the scanner returns
  `NeedMore` if `more_may_come()` is true and keeps its partial state;
  otherwise end of input inside a string or procedure is `syntaxerror`, and
  a clean end is `End`. `ChunkSource` compacts consumed bytes so a session
  never holds more than the current token plus unread input.
- **Binary encodings are deferred but reserved.** Bytes 128–159 introduce
  binary tokens and binary object sequences; the scanner recognizes the lead
  byte and reports a distinct `ScanError::BinaryEncoding` so the case is
  visible in parse-survival runs. Priority is decided by how often captured
  jobs use them.
- **Errors carry the span and the offending byte.** `ScanError` is a small
  enum with the PostScript error name it maps to; the interpreter turns it
  into errordict machinery.

## 4. Interaction with the VM

- All allocation goes through `Memory` in the current allocation mode; a
  procedure and the strings inside it land in the same space, so the
  global/local rule is not violated by scanning itself. An `//immediate`
  substitution stores through the checked path, so a local value into a
  global procedure raises `invalidaccess` at scan time.
- The scanner never sees the operand stack. `token` on a string is
  implemented by the operator: scan one token from a `StringSource`, then
  narrow the string object to the remainder with `with_interval`.

## 5. Verification

- Unit tests per lexeme class, written from the spec, each mirrored by a
  `corpus/unit/scanner/*.ps` file.
- Property tests: for token streams generated by `psgen`'s seed grammar,
  serialize → scan → compare objects; split input at every byte boundary and
  check chunked scanning yields the same tokens as one-shot.
- A `cargo fuzz` target: arbitrary bytes never panic and never allocate more
  than the input size times a constant.
- `xtask parse-survival`: scan every `.ps` in the corpus and, when
  `EFTERSCRIPT_HELLBOX` is set, every captured job in the private tier;
  report per-file outcome and the histogram of error kinds. This is the
  first compatibility metric of the project.

## 6. Alternatives considered

- **Lexer generators** (`logos`, `pest`, `lalrpop`, tree-sitter) — need a
  complete slice or a parse tree, cannot honour the exact-position rule.
- **Streaming parser combinators** (`winnow`) — the only tool that fits;
  rejected for the core to keep a dependency-free, obviously-correct byte
  loop in the most fuzzed component, without prejudice to using it in tools.
- **Scanner-owned buffers for files** — rejected: the file table owns the
  stream so `readline`, `readstring`, and the scanner share one position.
