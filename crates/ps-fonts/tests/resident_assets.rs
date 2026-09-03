// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The shipped outline assets, read from the data directory as files
//! (so the checks run whether or not the crate embeds them): every TeX
//! Gyre program parses with every glyph interpretable, its committed
//! metric table is exactly what the program yields, every glyph survives
//! subroutine pruning, and every Liberation face parses with names and a
//! Unicode cmap.

use std::path::PathBuf;

use ps_fonts::metrics::MetricTable;
use ps_fonts::type1::write::{
    Header, pruned_subrs, reachable_subrs, renumber, subset_names, write,
};
use ps_fonts::type1::{FileEncoding, parse_file};
#[cfg(feature = "resident-outlines")]
use ps_fonts::{ResidentFace, StdFont};
use ps_fonts::{STANDARD_ENCODING, TrueTypeProgram, Type1Program};

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/outlines")
}

const TEX_GYRE: [(&str, &str); 21] = [
    ("qagr", "TeXGyreAdventor-Regular"),
    ("qagri", "TeXGyreAdventor-Italic"),
    ("qagb", "TeXGyreAdventor-Bold"),
    ("qagbi", "TeXGyreAdventor-BoldItalic"),
    ("qbkr", "TeXGyreBonum-Regular"),
    ("qbkri", "TeXGyreBonum-Italic"),
    ("qbkb", "TeXGyreBonum-Bold"),
    ("qbkbi", "TeXGyreBonum-BoldItalic"),
    ("qcsr", "TeXGyreSchola-Regular"),
    ("qcsri", "TeXGyreSchola-Italic"),
    ("qcsb", "TeXGyreSchola-Bold"),
    ("qcsbi", "TeXGyreSchola-BoldItalic"),
    ("qplr", "TeXGyrePagella-Regular"),
    ("qplri", "TeXGyrePagella-Italic"),
    ("qplb", "TeXGyrePagella-Bold"),
    ("qplbi", "TeXGyrePagella-BoldItalic"),
    ("qzcmi", "TeXGyreChorus-MediumItalic"),
    ("qhvcr", "TeXGyreHerosCondensed-Regular"),
    ("qhvcri", "TeXGyreHerosCondensed-Italic"),
    ("qhvcb", "TeXGyreHerosCondensed-Bold"),
    ("qhvcbi", "TeXGyreHerosCondensed-BoldItalic"),
];

const LIBERATION: [&str; 12] = [
    "LiberationSans-Regular",
    "LiberationSans-Bold",
    "LiberationSans-Italic",
    "LiberationSans-BoldItalic",
    "LiberationSerif-Regular",
    "LiberationSerif-Bold",
    "LiberationSerif-Italic",
    "LiberationSerif-BoldItalic",
    "LiberationMono-Regular",
    "LiberationMono-Bold",
    "LiberationMono-Italic",
    "LiberationMono-BoldItalic",
];

fn tex_gyre_program(file: &str) -> Vec<u8> {
    std::fs::read(data().join(format!("tex-gyre/{file}.pfb"))).unwrap()
}

