// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The CFF reader against synthesised programs of both keyings: the
//! structures the builder writes come back as the builder meant them,
//! and glyphs are found by name, index, and CID.

use ps_fonts::cff::{CffEncoding, CffProgram, Privates, op, parse_fonts};
use ps_fonts::testing::{CffFd, CffFont, Type2Builder, corpus_cff, rectangle};
use ps_fonts::{FontError, OutlineOp, Program, ProgramKind};

fn cff(font: &CffFont) -> CffProgram {
    CffProgram::parse(&font.build()).expect("the builder's program parses")
}

#[test]
fn a_name_keyed_program_round_trips_through_the_builder() {
    let font = corpus_cff();
    let program = font.program().unwrap();
    assert_eq!(program.kind(), ProgramKind::Cff);
    assert_eq!(program.glyph_count(), 5);
    assert_eq!(
        program.glyph_names(),
        vec![&b".notdef"[..], b"a", b"b", b"c", b"f"]
    );
    assert!(program.has_glyph(b"a"));
    assert!(!program.has_glyph(b"z"));
    assert_eq!(program.units_per_em(), None);
    let a = program.glyph(b"a").unwrap().unwrap();
    assert_eq!(a.advance, (600.0, 0.0));
    assert_eq!(
        a.outline.ops,
        vec![
            OutlineOp::MoveTo(50.0, 0.0),
            OutlineOp::LineTo(550.0, 0.0),
            OutlineOp::LineTo(550.0, 500.0),
            OutlineOp::LineTo(50.0, 500.0),
            OutlineOp::Close,
        ]
    );
    let b = program.glyph(b"b").unwrap().unwrap();
    assert_eq!(b.advance, (400.0, 0.0));
    assert_eq!(b.outline.ops.len(), 5);
    let f = program.glyph(b"f").unwrap().unwrap();
    assert_eq!(f.advance, (600.0, 0.0));
    assert_eq!(f.outline.control_box(), Some([0.0, -50.0, 600.0, 200.0]));
    assert!(program.glyph(b"z").unwrap().is_none());
    assert_eq!(
        program.glyph(b".notdef").unwrap().unwrap().advance,
        (0.0, 0.0)
    );

    let Program::Cff(cff) = &program else {
        unreachable!()
    };
    assert_eq!(cff.name(), b"SynCFF");
    assert!(!cff.is_cid_keyed());
    assert_eq!(cff.font_bbox(), [0.0, -50.0, 600.0, 500.0]);
    assert_eq!(cff.font_matrix(), [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
    assert!(!cff.has_font_matrix());
    assert_eq!(
        cff.charset_names(),
        vec![&b".notdef"[..], b"a", b"b", b"c", b"f"]
    );
    assert_eq!(cff.charset(), &[0, 66, 67, 68, 71]);
    assert_eq!(cff.glyph_name(4), Some(&b"f"[..]));
    assert_eq!(cff.gid(b"c"), Some(3));
    assert_eq!(cff.gid_of_cid(3), None);
    assert!(cff.has_standard_encoding());
    assert_eq!(cff.cff_encoding(), &CffEncoding::Standard);
    let encoding = cff.encoding();
    assert_eq!(encoding[97], Some(1));
    assert_eq!(encoding[102], Some(4));
    assert_eq!(encoding[65], None, "A is not in the font");
    let private = cff.private().unwrap();
    assert_eq!(private.default_width_x, 0.0);
    assert_eq!(private.nominal_width_x, 500.0);
    assert_eq!(private.number(op::STD_VW), Some(80.0));
    assert_eq!(private.subrs.len(), 2);
    assert_eq!(cff.global_subrs().len(), 1);
    assert_eq!(
        cff.reached_subrs(2).unwrap().local,
        [0].into_iter().collect()
    );
    assert_eq!(
        cff.reached_subrs(2).unwrap().global,
        [0].into_iter().collect()
    );
    assert!(cff.reached_subrs(1).unwrap().local.is_empty());
    assert_eq!(cff.fd_index(1), None);
    assert_eq!(cff.paint_type(), 0);
    assert_eq!(cff.italic_angle(), 0.0);
    assert!(!cff.is_fixed_pitch());
    assert!(cff.ros().is_none());
    assert!(
        cff.strings().is_empty(),
        "every glyph name is a standard string"
    );
    assert_eq!(cff.charstring(1).unwrap().len(), font.glyphs[1].1.len());
    assert_eq!(cff.charstring(9), Err(FontError::GlyphIndex(9)));
    assert!(format!("{cff:?}").contains("SynCFF"));
    assert!(std::rc::Rc::ptr_eq(
        &cff.glyph_by_index(1).unwrap(),
        &program.glyph(b"a").unwrap().unwrap()
    ));
}

#[test]
fn custom_encodings_strings_and_matrices_come_back() {
    let font = CffFont::new("Enc")
        .widths(250, 500)
        .font_matrix([0.002, 0.0, 0.0, 0.002, 0.0, 0.0])
        .notice("a notice")
        .glyph("square", 600, &rectangle(0.0, 0.0, 500.0, 500.0))
        .glyph("A", 700, &rectangle(0.0, 0.0, 600.0, 600.0))
        .encode(65, "square")
        .encode(66, "A");
    let cff = cff(&font);
    assert!(!cff.has_standard_encoding());
    let encoding = cff.encoding();
    assert_eq!(encoding[65], Some(1));
    assert_eq!(encoding[66], Some(2));
    assert_eq!(encoding[97], None);
    assert!(cff.has_font_matrix());
    assert_eq!(cff.font_matrix(), [0.002, 0.0, 0.0, 0.002, 0.0, 0.0]);
    assert_eq!(cff.top_string(op::NOTICE), Some(&b"a notice"[..]));
    assert_eq!(cff.top_string(op::FULL_NAME), None);
    assert_eq!(cff.strings().len(), 2);
    assert_eq!(cff.sid_name(391), b"a notice");
    assert_eq!(cff.sid_name(392), b"square");
    assert_eq!(cff.sid_name(999), b"");
    assert_eq!(cff.glyph(b"square").unwrap().unwrap().advance.0, 600.0);
    assert_eq!(cff.glyph_by_index(0).unwrap().advance.0, 250.0);
    assert_eq!(cff.charset(), &[0, 392, 34]);
}

#[test]
fn a_cid_keyed_program_selects_private_data_per_glyph() {
    // Two font dictionaries with different widths and subroutines; CID
    // 5 selects the second.
    let first = CffFd {
        subrs: vec![Type2Builder::new().rlineto(100, 0).r#return().bytes()],
        default_width: 100,
        nominal_width: 0,
    };
    let second = CffFd {
        subrs: vec![Type2Builder::new().rlineto(0, 100).r#return().bytes()],
        default_width: 200,
        nominal_width: 1000,
    };
    let glyph = |width: Option<i32>| {
        let mut b = Type2Builder::new();
        if let Some(w) = width {
            b = b.num(w);
        }
        b.rmoveto(0, 0).callsubr(-107).endchar().bytes()
    };
    let font = CffFont::cid_keyed("SynCID", "Adobe", "Identity", 0)
        .fd(first)
        .fd(second)
        .cid_glyph(1, 0, glyph(None))
        .cid_glyph(5, 1, glyph(None))
        .cid_glyph(7, 1, glyph(Some(-500)));
    let cff = cff(&font);
    assert!(cff.is_cid_keyed());
    let ros = cff.ros().unwrap();
    assert_eq!(ros.registry, b"Adobe");
    assert_eq!(ros.ordering, b"Identity");
    assert_eq!(ros.supplement, 0);
    assert_eq!(cff.cid_count(), 8);
    assert_eq!(cff.glyph_count(), 4);
    assert_eq!(cff.charset(), &[0, 1, 5, 7]);
    assert_eq!(cff.gid_of_cid(5), Some(2));
    assert_eq!(cff.gid_of_cid(6), None);
    assert_eq!(cff.fd_index(1), Some(0));
    assert_eq!(cff.fd_index(2), Some(1));
    assert!(cff.charset_names().is_empty());
    assert_eq!(cff.glyph_name(1), None);
    assert!(cff.private().is_none());
    assert!(
        matches!(cff.privates(), Privates::Cid { dicts, select } if dicts.len() == 2 && select == &[0, 0, 1, 1])
    );
    assert_eq!(cff.private_for(2).unwrap().default_width_x, 200.0);
    assert_eq!(cff.encoding(), [None; 256]);
    assert!(
        cff.glyph(b"cid5").unwrap().is_none(),
        "no names in a CID-keyed program"
    );

    let one = cff.glyph_by_cid(1).unwrap().unwrap();
    assert_eq!(one.advance, (100.0, 0.0));
    assert_eq!(one.outline.ops[1], OutlineOp::LineTo(100.0, 0.0));
    let five = cff.glyph_by_cid(5).unwrap().unwrap();
    assert_eq!(five.advance, (200.0, 0.0));
    assert_eq!(five.outline.ops[1], OutlineOp::LineTo(0.0, 100.0));
    let seven = cff.glyph_by_cid(7).unwrap().unwrap();
    assert_eq!(seven.advance, (500.0, 0.0));
    assert!(cff.glyph_by_cid(6).unwrap().is_none());
    assert_eq!(
        cff.reached_subrs(2).unwrap().local,
        [0].into_iter().collect()
    );

    let program = Program::Cff(cff);
    assert_eq!(program.glyph_count(), 4);
    assert!(program.glyph_names().is_empty());
}

#[test]
fn structural_faults_are_reported() {
    let font = corpus_cff();
    let bytes = font.build();
    assert!(matches!(
        CffProgram::parse(&bytes[..bytes.len() - 10]),
        Err(FontError::Truncated(_))
    ));
    assert_eq!(
        CffProgram::parse(&[]).unwrap_err(),
        FontError::Truncated("header")
    );
    let fonts = parse_fonts(&bytes).unwrap();
    assert_eq!(fonts.len(), 1);
    // A predefined expert charset or encoding is refused, recorded as
    // unsupported rather than misread.
    let mut expert = bytes.clone();
    let charset_op = expert
        .windows(6)
        .position(|w| w[0] == 29 && w[5] == op::CHARSET as u8)
        .expect("the fixed charset entry");
    expert[charset_op + 1..charset_op + 5].copy_from_slice(&1i32.to_be_bytes());
    assert_eq!(
        CffProgram::parse(&expert).unwrap_err(),
        FontError::Unsupported("expert charset")
    );
    let font = CffFont::new("E")
        .glyph("a", 1, &rectangle(0.0, 0.0, 1.0, 1.0))
        .encode(97, "a");
    let mut bytes = font.build();
    let encoding_op = bytes
        .windows(6)
        .position(|w| w[0] == 29 && w[5] == op::ENCODING as u8)
        .expect("the fixed encoding entry");
    bytes[encoding_op + 1..encoding_op + 5].copy_from_slice(&1i32.to_be_bytes());
    assert_eq!(
        CffProgram::parse(&bytes).unwrap_err(),
        FontError::Unsupported("expert encoding")
    );
    assert_eq!(
        format!("{}", FontError::Unsupported("expert encoding")),
        "unsupported expert encoding"
    );
}

#[test]
fn the_font_set_file_wraps_the_program() {
    let font = corpus_cff();
    let data = font.build();
    let set = font.font_set("SynSet");
    let head = format!(
        "/FontSetInit /ProcSet findresource begin\n/SynSet {} StartData\n",
        data.len()
    );
    assert!(set.starts_with(head.as_bytes()));
    assert_eq!(&set[head.len()..head.len() + data.len()], &data[..]);
    // The canonical form: nothing follows the data, StartData ends the begin.
    assert_eq!(set.len(), head.len() + data.len() + 1);
    assert!(set.ends_with(b"\n"));
    assert_eq!(font.gid("f"), Some(4));
}
