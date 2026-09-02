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
