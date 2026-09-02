// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The spec scenarios through the interpreter: each runs a program with
//! `distill` and checks the document it left. Each has a corpus file
//! under `corpus/unit/graphics/` whose sidecar goldens pin the dump and
//! the bytes.

mod support;

use std::path::Path;

use ps_vm::{Config, Io, Outcome};
use remelt::{Options, Report};
use support::{
    Value, array, check, color_space, content, decoded, kids, media_box, number, resources, xobject,
};

struct Run {
    report: Report,
    pdf: Vec<u8>,
    output: String,
}

fn distil_with(program: &str, options: &Options) -> Run {
    let (io, out, _) = Io::capture();
    let config = Config {
        io,
        ..Default::default()
    };
    let (report, pdf) = remelt::distill(program.as_bytes(), config, options, Vec::new()).unwrap();
    Run {
        report,
        pdf,
        output: out.text(),
    }
}

fn distil(program: &str) -> Run {
    distil_with(program, &Options { compress: false })
}

// stroked-line.ps
#[test]
fn a_stroked_line_distils() {
    let run = distil("2 setlinewidth 10 10 moveto 100 10 lineto stroke showpage");
    assert_eq!(
        run.report,
        Report {
            outcome: Outcome::Ok,
            pages: 1
        }
    );
    let pdf = check(&run.pdf);
    assert_eq!(kids(&pdf).len(), 1);
    assert_eq!(media_box(&pdf, 0), [0.0, 0.0, 612.0, 792.0]);
    assert_eq!(content(&pdf, 0), "2 w\n10 10 m\n100 10 l\nS\n");
}

// three-pages.ps
#[test]
fn three_pages_in_order() {
    let run = distil(
        "0 0 100 100 rectfill showpage\n\
         100 100 100 100 rectfill showpage\n\
         200 200 100 100 rectfill showpage",
    );
    assert_eq!(run.report.pages, 3);
    let pdf = check(&run.pdf);
    assert_eq!(kids(&pdf).len(), 3);
    for k in 0..3 {
        let origin = 100 * k;
        let text = content(&pdf, k);
        assert!(
            text.starts_with(&format!("{origin} {origin} m\n")),
            "page {k}: {text}"
        );
        assert!(text.ends_with("h\nf\n"), "page {k}: {text}");
    }
}

#[test]
fn a_job_with_no_pages() {
    let run = distil("1 2 add =");
    assert_eq!(run.output, "3\n");
    assert_eq!(
        run.report,
        Report {
            outcome: Outcome::Ok,
            pages: 0
        }
    );
    let pdf = check(&run.pdf);
    assert!(kids(&pdf).is_empty());
}

// eofill-eoclip.ps
#[test]
fn even_odd_fill_and_clip() {
    let run = distil(
        "50 90 moveto 20 10 lineto 90 60 lineto 10 60 lineto 80 10 lineto closepath\n\
         eoclip eofill showpage",
    );
    let pdf = check(&run.pdf);
    let star = "50 90 m\n20 10 l\n90 60 l\n10 60 l\n80 10 l\nh\n";
    assert_eq!(content(&pdf, 0), format!("q\n{star}W* n\n{star}f*\nQ\n"));
}

// dash-parameters.ps
#[test]
fn dash_and_line_parameters() {
    let run = distil(
        "[3 1] 0 setdash 1 setlinecap 2 setlinejoin 4 setmiterlimit\n\
         10 10 moveto 100 10 lineto stroke showpage",
    );
    let pdf = check(&run.pdf);
    assert_eq!(
        content(&pdf, 0),
        "1 J\n2 j\n4 M\n[3 1] 0 d\n10 10 m\n100 10 l\nS\n"
    );
}

// scaled-stroke.ps
#[test]
fn scaled_stroke_keeps_its_width() {
    let run = distil("2 2 scale 1 setlinewidth 5 5 moveto 50 5 lineto stroke showpage");
    let pdf = check(&run.pdf);
    // A width of 1 is PDF's initial value too, so nothing sets it.
    assert_eq!(content(&pdf, 0), "q\n2 0 0 2 0 0 cm\n5 5 m\n50 5 l\nS\nQ\n");
}

// translate-then-draw.ps covers the IR; the PDF form is pinned here.
#[test]
fn translated_stroke_needs_no_width_change() {
    let run = distil("10 20 translate 0 0 moveto 30 0 lineto stroke showpage");
    let pdf = check(&run.pdf);
    assert_eq!(
        content(&pdf, 0),
        "q\n1 0 0 1 10 20 cm\n0 0 m\n30 0 l\nS\nQ\n"
    );
}

