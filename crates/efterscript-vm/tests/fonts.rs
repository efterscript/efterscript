// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Fonts the job defines, through the recording backend: a Type 1
//! program behind `eexec` and a Type 42 program in `sfnts` measure with
//! their own advances, show as runs, and outline through `charpath`.
//! The printed-output scenarios are corpus files under
//! `corpus/unit/fonts`, generated from the same synthesised fonts.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Call, Log, Recording};
use efterscript_fonts::Program;
use efterscript_fonts::testing::{CharstringBuilder, TrueTypeFont, Type1Font, rectangle};
use efterscript_vm::{Config, Glyph, Interp, Io, Outcome, Point, SliceSource};

struct Run {
    interp: Interp,
    outcome: Outcome,
    output: String,
    log: Log,
}

impl Run {
    fn calls(&self) -> Vec<Call> {
        self.log.borrow().clone()
    }

    fn error(&self) -> Option<&str> {
        match &self.outcome {
            Outcome::Error(summary) => Some(&summary.name),
            _ => None,
        }
    }

    fn command(&self) -> Option<&str> {
        match &self.outcome {
            Outcome::Error(summary) => Some(&summary.command),
            _ => None,
        }
    }

    fn shows(&self) -> Vec<Vec<Glyph>> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Show(glyphs) => Some(glyphs),
                _ => None,
            })
            .collect()
    }

    fn path_calls(&self) -> Vec<Call> {
        self.calls()
            .into_iter()
            .filter(|c| {
                matches!(
                    c,
                    Call::MoveTo(_) | Call::LineTo(_) | Call::CurveTo(..) | Call::ClosePath
                )
            })
            .collect()
    }

    fn top_numbers(&self, n: usize) -> Vec<f32> {
        let stack = self.interp.ostack();
        stack[stack.len() - n..]
            .iter()
            .map(|o| o.as_number().expect("number"))
            .collect()
    }
}

fn exec(program: &str) -> Run {
    let (io, out, _) = Io::capture();
    let config = Config {
        io,
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Recording::new(log.clone())));
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    Run {
        interp,
        outcome,
        output: out.text(),
        log,
    }
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

fn point_approx(p: Point, x: f32, y: f32) -> bool {
    approx(p.x, x) && approx(p.y, y)
}

/// The synthesised Type 1 font of the corpus: `a` is a square of advance
/// 600, `e` and `acute` compose `eacute`, and `b` is malformed.
fn syn() -> Type1Font {
    let e = CharstringBuilder::new()
        .hsbw(20, 500)
        .rmoveto(0, 0)
        .rlineto(400, 0)
        .rlineto(0, 400)
        .closepath()
        .endchar()
        .bytes();
    let acute = CharstringBuilder::new()
        .hsbw(30, 300)
        .rmoveto(0, 500)
        .rlineto(100, 100)
        .closepath()
        .endchar()
        .bytes();
    let eacute = CharstringBuilder::new()
        .hsbw(20, 500)
        .seac(30, 150, 20, 101, 194)
        .bytes();
    let malformed = CharstringBuilder::new().hsbw(0, 400).num(1).bytes();
    Type1Font::new("Syn")
        .bbox([0, 0, 750, 750])
        .glyph("a", 600, &rectangle(50.0, 0.0, 550.0, 500.0))
        .charstring("b", malformed)
        .charstring("e", e)
        .charstring("acute", acute)
        .charstring("eacute", eacute)
        .encode(97, "a")
        .encode(98, "b")
        .encode(101, "e")
        .encode(233, "eacute")
}

fn syn_tt() -> TrueTypeFont {
    TrueTypeFont::new(2048)
        .glyph(
            "a",
            1024,
            vec![vec![
                (0, 0, true),
                (1000, 0, true),
                (1000, 1000, true),
                (0, 1000, true),
            ]],
        )
        .glyph(
            "o",
            1200,
            vec![vec![
                (100, 500, true),
                (600, 1000, false),
                (1100, 500, true),
                (600, 0, false),
            ]],
        )
        .map(97, 1)
        .map(111, 2)
}

