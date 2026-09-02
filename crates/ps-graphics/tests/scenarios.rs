// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The spec scenarios that need the real backend, run through the
//! interpreter. Each has a corpus file under `corpus/unit/graphics/`
//! whose sidecar golden pins the dump; here the IR values are checked.

use std::cell::RefCell;
use std::rc::Rc;

use ps_graphics::{FillRule, Graphics, IrOp, Page, SpaceRef, dump};
use ps_vm::{Config, Interp, Io, Matrix, Outcome, Point, Seg, SliceSource, SpaceSpec};

struct Run {
    outcome: Outcome,
    output: String,
    pages: Vec<Page>,
}

fn exec(program: &str) -> Run {
    let (io, out, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let pages = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Graphics::new(pages.clone())));
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    let pages = pages.take();
    Run {
        outcome,
        output: out.text(),
        pages,
    }
}

fn ops(page: &Page) -> Vec<IrOp> {
    page.ops.iter().map(|o| o.op.clone()).collect()
}

fn p(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

// stroked-line.ps
#[test]
fn a_stroked_line() {
    let run = exec("2 setlinewidth 10 10 moveto 100 10 lineto stroke showpage");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.pages.len(), 1);
    assert_eq!(
        ops(&run.pages[0]),
        [
            IrOp::LineWidth(2.0),
            IrOp::Stroke {
                path: vec![Seg::Move(p(10.0, 10.0)), Seg::Line(p(100.0, 10.0))],
                ctm: Matrix::IDENTITY,
            },
        ]
    );
    assert_eq!(
        run.pages[0].dump(),
        "ir/1\npage 612 792\nresources:\nops:\nw 2\nm 10 10\nl 100 10\nS\n"
    );
}

// unpainted-paths.ps
#[test]
fn unpainted_paths_leave_no_trace() {
    let run = exec("0 0 moveto 5 5 lineto newpath 1 1 moveto 2 2 lineto stroke showpage");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        ops(&run.pages[0]),
        [IrOp::Stroke {
            path: vec![Seg::Move(p(1.0, 1.0)), Seg::Line(p(2.0, 2.0))],
            ctm: Matrix::IDENTITY,
        }]
    );
}

