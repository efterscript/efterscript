# Change: The engine as an npm package, and a page to try it

## Why

EfterScript's first use case is converting untrusted documents safely,
including in browsers, yet the only way to try it today is to install
a Rust toolchain. The npm name is held by a placeholder that throws.
A browser build removes both gaps at once: a WebAssembly package that
JavaScript hosts, browsers and Node.js alike, can depend on, and a
static page on the project's GitHub Pages site where anyone converts a
program without installing anything, with the conversion running on
their own computer. The page is the package's first consumer, so the
package's interface is exercised before its first real release, and
both are built from the tagged commit by the release workflow so they
can never drift from the crates.

## What Changes

- **The session library builds as a WebAssembly module** for
  `wasm32-unknown-unknown` (`cdylib`, symbols stripped): its existing C
  interface is the module's export list and the module imports
  nothing. No Rust source changes, no new unsafe code, no new
  dependencies.
- **The `efterscript` npm package** (`npm/efterscript/`) replaces the
  placeholder: a hand-written ES module wrapping the C interface
  (`load`, `Engine.convert`, `Engine.job`, `Job.feed`/`finish`/`free`),
  type declarations, a README, the licence texts and third-party
  notices the bundled font data requires, and the module. The manifest
  carries the workspace version, stamped on assembly.
- **The try-it page** (`site/`): an editor with samples, file open and
  drop, a worker running the package, the PDF shown in the browser's
  own viewer on a press sheet, device output, download, and a content
  security policy that allows connections to the page's own origin
  only. Typefaces are served from the page's origin.
- **Workspace tasks** `cargo xtask npm-package` and `cargo xtask site`
  build the module and assemble the package and the page under
  `target/`.
- **Continuous integration and releases.** CI and the release's
  `verify` job build the package and run its tests under Node.js; a
  tagged release publishes the package to npm through trusted
  publishing in the `release` environment, and deploys the page to
  GitHub Pages.
- Out of scope: a smaller module (the resident outlines are most of
  its size; a separate change), showing the vector IR on the page, a
  bundler-specific build, publishing the Emscripten archive to npm.

## Capabilities

### New Capabilities
- `browser`: the npm package's interface and the try-it page.

### Modified Capabilities
- `platen`: ADDED requirement for the WebAssembly module build.
- `publication`: ADDED requirements for publishing the npm package and
  deploying the page on release.

## Impact

- New: `npm/efterscript/{efterscript.js,efterscript.d.ts,
  THIRD-PARTY-NOTICES.md,test/package.test.mjs}`, `site/`,
  `xtask/src/web.rs`. Rewritten: `npm/efterscript/{package.json,
  README.md}`. Removed: `npm/efterscript/index.js`.
- Workflows: `.github/workflows/ci.yml`, `.github/workflows/release.yml`.
- `REUSE.toml` annotations for the page's fonts.
- No crate changes; no new Rust dependencies. The Rust target
  `wasm32-unknown-unknown` and Node.js join the CI toolchain.
- Owner setup: npm trusted publishing for the `efterscript` package,
  GitHub Pages with GitHub Actions as its source, and a deployment
  rule on the `github-pages` environment that admits `v*` tags.
