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
    exec_bytes(program.as_bytes())
}

fn exec_bytes(program: &[u8]) -> Run {
    let (io, out, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let pages = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Graphics::new(pages.clone())));
    let outcome = interp.run(&mut SliceSource::new(program));
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
                ..
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
        FontSpec::Resident { base: ps_fonts::ResidentFace::Helvetica, encoding }
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
            Glyph::simple(72, 722.0, 0.0),
            Glyph::simple(105, 222.0, 0.0)
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

// --- embedded fonts ------------------------------------------------------------------

use ps_fonts::ProgramKind;
use ps_fonts::testing::{corpus_truetype, corpus_type1};

fn with_syn(program: &str) -> String {
    format!("{}{program}", corpus_type1().pfa())
}

fn fills(page: &Page) -> Vec<Vec<Seg>> {
    page.ops
        .iter()
        .filter_map(|o| match &o.op {
            IrOp::Fill { path, .. } => Some(path.clone()),
            _ => None,
        })
        .collect()
}

fn seg_near(seg: &Seg, want: &Seg) -> bool {
    match (seg, want) {
        (Seg::Move(a), Seg::Move(b)) | (Seg::Line(a), Seg::Line(b)) => {
            (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
        }
        (Seg::Curve(a, b, c), Seg::Curve(d, e, f)) => [(a, d), (b, e), (c, f)]
            .iter()
            .all(|(p, q)| (p.x - q.x).abs() < 1e-3 && (p.y - q.y).abs() < 1e-3),
        (Seg::Close, Seg::Close) => true,
        _ => false,
    }
}

fn path_near(path: &[Seg], want: &[Seg]) -> bool {
    path.len() == want.len() && path.iter().zip(want).all(|(a, b)| seg_near(a, b))
}

// embedded-font-dump.ps
#[test]
fn an_embedded_font_is_one_resource_and_the_dump_lists_it_without_its_bytes() {
    let run = exec(&with_syn(
        "/Syn findfont 12 scalefont setfont 72 700 moveto (a) show showpage",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    assert_eq!(page.resources.fonts.len(), 1);
    assert!(matches!(
        &page.resources.fonts[0],
        FontSpec::Embedded { kind: ProgramKind::Type1, font_name, program, .. }
            if font_name == b"Syn" && program.glyph_count() == 6
    ));
    let text = text_ops(page);
    assert_eq!(text.len(), 1);
    assert_eq!(text[0].0, FontIndex(0));
    assert_eq!(text[0].2, [Glyph::simple(97, 600.0, 0.0)]);
    assert_eq!(
        page.dump(),
        "ir/1\npage 612 792\nresources:\n\
         font 0 embedded type1 Syn glyphs=6 enc=[97 /a 98 /b 101 /e 233 /eacute]\n\
         ops:\ntext 0 0.012 0 0 0.012 72 700 (a) 600 0\n"
    );
}

// type1-embedded-two-pages.ps
#[test]
fn every_page_gets_its_own_resource_over_the_shared_snapshot() {
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont 100 100 moveto (a) show showpage \
         /Syn findfont 20 scalefont setfont 100 200 moveto (e) show showpage",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.pages.len(), 2);
    let (
        FontSpec::Embedded { program: first, .. },
        FontSpec::Embedded {
            program: second, ..
        },
    ) = (
        &run.pages[0].resources.fonts[0],
        &run.pages[1].resources.fonts[0],
    )
    else {
        panic!("embedded resources on both pages");
    };
    assert_eq!(first, second, "the same snapshot");
    assert_eq!(
        run.pages[0].resources.fonts[0],
        run.pages[1].resources.fonts[0]
    );
}

// charpath-fill.ps
#[test]
fn filling_a_charpath_records_one_fill_and_no_text() {
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont 100 100 moveto (a) false charpath fill showpage",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    assert!(text_ops(page).is_empty());
    let fills = fills(page);
    assert_eq!(fills.len(), 1);
    assert_eq!(page.ops.len(), 1);
    // The square at (100, 100) scaled by 0.01 — the program's moveto is
    // replaced by the outline's own — and the trailing point charpath
    // leaves at the advance.
    assert!(
        path_near(
            &fills[0],
            &[
                Seg::Move(p(100.5, 100.0)),
                Seg::Line(p(105.5, 100.0)),
                Seg::Line(p(105.5, 105.0)),
                Seg::Line(p(100.5, 105.0)),
                Seg::Close,
                Seg::Move(p(106.0, 100.0)),
            ]
        ),
        "{:?}",
        fills[0]
    );
    assert!(page.resources.fonts.is_empty());
}

// type1-seac-glyphshow.ps
#[test]
fn a_seac_glyph_shows_with_the_composite_advance_and_outlines_both_components() {
    let run = exec(&with_syn(
        "/Syn findfont 10 scalefont setfont 0 0 moveto /eacute glyphshow \
         100 100 moveto (\\351) false charpath fill showpage",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    let text = text_ops(page);
    assert_eq!(text.len(), 1);
    assert_eq!(text[0].2, [Glyph::simple(233, 500.0, 0.0)]);
    let fills = fills(page);
    assert_eq!(fills.len(), 1);
    // The base e at its sidebearing, then the accent displaced by
    // (sbx - asb + adx, ady) = (140, 20) glyph units.
    let outline: Vec<Seg> = fills[0].clone();
    assert!(
        path_near(
            &outline,
            &[
                Seg::Move(p(100.2, 100.0)),
                Seg::Line(p(104.2, 100.0)),
                Seg::Line(p(104.2, 104.0)),
                Seg::Close,
                Seg::Move(p(101.7, 105.2)),
                Seg::Line(p(102.7, 106.2)),
                Seg::Close,
                Seg::Move(p(105.0, 100.0)),
            ]
        ),
        "{outline:?}"
    );
}

// type42-charpath-bbox.ps
#[test]
fn a_type42_charpath_box_is_the_cubic_control_box() {
    let run = exec(&format!(
        "{}/SynTT findfont [20 0 0 20 0 -5] makefont setfont 0 0 moveto (o) true charpath \
         pathbbox 4 {{ 4 -1 roll 1000 mul round 1000 div }} repeat \
         4 -1 roll = 3 -1 roll = exch = = fill showpage",
        corpus_truetype().type42("SynTT", &[(97, "a"), (111, "o")])
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0.977\n-3.372\n11.719\n3.138\n");
    let page = &run.pages[0];
    let fills = fills(page);
    assert_eq!(fills.len(), 1);
    let curves = fills[0]
        .iter()
        .filter(|s| matches!(s, Seg::Curve(..)))
        .count();
    assert_eq!(curves, 2);
    let scale = 20.0 / 2048.0;
    let ys: Vec<f32> = fills[0][..fills[0].len() - 1]
        .iter()
        .flat_map(|s| match s {
            Seg::Move(a) | Seg::Line(a) => vec![a.y],
            Seg::Curve(a, b, c) => vec![a.y, b.y, c.y],
            Seg::Close => Vec::new(),
        })
        .collect();
    let lo = ys.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    assert!((lo - (500.0 / 3.0 * scale - 5.0)).abs() < 1e-3, "{lo}");
    assert!((hi - (2500.0 / 3.0 * scale - 5.0)).abs() < 1e-3, "{hi}");
}

// --- composite fonts -----------------------------------------------------------------

use ps_fonts::testing::{corpus_cid_cff, corpus_cmap};

/// The corpus CID-keyed CFF as a FontSet, followed by `program`.
fn with_cid_set(program: &str) -> Vec<u8> {
    let mut out = corpus_cid_cff().font_set("SynCIDSet");
    out.extend_from_slice(program.as_bytes());
    out
}

fn cid_glyph(code: u32, len: u8, cid: u16, dx: f32, dy: f32) -> Glyph {
    Glyph {
        code,
        len,
        cid,
        dx,
        dy,
    }
}

// composite-dump.ps, composite-two-byte-width.ps
#[test]
fn a_composite_run_is_one_text_op_over_a_composite_resource() {
    let run = exec_bytes(&with_cid_set(
        "/SynComposite /Identity-H [ /SynCID /CIDFont findresource ] composefont \
         10 scalefont setfont 72 700 moveto <00010002> show currentpoint showpage",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    assert_eq!(page.resources.fonts.len(), 1);
    let FontSpec::Composite {
        cmap_name,
        wmode,
        unicode_based,
        descendant,
        cid_to_code,
    } = &page.resources.fonts[0]
    else {
        panic!("a composite resource, got {:?}", page.resources.fonts[0]);
    };
    assert_eq!(cmap_name, b"Identity-H");
    assert_eq!(*wmode, 0);
    assert!(!unicode_based);
    assert!(matches!(
        &**descendant,
        FontSpec::Embedded { kind: ProgramKind::Cff, font_name, .. } if font_name == b"SynCID"
    ));
    assert_eq!(cid_to_code.get(&1), Some(&(1, 2)));
    assert_eq!(cid_to_code.get(&2), Some(&(2, 2)));
    let text = text_ops(page);
    assert_eq!(text.len(), 1);
    assert!(near(text[0].1, [0.01, 0.0, 0.0, 0.01, 72.0, 700.0]));
    assert_eq!(
        text[0].2,
        [
            cid_glyph(1, 2, 1, 500.0, 0.0),
            cid_glyph(2, 2, 2, 700.0, 0.0)
        ]
    );
    assert_eq!(
        page.dump(),
        "ir/1\npage 612 792\nresources:\n\
         font 0 composite Identity-H wmode=0 cff SynCID glyphs=6\n\
         ops:\ntext 0 0.01 0 0 0.01 72 700 <00010002> 500 0 700 0\n"
    );
}

// composite-mixed-lengths.ps, composite-partial-match.ps
#[test]
fn mixed_byte_lengths_dump_each_code_padded_to_its_length() {
    let program = format!(
        "{}/SynMixed /Syn-H [ /SynCID /CIDFont findresource ] composefont 10 scalefont setfont \
         72 700 moveto <41814042> show 0 0 moveto <41812042> show showpage",
        corpus_cmap()
    );
    let run = exec_bytes(&with_cid_set(&program));
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    let text = text_ops(page);
    assert_eq!(text.len(), 2);
    assert_eq!(
        text[0].2,
        [
            cid_glyph(0x41, 1, 34, 500.0, 0.0),
            cid_glyph(0x8140, 2, 200, 300.0, 0.0),
            cid_glyph(0x42, 1, 35, 700.0, 0.0),
        ]
    );
    assert_eq!(
        text[1].2[1],
        cid_glyph(0x8120, 2, 0, 250.0, 0.0),
        "the notdef keeps its two bytes"
    );
    let dump = page.dump();
    assert!(dump.contains("font 0 composite Syn-H wmode=0 cff SynCID glyphs=6\n"));
    assert!(dump.contains("text 0 0.01 0 0 0.01 72 700 <41814042> 500 0 300 0 700 0\n"));
    assert!(dump.contains("text 0 0.01 0 0 0.01 0 0 <41812042> 500 0 250 0 700 0\n"));
}

// composite-vertical-width.ps
#[test]
fn a_vertical_run_carries_its_writing_mode() {
    let run = exec_bytes(&with_cid_set(
        "/r { 1000 mul round 1000 div } def \
         /SynV /Identity-V [ /SynCID /CIDFont findresource ] composefont 10 scalefont setfont \
         0 100 moveto <0001> show currentpoint r exch r = = showpage",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0.0\n90.0\n");
    let page = &run.pages[0];
    let modes: Vec<u8> = page
        .ops
        .iter()
        .filter_map(|o| match &o.op {
            IrOp::Text { wmode, .. } => Some(*wmode),
            _ => None,
        })
        .collect();
    assert_eq!(modes, [1]);
    assert_eq!(
        page.dump(),
        "ir/1\npage 612 792\nresources:\n\
         font 0 composite Identity-V wmode=1 cff SynCID glyphs=6\n\
         ops:\ntext 0 0.01 0 0 0.01 0 100 <0001> 0 -1000 wmode=1\n"
    );
}

// composite-charpath-fill.ps
#[test]
fn charpath_through_a_composite_font_fills_the_outline() {
    let run = exec_bytes(&with_cid_set(
        "/SynComposite /Identity-H [ /SynCID /CIDFont findresource ] composefont \
         10 scalefont setfont 100 100 moveto <0001> false charpath fill showpage",
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    assert!(text_ops(page).is_empty());
    assert!(page.resources.fonts.is_empty());
    let fills = fills(page);
    assert_eq!(fills.len(), 1);
    assert!(
        path_near(
            &fills[0],
            &[
                Seg::Move(p(100.5, 100.0)),
                Seg::Line(p(104.5, 100.0)),
                Seg::Line(p(104.5, 104.0)),
                Seg::Line(p(100.5, 104.0)),
                Seg::Close,
                Seg::Move(p(105.0, 100.0)),
            ]
        ),
        "{:?}",
        fills[0]
    );
}

#[test]
fn a_simple_descendant_shows_as_that_font_with_the_cid_as_its_code() {
    let run = exec(
        "/H /Identity-H [ /Helvetica findfont ] composefont 12 scalefont setfont \
         100 700 moveto <00480069> show showpage",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let page = &run.pages[0];
    assert!(matches!(
        &page.resources.fonts[0],
        FontSpec::Resident {
            base: ps_fonts::ResidentFace::Helvetica,
            ..
        }
    ));
    assert_eq!(
        page.dump(),
        "ir/1\npage 612 792\nresources:\nfont 0 Helvetica\n\
         ops:\ntext 0 0.012 0 0 0.012 100 700 <00480069> 722 0 222 0\n"
    );
}

// --- printer identity: screens, transfers, pathforall ------------------------------

// identity/screen-round-trip.ps
#[test]
fn screens_and_transfers_follow_the_graphics_state_and_leave_the_ir_alone() {
    let run = exec(
        "60 45 { pop } setscreen currentscreen pop pop = \
         gsave 30 0 { pop } setscreen { 1 exch sub } settransfer grestore \
         currentscreen pop pop = currenttransfer == \
         { dup mul } settransfer initgraphics currenttransfer == \
         save 10 10 { pop } setscreen restore currentscreen pop pop = \
         0 0 100 100 rectfill showpage",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "60.0\n60.0\n{}\n{dup mul}\n60.0\n");
    assert_eq!(run.pages.len(), 1);
    assert!(matches!(ops(&run.pages[0]).as_slice(), [IrOp::Fill { .. }]));
}

// graphics/pathforall-enumerates.ps
#[test]
fn pathforall_reports_segments_in_user_space() {
    let run = exec(
        "2 2 scale 10 10 moveto 20 10 lineto 30 30 40 40 50 10 curveto closepath \
         { (m) = pop pop } { (l) = pop pop } { (c) = 6 { pop } repeat } { (h) = } pathforall \
         currentpoint = = \
         1 1 moveto { 4 4 scale } { } { } { } pathforall currentpoint = = ",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    // The path survives the enumeration, and a CTM change inside a
    // procedure (run twice: the path now has two moves) does not move
    // the remaining segments.
    assert_eq!(run.output, "m\nl\nc\nh\n10.0\n10.0\n0.0625\n0.0625\n");
}
