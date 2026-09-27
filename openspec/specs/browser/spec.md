# browser Specification

## Purpose
The engine in the browser and in JavaScript hosts: the `efterscript`
npm package that wraps the session library's WebAssembly module, the
notices it ships, and the try-it page that runs it on the visitor's own
computer.

## Requirements

### Requirement: The npm package wraps the session library's C interface

The `efterscript` npm package SHALL be an ES module with no
dependencies that loads the session library's WebAssembly module and
exposes: `load(source?)`, compiling the module from a URL, a response,
its bytes, or a compiled module, by default from the file beside the
wrapper; `Engine.job(options)`, starting a job in a fresh instance of
the module; `Engine.convert(program, options)`, running a whole
program; and on a job `feed(input)`, returning the standard output and
error-report text produced since the previous call and whether the
program has ended, `finish()`, returning the outcome (`ok`, `error`
with its name and offending command, or `budget`), the PDF bytes, the
page count, and all output, and `free()`. Options SHALL cover the
execution budget (default 100 000 000 objects, `null` for none), the
identity entries, the prelude, page compression, embedding of the
standard fonts, and the server password. A trap inside the module
SHALL fail that job only; a later job SHALL run in a new instance.

#### Scenario: A page through the package

- **WHEN** `convert` runs a program that strokes a line and calls `showpage`
- **THEN** the outcome is `ok`, the page count is 1, and the bytes are a PDF document

#### Scenario: The same document as the command line

- **WHEN** every corpus program is converted through the package and through the command-line tool
- **THEN** the documents agree byte for byte apart from the writer's producer string and the header version the seekable command-line writer patches

#### Scenario: A runaway program

- **WHEN** `convert` runs `{ } loop` with a budget of 10 000
- **THEN** the outcome is `budget`

#### Scenario: A job fed in pieces

- **WHEN** a job is fed `(one) = (tw` and then `o) =` followed by a newline
- **THEN** the first feed returns `one` and a newline, the second `two` and a newline

### Requirement: The package carries its notices

The package SHALL ship the project's licence, the licence texts of the
font data compiled into the module, and the repository's third-party
notice, which reproduces the BSD-3-Clause copyright notices and
conditions of the glyph list and the predefined CMaps; each release
SHALL attach the same notice beside the session-library archives.

#### Scenario: The notice travels with the binary

- **WHEN** the package is assembled
- **THEN** it contains `LICENSE`, `LICENSES/` with each bundled licence text, and `THIRD-PARTY-NOTICES.md`

### Requirement: The try-it page converts on the visitor's computer

The project SHALL publish a static page that converts a program the
visitor edits, picks from samples, opens, or drops, by running the npm
package in a Web Worker on the visitor's computer, and shows the PDF,
its page count, size, conversion time and fonts, the program's output
and error reports, and a download. The page SHALL declare a content
security policy that permits connections to its own origin only, SHALL
serve its typefaces from its own origin, and SHALL let the visitor stop
a running job. Samples SHALL be the project's own programs.

#### Scenario: Converting a sample

- **WHEN** the page has loaded and converted its default sample
- **THEN** the outcome reads finished, the PDF is shown, and no request left the page's origin

#### Scenario: Stopping a job

- **WHEN** a job is running and the visitor stops it
- **THEN** the worker is terminated, the outcome reads stopped, and a new worker loads the engine for the next job

#### Scenario: A browser without an inline PDF viewer

- **WHEN** the browser reports that it cannot show PDFs inline
- **THEN** the sheet says so and the page offers the document to open or download
