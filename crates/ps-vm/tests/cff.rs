// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! CFF fonts loaded through the `FontSetInit` procedure set: `StartData`
//! reads the binary program from the current file, defines FontType 2
//! dictionaries whose programs are cached on the interpreter, and the
//! FontSet resource; the show family, `stringwidth`, and `charpath` then
//! work through the glyph engine. The printed-output scenarios are corpus
//! files under `corpus/unit/fonts`, generated from the same font.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Call, Log, Recording};
use ps_fonts::testing::{CffFd, CffFont, Type2Builder, corpus_cff, eexec_binary, rectangle};
use ps_fonts::{Program, ProgramKind};
use ps_vm::{Config, Glyph, Interp, Io, Outcome, Point, SliceSource, Type};

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

fn exec(program: &[u8]) -> Run {
    let (io, out, _) = Io::capture();
    let config = Config {
        io,
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Recording::new(log.clone())));
    let outcome = interp.run(&mut SliceSource::new(program));
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

/// The FontSet file of `font` followed by `program`.
fn with_set(font: &CffFont, program: &str) -> Vec<u8> {
    let mut out = font.font_set("SynSet");
    out.extend_from_slice(program.as_bytes());
    out
}

fn with_syn(program: &str) -> Vec<u8> {
    with_set(&corpus_cff(), program)
}

