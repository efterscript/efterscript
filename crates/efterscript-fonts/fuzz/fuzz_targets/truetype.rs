// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Parses arbitrary bytes as a TrueType program and interprets some of
//! its glyphs. A `FontError` is acceptable; a panic is not.

#![no_main]

mod glyphs;

use efterscript_fonts::{Program, TrueTypeProgram};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(program) = TrueTypeProgram::parse(data.to_vec()) {
        glyphs::exercise(&Program::TrueType(program));
    }
});