fn with_syn(program: &str) -> String {
    format!("{}{program}", syn().pfa())
}

fn with_syn_tt(program: &str) -> String {
    format!(
        "{}{program}",
        syn_tt().type42("SynTT", &[(97, "a"), (111, "o")])
    )
}

// --- Type 1 -----------------------------------------------------------------------

#[test]
fn a_type1_program_defines_a_font_that_measures_with_its_charstrings() {
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont (aa) stringwidth \
         /Syn findfont /CharStrings get length \
         /Syn findfont /Private get { /lenIV get } stopped",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    let top: Vec<f32> = stack[..3]
        .iter()
        .map(|o| o.as_number().expect("number"))
        .collect();
    assert!(approx(top[0], 12.0) && approx(top[1], 0.0), "{top:?}");
    assert_eq!(top[2], 6.0);
    // The private dictionary is sealed: the program cannot read lenIV,
    // the snapshot could.
    assert_eq!(stack[stack.len() - 1].as_bool(), Some(true));
    assert!(run.calls().iter().all(|c| !matches!(c, Call::Show(_))));
}

#[test]
fn a_type1_show_is_one_run_of_charstring_advances() {
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont 100 100 moveto (aa) show currentpoint",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let shows = run.shows();
    assert_eq!(shows.len(), 1);
    assert_eq!(shows[0].len(), 2);
    assert!(
        shows[0]
            .iter()
            .all(|g| g.code == 97 && approx(g.dx, 600.0) && g.dy == 0.0)
    );
    let top = run.top_numbers(2);
    assert!(approx(top[0], 112.0) && approx(top[1], 100.0), "{top:?}");
}

#[test]
fn seac_glyphs_carry_the_composite_advance_and_both_outlines() {
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont 0 0 moveto /eacute glyphshow currentpoint \
         (\\351) stringwidth",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let shows = run.shows();
    assert_eq!(shows.len(), 1);
    assert_eq!(shows[0][0].code, 233);
    assert!(approx(shows[0][0].dx, 500.0));
    let top = run.top_numbers(4);
    assert!(approx(top[0], 5.0) && approx(top[2], 5.0), "{top:?}");

    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont 0 0 moveto (\\351) false charpath",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let path = run.path_calls();
    let moves: Vec<Point> = path
        .iter()
        .filter_map(|c| match c {
            Call::MoveTo(p) => Some(*p),
            _ => None,
        })
        .collect();
    // The program's own moveto, the base, the accent displaced by
    // (sbx − asb + adx, ady) = (140, 20), and the advance to the end of
    // the run.
    assert_eq!(moves.len(), 4, "{path:?}");
    assert!(point_approx(moves[1], 0.2, 0.0));
    assert!(point_approx(moves[2], 1.7, 5.2));
    assert!(point_approx(moves[3], 5.0, 0.0));
}

#[test]
fn a_malformed_charstring_is_invalidfont_from_the_operator() {
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont 0 0 moveto (b) show",
    ));
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("show"));
    assert!(run.shows().is_empty());
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont (ab) stringwidth",
    ));
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("stringwidth"));
}

#[test]
fn glyphs_the_program_lacks_fall_back_to_notdef() {
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont 0 0 moveto (z) show currentpoint \
         (za) false charpath currentpoint",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let shows = run.shows();
    assert_eq!(shows.len(), 1);
    assert_eq!(shows[0][0].code, 122);
    assert_eq!(shows[0][0].dx, 0.0);
    let top = run.top_numbers(4);
    assert!(approx(top[0], 0.0) && approx(top[2], 6.0), "{top:?}");
    let describes = run
        .calls()
        .iter()
        .filter(|c| matches!(c, Call::SetFont(_)))
        .count();
    assert_eq!(describes, 1);
}

