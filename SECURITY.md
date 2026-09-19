<!-- SPDX-FileCopyrightText: 2026 EfterScript contributors -->
<!-- SPDX-License-Identifier: MIT -->

# Security

EfterScript exists to process documents it does not trust. A program in
the PostScript language is code, and the interpreter is designed so that
running one can affect nothing but the output the embedder asked for. A
report that shows otherwise is welcome, and is treated as a defect of the
first order.

## What counts as a vulnerability

- A document that reaches anything it was not handed: a file, the clock,
  the network, the host's fonts, another job's state. Capabilities are
  injected by the embedder; the interpreter must hold no ambient
  authority.
- A panic, abort, or memory fault on any input, however malformed. Every
  parser and decoder is expected to return an error, never crash; the
  fuzz targets exist to keep that true.
- Resource use that a declared limit does not bound: unbounded memory,
  execution that ignores the step budget, output that grows without the
  document growing.
- A produced PDF that carries content the document did not describe, or
  that misrepresents the document's text in extraction.
- Anything in the C interface of the session library that a correct
  caller can misuse into undefined behaviour.

Out of scope: behaviour that differs from another implementation without
one of the effects above (report those as compatibility issues), and
malicious documents that only produce a large or slow but bounded output.

## Reporting

Please report privately rather than in a public issue. Use GitHub's
private vulnerability reporting on this repository (the Security tab,
"Report a vulnerability"), which reaches the maintainers alone. Include
the input that triggers the problem when you can; a minimal program in the
PostScript language is ideal, and the corpus format under `corpus/unit/`
is the easiest form for us to turn into a regression test.

This is a one-person hobby project maintained in spare time, so no
response time can be promised. Reports are read and taken seriously, and
a confirmed problem is fixed as time allows, with credit to the reporter
unless they prefer otherwise. There is no bounty programme.

## Supported versions

The project is pre-1.0: fixes land on the main branch and in the next
published version. There are no maintained older lines.

## Design notes for reviewers

- Library crates contain no unsafe code and no external dependencies; the
  session library's C interface is the one permitted site of unsafe code,
  and it is confined to that module by a compiler attribute.
- The embedder supplies files, the clock, and the output sink as trait
  objects; a build without them has no way to reach the host.
- Deterministic output makes every fix verifiable byte for byte against
  the corpus goldens.
