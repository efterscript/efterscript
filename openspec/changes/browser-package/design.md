# Design: The engine as an npm package, and a page to try it

See proposal.md and the spec deltas.

## Context

- The session library (`efterscript-platen`, lib name `platen`) has a
  C interface in `src/ffi.rs`, declared by `include/platen.h`: a job
  created from a `platen_config`, fed bytes, drained of reply and
  error bytes, finished, and read for its PDF and outcome. Unsafe code
  is confined to that module.
- The only WebAssembly build today is the Emscripten archive for the
  paired emulator. The library crates use no threads, file system, or
  time source.
- The npm name `efterscript` holds a placeholder (0.0.1) that throws.
  The release workflow publishes the crates by trusted publishing from
  the `release` environment and attaches the session archives.

## Goals / Non-Goals

**Goals:** a package JavaScript hosts can depend on, with no
dependencies and no build step for its users; a page that shows the
engine working with nothing installed and nothing uploaded; both built
and released from the tagged commit; no change to the crates.

**Non-Goals:** a smaller module (a later change: most of the module is
the resident outlines); a bindings generator; bundler plugins; a
rasteriser to draw the PDF on the page.

## Decisions

**D1. `wasm32-unknown-unknown`, the existing C interface, no bindings
generator.** `cargo rustc -p efterscript-platen --target
wasm32-unknown-unknown --crate-type cdylib` yields a module whose
exports are the `platen_*` functions and `memory` and whose import
list is empty. Everything the wrapper needs is already in the C
interface, so the build adds no Rust code, no unsafe code, and no
dependency. *Alternatives:* a bindings generator would add a
build-time dependency pinned to a matching command-line tool and new
exported glue; the Emscripten target would need its SDK in every
consumer's build and brings a runtime of its own.

