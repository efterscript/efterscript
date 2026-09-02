// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The shipped outline assets, read from the data directory as files
//! (so the checks run whether or not the crate embeds them): every TeX
//! Gyre program parses with every glyph interpretable and every AFM
//! width agreeing with its charstring, and every Liberation face parses
//! with names and a Unicode cmap.

use std::path::PathBuf;

use ps_fonts::type1::{FileEncoding, parse_file};
use ps_fonts::{Afm, TrueTypeProgram};
#[cfg(feature = "resident-outlines")]
use ps_fonts::{ResidentFace, StdFont};

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

#[test]
fn every_tex_gyre_program_parses_with_every_glyph_interpretable() {
    for (file, name) in TEX_GYRE {
        let bytes = std::fs::read(data().join(format!("tex-gyre/{file}.pfb"))).unwrap();
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

#[test]
fn every_tex_gyre_afm_parses_and_agrees_with_its_program_on_advances() {
    for (file, name) in TEX_GYRE {
        let text = std::fs::read_to_string(data().join(format!("tex-gyre/{file}.afm"))).unwrap();
        let afm = Afm::parse(&text).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(afm.font_name, name);
        assert!(afm.chars().len() > 800, "{file}");
        let bytes = std::fs::read(data().join(format!("tex-gyre/{file}.pfb"))).unwrap();
        let program = parse_file(&bytes).unwrap().program;
        let mut compared = 0;
        for metric in afm.chars() {
            let Some(glyph) = program.glyph(metric.name.as_bytes()).unwrap() else {
                panic!("{file}: AFM glyph {} has no charstring", metric.name);
            };
            // A few charstrings compute their advance with `div` and the
            // AFM rounds it (Bonum Italic's `tie` is 500.667 against 501).
            assert!(
                (glyph.advance.0 - f32::from(metric.width)).abs() < 1.0,
                "{file} /{}: {} against {}",
                metric.name,
                glyph.advance.0,
                metric.width
            );
            compared += 1;
        }
        assert_eq!(compared, afm.chars().len());
        for name in ps_fonts::STANDARD_ENCODING.iter().flatten() {
            assert!(afm.width(name).is_some(), "{file} lacks /{name}");
        }
    }
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
        for metric in face.metrics().chars() {
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
        for name in ps_fonts::STANDARD_ENCODING.iter().flatten() {
            assert!(
                face.outline(name.as_bytes()).unwrap().is_some(),
                "{} lacks /{name}",
                face.postscript_name()
            );
        }
        assert_eq!(
            face.outlines().unwrap().program().kind(),
            ps_fonts::ProgramKind::Type1
        );
    }
}
