# Design: eexec-string-section-is-a-file

## D1: Give the string a file, rather than teach `currentfile` about strings

`Interp::current_file` walks the execution stack for the innermost file
source and treats a string source as transparent. That rule is right for
every other string source — a procedure body, a `run` of a string — where
`currentfile` genuinely means the enclosing file. Making `eexec`'s string
section the exception would need a second kind of string source and a
special case in the walk.

Opening the plaintext as a file instead makes the two operand forms one
path after decryption, which is what the manual describes: the file form
already produced a file (a decrypting layer over the base), and now the
string form produces one too (the plaintext, already decrypted). The
existing `Eexec` marker takes the handle and closes it when the section
ends however it ends, so no new lifetime rule appears.

## D2: The plaintext is read-only and not positionable

`FileStore::open_bytes` gives a plain forward byte stream, which is what
a source needs; the object carries `ReadOnly` access, as the file form's
layer does. Nothing in the manual asks a string section to be seekable,
and leaving it unseekable keeps `setfileposition` on it an error rather
than a silently different behaviour from the file form, whose layer is
not positionable either.

## D3: What `closefile` then does

`closefile` already distinguishes the job's own source (discard its
remaining input) from any other file (close it). With the section a real
file, the section's `currentfile` hands `closefile` the section, which is
closed; the source frame ends at its next read, and unwinding the marker
pops `systemdict`. The job's file is only reached by `currentfile` when
no section is open, which is the intended meaning.

## Implementation notes

- **As built.** The `Type::String` arm of `eexec` opens the decrypted
  bytes with `open_bytes`, wraps the handle in a read-only file object,
  and begins the section with `SourceSlot::File` and `Some(handle)` —
  the same shape the `Type::File` arm already used. The operator's doc
  comment and a comment at the arm record why.
- **Verified here.** A new corpus file,
  `corpus/unit/fonts/eexec-string-closefile.ps`, runs a string section
  whose plaintext ends in `currentfile closefile` and asserts that the
  job continues afterwards and sees what the section defined; it fails
  before this change (the job ends silently after the section). The
  whole corpus (46 difftest tests) and the workspace suite pass, as do
  formatting, clippy on the interpreter crate, and the string lint.
- **Scope.** Reachable by any job whose prologue uses the manual's
  "encrypted text followed by unencrypted text" shape with a string
  operand. The driver that led here no longer reaches it, because the
  operator its section called is now undefined
  (`cexec-not-an-operator`); this change is what makes the shape work
  for everyone else.
