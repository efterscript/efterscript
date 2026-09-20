# Design: cexec-not-an-operator

## D1: Remove the name, do not stub it

The alternative to removing `cexec` is defining it to raise `undefined`
explicitly, or to pop its operand and do nothing. Both are worse than
absence: a driver asks `systemdict /cexec known` before it commits to a
native-code path, and only an undefined *name* answers that question
honestly. Removing the table entry gives both the `known` answer and the
`undefined` error for free, with the interpreter's ordinary machinery.

## D2: What the probe needs, from the manual

PLRM3's `eexec` entry (§8.2) is the ground the probe stands on: `eexec`
on a string "creates a new file object that serves as a decryption
filter", executes it, and the section ends at the string's end or at an
explicit `closefile`. Errors inside the section unwind to the enclosing
`stopped` like any others. So the driver's shape —

```
false <encrypted>{eexec}stopped{dup type/stringtype eq{pop}if}if and
```

— has exactly two outcomes. With the operator absent: `undefined`,
`stopped` true, the pushed string discarded by the cleanup, `and` sees
two booleans. With the operator present: no error, `stopped` false, the
cleanup skipped, and the string is still there when `and` runs. The
manual gives no third reading, and `and` (PLRM3: bool bool | int int)
is right to refuse a string.

## D3: The corpus records the idiom, not the operator

`corpus/unit/identity/cexec.ps` asserted the alias. It is replaced by
`cexec-undefined.ps`, which asserts what a driver actually depends on:
the name is unknown, the guarded call stops, the operand count after the
guard, and the error's name and offending command. A future change that
re-defines the name fails that file for the right reason.

## Implementation notes

- **As built.** The `"cexec" => exec` row is gone from
  `ops/control.rs`, with a comment there saying why the absence is
  deliberate; the ops-table test in `ops/mod.rs` now asserts the name is
  *not* found; `("cexec-defined", 1)` is gone from the difftest
  divergence registry; `corpus/unit/identity/cexec.ps` is replaced by
  `cexec-undefined.ps` (no divergence marker).
- **Verified here.** The failing job that prompted this — a Finder
  window printed from System 7.1 through the emulated LaserWriter,
  captured as 40 KB of PostScript — replays clean: before, `typecheck in
  and` and a 317-byte empty PDF; after, no error and a 7,851-byte
  one-page PDF with Helvetica as a Type 1 face. The corpus (46 difftest
  tests), the interpreter's 185 unit tests, formatting, and the string
  lint all pass.
- **A second cause, on the emulator's side.** With `cexec` absent the
  probe works, but the same driver then downloads a native-code path
  anyway, because the printer's `product` began with `LaserWriter` and
  the driver keys that path off exactly that prefix. That is the
  emulator's identity to choose, not this interpreter's behaviour, and
  is fixed there.