#[test]
fn a_font_set_defines_type2_fonts_and_the_resource() {
    let run = exec(&with_syn(
        "/SynSet /FontSet resourcestatus \
         /SynSet /FontSet findresource dup length exch 0 get \
         /SynCFF findfont dup /FontType get exch dup /FID known exch \
         dup /CharStrings get /f get exch dup /Encoding get 97 get exch \
         dup /FontMatrix get 0 get exch /FontBBox get 1 get \
         /SynCFF /Font resourcestatus pop pop",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    let mem = run.interp.memory();
    assert_eq!(stack[0].as_i32(), Some(0), "defined FontSet");
    assert_eq!(stack[1].as_i32(), Some(0));
    assert_eq!(stack[2].as_bool(), Some(true));
    assert_eq!(stack[3].as_i32(), Some(1), "one font in the set");
    assert_eq!(mem.name_text(stack[4].as_name().unwrap()), b"SynCFF");
    assert_eq!(stack[5].as_i32(), Some(2));
    assert_eq!(stack[6].as_bool(), Some(true));
    assert_eq!(stack[7].as_i32(), Some(4), "f is glyph 4");
    assert_eq!(mem.name_text(stack[8].as_name().unwrap()), b"a");
    assert!(approx(stack[9].as_number().unwrap(), 0.001));
    assert_eq!(stack[10].as_i32(), Some(-50));
    assert_eq!(stack[11].as_i32(), Some(0), "defined font");
    assert_eq!(stack.len(), 12);
    // The dictionary is read-only and the standard encoding is shared.
    let run = exec(&with_syn(
        "/SynCFF findfont /Encoding get StandardEncoding eq \
         /SynCFF findfont /X 1 put",
    ));
    assert_eq!(run.error(), Some("invalidaccess"));
    assert_eq!(run.interp.ostack()[0].as_bool(), Some(true));
}

#[test]
fn widths_and_shows_come_from_the_type2_charstrings() {
    let run = exec(&with_syn(
        "/SynCFF findfont 10 scalefont setfont (a) stringwidth (ab) stringwidth \
         100 100 moveto (a) show currentpoint",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let top = run.top_numbers(6);
    assert!(approx(top[0], 6.0) && approx(top[1], 0.0), "{top:?}");
    assert!(approx(top[2], 10.0), "{top:?}");
    assert!(approx(top[4], 106.0) && approx(top[5], 100.0), "{top:?}");
    let shows = run.shows();
    assert_eq!(shows.len(), 1);
    assert_eq!(shows[0][0].code, 97);
    assert!(approx(shows[0][0].dx, 600.0), "charstring units, scale 1");
    // The program behind the dictionary is the cached CFF snapshot, and
    // it is built once per family.
    let mut run = exec(&with_syn("/SynCFF findfont dup 10 scalefont"));
    let stack = run.interp.ostack().to_vec();
    let base = run.interp.font_program(stack[0]).unwrap();
    assert_eq!(base.kind(), ProgramKind::Cff);
    let scaled = run.interp.font_program(stack[1]).unwrap();
    assert!(Rc::ptr_eq(&base, &scaled));
}

#[test]
fn charpath_appends_flex_and_hint_mask_outlines() {
    let run = exec(&with_syn(
        "/SynCFF findfont 10 scalefont setfont 0 0 moveto (f) true charpath currentpoint",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let path = run.path_calls();
    // The run's start, the glyph's own moveto, two flex curves, two
    // lines, the implicit close, and the advance.
    assert_eq!(path.len(), 8, "{path:?}");
    assert!(matches!(path[1], Call::MoveTo(p) if point_approx(p, 0.0, 0.0)));
    assert!(matches!(path[2], Call::CurveTo(a, b, c)
        if point_approx(a, 1.0, 0.0) && point_approx(b, 2.0, 2.0) && point_approx(c, 3.0, 2.0)));
    assert!(matches!(path[3], Call::CurveTo(a, b, c)
        if point_approx(a, 4.0, 2.0) && point_approx(b, 5.0, 0.0) && point_approx(c, 6.0, 0.0)));
    assert!(matches!(path[4], Call::LineTo(p) if point_approx(p, 6.0, -0.5)));
    assert!(matches!(path[5], Call::LineTo(p) if point_approx(p, 0.0, -0.5)));
    assert!(matches!(path[6], Call::ClosePath));
    assert!(matches!(path[7], Call::MoveTo(p) if point_approx(p, 6.0, 0.0)));
    let top = run.top_numbers(2);
    assert!(approx(top[0], 6.0) && approx(top[1], 0.0), "{top:?}");
}

#[test]
fn a_custom_encoding_becomes_a_name_array_and_a_matrix_is_honoured() {
    let font = CffFont::new("Enc")
        .widths(0, 0)
        .font_matrix([0.002, 0.0, 0.0, 0.002, 0.0, 0.0])
        .glyph("square", 500, &rectangle(0.0, 0.0, 500.0, 500.0))
        .encode(65, "square");
    let run = exec(&with_set(
        &font,
        "/Enc findfont /Encoding get dup 65 get exch dup 66 get exch \
         StandardEncoding eq \
         /Enc findfont 10 scalefont setfont (A) stringwidth",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    let mem = run.interp.memory();
    assert_eq!(mem.name_text(stack[0].as_name().unwrap()), b"square");
    assert_eq!(mem.name_text(stack[1].as_name().unwrap()), b".notdef");
    assert_eq!(stack[2].as_bool(), Some(false));
    let top = run.top_numbers(2);
    assert!(
        approx(top[0], 10.0),
        "500 units under a 0.002 matrix: {top:?}"
    );
}

#[test]
fn cid_keyed_fonts_are_held_by_name_and_not_defined() {
    let font = CffFont::cid_keyed("SynCID", "Adobe", "Identity", 0)
        .fd(CffFd {
            subrs: Vec::new(),
            default_width: 100,
            nominal_width: 0,
        })
        .cid_glyph(5, 0, Type2Builder::new().rmoveto(0, 0).endchar().bytes());
    let run = exec(&with_set(
        &font,
        "/SynSet /FontSet findresource length /SynCID /Font resourcestatus",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(0), "no name-keyed font defined");
    assert_eq!(stack[1].as_bool(), Some(false));
    let program = run.interp.cid_program(b"SynCID").expect("held by name");
    let Program::Cff(cff) = &*program else {
        panic!("a CFF program");
    };
    assert!(cff.is_cid_keyed());
    assert_eq!(cff.glyph_by_cid(5).unwrap().unwrap().advance.0, 100.0);
    assert!(run.interp.cid_program(b"Other").is_none());
}

#[test]
fn short_data_and_bad_programs_are_invalidfont() {
    let mut short = corpus_cff().font_set("SynSet");
    let cut = short.len() - 40;
    short.truncate(cut);
    let run = exec(&short);
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("StartData"));
    assert_eq!(run.interp.ostack().len(), 2, "operands stay on failure");

    let run = exec(b"/FontSetInit /ProcSet findresource begin /S 4 StartData\nabcd\nend");
    assert_eq!(run.error(), Some("invalidfont"));
    let run = exec(b"/FontSetInit /ProcSet findresource begin /S -1 StartData\n");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec(b"/FontSetInit /ProcSet findresource begin /S (4) StartData\n");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec(b"StartData");
    assert_eq!(
        run.error(),
        Some("undefined"),
        "only inside the procedure set"
    );
}

#[test]
fn start_data_reads_through_an_eexec_layer() {
    // The FontSet inside a binary eexec section: StartData reads its
    // bytes through the decrypting layer, whose file is the current one.
    let font = corpus_cff();
    let mut plain = font.font_set("SynSet");
    plain.extend_from_slice(b"mark currentfile closefile\n");
    let mut program = b"currentfile eexec\n".to_vec();
    program.extend(eexec_binary(&plain));
    program
        .extend_from_slice(b"\ncleartomark /SynCFF findfont 10 scalefont setfont (a) stringwidth");
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert!(approx(run.top_numbers(2)[0], 6.0));
}

#[test]
fn hand_built_type2_dictionaries_define_but_cannot_draw() {
    let run = exec(
        b"/Hand << /FontType 2 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding StandardEncoding >> \
          definefont dup /FID known exch 10 scalefont setfont (a) stringwidth",
    );
    assert_eq!(run.error(), Some("invalidfont"));
    assert_eq!(run.command(), Some("stringwidth"));
    assert_eq!(run.interp.ostack()[0].as_bool(), Some(true));
    let run = exec(b"/Bad << /FontType 2 /FontMatrix [0.001 0 0 0.001 0 0] >> definefont");
    assert_eq!(run.error(), Some("invalidfont"));
}

#[test]
fn the_procset_and_fontset_categories_answer_the_resource_operators() {
    let run = exec(&with_syn(
        "/FontSetInit /ProcSet resourcestatus \
         /NoSuchSet /ProcSet resourcestatus \
         /FontSetInit /ProcSet findresource /StartData known \
         /Mine 1 dict /ProcSet defineresource pop /Mine /ProcSet resourcestatus pop pop \
         /Mine /ProcSet undefineresource /Mine /ProcSet resourcestatus \
         /Other [ /SynCFF ] /FontSet defineresource pop /Other /FontSet resourcestatus pop pop \
         /Other /FontSet undefineresource /Other /FontSet resourcestatus \
         (*) { = } 32 string /ProcSet resourceforall \
         (*) { = } 32 string /FontSet resourceforall",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let stack = run.interp.ostack();
    assert_eq!(
        stack[0].as_i32(),
        Some(2),
        "built-in procedure sets are resident"
    );
    assert_eq!(stack[1].as_i32(), Some(0));
    assert_eq!(stack[2].as_bool(), Some(true));
    assert_eq!(stack[3].as_bool(), Some(false));
    assert_eq!(stack[4].as_bool(), Some(true));
    assert_eq!(
        stack[5].as_i32(),
        Some(0),
        "a user procedure set is defined"
    );
    assert_eq!(stack[6].as_bool(), Some(false));
    assert_eq!(stack[7].as_i32(), Some(0));
    assert_eq!(stack[8].as_bool(), Some(false));
    assert_eq!(stack.len(), 9);
    assert_eq!(run.output, "CIDInit\nFontSetInit\nSynSet\n");
    let run = exec(b"/NoSuchSet /ProcSet findresource");
    assert_eq!(run.error(), Some("undefined"));
    let run = exec(b"/NoSuchSet /FontSet findresource");
    assert_eq!(run.error(), Some("undefined"));
    let run = exec(b"/P 5 /ProcSet defineresource");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec(b"/F 5 /FontSet defineresource");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec(b"/FontSetInit /ProcSet findresource /X 1 put");
    assert_eq!(run.error(), Some("invalidaccess"));
    let run = exec(b"/FontSetInit /ProcSet findresource dup type exch length");
    assert_eq!(run.interp.ostack()[0].ty(), Type::Name);
    assert_eq!(run.interp.ostack()[1].as_i32(), Some(1));
}
