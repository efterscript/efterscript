// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Shared by the font targets: once a parse succeeded, the glyph
//! interpreter runs over a bounded number of its glyphs, so charstrings
//! and outlines are fuzzed as well as the container. Every result is
//! acceptable.

use efterscript_fonts::Program;

/// Glyphs interpreted per parsed program.
const GLYPHS: usize = 32;

pub fn exercise(program: &Program) {
    for name in program.glyph_names().into_iter().take(GLYPHS) {
        let _ = program.glyph(name);
    }
    for cid in 0..8 {
        let _ = program.glyph_by_cid(cid);
    }
    let _ = (program.glyph_count(), program.units_per_em());
}
