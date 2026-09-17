// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Fonts and text through the recording backend: what the VM hands the
//! boundary for resident and Type 3 fonts, the show frame's stepping, the
//! font in save/restore, substitution, and the resource operators.
//! Scenarios that only need printed output live in `corpus/unit/text`.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Call, Log, Recording};
use ps_vm::{
    Bounds, Config, FontConfig, Glyph, Interp, Io, Matrix, Outcome, Point, SliceSource, Type,
};

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

    fn top_numbers(&self, n: usize) -> Vec<f32> {
        let stack = self.interp.ostack();
        stack[stack.len() - n..]
            .iter()
            .map(|o| o.as_number().expect("number"))
            .collect()
    }
}

fn run_with(program: &str, fonts: FontConfig, backend: bool) -> Run {
    let (io, out, _) = Io::capture();
    let config = Config {
        io,
        fonts,
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    if backend {
        interp.set_graphics_backend(Box::new(Recording::new(log.clone())));
    }
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    Run {
        interp,
        outcome,
        output: out.text(),
        log,
    }
}

fn exec(program: &str) -> Run {
    run_with(program, FontConfig::default(), true)
}

fn exec_without_backend(program: &str) -> Run {
    run_with(program, FontConfig::default(), false)
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

fn glyphs_approx(got: &[Glyph], want: &[(u8, f32, f32)]) -> bool {
    got.len() == want.len()
        && got.iter().zip(want).all(|(g, &(code, dx, dy))| {
            g.code == u32::from(code) && approx(g.dx, dx) && approx(g.dy, dy)
        })
}

fn matrix_approx(a: Matrix, b: Matrix) -> bool {
    a.0.iter().zip(b.0).all(|(x, y)| (x - y).abs() < 1e-5)
}

const SQUARE: &str = "/Sq 7 dict dup begin \
    /FontType 3 def /FontMatrix [0.001 0 0 0.001 0 0] def \
    /Encoding StandardEncoding def /FontBBox [0 0 1000 1000] def \
    /BuildGlyph { pop pop 1000 0 0 0 750 750 setcachedevice 0 0 750 750 rectfill } def \
    end definefont pop ";

// --- resident fonts -------------------------------------------------------------

#[test]
fn a_resident_show_is_one_run_of_glyph_widths() {
    let run =
        exec("/Helvetica findfont 12 scalefont setfont 100 700 moveto (Hi) show currentpoint");
    assert_eq!(run.outcome, Outcome::Ok);
    let calls = run.calls();
    let font = calls
        .iter()
        .find_map(|c| match c {
            Call::SetFont(Some(font)) => Some(*font),
            _ => None,
        })
        .expect("setfont reached the backend");
    assert!(matrix_approx(font.matrix, Matrix::scaling(0.012, 0.012)));
    assert_eq!(run.interp.current_font(), Some(font));
    let dict = run.interp.font_dict(font.instance).expect("instance");
    assert_eq!(dict.ty(), Type::Dict);
    let shows = run.shows();
    assert_eq!(shows.len(), 1);
    assert_eq!(
        shows[0],
        [
            Glyph::simple(72, 722.0, 0.0),
            Glyph::simple(105, 222.0, 0.0)
        ]
    );
    let point = run.top_numbers(2);
    assert!(approx(point[0], 111.328) && approx(point[1], 700.0));
    assert!(!calls.iter().any(|c| matches!(c, Call::BeginGlyph(..))));
}

#[test]
fn symbol_measures_with_its_own_encoding_and_unknown_glyphs_are_notdef() {
    let run = exec(
        "/Symbol findfont 10 scalefont setfont (a) stringwidth pop \
         /Helvetica findfont 10 scalefont setfont (\\001) stringwidth pop \
         /ZapfDingbats findfont /Encoding get 97 get",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    assert!(approx(stack[0].as_number().unwrap(), 6.31));
    // An unencoded code selects `.notdef`, whose width the Core 14
    // metrics lack: the outline asset's stands in when embedded.
    let notdef = ps_fonts::ResidentFace::Helvetica.notdef_width() / 100.0;
    assert!(approx(stack[1].as_number().unwrap(), notdef), "{stack:?}");
    let atom = stack[2].as_name().expect("name");
    assert_eq!(run.interp.memory().name_text(atom), b"a60");
}

#[test]
fn show_variants_record_displacements_in_glyph_space() {
    let run = exec(
        "/Helvetica findfont 10 scalefont setfont \
         0 0 moveto (abc) [10 20 30] xshow \
         0 0 moveto 1 2 (ab) ashow \
         0 0 moveto 5 0 32 (a b) widthshow \
         0 0 moveto 5 0 32 1 0 (a b) awidthshow \
         0 0 moveto (ab) [3 4] yshow \
         0 0 moveto (ab) [1 2 3 4] xyshow \
         0 0 moveto /W glyphshow currentpoint",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let shows = run.shows();
    assert_eq!(shows.len(), 7);
    assert!(glyphs_approx(
        &shows[0],
        &[(97, 1000.0, 0.0), (98, 2000.0, 0.0), (99, 3000.0, 0.0)]
    ));
    assert!(glyphs_approx(
        &shows[1],
        &[(97, 656.0, 200.0), (98, 656.0, 200.0)]
    ));
    assert!(glyphs_approx(
        &shows[2],
        &[(97, 556.0, 0.0), (32, 778.0, 0.0), (98, 556.0, 0.0)]
    ));
    assert!(glyphs_approx(
        &shows[3],
        &[(97, 656.0, 0.0), (32, 878.0, 0.0), (98, 656.0, 0.0)]
    ));
    assert!(glyphs_approx(
        &shows[4],
        &[(97, 0.0, 300.0), (98, 0.0, 400.0)]
    ));
    assert!(glyphs_approx(
        &shows[5],
        &[(97, 100.0, 200.0), (98, 300.0, 400.0)]
    ));
    assert!(glyphs_approx(&shows[6], &[(87, 944.0, 0.0)]));
    let point = run.top_numbers(2);
    assert!(approx(point[0], 9.44) && approx(point[1], 0.0));
}

#[test]
fn kshow_shows_each_segment_before_its_procedure() {
    let run = exec(
        "/Courier findfont 10 scalefont setfont 0 0 moveto \
         { pop pop 5 0 rmoveto } (abc) kshow currentpoint",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let text: Vec<Call> = run
        .calls()
        .into_iter()
        .filter(|c| matches!(c, Call::Show(_) | Call::MoveTo(_)))
        .collect();
    let glyph = |code| Glyph::simple(code, 600.0, 0.0);
    assert_eq!(
        text,
        [
            Call::MoveTo(Point::new(0.0, 0.0)),
            Call::Show(vec![glyph(97)]),
            Call::MoveTo(Point::new(11.0, 0.0)),
            Call::Show(vec![glyph(98)]),
            Call::MoveTo(Point::new(22.0, 0.0)),
            Call::Show(vec![glyph(99)]),
        ]
    );
    let point = run.top_numbers(2);
    assert!(approx(point[0], 28.0));
}

// --- Type 3 fonts -----------------------------------------------------------------

#[test]
fn type3_glyphs_run_inside_a_captured_graphics_state() {
    let program =
        format!("{SQUARE} /Sq findfont 20 scalefont setfont 10 10 moveto (aa) show currentpoint");
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    let font = run.interp.current_font().expect("font");
    assert!(matrix_approx(font.matrix, Matrix::scaling(0.02, 0.02)));
    let calls = run.calls();
    let text: Vec<Call> = calls
        .into_iter()
        .filter(|c| {
            !matches!(
                c,
                Call::MediaBox(_) | Call::SetFont(_) | Call::MoveTo(_) | Call::NewPath
            )
        })
        .collect();
    let bbox = Some(Bounds::new(0.0, 0.0, 750.0, 750.0));
    let expected_matrices = [
        Matrix([0.02, 0.0, 0.0, 0.02, 10.0, 10.0]),
        Matrix([0.02, 0.0, 0.0, 0.02, 30.0, 10.0]),
    ];
    let mut at = 0;
    for expected in expected_matrices {
        assert_eq!(text[at], Call::GSave);
        let Call::SetMatrix(matrix) = text[at + 1] else {
            panic!("expected the glyph CTM, got {:?}", text[at + 1]);
        };
        assert!(matrix_approx(matrix, expected), "{matrix:?}");
        assert_eq!(
            text[at + 2],
            Call::BeginGlyph(font, 97, b"a".to_vec(), false)
        );
        assert!(matches!(text[at + 3], Call::RectFill(_)));
        assert_eq!(text[at + 4], Call::EndGlyph((1000.0, 0.0), bbox));
        assert_eq!(text[at + 5], Call::GRestoreTo(0));
        at += 6;
    }
    assert_eq!(
        text[at],
        Call::Show(vec![Glyph::simple(97, 1000.0, 0.0); 2])
    );
    assert_eq!(text.len(), at + 1);
    let point = run.top_numbers(2);
    assert!(approx(point[0], 50.0) && approx(point[1], 10.0));
    assert_eq!(run.interp.ostack().len(), 2);
    assert_eq!(run.interp.dstack().len(), 3);
}

#[test]
fn stringwidth_measures_type3_glyphs_without_showing() {
    let program = format!("{SQUARE} /Sq findfont 20 scalefont setfont (a) stringwidth");
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    let width = run.top_numbers(2);
    assert!(approx(width[0], 20.0) && approx(width[1], 0.0));
    let calls = run.calls();
    assert!(calls.iter().any(|c| matches!(
        c,
        Call::BeginGlyph(_, 97, name, true) if name == b"a"
    )));
    assert!(
        calls
            .iter()
            .any(|c| *c == Call::EndGlyph((1000.0, 0.0), Some(Bounds::new(0.0, 0.0, 750.0, 750.0))))
    );
    assert!(run.shows().is_empty());
    assert!(calls.contains(&Call::GRestoreTo(0)));
}

/// The `end_glyph` calls of a run, in order.
fn end_glyphs(run: &Run) -> Vec<((f32, f32), Option<Bounds>)> {
    run.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::EndGlyph(width, bbox) => Some((width, bbox)),
            _ => None,
        })
        .collect()
}

fn metrics_approx(
    got: ((f32, f32), Option<Bounds>),
    width: (f32, f32),
    bbox: Option<Bounds>,
) -> bool {
    let (w, b) = got;
    approx(w.0, width.0)
        && approx(w.1, width.1)
        && match (b, bbox) {
            (None, None) => true,
            (Some(b), Some(want)) => {
                approx(b.llx, want.llx)
                    && approx(b.lly, want.lly)
                    && approx(b.urx, want.urx)
                    && approx(b.ury, want.ury)
            }
            _ => false,
        }
}

/// A Type 3 font whose glyph procedure runs `prefix` before declaring
/// `metrics` (the operator's operands and name) and filling a box.
fn transformed_font(prefix: &str, metrics: &str) -> String {
    format!(
        "/T << /FontType 3 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding StandardEncoding \
         /FontBBox [0 0 1000 1000] /BuildGlyph {{ pop pop {prefix} {metrics} \
         0 0 500 500 rectfill }} >> definefont 20 scalefont setfont \
         100 100 moveto (aa) show currentpoint (a) stringwidth"
    )
}

#[test]
fn metrics_declared_after_a_scale_are_carried_into_glyph_space() {
    let run = exec(&transformed_font(
        "0.5 0.5 scale",
        "1200 0 0 0 1200 1200 setcachedevice",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let ends = end_glyphs(&run);
    assert_eq!(ends.len(), 3);
    let box_ = Some(Bounds::new(0.0, 0.0, 600.0, 600.0));
    assert!(ends.iter().all(|&e| metrics_approx(e, (600.0, 0.0), box_)));
    assert!(glyphs_approx(
        &run.shows()[0],
        &[(97, 600.0, 0.0), (97, 600.0, 0.0)]
    ));
    let numbers = run.top_numbers(4);
    assert!(approx(numbers[0], 124.0) && approx(numbers[1], 100.0));
    assert!(approx(numbers[2], 12.0) && approx(numbers[3], 0.0));
}

#[test]
fn metrics_declared_after_a_translation_move_the_box_only() {
    let run = exec(&transformed_font(
        "100 -50 translate",
        "700 0 0 0 500 500 setcachedevice",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let ends = end_glyphs(&run);
    let box_ = Some(Bounds::new(100.0, -50.0, 600.0, 450.0));
    assert!(ends.iter().all(|&e| metrics_approx(e, (700.0, 0.0), box_)));
    let numbers = run.top_numbers(4);
    assert!(approx(numbers[0], 128.0) && approx(numbers[2], 14.0));
}

#[test]
fn metrics_declared_after_a_rotation_turn_the_width_and_envelope_the_box() {
    let run = exec(&transformed_font(
        "90 rotate",
        "600 0 0 0 600 400 setcachedevice",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let ends = end_glyphs(&run);
    let box_ = Some(Bounds::new(-400.0, 0.0, 0.0, 600.0));
    assert!(ends.iter().all(|&e| metrics_approx(e, (0.0, 600.0), box_)));
    let numbers = run.top_numbers(4);
    assert!(approx(numbers[0], 100.0) && approx(numbers[1], 124.0));
    assert!(approx(numbers[2], 0.0) && approx(numbers[3], 12.0));
}

#[test]
fn setcharwidth_and_setcachedevice2_follow_the_same_rule() {
    let run = exec(&transformed_font("2 2 scale", "300 0 setcharwidth"));
    assert_eq!(run.outcome, Outcome::Ok);
    assert!(
        end_glyphs(&run)
            .iter()
            .all(|&e| metrics_approx(e, (600.0, 0.0), None))
    );
    assert!(approx(run.top_numbers(4)[2], 12.0));
    let run = exec(&transformed_font(
        "0.5 0.5 scale",
        "1000 0 0 0 1000 1000 0 -2000 500 1800 setcachedevice2",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let box_ = Some(Bounds::new(0.0, 0.0, 500.0, 500.0));
    assert!(
        end_glyphs(&run)
            .iter()
            .all(|&e| metrics_approx(e, (500.0, 0.0), box_))
    );
}

#[test]
fn metrics_declared_before_any_change_are_the_operands_bit_for_bit() {
    let run = exec(&transformed_font(
        "",
        "1000.25 0.5 0 0 750.125 750 setcachedevice 0.5 0.5 scale",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let box_ = Some(Bounds::new(0.0, 0.0, 750.125, 750.0));
    assert!(
        end_glyphs(&run)
            .iter()
            .all(|&(w, b)| w == (1000.25, 0.5) && b == box_)
    );
}

#[test]
fn a_singular_font_matrix_leaves_declared_metrics_alone() {
    let run = exec(
        "/T << /FontType 3 /FontMatrix [0 0 0 0.001 0 0] /Encoding StandardEncoding \
         /BuildGlyph { pop pop 0.5 0.5 scale 800 0 setcharwidth } >> definefont setfont \
         100 100 moveto (a) show",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert!(end_glyphs(&run).contains(&((800.0, 0.0), None)));
}

#[test]
fn setcharwidth_glyphs_carry_no_box_and_buildchar_gets_the_code() {
    let run = exec(
        "/Codes << /FontType 3 /FontMatrix [0.01 0 0 0.01 0 0] /Encoding StandardEncoding \
         /BuildChar { exch pop /seen exch def 50 0 setcharwidth \
         /Helvetica findfont 40 scalefont setfont 0 0 moveto (H) show } >> definefont \
         10 scalefont setfont 0 0 moveto (b) show currentpoint seen",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let calls = run.calls();
    assert!(calls.contains(&Call::EndGlyph((50.0, 0.0), None)));
    assert!(
        calls
            .iter()
            .any(|c| matches!(c, Call::BeginGlyph(_, 98, name, false) if name == b"b"))
    );
    let shows = run.shows();
    assert_eq!(shows.len(), 2);
    assert!(glyphs_approx(&shows[0], &[(72, 722.0, 0.0)]));
    assert!(glyphs_approx(&shows[1], &[(98, 50.0, 0.0)]));
    let begin = calls
        .iter()
        .position(|c| matches!(c, Call::BeginGlyph(..)))
        .unwrap();
    let end = calls
        .iter()
        .position(|c| matches!(c, Call::EndGlyph(..)))
        .unwrap();
    let nested = calls
        .iter()
        .position(|c| matches!(c, Call::Show(g) if g[0].code == 72))
        .unwrap();
    assert!(begin < nested && nested < end);
    let stack = run.interp.ostack();
    assert_eq!(stack[2].as_i32(), Some(98));
    assert!(approx(stack[0].as_number().unwrap(), 5.0));
    // The outer font is back after the glyph.
    assert!(matrix_approx(
        run.interp.current_font().unwrap().matrix,
        Matrix::scaling(0.1, 0.1)
    ));
}

#[test]
fn a_failing_glyph_procedure_restores_the_graphics_state() {
    let run = exec(
        "/Bad << /FontType 3 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding StandardEncoding \
         /BuildGlyph { pop pop 1 0 div } >> definefont 10 scalefont setfont \
         5 5 moveto { (a) show } stopped $error /errorname get currentpoint",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let calls = run.calls();
    let begin = calls
        .iter()
        .position(|c| matches!(c, Call::BeginGlyph(..)))
        .unwrap();
    assert_eq!(calls[begin + 1], Call::EndGlyph((0.0, 0.0), None));
    assert_eq!(calls[begin + 2], Call::GRestoreTo(0));
    assert!(run.shows().is_empty());
    let point = run.top_numbers(2);
    assert!(approx(point[0], 5.0) && approx(point[1], 5.0));
    // Below the point: the caught error's name and `stopped`'s true, over
    // the operands the failing `div` left behind.
    let stack = run.interp.ostack();
    let n = stack.len();
    assert_eq!(stack[n - 4].as_bool(), Some(true));
    assert_eq!(
        run.interp
            .memory()
            .name_text(stack[n - 3].as_name().unwrap()),
        b"undefinedresult"
    );
    assert!(run.interp.estack().is_empty());
}

#[test]
fn a_glyph_procedure_that_declares_no_width_advances_nothing() {
    let run = exec(
        "/Silent << /FontType 3 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding StandardEncoding \
         /BuildGlyph { pop pop } >> definefont 10 scalefont setfont \
         0 0 moveto (ab) show currentpoint",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert!(run.calls().contains(&Call::EndGlyph((0.0, 0.0), None)));
    assert!(approx(run.top_numbers(2)[0], 0.0));
}

// --- errors and operand discipline -------------------------------------------------

#[test]
fn text_errors_leave_operands_in_place() {
    let run = exec("(x) show");
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("show"));

    let run = exec("/Helvetica findfont 10 scalefont setfont (x) show");
    assert_eq!(run.error(), Some("nocurrentpoint"));
    assert_eq!(run.command(), Some("show"));

    let run = exec(
        "/Helvetica findfont 10 scalefont setfont 0 0 moveto \
         { (abc) [1 2] xshow } stopped pop count $error /errorname get",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    assert_eq!(stack[2].as_i32(), Some(2));
    assert_eq!(
        run.interp.memory().name_text(stack[3].as_name().unwrap()),
        b"rangecheck"
    );

    let run = exec("1 0 0 0 1 1 setcachedevice");
    assert_eq!(run.error(), Some("undefined"));
    assert_eq!(run.command(), Some("setcachedevice"));
    let run = exec("1 0 setcharwidth");
    assert_eq!(run.error(), Some("undefined"));

    // Symbol has no outline asset in any build; Helvetica outlines only
    // with the assets embedded.
    let run = exec("/Symbol findfont 10 scalefont setfont 0 0 moveto (a) false charpath");
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("charpath"));
    let run = exec("/Helvetica findfont 10 scalefont setfont 0 0 moveto (x) false charpath");
    if ps_fonts::has_resident_outlines() {
        assert_eq!(run.error(), None);
    } else {
        assert_eq!(run.error(), Some("invalidfont"));
        assert_eq!(run.command(), Some("charpath"));
    }

    let run = exec("/NoFID 3 dict dup /FontMatrix [1 0 0 1 0 0] put setfont");
    assert_eq!(run.error(), Some("invalidfont"));
    let run = exec("currentfont");
    assert_eq!(run.error(), Some("invalidfont"));

    let run = exec("/T << /FontType 3 /FontMatrix [1 0 0 1 0 0] /Encoding 256 array >> definefont");
    assert_eq!(run.error(), Some("invalidfont"));
    let run = exec(
        "/T << /FontType 3 /FontMatrix [1 0 0 1 0] /Encoding 256 array /BuildGlyph {} >> definefont",
    );
    assert_eq!(run.error(), Some("invalidfont"));
    let run = exec("/T << /FontType 0 /FontMatrix [1 0 0 1 0 0] /Encoding 256 array >> definefont");
    assert_eq!(run.error(), Some("invalidfont"));
    let run = exec("/T << /FontType 1 /FontMatrix [1 0 0 1 0 0] /Encoding 255 array >> definefont");
    assert_eq!(run.error(), Some("invalidfont"));
    let run = exec("{ /Bad 3 dict definefont } stopped pop count");
    assert_eq!(run.top_numbers(1), [2.0]);

    let run = exec(
        "/Helvetica findfont 10 scalefont setfont 0 0 moveto /Codes << /FontType 3 \
         /FontMatrix [1 0 0 1 0 0] /Encoding 256 array /BuildChar { pop pop 1 0 setcharwidth } >> \
         definefont setfont /a glyphshow",
    );
    assert_eq!(run.error(), Some("rangecheck"));
    assert_eq!(run.command(), Some("glyphshow"));
}

// --- the font in save and restore --------------------------------------------------

#[test]
fn restore_returns_to_the_font_of_the_save() {
    let run = exec(
        "/Helvetica findfont 12 scalefont setfont /s save def \
         /T3 << /FontType 3 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding StandardEncoding \
         /BuildGlyph { pop pop 500 0 setcharwidth } >> definefont 10 scalefont setfont \
         0 0 moveto (a) show FontDirectory /T3 known \
         s restore FontDirectory /T3 known currentfont /FontName get \
         0 0 moveto (a) show currentpoint",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    assert_eq!(stack[0].as_bool(), Some(true));
    assert_eq!(stack[1].as_bool(), Some(false));
    assert_eq!(
        run.interp.memory().name_text(stack[2].as_name().unwrap()),
        b"Helvetica"
    );
    assert!(approx(stack[3].as_number().unwrap(), 6.672));
    let shows = run.shows();
    assert!(glyphs_approx(&shows[0], &[(97, 500.0, 0.0)]));
    assert!(glyphs_approx(&shows[1], &[(97, 556.0, 0.0)]));
    assert!(run.calls().contains(&Call::GRestoreTo(0)));
}

#[test]
fn findfont_enters_resident_faces_in_the_directory() {
    let run = exec_without_backend(
        "FontDirectory /Helvetica known /Helvetica findfont pop FontDirectory /Helvetica known \
         /Helvetica /Font resourcestatus pop pop \
         /s save def /Courier findfont pop FontDirectory /Courier known \
         s restore FontDirectory /Courier known \
         /Helvetica undefinefont FontDirectory /Helvetica known \
         /Helvetica findfont pop FontDirectory /Helvetica known \
         /Helvetica /Font findresource pop FontDirectory /Helvetica known \
         /Arial findfont pop FontDirectory /Arial known /Arial /Font resourcestatus pop pop \
         true setglobal /Times-Roman findfont pop false setglobal \
         GlobalFontDirectory /Times-Roman known FontDirectory /Times-Roman known",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    let flags: Vec<Option<bool>> = stack.iter().map(|o| o.as_bool()).collect();
    assert_eq!(flags[0..2], [Some(false), Some(true)]);
    assert_eq!(stack[2].as_i32(), Some(1));
    // The registration is a local-VM dictionary change and goes with
    // the restore.
    assert_eq!(flags[3..5], [Some(true), Some(false)]);
    assert_eq!(flags[5..8], [Some(false), Some(true), Some(true)]);
    assert_eq!(flags[8], Some(true));
    assert_eq!(stack[9].as_i32(), Some(1));
    assert_eq!(flags[10..12], [Some(true), Some(true)]);
    assert_eq!(run.interp.font_substitutions().len(), 1);
    // Cached, not re-materialised: the same dictionary comes back.
    let run =
        exec_without_backend("/Helvetica findfont /Helvetica undefinefont /Helvetica findfont eq");
    assert_eq!(run.interp.ostack()[0].as_bool(), Some(true));
}

#[test]
fn a_font_the_program_defines_wins_over_a_registered_face() {
    let run = exec(
        "/Courier findfont pop /Courier << /FontType 3 /FontMatrix [0.001 0 0 0.001 0 0] \
         /Encoding StandardEncoding /BuildGlyph { pop pop 500 0 setcharwidth } >> definefont pop \
         /Courier findfont /FontType get /Courier /Font resourcestatus pop pop \
         /Courier findfont 10 scalefont setfont 0 0 moveto (a) show currentpoint pop",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(3));
    assert_eq!(stack[1].as_i32(), Some(0));
    assert!(approx(stack[2].as_number().unwrap(), 5.0));
}

#[test]
fn global_fonts_survive_restore_and_show_in_both_directories() {
    let run = exec(
        "true setglobal /G /Helvetica findfont dup length dict copy definefont pop false setglobal \
         GlobalFontDirectory /G known FontDirectory /G known \
         /s save def /L /Courier findfont dup length dict copy definefont pop \
         /L /Font resourcestatus exch pop exch pop s restore /L /Font resourcestatus \
         /G findfont /FontName get /L findfont /FontName get",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    assert_eq!(stack[0].as_bool(), Some(true));
    assert_eq!(stack[1].as_bool(), Some(true));
    assert_eq!(stack[2].as_bool(), Some(true));
    assert_eq!(stack[3].as_bool(), Some(false));
    // The local definition went with the restore; the name substitutes.
    let names: Vec<&[u8]> = stack[4..]
        .iter()
        .map(|o| run.interp.memory().name_text(o.as_name().unwrap()))
        .collect();
    assert_eq!(names, [&b"Helvetica"[..], b"Helvetica"]);
    assert_eq!(run.interp.font_substitutions().len(), 1);
    assert_eq!(run.interp.font_substitutions()[0].requested, b"L");
    // A local font cannot enter the global directory.
    let run = exec(
        "<< /FontType 3 /FontMatrix [1 0 0 1 0 0] /Encoding 256 array /BuildGlyph {} >> \
         true setglobal /G exch definefont",
    );
    assert_eq!(run.error(), Some("invalidaccess"));
    assert_eq!(run.command(), Some("definefont"));
}

// --- substitution and configuration ------------------------------------------------

#[test]
fn substitutions_are_recorded_and_switchable() {
    let run =
        exec("/Arial findfont pop /Helvetica findfont pop /Times-BoldMT findfont /FontName get");
    assert_eq!(run.outcome, Outcome::Ok);
    let subs = run.interp.font_substitutions();
    assert_eq!(subs.len(), 2);
    assert_eq!(subs[0].requested, b"Arial");
    assert_eq!(subs[0].substitute, "Helvetica");
    assert_eq!(subs[1].requested, b"Times-BoldMT");
    assert_eq!(subs[1].substitute, "Times-Bold");
    let name = run.interp.ostack()[0].as_name().unwrap();
    assert_eq!(run.interp.memory().name_text(name), b"Times-Bold");

    let strict = FontConfig { substitute: false };
    let run = run_with("/Helvetica findfont pop /NoSuchFont findfont", strict, true);
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("findfont"));
    assert!(run.interp.font_substitutions().is_empty());
    let run = run_with("{ /NoSuchFont findfont } stopped pop count", strict, true);
    assert_eq!(run.top_numbers(1), [1.0]);
    let run = run_with("/NoSuchFont 10 selectfont", strict, true);
    assert_eq!(run.error(), Some("invalidfont"));
}

#[test]
fn resident_fonts_are_shared_read_only_global_dictionaries() {
    let run = exec(
        "/Helvetica findfont dup /Helvetica findfont eq exch gcheck \
         /Helvetica findfont /FontType get \
         /Helvetica findfont /Encoding get StandardEncoding eq \
         /Helvetica findfont /FontMatrix get \
         { /Helvetica findfont /X 1 put } stopped pop $error /errorname get",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    assert_eq!(stack[0].as_bool(), Some(true));
    assert_eq!(stack[1].as_bool(), Some(true));
    assert_eq!(stack[2].as_i32(), Some(1));
    assert_eq!(stack[3].as_bool(), Some(true));
    let matrix: Vec<f32> = run
        .interp
        .memory()
        .array(stack[4])
        .unwrap()
        .iter()
        .map(|o| o.as_number().unwrap())
        .collect();
    assert_eq!(matrix, [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
    // The failing put left its three operands behind the error name.
    assert_eq!(stack.len(), 9);
    assert_eq!(
        run.interp.memory().name_text(stack[8].as_name().unwrap()),
        b"invalidaccess"
    );
}

// --- without a backend ----------------------------------------------------------------

#[test]
fn a_scripting_vm_keeps_its_own_current_font() {
    let run = exec_without_backend(
        "/Helvetica findfont 12 scalefont setfont (Hello) stringwidth \
         currentfont /FontName get systemdict /show known systemdict /stringwidth known",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let font = run.interp.current_font().expect("font kept by the VM");
    assert!(matrix_approx(font.matrix, Matrix::scaling(0.012, 0.012)));
    let stack = run.interp.ostack();
    assert!(approx(stack[0].as_number().unwrap(), 27.336));
    assert_eq!(
        run.interp.memory().name_text(stack[2].as_name().unwrap()),
        b"Helvetica"
    );
    assert_eq!(stack[3].as_bool(), Some(false));
    assert_eq!(stack[4].as_bool(), Some(true));
    assert!(run.calls().is_empty());

    let run = exec_without_backend("0 0 moveto (x) show");
    assert_eq!(run.error(), Some("undefined"));
    assert_eq!(run.command(), Some("moveto"));

    let run = exec_without_backend(
        "/T3 << /FontType 3 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding StandardEncoding \
         /BuildGlyph { pop pop 500 0 0 0 1 1 setcachedevice } >> definefont 10 scalefont setfont \
         (xy) stringwidth",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let width = run.top_numbers(2);
    assert!(approx(width[0], 10.0) && approx(width[1], 0.0));
    assert!(run.interp.estack().is_empty());
}

// --- resources ---------------------------------------------------------------------------

#[test]
fn resource_operators_cover_fonts_and_encodings() {
    let run = exec(
        "/Mine ISOLatin1Encoding /Encoding defineresource ISOLatin1Encoding eq \
         /Mine /Encoding findresource 233 get \
         /Mine /Encoding resourcestatus \
         /Mine /Encoding undefineresource \
         /Mine /Encoding resourcestatus \
         /StandardEncoding /Encoding resourcestatus pop pop \
         /F /Helvetica findfont dup length dict copy /Font defineresource /FontName get \
         /F /Font resourcestatus pop pop \
         /F /Font undefineresource /F /Font resourcestatus",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    let mem = run.interp.memory();
    assert_eq!(stack[0].as_bool(), Some(true));
    assert_eq!(mem.name_text(stack[1].as_name().unwrap()), b"eacute");
    assert_eq!(stack[2].as_i32(), Some(0));
    assert_eq!(stack[3].as_i32(), Some(0));
    assert_eq!(stack[4].as_bool(), Some(true));
    assert_eq!(stack[5].as_bool(), Some(false));
    assert_eq!(stack[6].as_i32(), Some(0));
    assert_eq!(mem.name_text(stack[7].as_name().unwrap()), b"Helvetica");
    assert_eq!(stack[8].as_i32(), Some(0));
    assert_eq!(stack[9].as_bool(), Some(false));
    assert_eq!(stack.len(), 10);

    let run = exec("/Nothing /Encoding findresource");
    assert_eq!(run.error(), Some("undefined"));
    assert_eq!(run.command(), Some("findresource"));
    let run = exec("/Helvetica /Halftone findresource");
    assert_eq!(run.error(), Some("undefined"));
    let run = exec("/Helvetica /Halftone resourcestatus");
    assert_eq!(run.error(), Some("undefined"));
    let run = exec("/Arial /Font findresource");
    assert_eq!(run.error(), Some("undefined"));
    let run = exec("/E 255 array /Encoding defineresource");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec("/F 3 dict /Font defineresource");
    assert_eq!(run.error(), Some("invalidfont"));
    let run = exec("/F 5 /Font defineresource");
    assert_eq!(run.error(), Some("typecheck"));
}

#[test]
fn resourceforall_writes_names_into_the_scratch_string() {
    let run = exec(
        "/Extra ISOLatin1Encoding /Encoding defineresource pop \
         (*) { = } 32 string /Encoding resourceforall \
         (*Bold*) { = } 32 string /Font resourceforall \
         (Courier) { = } 32 string /Font resourceforall \
         /Zed /Helvetica findfont dup length dict copy definefont pop \
         (Z??) { dup = length } 32 string /Font resourceforall",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "Extra\nISOLatin1Encoding\nStandardEncoding\n\
         Courier-Bold\nCourier-BoldOblique\nHelvetica-Bold\nHelvetica-BoldOblique\n\
         Helvetica-Narrow-Bold\nHelvetica-Narrow-BoldOblique\n\
         NewCenturySchlbk-Bold\nNewCenturySchlbk-BoldItalic\n\
         Palatino-Bold\nPalatino-BoldItalic\n\
         Times-Bold\nTimes-BoldItalic\nCourier\nZed\n"
    );
    assert_eq!(run.top_numbers(1), [3.0]);

    let run = exec("(*) { pop } 4 string /Font resourceforall");
    assert_eq!(run.error(), Some("rangecheck"));
    assert_eq!(run.command(), Some("resourceforall"));
    let run = exec("(*) { pop } 32 string readonly /Font resourceforall");
    assert_eq!(run.error(), Some("invalidaccess"));
    let run = exec("(*) { exit } 32 string /Font resourceforall 7");
    assert_eq!(run.top_numbers(1), [7.0]);
}

#[test]
fn built_in_resources_report_loaded_once_materialised() {
    let run = exec(
        "/Helvetica /Font resourcestatus pop pop \
         /Helvetica findfont pop /Helvetica /Font resourcestatus pop pop \
         /Palatino-Roman /Font resourcestatus pop pop \
         /BookAntiqua findfont pop /Palatino-Roman /Font resourcestatus pop pop \
         /Courier /Font resourcestatus pop pop \
         /FontSetInit /ProcSet resourcestatus pop pop \
         /FontSetInit /ProcSet findresource pop /FontSetInit /ProcSet resourcestatus pop pop \
         /CIDInit /ProcSet resourcestatus pop pop \
         /Identity-H /CMap resourcestatus pop pop \
         /C /Identity-H [ /Helvetica findfont ] composefont pop \
         /Identity-H /CMap resourcestatus pop pop \
         /Identity-V /CMap resourcestatus pop pop \
         save /Times-Roman findfont pop restore /Times-Roman /Font resourcestatus pop pop \
         /StandardEncoding /Encoding resourcestatus pop pop",
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    let statuses: Vec<i32> = run
        .interp
        .ostack()
        .iter()
        .map(|o| o.as_i32().unwrap())
        .collect();
    assert_eq!(
        statuses,
        [2, 1, 2, 1, 2, 2, 1, 2, 2, 1, 2, 1, 0],
        "before and after loading; an alias loads its face; restore keeps the status; encodings are always in VM"
    );
}
