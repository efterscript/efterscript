// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Parses arbitrary bytes as a Type 1 font file in PFB or PFA form and
//! interprets some of its glyphs. A `FontError` is acceptable; a panic
//! is not.

#![no_main]

mod glyphs;

use efterscript_fonts::Program;
use efterscript_fonts::type1::file::parse_file;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(parsed) = parse_file(data) {
        glyphs::exercise(&Program::Type1(parsed.program));
    }
});