#[test]
fn a_program_is_built_once_per_font_and_missing_programs_are_invalidfont() {
    let program = with_syn(
        "/Syn findfont 10 scalefont setfont (a) stringwidth pop pop \
         /Syn findfont 20 scalefont setfont (a) stringwidth",
    );
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert!(approx(run.top_numbers(2)[0], 12.0));

    let run = exec(
        "/NoProgram << /FontType 1 /FontMatrix [0.001 0 0 0.001 0 0] \
         /Encoding StandardEncoding >> definefont setfont (a) stringwidth",
    );
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("definefont"));

    let run = exec(
        "/NoPrivate << /FontType 1 /FontMatrix [0.001 0 0 0.001 0 0] \
         /Encoding StandardEncoding /CharStrings << /a <00> >> >> definefont",
    );
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("definefont"));

    let run = exec(
        "/NoSfnts << /FontType 42 /FontMatrix [1 0 0 1 0 0] /Encoding StandardEncoding \
         /CharStrings << /a 1 >> >> definefont setfont (a) stringwidth",
    );
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("definefont"));

    let run = exec(
        "/NoCharStrings << /FontType 42 /FontMatrix [1 0 0 1 0 0] /Encoding StandardEncoding \
         /sfnts [ <00010000000100> ] >> definefont",
    );
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("definefont"));

    // A program that is present but unreadable is found out at the
    // first glyph.
    let run = exec(
        "/ShortSfnts << /FontType 42 /FontMatrix [1 0 0 1 0 0] /Encoding StandardEncoding \
         /CharStrings << /a 1 >> /sfnts [ <00010000000100> ] >> definefont setfont \
         (a) stringwidth",
    );
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("stringwidth"));
}

#[test]
fn the_snapshot_carries_the_dictionary_entries_the_writer_needs() {
    let mut run = exec(&with_syn("/Syn findfont"));
    assert_eq!(run.outcome, Outcome::Ok);
    let dict = run.interp.ostack()[0];
    let program = run.interp.font_program(dict).unwrap();
    let Program::Type1(type1) = &*program else {
        panic!("a Type 1 program");
    };
    let entries = type1.dict();
    assert_eq!(entries.font_bbox, [0.0, 0.0, 750.0, 750.0]);
    assert_eq!(entries.paint_type, 0);
    let text = |list: &[(Vec<u8>, Vec<u8>)]| -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| {
                (
                    String::from_utf8_lossy(k).into_owned(),
                    String::from_utf8_lossy(v).into_owned(),
                )
            })
            .collect()
    };
    let mut font_info = text(&entries.font_info);
    font_info.sort();
    assert_eq!(
        font_info,
        [
            ("ItalicAngle".to_string(), "0".to_string()),
            ("isFixedPitch".to_string(), "false".to_string()),
        ]
    );
    let private = text(&entries.private);
    let keys: Vec<&str> = private.iter().map(|(k, _)| k.as_str()).collect();
    for key in ["BlueValues", "OtherSubrs", "MinFeature", "password"] {
        assert!(keys.contains(&key), "{keys:?}");
    }
    // The reading procedures are executeonly and print as nothing the
    // writer could use; the subroutines and lenIV are the writer's own.
    for key in ["RD", "ND", "NP", "Subrs", "lenIV"] {
        assert!(!keys.contains(&key), "{keys:?}");
    }
    let value = |key: &str| {
        private
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(value("BlueValues").as_deref(), Some("[]"));
    assert_eq!(value("OtherSubrs").as_deref(), Some("[{} {} {} {}]"));
    assert_eq!(value("MinFeature").as_deref(), Some("{16 16}"));
    assert_eq!(value("password").as_deref(), Some("5839"));
}

// --- charpath -------------------------------------------------------------------

