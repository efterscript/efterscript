# Tasks: browser-package

## 1. The module and the package

- [x] 1.1 `cargo xtask npm-package`: the `cdylib` build for `wasm32-unknown-unknown` (D1, D2) and the package assembly with the stamped version (D6); verified by the module's import and export lists
- [x] 1.2 `npm/efterscript/efterscript.js` and `efterscript.d.ts` per D3–D5; README; the root `THIRD-PARTY-NOTICES.md` and the licence texts per D9, also attached to each release; the placeholder `index.js` removed
- [x] 1.3 `npm/efterscript/test/package.test.mjs`; verified by `node --test` and by converting the whole corpus through the package and comparing with the command-line tool

## 2. The page

- [x] 2.1 `site/` per D7: page, styles, worker, samples, fonts with provenance; `cargo xtask site`; REUSE annotations for the fonts
- [x] 2.2 Checked in a headless browser, both themes, desktop and phone widths
- [x] 2.3 Look and feel approved by the maintainer

## 3. Workflows

- [x] 3.1 CI builds the package, runs its tests, and assembles the page
- [x] 3.2 `release.yml`: `verify` builds and tests the package; `npm` publishes it by trusted publishing; `pages-build` and `pages` deploy the page after `npm`, in one concurrency group (D8); verified by YAML parsing and a review of every step
- [x] 3.4 Links to the page per D10: README top, `homepage` in every crate manifest, the facade crate's documentation, the npm README; verified by `cargo metadata`, `cargo doc`, and `cargo package --workspace`
- [ ] 3.3 Owner setup: npm trusted publisher for `efterscript` (repository `efterscript/efterscript`, workflow `release.yml`, environment `release`); GitHub Pages source set to GitHub Actions; the `github-pages` environment admits `v*` tags

## 4. Gates

- [x] 4.1 fmt, clippy, tests, corpus run, strings lint, `openspec validate browser-package`
