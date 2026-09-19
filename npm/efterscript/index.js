// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT
//
// Placeholder for the WebAssembly build of EfterScript. Until it ships,
// the engine is available as Rust crates: https://crates.io/crates/efterscript

"use strict";

const message =
  "efterscript: the JavaScript/WebAssembly build is not published yet; " +
  "use the Rust crate `efterscript` (https://crates.io/crates/efterscript) " +
  "or watch https://github.com/efterscript/efterscript for the release.";

function notYet() {
  throw new Error(message);
}

module.exports = { distill: notYet, version: "0.0.1", message };
