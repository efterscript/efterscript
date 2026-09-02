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
            pages: 1,
            substitutions: Vec::new(),
            notes: Vec::new(),
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
            pages: 0,
            substitutions: Vec::new(),
            notes: Vec::new(),
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
fn two_runs_agree_on_every_corpus_graphics_and_text_file() {
    let unit = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/unit");
    let mut files: Vec<_> = ["graphics", "text", "fonts"]
        .into_iter()
        .flat_map(|dir| std::fs::read_dir(unit.join(dir)).unwrap().flatten())
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

// --- text -----------------------------------------------------------------------------

use ps_vm::FontSubstitution;
use support::{font, font_ref};

// text-operation-shape.ps
#[test]
fn standard_font_text() {
    let run = distil("/Helvetica findfont 12 scalefont setfont 100 700 moveto (Hi) show showpage");
    assert_eq!(run.report.outcome, Outcome::Ok);
    assert!(run.report.substitutions.is_empty());
    assert!(run.report.notes.is_empty());
    let pdf = check(&run.pdf);
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n12 0 0 12 100 700 Tm\n(Hi) Tj\nET\n"
    );
    let font = font(&pdf, 0, "F0");
    assert_eq!(font.get("Subtype").unwrap().as_name(), b"Type1");
    assert_eq!(font.get("BaseFont").unwrap().as_name(), b"Helvetica");
    let first = font.get("FirstChar").unwrap().as_int();
    let widths = array(font.get("Widths").unwrap());
    assert_eq!(number(&widths[(72 - first) as usize]), 722.0);
    let cmap = decoded(pdf.resolve(font.get("ToUnicode").unwrap().as_reference()));
    assert!(String::from_utf8(cmap).unwrap().contains("<48> <0048>\n"));
}

// type3-square-glyph.ps
#[test]
fn type3_charprocs() {
    let run = distil(
        "/Sq 7 dict dup begin /FontType 3 def /FontMatrix [0.001 0 0 0.001 0 0] def \
         /Encoding StandardEncoding def /FontBBox [0 0 1000 1000] def \
         /BuildGlyph { pop pop 1000 0 0 0 1000 1000 setcachedevice 0 0 1000 1000 rectfill } def \
         end definefont pop /Sq findfont 20 scalefont setfont 10 10 moveto (a) show showpage",
    );
    let pdf = check(&run.pdf);
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n20 0 0 20 10 10 Tm\n(a) Tj\nET\n"
    );
    let font = font(&pdf, 0, "F0");
    assert_eq!(font.get("Subtype").unwrap().as_name(), b"Type3");
    let matrix: Vec<f64> = array(font.get("FontMatrix").unwrap())
        .iter()
        .map(number)
        .collect();
    assert_eq!(matrix, [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
    let differences = array(font.get("Encoding").unwrap().get("Differences").unwrap());
    assert_eq!(differences[0].as_int(), 97);
    assert_eq!(differences[1].as_name(), b"a");
    let charprocs = font.get("CharProcs").unwrap();
    let charproc = pdf.resolve(charprocs.get("a").unwrap().as_reference());
    assert!(
        String::from_utf8(decoded(charproc))
            .unwrap()
            .starts_with("1000 0 0 0 1000 1000 d1\n0 0 m\n1000 0 l\n")
    );
}

// fonts-shared-across-pages.ps
#[test]
fn fonts_shared_across_pages() {
    let run = distil(
        "/Helvetica findfont 12 scalefont setfont 72 700 moveto (First) show showpage \
         /Helvetica findfont 24 scalefont setfont 72 700 moveto (Second) show showpage",
    );
    assert_eq!(run.report.pages, 2);
    let pdf = check(&run.pdf);
    assert_eq!(font_ref(&pdf, 0, "F0"), font_ref(&pdf, 1, "F0"));
    assert_eq!(
        String::from_utf8_lossy(&run.pdf)
            .matches("/BaseFont /Helvetica")
            .count(),
        1
    );
}

// substitution-arial.ps
#[test]
fn substitutions_are_reported() {
    let run = distil("/Arial findfont 12 scalefont setfont 72 72 moveto (Arial) show showpage");
    assert_eq!(
        run.report.substitutions,
        [FontSubstitution {
            requested: b"Arial".to_vec(),
            substitute: "Helvetica",
        }]
    );
    let pdf = check(&run.pdf);
    assert_eq!(
        font(&pdf, 0, "F0").get("BaseFont").unwrap().as_name(),
        b"Helvetica"
    );
}

// --- embedded fonts -----------------------------------------------------------------------

use std::cell::RefCell;
use std::rc::Rc;

use ps_fonts::TrueTypeProgram;
use ps_fonts::testing::{corpus_truetype, corpus_type1};
use ps_fonts::type1::decrypt_section;
use ps_graphics::{Graphics, IrOp, Page};
use ps_vm::{Interp, SliceSource};

fn with_syn(program: &str) -> String {
    format!("{}{program}", corpus_type1().pfa())
}

/// The pages `program` delivers through the real backend.
fn pages_of(program: &[u8]) -> (Outcome, String, Vec<Page>) {
    let (io, out, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let pages = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Graphics::new(pages.clone())));
    let outcome = interp.run(&mut SliceSource::new(program));
    (outcome, out.text(), pages.take())
}