// separation-survives.ps
#[test]
fn separation_reaches_the_page_resources() {
    let run = distil(
        "[/Separation /Spot /DeviceCMYK {0 0 0 4 -1 roll}] setcolorspace 0.6 setcolor\n\
         0 0 moveto 10 0 lineto 10 10 lineto closepath fill showpage",
    );
    let pdf = check(&run.pdf);
    assert_eq!(
        content(&pdf, 0),
        "/CS0 cs\n0.6 scn\n0 0 m\n10 0 l\n10 10 l\nh\nf\n"
    );
    let space = array(color_space(&pdf, 0, "CS0"));
    assert_eq!(space[0].as_name(), b"Separation");
    assert_eq!(space[1].as_name(), b"Spot");
    assert_eq!(space[2].as_name(), b"DeviceCMYK");
    let function = pdf.resolve(space[3].as_reference());
    assert_eq!(function.get("FunctionType").unwrap().as_int(), 4);
    assert_eq!(array(function.get("Domain").unwrap()).len(), 2);
    assert_eq!(array(function.get("Range").unwrap()).len(), 8);
    assert_eq!(decoded(function), b"{0 0 0 4 -1 roll}");
}

#[test]
fn device_colour_is_direct() {
    let run = distil("0.2 0.4 0.6 setrgbcolor 0 0 10 10 rectfill showpage");
    let pdf = check(&run.pdf);
    assert!(
        content(&pdf, 0).starts_with("/DeviceRGB cs\n0.2 0.4 0.6 rg\n"),
        "{}",
        content(&pdf, 0)
    );
    assert!(resources(&pdf, 0).get("ColorSpace").is_none());
}

// gray-image.ps
#[test]
fn a_two_by_two_gray_image() {
    let run = distil(
        "100 100 translate 50 50 scale\n\
         2 2 8 [2 0 0 -2 0 2] <0055AAFF> image showpage",
    );
    let pdf = check(&run.pdf);
    assert_eq!(content(&pdf, 0), "q 50 0 0 50 100 100 cm /Im0 Do Q\n");
    let image = xobject(&pdf, 0, "Im0");
    assert_eq!(image.get("Width").unwrap().as_int(), 2);
    assert_eq!(image.get("Height").unwrap().as_int(), 2);
    assert_eq!(image.get("BitsPerComponent").unwrap().as_int(), 8);
    assert_eq!(image.get("ColorSpace").unwrap().as_name(), b"DeviceGray");
    assert!(image.get("Decode").is_none());
    assert_eq!(decoded(image), [0x00, 0x55, 0xAA, 0xFF]);
}

// imagemask-in-separation.ps
#[test]
fn an_image_mask_paints_the_current_colour() {
    let run = distil(
        "[/Separation /Spot /DeviceCMYK {0 0 0 4 -1 roll}] setcolorspace 0.6 setcolor\n\
         100 100 translate 50 50 scale\n\
         2 2 true [2 0 0 -2 0 2] <4080> imagemask showpage",
    );
    let pdf = check(&run.pdf);
    assert_eq!(
        content(&pdf, 0),
        "/CS0 cs\n0.6 scn\nq 50 0 0 50 100 100 cm /Im0 Do Q\n"
    );
    let mask = xobject(&pdf, 0, "Im0");
    assert_eq!(mask.get("ImageMask"), Some(&Value::Bool(true)));
    assert!(mask.get("ColorSpace").is_none());
    let decode: Vec<f64> = array(mask.get("Decode").unwrap())
        .iter()
        .map(number)
        .collect();
    assert_eq!(decode, [1.0, 0.0]);
    assert_eq!(
        array(color_space(&pdf, 0, "CS0"))[0].as_name(),
        b"Separation"
    );
}

// error-after-page.ps
#[test]
fn error_after_the_first_page() {
    let run = distil("0 0 100 100 rectfill showpage nosuchname");
    match &run.report.outcome {
        Outcome::Error(summary) => assert_eq!(summary.name, "undefined"),
        other => panic!("expected an error outcome, got {other:?}"),
    }
    assert_eq!(run.report.pages, 1);
    let pdf = check(&run.pdf);
    assert_eq!(kids(&pdf).len(), 1);
}

#[test]
fn two_runs_agree_on_every_corpus_graphics_file() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/unit/graphics");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "ps"))
        .collect();
    files.sort();
    assert!(!files.is_empty());
    for path in files {
        let program = std::fs::read_to_string(&path).unwrap();
        for options in [Options { compress: false }, Options { compress: true }] {
            let first = distil_with(&program, &options);
            let second = distil_with(&program, &options);
            assert_eq!(first.report, second.report, "{}", path.display());
            assert!(
                first.pdf == second.pdf,
                "{} differs between runs",
                path.display()
            );
            check(&first.pdf);
        }
    }
}

#[test]
fn the_default_options_compress() {
    let run = distil_with("0 0 10 10 rectfill showpage", &Options::default());
    let pdf = check(&run.pdf);
    let contents = support::page(&pdf, 0)
        .get("Contents")
        .unwrap()
        .as_reference();
    assert_eq!(
        pdf.resolve(contents).get("Filter").unwrap().as_name(),
        b"FlateDecode"
    );
    assert!(content(&pdf, 0).ends_with("h\nf\n"));
}