// gsave-grestore-no-bloat.ps
#[test]
fn gsave_grestore_does_not_bloat_the_ir() {
    let run = exec(
        "50 { gsave 3 setlinewidth 0.5 setgray grestore } repeat \
         0 0 moveto 10 0 lineto 10 10 lineto closepath fill showpage",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let ops = ops(&run.pages[0]);
    assert_eq!(ops.len(), 1);
    assert!(matches!(
        ops[0],
        IrOp::Fill {
            rule: FillRule::NonZero,
            ..
        }
    ));
}

// separation-survives.ps
#[test]
fn separation_survives() {
    let run = exec(
        "[/Separation /Spot /DeviceCMYK {dup 0 0 0}] setcolorspace 0.6 setcolor \
         0 0 moveto 10 0 lineto 10 10 lineto closepath fill showpage",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    assert_eq!(
        page.resources.color_spaces,
        [SpaceSpec::Separation {
            name: b"Spot".to_vec(),
            alternate: Box::new(SpaceSpec::DeviceCMYK),
            tint_source: b"{dup 0 0 0}".to_vec(),
        }]
    );
    let ops = ops(page);
    assert_eq!(ops[0], IrOp::SetColorSpace(SpaceRef(0)));
    assert_eq!(ops[1], IrOp::SetColor(vec![0.6]));
    assert!(matches!(ops[2], IrOp::Fill { .. }));
    assert!(
        page.dump()
            .contains("cs 0 Separation (Spot) alt=DeviceCMYK tint=11 bytes\n")
    );
    assert!(page.dump().contains("\ncs 0\nsc 0.6\n"));
}

// translate-then-draw.ps
#[test]
fn translate_then_draw() {
    let run = exec("72 72 translate 0 0 moveto 72 0 lineto stroke showpage");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        ops(&run.pages[0]),
        [IrOp::Stroke {
            path: vec![Seg::Move(p(72.0, 72.0)), Seg::Line(p(144.0, 72.0))],
            ctm: Matrix::translation(72.0, 72.0),
        }]
    );
    assert!(
        run.pages[0]
            .dump()
            .contains("stroke-ctm 1 0 0 1 72 72\nm 72 72\n")
    );
}

// restore-restores-linewidth.ps
#[test]
fn restore_restores_line_width() {
    let run = exec("1 setlinewidth save 5 setlinewidth restore currentlinewidth =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "1.0\n");
    // The current path is part of the state too (PLRM3 §4.2).
    let run = exec(
        "10 10 moveto gsave 20 20 lineto grestore currentpoint = = \
         save 30 30 lineto restore currentpoint = =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "10.0\n10.0\n10.0\n10.0\n");
}

#[test]
fn clip_inside_gsave_is_bracketed() {
    let run = exec(
        "gsave 10 10 50 50 rectclip 0 setgray 0 0 moveto 100 100 lineto stroke grestore \
         0 0 moveto 5 5 lineto stroke showpage",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.pages[0].dump(),
        "ir/1\npage 612 792\nresources:\nops:\nq\nm 10 10\nl 60 10\nl 60 60\nl 10 60\nh\nW n\n\
         m 0 0\nl 100 100\nS\nQ\nm 0 0\nl 5 5\nS\n"
    );
}

#[test]
fn pages_are_delivered_in_order_and_the_dump_is_stable() {
    let program = "0.5 setgray 0 0 10 10 rectfill showpage 1 0 0 setrgbcolor 0 0 5 5 rectfill \
                   copypage 0 0 1 1 rectfill showpage";
    let first = exec(program);
    let second = exec(program);
    assert_eq!(first.pages.len(), 3);
    assert_eq!(first.pages, second.pages);
    let text = dump::pages(&first.pages);
    assert_eq!(text, dump::pages(&second.pages));
    assert_eq!(text.matches("ir/1\n").count(), 3);
    assert!(text.contains("\n\nir/1\n"));
    assert!(
        first.pages[0]
            .dump()
            .starts_with("ir/1\npage 612 792\nresources:\nops:\nsc 0.5\n")
    );
    assert!(first.pages[1].dump().contains("cs 0 DeviceRGB\n"));
}

#[test]
fn media_box_follows_the_page_device() {
    let run = exec("<< /PageSize [200 100] >> setpagedevice 0 0 1 1 rectfill showpage showpage");
    assert_eq!(run.outcome, Outcome::Ok);
    assert!(run.pages[0].dump().starts_with("ir/1\npage 200 100\n"));
    assert!(
        run.pages[1]
            .dump()
            .starts_with("ir/1\npage 200 100\nresources:\nops:\n")
    );
}

#[test]
fn arcto_returns_tangent_points_to_the_program() {
    let run = exec("0 0 moveto 100 0 100 100 10 arcto 4 { = } repeat");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "10.0\n100.0\n0.0\n90.0\n");
    let run = exec("0 0 moveto 100 0 100 100 -1 arcto");
    assert!(matches!(run.outcome, Outcome::Error(e) if e.name == "undefinedresult"));
}

// --- text -----------------------------------------------------------------------------

use ps_graphics::{FontIndex, FontSpec};
use ps_vm::Glyph;

const SQUARE: &str = "/Sq 7 dict dup begin /FontType 3 def /FontMatrix [0.001 0 0 0.001 0 0] def \
    /Encoding StandardEncoding def /FontBBox [0 0 1000 1000] def \
    /BuildGlyph { pop pop 1000 0 0 0 1000 1000 setcachedevice 0 0 1000 1000 rectfill } def \
    end definefont pop ";

fn near(m: Matrix, want: [f32; 6]) -> bool {
    m.0.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-5)
}

fn text_ops(page: &Page) -> Vec<(FontIndex, Matrix, Vec<Glyph>)> {
    page.ops
        .iter()
        .filter_map(|o| match &o.op {
            IrOp::Text {
                font,
                matrix,
                glyphs,
            } => Some((*font, *matrix, glyphs.clone())),
            _ => None,
        })
        .collect()
}

// text-operation-shape.ps
#[test]
fn text_operation_shape() {
    let run = exec("/Helvetica findfont 12 scalefont setfont 100 700 moveto (Hi) show showpage");
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    assert_eq!(page.resources.fonts.len(), 1);
    assert!(matches!(
        &page.resources.fonts[0],
        FontSpec::Resident { base: ps_fonts::StdFont::Helvetica, encoding }
            if encoding[72].as_deref() == Some(b"H".as_slice()) && encoding[0].is_none()
    ));
    let text = text_ops(page);
    assert_eq!(text.len(), 1);
    assert_eq!(page.ops.len(), 1);
    assert_eq!(text[0].0, FontIndex(0));
    assert!(near(text[0].1, [0.012, 0.0, 0.0, 0.012, 100.0, 700.0]));
    assert_eq!(
        text[0].2,
        [
            Glyph {
                code: 72,
                dx: 722.0,
                dy: 0.0
            },
            Glyph {
                code: 105,
                dx: 222.0,
                dy: 0.0
            }
        ]
    );
    assert_eq!(
        page.dump(),
        "ir/1\npage 612 792\nresources:\nfont 0 Helvetica\nops:\n\
         text 0 0.012 0 0 0.012 100 700 (Hi) 722 0 222 0\n"
    );
}

// type3-square-glyph.ps
#[test]
fn a_square_glyph_is_captured_and_the_point_advances() {
    let run = exec(&format!(
        "{SQUARE} /Sq findfont 20 scalefont setfont 10 10 moveto (a) show \
         currentpoint round cvi = round cvi = showpage"
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "10\n30\n");
    let page = &run.pages[0];
    assert_eq!(
        page.dump(),
        "ir/1\npage 612 792\nresources:\n\
         font 0 type3 0.001 0 0 0.001 0 0 bbox=[0 0 1000 1000] enc=[97 /a]\n\
         glyph /a 1000 0 [0 0 1000 1000] {\n  m 0 0\n  l 1000 0\n  l 1000 1000\n  l 0 1000\n  h\n  f\n}\n\
         ops:\ntext 0 0.02 0 0 0.02 10 10 (a) 1000 0\n"
    );
}

// type3-stringwidth-paints-nothing.ps
#[test]
fn stringwidth_paints_nothing() {
    let run = exec(&format!(
        "{SQUARE} /Sq findfont 20 scalefont setfont (a) stringwidth round cvi = round cvi = showpage"
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0\n20\n");
    assert!(run.pages[0].ops.is_empty());
    assert!(run.pages[0].resources.fonts.is_empty());
}

// type3-glyph-shown-twice.ps
#[test]
fn a_glyph_shown_twice_yields_one_procedure() {
    let run = exec(&format!(
        "{SQUARE} /Sq findfont 20 scalefont setfont 10 10 moveto (aa) show showpage"
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    let FontSpec::Type3 { glyphs, .. } = &page.resources.fonts[0] else {
        panic!("a Type 3 resource");
    };
    assert_eq!(glyphs.len(), 1);
    let text = text_ops(page);
    assert_eq!(text.len(), 1);
    assert_eq!(text[0].2.len(), 2);
    assert!(text[0].2.iter().all(|g| g.code == 97 && g.dx == 1000.0));
}

// type3-glyph-space.ps
#[test]
fn type3_glyphs_are_captured_in_glyph_space() {
    let run = exec(
        "/Sm << /FontType 3 /FontMatrix [0.01 0 0 0.01 0 0] /Encoding StandardEncoding \
         /FontBBox [0 0 100 100] \
         /BuildGlyph { pop pop 60 0 0 0 50 50 setcachedevice 0 0 50 50 rectfill } >> definefont pop \
         2 2 scale /Sm findfont setfont 5 5 moveto (a) show showpage",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    let FontSpec::Type3 { glyphs, .. } = &page.resources.fonts[0] else {
        panic!("a Type 3 resource");
    };
    let IrOp::Fill { path, .. } = &glyphs[b"a".as_slice()].ops[0].op else {
        panic!("a fill");
    };
    assert_eq!(
        path,
        &[
            Seg::Move(p(0.0, 0.0)),
            Seg::Line(p(50.0, 0.0)),
            Seg::Line(p(50.0, 50.0)),
            Seg::Line(p(0.0, 50.0)),
            Seg::Close
        ]
    );
    let text = text_ops(page);
    assert!(near(text[0].1, [0.02, 0.0, 0.0, 0.02, 10.0, 10.0]));
}

// text-under-clip-and-colour.ps
#[test]
fn text_under_a_clip_and_colour() {
    let run = exec(
        "/Helvetica findfont 12 scalefont setfont 0 0 200 200 rectclip 1 0 0 setrgbcolor \
         50 50 moveto (Hi) show showpage",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.pages[0].dump(),
        "ir/1\npage 612 792\nresources:\ncs 0 DeviceRGB\nfont 0 Helvetica\nops:\n\
         q\nm 0 0\nl 200 0\nl 200 200\nl 0 200\nh\nW n\ncs 0\nsc 1 0 0\n\
         text 0 0.012 0 0 0.012 50 50 (Hi) 722 0 222 0\nQ\n"
    );
}

// xshow-text-op.ps
#[test]
fn xshow_displacements_reach_the_text_op() {
    let run =
        exec("/Helvetica findfont 10 scalefont setfont 0 0 moveto (abc) [10 20 30] xshow showpage");
    assert_eq!(run.outcome, Outcome::Ok);
    let text = text_ops(&run.pages[0]);
    let displacements: Vec<f32> = text[0].2.iter().map(|g| g.dx).collect();
    assert!(
        displacements
            .iter()
            .zip([1000.0, 2000.0, 3000.0])
            .all(|(a, b)| (a - b).abs() < 1e-2),
        "{displacements:?}"
    );
    assert!(
        run.pages[0]
            .dump()
            .ends_with("text 0 0.01 0 0 0.01 0 0 (abc) 1000 0 2000 0 3000 0\n")
    );
}

#[test]
fn a_showpage_inside_a_glyph_is_invalidaccess_and_the_glyph_is_discarded() {
    let run = exec(
        "/Bad << /FontType 3 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding StandardEncoding \
         /BuildGlyph { pop pop 500 0 setcharwidth 0 0 100 100 rectfill showpage } >> definefont \
         10 scalefont setfont 0 0 moveto { (a) show } stopped pop 0 0 5 5 rectfill showpage",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    assert!(page.resources.fonts.is_empty());
    assert_eq!(page.ops.len(), 1);
    assert!(matches!(page.ops[0].op, IrOp::Fill { .. }));
}

#[test]
fn a_nested_run_and_a_charwidth_glyph_keep_their_own_colour() {
    let run = exec(
        "/Nest << /FontType 3 /FontMatrix [0.01 0 0 0.01 0 0] /Encoding StandardEncoding \
         /BuildChar { exch pop pop 50 0 setcharwidth 0 0 1 setrgbcolor \
         /Helvetica findfont 40 scalefont setfont 0 0 moveto (H) show } >> definefont \
         10 scalefont setfont 1 0 0 setrgbcolor 0 0 moveto (b) show showpage",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    assert_eq!(page.resources.fonts.len(), 2);
    let FontSpec::Type3 { glyphs, .. } = &page.resources.fonts[1] else {
        panic!("the Type 3 resource comes second: its nested font was interned first");
    };
    let proc_ = &glyphs[b"b".as_slice()];
    assert_eq!(proc_.width, (50.0, 0.0));
    assert_eq!(proc_.bbox, None);
    let kinds: Vec<String> = proc_
        .ops
        .iter()
        .map(|o| {
            format!("{:?}", o.op)
                .split(['(', ' ', '{'])
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(kinds, ["SetColorSpace", "SetColor", "Text"]);
    let IrOp::Text { font, matrix, .. } = &proc_.ops[2].op else {
        panic!("a nested run");
    };
    assert_eq!(*font, FontIndex(0));
    assert!(near(*matrix, [0.04, 0.0, 0.0, 0.04, 0.0, 0.0]));
}
