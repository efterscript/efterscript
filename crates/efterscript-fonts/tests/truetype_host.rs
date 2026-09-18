// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The TrueType parser against a real font when the host has one: the
//! DejaVu Sans file common on Linux systems. Skips with a message when
//! the file is absent; the file is never copied into the repository.

use std::path::Path;

use efterscript_fonts::{FontError, OutlineOp, TrueTypeProgram};

const DEJAVU: &str = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf";

fn dejavu() -> Option<TrueTypeProgram> {
    let path = Path::new(DEJAVU);
    if !path.is_file() {
        println!("skipped: {DEJAVU} not present");
        return None;
    }
    let bytes = std::fs::read(path).expect("readable");
    Some(TrueTypeProgram::parse(bytes).expect("DejaVu Sans parses"))
}

#[test]
fn dejavu_sans_parses_with_names_metrics_and_outlines() {
    let Some(font) = dejavu() else {
        return;
    };
    assert_eq!(font.units_per_em(), 2048);
    assert!(font.num_glyphs() > 1000);
    assert!(font.ascender() > 0 && font.descender() < 0);
    assert!(!font.is_fixed_pitch());
    assert_eq!(font.italic_angle(), 0.0);
    for tag in [
        b"head", b"hhea", b"maxp", b"hmtx", b"loca", b"glyf", b"post", b"cmap",
    ] {
        assert!(
            font.table(tag).is_some(),
            "{}",
            String::from_utf8_lossy(tag)
        );
    }

    let unicode = font.cmap(3, 1).unwrap().expect("a (3,1) cmap");
    let gid_a = *unicode.get(&u32::from(b'A')).expect("A is mapped");
    assert_eq!(font.gid(b"A"), Some(gid_a), "post names agree with cmap");
    assert_eq!(font.post_name(gid_a), Some(&b"A"[..]));
    assert_eq!(font.post_name(0), Some(&b".notdef"[..]));

    let a = font.glyph(b"A").unwrap().expect("A has a glyph");
    assert!(a.advance.0 > 1000.0 && a.advance.0 < 2048.0);
    let moves = a
        .outline
        .ops
        .iter()
        .filter(|op| matches!(op, OutlineOp::MoveTo(..)))
        .count();
    assert_eq!(moves, 2, "A has an outer contour and a counter");
    let bbox = a.outline.control_box().unwrap();
    assert!(bbox[0] >= 0.0 && bbox[3] > 1000.0 && bbox[1] >= -10.0);
    let space = font.glyph(b"space").unwrap().expect("space");
    assert!(space.outline.is_empty());
    assert!(space.advance.0 > 0.0);

    // Composite glyphs resolve through their components.
    let aacute = font.glyph(b"Aacute").unwrap().expect("Aacute");
    let components = font.components(font.gid(b"Aacute").unwrap()).unwrap();
    assert!(!components.is_empty(), "Aacute is composite in DejaVu Sans");
    assert!(components.contains(&gid_a));
    let accent_moves = aacute
        .outline
        .ops
        .iter()
        .filter(|op| matches!(op, OutlineOp::MoveTo(..)))
        .count();
    assert_eq!(accent_moves, 3);
    assert_eq!(aacute.advance, a.advance);

    assert_eq!(
        font.glyph_by_index(font.num_glyphs()).unwrap_err(),
        FontError::GlyphIndex(font.num_glyphs())
    );
    assert!(font.glyph(b"no-such-glyph").unwrap().is_none());
    // The Macintosh subtable is optional and may be in a format the
    // parser does not read; when it is read it must agree.
    if let Some(mac) = font.cmap(1, 0).unwrap() {
        assert_eq!(mac.get(&u32::from(b'A')), Some(&gid_a));
    }
    assert_eq!(font.cmap(7, 7).unwrap(), None);
}
