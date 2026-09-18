// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Runs arbitrary bytes as a program with captured output and the
//! recording graphics backend the boundary tests use (included by path,
//! since it needs only the crate's public surface), under an execution
//! budget so every input ends. Any outcome — success, an error, a
//! suspension at the end of the bytes — is acceptable; a panic is not.

#![no_main]

#[path = "../../tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use efterscript_vm::{Config, Interp, Io, Limits, SliceSource};
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
    let mut interp = Interp::with_config(config);
    let log = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(common::Recording::new(log)));
    let _outcome = interp.run(&mut SliceSource::new(data));
});