#[test]
fn charpath_appends_the_outline_through_the_font_matrix_and_advances() {
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont 100 100 moveto (a) false charpath currentpoint",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let path = run.path_calls();
    // The moveto of the program itself, then the square at (100,100)
    // scaled by 0.01, then the advance.
    assert_eq!(path.len(), 7, "{path:?}");
    assert!(matches!(path[1], Call::MoveTo(p) if point_approx(p, 100.5, 100.0)));
    assert!(matches!(path[2], Call::LineTo(p) if point_approx(p, 105.5, 100.0)));
    assert!(matches!(path[3], Call::LineTo(p) if point_approx(p, 105.5, 105.0)));
    assert!(matches!(path[4], Call::LineTo(p) if point_approx(p, 100.5, 105.0)));
    assert!(matches!(path[5], Call::ClosePath));
    assert!(matches!(path[6], Call::MoveTo(p) if point_approx(p, 106.0, 100.0)));
    assert!(run.shows().is_empty());
    let top = run.top_numbers(2);
    assert!(approx(top[0], 106.0) && approx(top[1], 100.0), "{top:?}");
}

#[test]
fn charpath_advances_like_show_and_accepts_the_boolean() {
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont 0 0 moveto (aa) false charpath currentpoint \
         0 0 moveto (aa) true charpath currentpoint",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let top = run.top_numbers(4);
    assert!(approx(top[0], 12.0) && approx(top[1], 0.0), "{top:?}");
    assert!(approx(top[2], 12.0) && approx(top[3], 0.0), "{top:?}");
}

#[test]
fn charpath_follows_the_ctm_through_the_backend_and_a_translated_matrix() {
    let run = exec(&with_syn(
        "/Syn findfont [10 0 0 10 3 4] makefont setfont 2 2 scale 10 10 moveto \
         (a) false charpath currentpoint",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let path = run.path_calls();
    // User-space points: font matrix (0.01 scale, translation 3 4) then
    // the current point; the backend maps through the CTM itself. The
    // advance is a vector, so the matrix's translation does not move it.
    assert!(matches!(path[1], Call::MoveTo(p) if point_approx(p, 13.5, 14.0)));
    assert!(matches!(path[6], Call::MoveTo(p) if point_approx(p, 16.0, 10.0)));
    let top = run.top_numbers(2);
    assert!(approx(top[0], 16.0) && approx(top[1], 10.0), "{top:?}");
}

#[test]
fn charpath_needs_a_current_point_and_a_font_with_outlines() {
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont (a) false charpath",
    ));
    assert_eq!(run.error(), Some("nocurrentpoint"));
    // Symbol has no outline asset in any build.
    let run = exec("/Symbol findfont 10 scalefont setfont 0 0 moveto (x) false charpath");
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("charpath"));
    assert_eq!(run.interp.ostack().len(), 2, "operands stay on failure");
    let run = exec("/Helvetica findfont 10 scalefont setfont 0 0 moveto (x) false charpath");
    if efterscript_fonts::has_resident_outlines() {
        assert_eq!(run.error(), None);
        assert!(run.path_calls().len() > 2, "the outline joined the path");
    } else {
        assert_eq!(run.error(), Some("invalidfont"));
    }
    let run = exec(
        "/Sq << /FontType 3 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding StandardEncoding \
         /BuildGlyph { pop pop 500 0 setcharwidth } >> definefont 10 scalefont setfont \
         0 0 moveto (a) false charpath",
    );
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("charpath"));
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont 0 0 moveto (a) charpath",
    ));
    assert_eq!(run.error(), Some("stackunderflow"));
}