// type1-embedded-two-pages.ps
#[test]
fn type1_embedded_on_two_pages() {
    let run = distil(&with_syn(
        "/Syn findfont 10 scalefont setfont 100 100 moveto (a) show showpage \
         /Syn findfont 20 scalefont setfont 100 200 moveto (e) show showpage",
    ));
    assert_eq!(run.report.outcome, Outcome::Ok);
    assert_eq!(run.report.pages, 2);
    let pdf = check(&run.pdf);
    assert_eq!(font_ref(&pdf, 0, "F0"), font_ref(&pdf, 1, "F0"));
    let font = font(&pdf, 0, "F0");
    assert_eq!(font.get("Subtype").unwrap().as_name(), b"Type1");
    let descriptor = pdf.resolve(font.get("FontDescriptor").unwrap().as_reference());
    let font_file = pdf.resolve(descriptor.get("FontFile").unwrap().as_reference());
    let data = decoded(font_file);
    let lengths: Vec<usize> = ["Length1", "Length2", "Length3"]
        .iter()
        .map(|k| font_file.get(k).unwrap().as_int() as usize)
        .collect();
    assert_eq!(data.len(), lengths.iter().sum::<usize>());
    let plain = decrypt_section(&data[lengths[0]..lengths[0] + lengths[1]]);
    let text = String::from_utf8_lossy(&plain);
    assert!(
        text.contains("dup /CharStrings 3 dict dup begin\n"),
        "{text}"
    );
    assert!(text.contains("/.notdef ") && text.contains("/a ") && text.contains("/e "));
    assert!(!text.contains("/b ") && !text.contains("/acute "));
    assert_eq!(
        String::from_utf8_lossy(&run.pdf)
            .matches("/Subtype /Type1")
            .count(),
        1
    );
}

// type1-subset-round-trip.ps
#[test]
fn a_type1_subset_round_trips_through_the_interpreter() {
    let program = with_syn("/Syn findfont 10 scalefont setfont 100 100 moveto (a) show showpage");
    let run = distil(&program);
    let pdf = check(&run.pdf);
    let font = font(&pdf, 0, "F0");
    let base = String::from_utf8(font.get("BaseFont").unwrap().as_name().to_vec()).unwrap();
    let descriptor = pdf.resolve(font.get("FontDescriptor").unwrap().as_reference());
    let font_file = decoded(pdf.resolve(descriptor.get("FontFile").unwrap().as_reference()));

    // The regenerated program defines the tagged font with exactly the
    // two glyphs, and its glyph a measures and outlines as the original.
    let check_program = format!(
        "/{base} findfont /CharStrings get length = \
         /{base} findfont /CharStrings get /a known = \
         /{base} findfont /CharStrings get /e known = \
         /{base} findfont 10 scalefont setfont (a) stringwidth pop 1000 mul round 1000 div = \
         100 100 moveto (a) false charpath fill showpage"
    );
    let mut regenerated = font_file.clone();
    regenerated.extend_from_slice(check_program.as_bytes());
    let (outcome, output, pages) = pages_of(&regenerated);
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(output, "2\ntrue\nfalse\n6.0\n");
    let (_, _, original) = pages_of(
        with_syn(
            "/Syn findfont 10 scalefont setfont 100 100 moveto (a) false charpath fill showpage",
        )
        .as_bytes(),
    );
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].ops, original[0].ops);
    assert!(matches!(&pages[0].ops[0].op, IrOp::Fill { path, .. } if path.len() == 7));
}

// truetype-embedded.ps
#[test]
fn truetype_embedded() {
    let run = distil(&format!(
        "{}/SynTT findfont 20 scalefont setfont 100 100 moveto (ao) show showpage",
        corpus_truetype().type42("SynTT", &[(97, "a"), (111, "o")])
    ));
    assert_eq!(run.report.outcome, Outcome::Ok);
    let pdf = check(&run.pdf);
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n20 0 0 20 100 100 Tm\n(ao) Tj\nET\n"
    );
    let font = font(&pdf, 0, "F0");
    assert_eq!(font.get("Subtype").unwrap().as_name(), b"TrueType");
    assert!(font.get("Encoding").is_none());
    let descriptor = pdf.resolve(font.get("FontDescriptor").unwrap().as_reference());
    assert_eq!(descriptor.get("Flags").unwrap().as_int(), 4);
    let data = decoded(pdf.resolve(descriptor.get("FontFile2").unwrap().as_reference()));
    let subset = TrueTypeProgram::parse(data).unwrap();
    assert_eq!(subset.num_glyphs(), 3);
    assert_eq!(subset.cmap(3, 0).unwrap().unwrap().get(&111), Some(&2));
}

// truetype-subset-cmap.ps
#[test]
fn a_truetype_subset_has_a_cmap() {
    let run = distil(&format!(
        "{}/SynTT findfont 20 scalefont setfont 100 100 moveto (AB) show showpage",
        corpus_truetype().type42("SynTT", &[(65, "a"), (66, "o")])
    ));
    let pdf = check(&run.pdf);
    let font = font(&pdf, 0, "F0");
    assert_eq!(font.get("FirstChar").unwrap().as_int(), 65);
    assert_eq!(font.get("LastChar").unwrap().as_int(), 66);
    let widths: Vec<f64> = array(font.get("Widths").unwrap())
        .iter()
        .map(number)
        .collect();
    assert_eq!(widths, [500.0, 585.938]);
    let descriptor = pdf.resolve(font.get("FontDescriptor").unwrap().as_reference());
    let data = decoded(pdf.resolve(descriptor.get("FontFile2").unwrap().as_reference()));
    let subset = TrueTypeProgram::parse(data).unwrap();
    assert_eq!(subset.num_glyphs(), 3);
    let cmap = subset.cmap(3, 0).unwrap().unwrap();
    assert_eq!(cmap.get(&65), Some(&1));
    assert_eq!(cmap.get(&66), Some(&2));
    assert_eq!(
        cmap.len(),
        4,
        "each code by its bare value and in the 0xF0xx range"
    );
}
