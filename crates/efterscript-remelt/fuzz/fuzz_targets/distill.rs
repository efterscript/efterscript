// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Distils arbitrary bytes as a program into an in-memory PDF with the
//! default options, captured output, and an execution budget so every
//! input ends. The report, an error, or a document with no pages are
//! all acceptable; a panic is not.

#![no_main]

use efterscript_remelt::{Options, distill};
use efterscript_vm::{Config, Io, Limits};
use libfuzzer_sys::fuzz_target;

/// Objects executed before `limitcheck` ends the run.
const STEPS: u64 = 20_000;

fuzz_target!(|data: &[u8]| {
    let (io, _out, _err) = Io::capture();
    let config = Config {
        io,
        limits: Limits {
            steps: Some(STEPS),
            ..Limits::default()
        },
        ..Default::default()
    };
    let _ = distill(data, config, &Options::default(), Vec::new());
});