#[test]
fn a_stroked_font_outlines_like_a_filled_one() {
    let mut font = syn();
    font.name = "SynStroked".to_string();
    let program = font.pfa().replace("/PaintType 0 def", "/PaintType 2 def");
    let run = exec(&format!(
        "{program}/SynStroked findfont 10 scalefont setfont 0 0 moveto (a) false charpath \
         currentpoint"
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.path_calls().len(), 7);
    assert!(approx(run.top_numbers(2)[0], 6.0));
}

// --- Type 42 -------------------------------------------------------------------

#[test]
fn a_type42_program_measures_in_units_of_the_em() {
    let run = exec(&with_syn_tt(
        "/SynTT findfont 20 scalefont setfont (a) stringwidth (o) stringwidth \
         0 0 moveto (ao) show currentpoint",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let top = run.top_numbers(6);
    assert!(approx(top[0], 10.0) && approx(top[1], 0.0), "{top:?}");
    assert!(approx(top[2], 1200.0 / 2048.0 * 20.0), "{top:?}");
    assert!(approx(top[4], 10.0 + 1200.0 / 2048.0 * 20.0), "{top:?}");
    let shows = run.shows();
    assert_eq!(shows.len(), 1);
    assert!(approx(shows[0][0].dx, 0.5), "glyph units are the unit em");
    assert!(approx(shows[0][1].dx, 1200.0 / 2048.0));
}

#[test]
fn a_type42_charpath_converts_quadratics_to_cubics() {
    let run = exec(&with_syn_tt(
        "/SynTT findfont 20 scalefont setfont 0 0 moveto (o) true charpath currentpoint",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let path = run.path_calls();
    let curves = path
        .iter()
        .filter(|c| matches!(c, Call::CurveTo(..)))
        .count();
    assert_eq!(curves, 2, "{path:?}");
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for call in &path[1..path.len() - 1] {
        let points: Vec<Point> = match call {
            Call::MoveTo(p) | Call::LineTo(p) => vec![*p],
            Call::CurveTo(a, b, c) => vec![*a, *b, *c],
            _ => Vec::new(),
        };
        for p in points {
            xs.push(p.x);
            ys.push(p.y);
        }
    }
    let range = |v: &[f32]| {
        (
            v.iter().copied().fold(f32::INFINITY, f32::min),
            v.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        )
    };
    let scale = 20.0 / 2048.0;
    let (x0, x1) = range(&xs);
    let (y0, y1) = range(&ys);
    assert!(
        approx(x0, 100.0 * scale) && approx(x1, 1100.0 * scale),
        "{xs:?}"
    );
    assert!(y0 >= -1e-3 && y1 <= 1000.0 * scale + 1e-3, "{ys:?}");
    let top = run.top_numbers(2);
    assert!(approx(top[0], 1200.0 * scale), "{top:?}");
}

#[test]
fn type42_glyphs_missing_from_charstrings_use_notdef() {
    let run = exec(&with_syn_tt(
        "/SynTT findfont 20 scalefont setfont (z) stringwidth",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    assert!(
        approx(run.top_numbers(2)[0], 10.0),
        "notdef advances half an em"
    );
    assert!(run.output.is_empty());
}

#[test]
fn a_name_the_resident_face_lacks_advances_by_its_notdef_width() {
    let reencode = |face: &str| {
        format!(
            "/R /{face} findfont dup length dict copy dup /Encoding StandardEncoding \
             dup length array copy dup 65 /nosuchglyph put put definefont 10 scalefont setfont \
             (A) stringwidth (AA) stringwidth"
        )
    };
    // The derived tables carry a `.notdef` width of their own.
    let run = exec(&reencode("Palatino-Roman"));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let top = run.top_numbers(4);
    assert!(approx(top[0], 5.0) && approx(top[2], 10.0), "{top:?}");
    // The Core 14 metrics lack one: the outline asset's `.notdef`
    // advance when the assets are embedded, else 0.
    let run = exec(&reencode("Times-Roman"));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let top = run.top_numbers(4);
    let expected = efterscript_fonts::ResidentFace::TimesRoman.notdef_width() / 100.0;
    if efterscript_fonts::has_resident_outlines() {
        assert!(expected > 7.0 && expected < 8.0, "{expected}");
    } else {
        assert_eq!(expected, 0.0);
    }
    assert!(
        approx(top[0], expected) && approx(top[2], 2.0 * expected),
        "{top:?}"
    );
    // A face without an asset advances by nothing.
    let run = exec(&reencode("Symbol"));
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert!(approx(run.top_numbers(4)[0], 0.0));
}
