// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The CFF reader against a real program: the `CFF ` table of the TeX
//! Gyre Pagella OpenType file, which `cargo xtask fetch-fonts
//! --test-assets` places under `target/test-fonts/`. Every glyph is
//! interpreted and every advance compared with the committed metric
//! table of the resident Palatino face, which was derived from the same
//! family's Type 1 program. Skips with a message when the file is
//! absent; it is never copied into the repository.

use std::path::PathBuf;

use ps_fonts::cff::CffProgram;
use ps_fonts::truetype::table_directory;
use ps_fonts::{FontError, ResidentFace};

fn asset() -> Option<Vec<u8>> {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "target",
        "test-fonts",
        "texgyrepagella-regular.otf",
    ]
    .iter()
    .collect();
    if !path.is_file() {
        println!(
            "skipped: {} not present (run `cargo xtask fetch-fonts --test-assets`)",
            path.display()
        );
        return None;
    }
    Some(std::fs::read(&path).expect("readable"))
}

#[test]
fn pagella_opentype_interprets_every_glyph_with_the_metric_tables_advances() {
    let Some(bytes) = asset() else {
        return;
    };
    let tables = table_directory(&bytes).expect("an sfnt directory");
    let &(offset, length) = tables.get(b"CFF ").expect("a CFF table");
    let cff = CffProgram::parse(&bytes[offset..offset + length]).expect("the CFF table parses");
    assert!(!cff.is_cid_keyed());
    assert!(cff.glyph_count() > 1000, "{}", cff.glyph_count());
    assert_eq!(cff.font_matrix(), [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
    let bbox = cff.font_bbox();
    assert!(bbox[0] < 0.0 && bbox[3] > 900.0, "{bbox:?}");
    assert!(cff.private().is_some());

    let metrics = ResidentFace::PalatinoRoman.metrics();
    let mut compared = 0;
    let mut errors: Vec<(String, FontError)> = Vec::new();
    for gid in 0..cff.glyph_count() {
        let name = String::from_utf8_lossy(cff.glyph_name(gid).unwrap()).into_owned();
        let glyph = match cff.glyph_by_index(gid) {
            Ok(glyph) => glyph,
            Err(e) => {
                errors.push((name, e));
                continue;
            }
        };
        if let Some(width) = metrics.width(&name) {
            assert_eq!(glyph.advance.0.round() as u16, width, "advance of {name}");
            compared += 1;
        }
    }
    assert!(errors.is_empty(), "{errors:?}");
    assert!(compared > 1000, "{compared} advances compared");

    let a = cff.glyph(b"a").unwrap().expect("a");
    assert_eq!(a.advance.0, 500.0);
    let moves = a
        .outline
        .ops
        .iter()
        .filter(|op| matches!(op, ps_fonts::OutlineOp::MoveTo(..)))
        .count();
    assert_eq!(moves, 2, "a has an outer contour and a counter");
    assert!(cff.glyph(b"no-such-glyph").unwrap().is_none());
    assert!(cff.has_standard_encoding() || cff.encoding()[97].is_some());
    // Subroutine tracing runs over the real program too.
    let reached = cff.reached_subrs(cff.gid(b"a").unwrap()).unwrap();
    assert!(!reached.local.is_empty() || !reached.global.is_empty() || a.outline.ops.len() > 4);
}

#[test]
fn pagella_opentype_subsets_to_two_glyphs_that_round_trip() {
    use ps_fonts::cff::write::{subset, subset_names};
    let Some(bytes) = asset() else {
        return;
    };
    let tables = table_directory(&bytes).expect("an sfnt directory");
    let &(offset, length) = tables.get(b"CFF ").expect("a CFF table");
    let table = &bytes[offset..offset + length];
    let cff = CffProgram::parse(table).expect("the CFF table parses");
    let keep = subset_names(&cff, [&b"P"[..], b"a"]);
    assert_eq!(keep.len(), 3);
    let written = subset(&cff, b"ABCDEF+TeXGyrePagella-Regular", &keep).unwrap();
    let again = CffProgram::parse(&written).expect("the subset parses");
    assert_eq!(again.name(), b"ABCDEF+TeXGyrePagella-Regular");
    assert_eq!(again.glyph_count(), 3);
    assert_eq!(again.charset_names(), vec![&b".notdef"[..], b"P", b"a"]);
    for name in [&b".notdef"[..], b"P", b"a"] {
        let original = cff.glyph(name).unwrap().unwrap();
        let subset = again.glyph(name).unwrap().unwrap();
        assert_eq!(subset.advance, original.advance);
        assert_eq!(subset.outline, original.outline);
    }
    let private = again.private().unwrap();
    let full = cff.private().unwrap();
    assert!(private.subrs.len() < full.subrs.len());
    assert_eq!(
        private.number(ps_fonts::cff::op::STD_VW),
        full.number(ps_fonts::cff::op::STD_VW)
    );
    assert_eq!(again.font_bbox(), cff.font_bbox());
    assert_eq!(
        again.top_string(ps_fonts::cff::op::NOTICE),
        cff.top_string(ps_fonts::cff::op::NOTICE)
    );
    println!(
        "pagella CFF table {} bytes, subset of P and a {} bytes ({} of {} local, {} of {} global subroutines)",
        table.len(),
        written.len(),
        private.subrs.len(),
        full.subrs.len(),
        again.global_subrs().len(),
        cff.global_subrs().len()
    );
    // Every glyph of the whole face survives the writer as well.
    let all = subset_names(&cff, cff.glyph_names());
    let whole = CffProgram::parse(&subset(&cff, b"Whole", &all).unwrap()).unwrap();
    assert_eq!(whole.glyph_count(), cff.glyph_count());
    for gid in 0..cff.glyph_count() {
        let name = cff.glyph_name(gid).unwrap();
        assert_eq!(
            whole.glyph(name).unwrap().unwrap(),
            cff.glyph(name).unwrap().unwrap(),
            "{}",
            String::from_utf8_lossy(name)
        );
    }
}