**D2. The crate's crate types stay as they are.** `cargo rustc
--crate-type cdylib` overrides them for that one build, so native
builds do not start producing a shared library. Symbols are stripped
(`CARGO_PROFILE_RELEASE_STRIP=symbols`, about 0.35 MB); link-time
optimisation was measured and saves nothing, the module being mostly
font data.

**D3. The wrapper owns a scratch region it grows itself.** The module
exports no allocator. The wrapper calls `memory.grow` once per
instance for a region that holds the configuration struct, identity
strings, the prelude, and a 64 KiB window through which feeds and
drains pass. The allocator inside the module only uses pages it grew
itself, so pages grown from outside are never handed out. Views of
memory are taken afresh after every call because a call may grow
memory. The `platen_config` layout on a 32-bit target is written
field by field (four-byte pointers and sizes, the `u64` budget at
offset 32).

**D4. One instance per job.** `Engine` holds the compiled module;
`job()` instantiates it. Nothing survives a job, which is already the
session library's contract, memory is returned when the job is
dropped, and a trap (a panic aborts on this target) costs only that
job, after which the job refuses further calls. Instantiating takes
about 6 ms.

**D5. The interface.** `load(source?)` compiles once (streaming when
the server sends the wasm media type, from the file system under
Node.js); `convert(program, options)` for the common case;
`job(options)` with `feed`/`finish`/`free` for hosts that receive a
program in pieces. Output is decoded as UTF-8 text; the PDF is a
`Uint8Array`. The default budget is the command-line tool's.
Declarations ship as `efterscript.d.ts`.

**D6. The manifest version is stamped on assembly.** The tracked
`package.json` carries `"version": "0.0.0"`; `cargo xtask
npm-package` writes the workspace version into the assembled copy, so
a version bump stays one edit and the package cannot disagree with the
crates. The assembled package lives in `target/npm/efterscript/`.

**D7. The page.** Static files in `site/`, assembled with the package
under `efterscript/` into `target/site/` by `cargo xtask site`: the
page runs exactly the files that are published. The engine runs in a
module worker that downloads the module with progress and keeps it
compiled; stopping a job terminates the worker and starts another. The
PDF is shown by the browser's own viewer in a frame (a blob URL);
where the browser reports it has none, the page offers open and
download instead. A content security policy limits connections to the
page's origin; inline styles are allowed because a blob document
inherits the policy and the browser's viewer styles itself inline.
Typefaces (Bodoni Moda, IBM Plex Sans and Mono, all OFL-1.1) are
served from the page's origin with provenance in
`site/fonts/PROVENANCE.md`. The samples are the project's own
programs.

**D8. Releases.** `verify` builds the package and runs its tests. A
new `npm` job in the `release` environment (reviewer approval, OIDC
token) builds and tests the package and runs `npm stage publish`, which
authenticates through the registry's trusted publishing, attaches
provenance, and leaves the version staged; a version already live is
skipped. The trusted publisher is configured stage-only, so the
version goes live only when a maintainer approves it on the registry
with two-factor authentication, which no workflow credential can do:
a second human gate, outside GitHub, on top of the environment
approval. Staging needs npm 11.15.0 or later, installed in the job. The
page is built in `pages-build` and deployed by `pages` with the Pages
actions; `pages` runs after `npm` and first waits (up to three hours)
until the registry serves the version, so the page never advertises a
package version npm does not serve, and it runs in one concurrency
group so two deployments cannot overlap. CI builds both on every push.

**D10. Every landing page links to the try-it page.** The README opens
with the link (every crate's registry page shows the README), each
crate manifest's `homepage` is the page, the facade crate's
documentation names it, and so does the top of the npm README, whose
manifest `homepage` is the page as well.

**D9. Notices.** The module contains the fonts crate's data, so the
package ships the licence texts from `LICENSES/` and the repository's
`THIRD-PARTY-NOTICES.md`, which reproduces the BSD-3-Clause notices of
the glyph list and CMaps (the duty the README describes for binaries).
The file lives at the repository root because every binary built with
the fonts crate owes it: the package copies it on assembly, the release
attaches it beside the session-library archives, and the page shows it.

## Risks / Trade-offs

- [A 7 MB download on first use] → loaded in the worker with a progress
  meter, cached by the browser, and trimmed in a later change.
- [The config layout is written by hand in JavaScript] → the ABI
  version field guards it, and the package tests exercise every field.
- [Inline PDF viewers differ between browsers] → the page falls back to
  open and download where there is none.
- [Pages deploys from tags need an environment rule] → recorded as
  owner setup; the job fails visibly until it exists.

## Implementation notes

- **As built.** Module: 11.1 MB, 6.8 MB gzipped, empty import list.
  Through the package, all 343 corpus programs produce the same
  document as the command-line tool except for the producer string
  (the job writer is versioned) and, for one file, the header version:
  the command-line writer is seekable and patches the header to the
  version the program asked for, the job writer is not (existing
  behaviour of the session library, unchanged here). The corpus runs
  about 1.5 times slower than natively; a runaway loop spends the
  default budget in about 21 s.
- The package tests (`npm/efterscript/test/package.test.mjs`, run by
  `node --test` after `cargo xtask npm-package`) cover a page, text
  and font embedding, output and errors, the budget, identity and
  prelude, a malformed identity, feeding in pieces, a program larger
  than the feed window, and loading from bytes and from a compiled
  module.
- The page was checked in a headless browser in light and dark themes
  at desktop and phone widths: the engine loads, the default sample
  converts, the PDF shows in the browser's viewer, and no request
  leaves the origin. The first check showed the viewer broken by the
  policy's `style-src 'self'`, which the blob document inherits; the
  policy now allows inline styles.
- The third-party notices open in a dialog over the page (the link
  itself opens them in a new tab where dialogs are unavailable), so
  reading them never navigates away from a conversion in progress.
  The dialog shows the package's notices and then the page's own
  typefaces (`site/fonts/NOTICE.md`); the OFL text sits beside the font
  files, and the IBM Plex copyright line carries its reserved font name
  as the family's licence file states it. The fonts are Google Fonts'
  latin subsets, which the OFL's FAQ counts as modified versions;
  recorded here because Plex reserves its name, and replaceable by
  IBM's unmodified web fonts if that reading ever matters.
- **Released as 0.0.5** (2026-09-27): nine crates on crates.io, the
  package staged by the workflow and approved on the registry by the
  maintainer, the page deployed once npm served the version, and the
  archives with the notices attached to the GitHub release. One defect
  surfaced: the `attach` job downloaded every artifact of the run, and
  the page's Pages artifact, now built alongside the archives, was
  attached as `artifact.tar` and listed in `SHA256SUMS`. The archive
  artifacts are now named `archive-*` and `attach` downloads only those.
