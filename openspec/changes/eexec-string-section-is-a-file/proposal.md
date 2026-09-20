# eexec-string-section-is-a-file

## Why

`eexec` given a **string** ran the decrypted plaintext as a bare string
source, with no file object behind it. `currentfile` looks for the
innermost *file* on the execution stack, so inside such a section it
skipped past and returned the job's own source. The section's
`currentfile closefile` then closed the job.

PLRM3 §8.2 is explicit that both operand forms behave alike: `eexec`
"creates a new file object that serves as a decryption filter on file or
string. It pushes the new file object on the execution stack, making it
the current file", it is "closed automatically when the end of the
original file or string is encountered, or it can be closed explicitly
by `closefile`", and the manual names the very shape this broke — "the
file may consist of encrypted text followed by unencrypted text if the
last thing executed in the encrypted text is `currentfile closefile`".

Printer drivers end their downloads that way, so a job could stop after
its prologue with no error and no page. Found while tracing a System 7.1
LaserWriter driver, whose prologue does exactly this; it stopped being
reachable for that driver once `cexec` was undefined
(`cexec-not-an-operator`), but the defect is general.

## What changes

- A string section's plaintext is opened as a read-only file and run as
  a file source, so `currentfile` returns the section, `closefile` on it
  ends the section alone, and the outer source continues after it — the
  file form's behaviour, which was already correct.

## Non-goals

The decryption itself, the file form, and what a section may do to the
dictionary stack are unchanged.
