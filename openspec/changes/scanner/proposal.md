# Change: Scanner

## Why

The scanner turns bytes into `Object` values and is the entry point for
every PostScript program, every `exec` of a string, every `token` call, and
every job arriving over a session. Its position semantics are load-bearing:
font downloads (`eexec`), inline image data, and `currentfile … readline`
all depend on the scanner stopping at exactly the right byte and handing the
stream to an operator. That property is easy to build in and impossible to
retrofit onto a tokenizer that buffers ahead, so it must be designed now,
and it must be hand-written rather than generated.

The scanner is also the first component that can be measured against real
input: a parse-survival run over the corpus and over captured jobs gives a
compatibility signal before any operator exists.

## What changes

- Defines a hand-written, byte-level, resumable scanner core with exact
  stream-position semantics and no I/O of its own.
- Defines the `Source` abstraction the scanner reads from (byte slices,
  string objects, file-table entries, and an incremental session source).
- Defines how procedure bodies, names, strings, and numbers become `Object`
  values allocated through `Memory` in the current VM space.
- Defines error reporting (`syntaxerror`, `limitcheck`, `undefined` for
  immediate names) with source spans, and the DSC-comment observer hook.
- Defers binary token encodings to a later change while reserving their
  lead bytes.

## Impact

- New capability spec: `scanner`.
- Code: `crates/ps-vm` (`scanner` module, `source` module), a fuzz target,
  `corpus/unit/scanner/`, and an `xtask parse-survival` command.
- Depends on `vm-object-model` (names, strings, arrays, `Memory` allocation).
