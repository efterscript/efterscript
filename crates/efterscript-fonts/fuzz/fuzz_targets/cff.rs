// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Parses arbitrary bytes as a CFF font set and interprets some glyphs
//! of its first fonts. A `FontError` is acceptable; a panic is not.

#![no_main]

mod glyphs;

use efterscript_fonts::Program;
use efterscript_fonts::cff::parse_fonts;
use libfuzzer_sys::fuzz_target;

/// Fonts of a set exercised.
const FONTS: usize = 4;

fuzz_target!(|data: &[u8]| {
    if let Ok(programs) = parse_fonts(data) {
        for program in programs.into_iter().take(FONTS) {
            glyphs::exercise(&Program::Cff(program));
        }
    }
});