#[test]
fn every_tex_gyre_program_parses_with_every_glyph_interpretable() {
    for (file, name) in TEX_GYRE {
        let bytes = tex_gyre_program(file);
        let font = parse_file(&bytes).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(font.font_name, name.as_bytes(), "{file}");
        assert_eq!(font.font_matrix, [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
        assert!(matches!(font.encoding, FileEncoding::Custom(_)), "{file}");
        let program = &font.program;
        assert!(program.charstrings().len() > 800, "{file}");
        assert!(!program.subrs().is_empty(), "{file}");
        assert!(program.charstring(b".notdef").is_some());
        let dict = program.dict();
        assert_eq!(dict.paint_type, 0);
        assert!(dict.font_bbox[2] > dict.font_bbox[0]);
        assert!(dict.private_number("StdVW").is_some(), "{file}");
        assert!(dict.private_number("BlueValues").is_some(), "{file}");
        assert_eq!(dict.font_info_bool("isFixedPitch"), Some(false), "{file}");
        assert!(
            dict.private.iter().any(|(k, _)| k == b"OtherSubrs"),
            "{file}"
        );
        assert!(
            !dict
                .private
                .iter()
                .any(|(k, _)| k == b"RD" || k == b"Subrs"),
            "{file}"
        );
        for glyph in program.charstrings().keys() {
            program
                .glyph(glyph)
                .unwrap_or_else(|e| panic!("{file} /{}: {e}", String::from_utf8_lossy(glyph)))
                .expect("listed");
        }
    }
}

/// The glyphs whose charstrings compute a non-integral advance with
/// `div`; the tables carry them rounded, as the metrics files they
/// replaced did.
fn rounded_glyphs(file: &str) -> &'static [&'static str] {
    match file {
        "qbkri" | "qbkbi" => &["tie", "undertie", "undertieinverted"],
        "qcsr" | "qcsb" | "qhvcr" | "qhvcb" => &["hyphen.alt", "hyphendbl.alt"],
        "qcsri" | "qcsbi" | "qhvcri" | "qhvcbi" => &[
            "hyphen.alt",
            "hyphendbl.alt",
            "tie",
            "undertie",
            "undertieinverted",
        ],
        _ => &[],
    }
}

#[test]
fn every_metric_table_regenerates_from_its_program_byte_for_byte() {
    for (file, _) in TEX_GYRE {
        let bytes = tex_gyre_program(file);
        let font = parse_file(&bytes).unwrap();
        let derived = MetricTable::derive(&font).unwrap_or_else(|e| panic!("{file}: {e}"));
        let text = derived.table.render(&format!("{file}.pfb"));
        let committed =
            std::fs::read_to_string(data().join(format!("tex-gyre/{file}.metrics"))).unwrap();
        assert!(
            text == committed,
            "{file}.metrics is not what its program yields; run `cargo xtask fetch-fonts --force`"
        );
        let rounded: Vec<&str> = derived.rounded.iter().map(|(name, _)| *name).collect();
        assert_eq!(rounded, rounded_glyphs(file), "{file}");
        let table = MetricTable::parse(&committed).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(table, derived.table);
        assert_eq!(table.widths().len(), font.program.charstrings().len());
        assert_eq!(table.bbox(), font.program.dict().font_bbox);
        for name in STANDARD_ENCODING.iter().flatten() {
            assert!(table.width(name).is_some(), "{file} lacks /{name}");
        }
    }
}

/// Every glyph of every extra face draws and advances the same after its
/// subroutines are pruned to what it reaches: alone, both renumbered and
/// stubbed in memory, and together with every other glyph through the
/// writer and back through the file parser.
#[test]
fn every_extra_face_survives_subroutine_pruning() {
    for (file, _) in TEX_GYRE {
        let bytes = tex_gyre_program(file);
        let font = parse_file(&bytes).unwrap();
        let program = &font.program;
        for name in program.charstrings().keys() {
            let shown = String::from_utf8_lossy(name);
            let keep = subset_names(program, [name.as_slice()]);
            let reached =
                reachable_subrs(program, &keep).unwrap_or_else(|| panic!("{file} /{shown}"));
            let dense = renumber(program, &reached, &keep)
                .unwrap_or_else(|| panic!("{file} /{shown}: a call operand is not a literal"));
            assert_eq!(dense.subrs.len(), reached.len(), "{file} /{shown}");
            let dense = Type1Program::from_decrypted(4, dense.subrs, dense.charstrings);
            assert_eq!(dense.glyph(name), program.glyph(name), "{file} /{shown}");
            let charstrings = keep
                .iter()
                .filter_map(|n| Some((n.clone(), program.charstring(n)?.to_vec())))
                .collect();
            let stubbed =
                Type1Program::from_decrypted(4, pruned_subrs(program, &reached), charstrings);
            assert_eq!(stubbed.glyph(name), program.glyph(name), "{file} /{shown}");
        }
        let encoding = font.encoding.names();
        let header = Header {
            font_name: &font.font_name,
            font_matrix: font.font_matrix,
            encoding: &encoding,
        };
        let all = subset_names(program, program.charstrings().keys().map(Vec::as_slice));
        let written = write(program, &header, &all);
        let again = parse_file(&written.bytes)
            .unwrap_or_else(|e| panic!("{file}: {e}"))
            .program;
        assert!(again.subrs().len() <= program.subrs().len(), "{file}");
        for name in program.charstrings().keys() {
            assert_eq!(
                again.glyph(name),
                program.glyph(name),
                "{file} /{}",
                String::from_utf8_lossy(name)
            );
        }
    }
}

/// The Palatino scenario's two glyphs embed a Pagella program well under
/// the twenty kilobytes the specification allows: sixteen of the 1762
/// subroutines, renumbered densely.
#[test]
fn a_two_glyph_pagella_subset_is_small() {
    let bytes = tex_gyre_program("qplr");
    let font = parse_file(&bytes).unwrap();
    let program = &font.program;
    let encoding = FileEncoding::Standard.names();
    let header = Header {
        font_name: b"ABCDEF+TeXGyrePagella-Regular",
        font_matrix: font.font_matrix,
        encoding: &encoding,
    };
    let keep = subset_names(program, [&b"P"[..], b"a"]);
    let written = write(program, &header, &keep);
    assert!(
        written.bytes.len() < 20_000,
        "{} bytes",
        written.bytes.len()
    );
    let again = parse_file(&written.bytes).unwrap().program;
    for name in [&b"P"[..], b"a", b".notdef"] {
        assert_eq!(again.glyph(name), program.glyph(name));
    }
    assert_eq!(again.subrs().len(), 16);
}

#[test]
fn every_liberation_face_parses_with_names_and_a_unicode_cmap() {
    for file in LIBERATION {
        let bytes = std::fs::read(data().join(format!("liberation/{file}.ttf"))).unwrap();
        let program = TrueTypeProgram::parse(bytes).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(program.units_per_em(), 2048, "{file}");
        assert!(program.num_glyphs() > 2000, "{file}");
        assert!(program.gid(b"A").is_some(), "{file} has no post names");
        let cmap = program.cmap(3, 1).unwrap().expect("a (3,1) cmap");
        assert_eq!(cmap.get(&0x41), program.gid(b"A").as_ref(), "{file}");
        assert!(cmap.contains_key(&0x20AC), "{file} lacks the euro sign");
        assert_eq!(program.is_fixed_pitch(), file.starts_with("LiberationMono"));
        assert_eq!(
            program.italic_angle() != 0.0,
            file.contains("Italic"),
            "{file}"
        );
    }
}

/// Every glyph name of each Core 14 text face resolves in its Liberation
/// face — by post name or through the Unicode fallback — except the one
/// known gap, and every StandardEncoding name in each TeX Gyre face, so
/// a re-encoding to any name the metrics know has an outline.
#[cfg(feature = "resident-outlines")]
#[test]
fn every_metric_glyph_name_resolves_in_its_outline_asset() {
    for font in StdFont::ALL.into_iter().filter(|f| !f.is_symbolic()) {
        let face = font.face();
        let mut missing = Vec::new();
        for metric in font.metrics().chars() {
            match face.outline(metric.name.as_bytes()) {
                Ok(Some(glyph)) => assert_eq!(glyph.advance.0, f32::from(metric.width)),
                Ok(None) => missing.push(metric.name.to_string()),
                Err(e) => panic!("{} /{}: {e}", face.postscript_name(), metric.name),
            }
        }
        // `commaaccent` (Courier and Times list it) maps to a private-use
        // code point no cmap carries; it draws nothing and keeps its width.
        assert!(
            missing.is_empty() || missing == ["commaaccent"],
            "{} lacks outlines for {}",
            face.postscript_name(),
            missing.join(" ")
        );
    }
    for face in ResidentFace::ALL
        .into_iter()
        .filter(|f| f.std_font().is_none())
    {
        for name in STANDARD_ENCODING.iter().flatten() {
            let glyph = face.outline(name.as_bytes()).unwrap();
            let glyph = glyph.unwrap_or_else(|| panic!("{} lacks /{name}", face.postscript_name()));
            assert_eq!(
                glyph.advance.0,
                f32::from(face.width(name).unwrap()),
                "{} /{name}",
                face.postscript_name()
            );
        }
        assert_eq!(
            face.outlines().unwrap().program().kind(),
            ps_fonts::ProgramKind::Type1
        );
    }
}
