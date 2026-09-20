# cexec-not-an-operator

## Why

`cexec` is defined in `systemdict` as an alias for `exec`. It is not a
PLRM operator: it is a printer extension that hands a string of native
code to the printer's own processor. A driver that wants it probes for
it, and the probe is written to fail:

```
… false <encrypted>{eexec}stopped{dup type/stringtype eq{pop}if}if and …
```

Inside the encrypted section the driver pushes the code string and calls
`cexec`. On an interpreter without the operator that raises `undefined`,
`stopped` returns true, and the guard's cleanup pops the string, leaving
the boolean the following `and` needs. With `cexec` defined as `exec`,
the section succeeds instead, the string stays on the operand stack, and
the `and` fails with `typecheck` — the job ends with no page.

This was found in the field: printing a Finder window from System 7.1
through the emulated LaserWriter produced `typecheck in and`, zero pages,
and a PDF no reader would open. The same driver family on System 6 did
not trip it, which is why the alias survived the change that introduced
it (`printer-identity-mechanism`, whose reasoning — that `exec` of a
literal string pushes it back unharmed — is right about the string and
wrong about what the probe needs).

## What changes

- `cexec` is no longer defined. Executing the name raises `undefined`,
  as the reference converter does, so a driver's probe takes the path it
  was written for.
- The expected-divergence `cexec-defined` is retired: nothing diverges
  any more.
- The identity corpus covers the probe idiom rather than the alias.

## Non-goals

The other accepted device operators (`setscreen` and friends,
`framedevice`) are unaffected. Nothing here changes what a job that
never mentions `cexec` produces.
