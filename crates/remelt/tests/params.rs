// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The parameters through the writer: `CompressPages` per page, a locked
//! key, the compatibility header on seekable and plain sinks, what is
//! reported as not honoured, `SubsetFonts` off, `EmbedAllFonts`, the
//! downsampling keys, and the job's view of the embedder's values.

mod support;

use std::io::Cursor;

use ps_fonts::testing::corpus_type1;
use ps_fonts::type1::decrypt_section;
use ps_graphics::{DocMark, IrOp, Page, PageSink};
use ps_vm::{Bounds, Config, Io, MarkValue, Outcome, Point, Seg};
use remelt::{Options, PdfSink, Report};
use support::{Value, check, decoded, font, page, resources, xobject};

fn config() -> (Config, ps_vm::Capture) {
    let (io, out, _) = Io::capture();
    (
        Config {
            io,
            ..Default::default()
        },
        out,
    )
}

fn run(program: &str, options: &Options) -> (Report, Vec<u8>, String) {
    let (config, out) = config();
    let (report, pdf) = remelt::distill(program.as_bytes(), config, options, Vec::new()).unwrap();
    (report, pdf, out.text())
}

fn run_seekable(program: &str, options: &Options) -> (Report, Vec<u8>) {
    let (config, _) = config();
    let (report, cursor) =
        remelt::distill_seekable(program.as_bytes(), config, options, Cursor::new(Vec::new()))
            .unwrap();
    (report, cursor.into_inner())
}

fn contents(pdf: &support::Pdf, index: usize) -> &Value {
    pdf.resolve(page(pdf, index).get("Contents").unwrap().as_reference())
}

fn entry(key: &str, value: MarkValue) -> (Vec<u8>, MarkValue) {
    (key.as_bytes().to_vec(), value)
}

fn refused(key: &str, text: &str) -> (String, String) {
    (key.to_string(), text.to_string())
}

const FILL_A: &str = "0 0 10 10 rectfill showpage\n";
const FILL_B: &str = "0 0 20 20 rectfill showpage\n";

// compress-off.ps
#[test]
fn compress_pages_governs_the_pages_written_after_the_request() {
    let program = format!(
        "<< /CompressPages false >> setdistillerparams {FILL_A}\
         << /CompressPages true >> setdistillerparams {FILL_B}"
    );
    let (report, pdf, _) = run(&program, &Options::default());
    assert_eq!(report.outcome, Outcome::Ok);
    assert_eq!(report.pages, 2);
    let pdf = check(&pdf);
    assert!(contents(&pdf, 0).get("Filter").is_none());
    assert_eq!(
        contents(&pdf, 1).get("Filter").unwrap().as_name(),
        b"FlateDecode"
    );
    assert_eq!(
        decoded(contents(&pdf, 1)),
        b"0 0 m\n20 0 l\n20 20 l\n0 20 l\nh\nf\n"
    );
    assert!(report.params.compress_pages);
    assert!(report.not_honoured.is_empty());
}

// The locked-key scenario, at the library level.
#[test]
fn a_locked_key_keeps_the_embedders_value_and_the_report_says_so() {
    let options = Options::compress(false).lock("CompressPages");
    let program = format!("<< /CompressPages true >> setdistillerparams {FILL_A}");
    let (report, pdf, _) = run(&program, &options);
    let pdf = check(&pdf);
    assert!(contents(&pdf, 0).get("Filter").is_none());
    assert!(!report.params.compress_pages);
    assert_eq!(
        report.not_honoured,
        vec![refused("CompressPages", "true (locked at false)")]
    );
}

// compat-header.ps
#[test]
fn the_compatibility_level_names_the_header_when_the_sink_can_seek() {
    let program = format!("<< /CompatibilityLevel 1.4 >> setdistillerparams {FILL_A}");
    let (report, pdf) = run_seekable(&program, &Options::default());
    assert!(pdf.starts_with(b"%PDF-1.4\n%"), "{:?}", &pdf[..12]);
    check(&pdf);
    assert_eq!(report.params.compatibility_level, (1, 4));
    assert!(report.not_honoured.is_empty());

    // Set through the options rather than the job, and at 1.7 the header
    // is left as written.
    let mut options = Options::default();
    options.params.compatibility_level = (1, 3);
    let (_, pdf) = run_seekable(FILL_A, &options);
    assert!(pdf.starts_with(b"%PDF-1.3\n"));
    let (_, pdf) = run_seekable(FILL_A, &Options::default());
    assert!(pdf.starts_with(b"%PDF-1.7\n"));
}

#[test]
fn a_plain_sink_keeps_the_written_version_and_reports_it() {
    let program = format!("<< /CompatibilityLevel 1.4 >> setdistillerparams {FILL_A}");
    let (report, pdf, _) = run(&program, &Options::default());
    assert!(pdf.starts_with(b"%PDF-1.7\n"));
    assert_eq!(report.params.compatibility_level, (1, 4));
    assert_eq!(
        report.not_honoured,
        vec![refused(
            "CompatibilityLevel",
            "1.4 (the output cannot be rewound; 1.7 written)"
        )]
    );
}

// not-honoured-reported.ps
#[test]
fn unknown_keys_and_unsupported_values_are_reported_and_recorded() {
    let program = format!(
        "<< /AutoRotatePages /All /CompatibilityLevel 1.2 /Foo (bar)\n\
            /ColorConversionStrategy /sRGB >> setdistillerparams {FILL_A}"
    );
    let (report, pdf, _) = run(&program, &Options::default());
    assert_eq!(report.outcome, Outcome::Ok);
    check(&pdf);
    assert_eq!(
        report.not_honoured,
        vec![
            refused("AutoRotatePages", "/All"),
            refused("CompatibilityLevel", "1.2 (1.3 to 1.7)"),
            refused("Foo", "(bar)"),
            refused(
                "ColorConversionStrategy",
                "/sRGB (only LeaveColorUnchanged)"
            ),
        ]
    );
    assert_eq!(report.params.compatibility_level, (1, 7));
    assert_eq!(report.params.color_conversion_strategy, "sRGB");
    assert_eq!(
        report.params.others.get("AutoRotatePages"),
        Some(&MarkValue::Name(b"All".to_vec()))
    );
    assert_eq!(
        report.params.others.get("Foo"),
        Some(&MarkValue::String(b"bar".to_vec()))
    );
}

fn font_file(pdf: &support::Pdf) -> (String, Vec<u8>) {
    let fonts = resources(pdf, 0).get("Font").unwrap();
    let font = pdf.resolve(fonts.get("F0").unwrap().as_reference());
    let base = String::from_utf8(font.get("BaseFont").unwrap().as_name().to_vec()).unwrap();
    let descriptor = pdf.resolve(font.get("FontDescriptor").unwrap().as_reference());
    let stream = pdf.resolve(descriptor.get("FontFile").unwrap().as_reference());
    let data = decoded(stream);
    let length1 = stream.get("Length1").unwrap().as_int() as usize;
    let length2 = stream.get("Length2").unwrap().as_int() as usize;
    let plain = decrypt_section(&data[length1..length1 + length2]);
    (base, plain)
}

#[test]
fn subset_fonts_off_embeds_the_whole_program_under_its_own_name() {
    let show = "/Syn findfont 10 scalefont setfont 100 100 moveto (a) show showpage";
    let whole = format!(
        "{}<< /SubsetFonts false >> setdistillerparams {show}",
        corpus_type1().pfa()
    );
    let (report, pdf, _) = run(&whole, &Options::compress(false));
    assert!(!report.params.subset_fonts);
    let pdf = check(&pdf);
    let (base, plain) = font_file(&pdf);
    assert_eq!(base, "Syn");
    let text = String::from_utf8_lossy(&plain);
    assert!(text.contains("/a ") && text.contains("/b ") && text.contains("/acute "));

    let subset = format!("{}{show}", corpus_type1().pfa());
    let (_, pdf, _) = run(&subset, &Options::compress(false));
    let pdf = check(&pdf);
    let (base, plain) = font_file(&pdf);
    assert!(base.ends_with("+Syn"), "{base}");
    let text = String::from_utf8_lossy(&plain);
    assert!(text.contains("/a ") && !text.contains("/b "));
}

#[test]
fn the_job_reads_the_embedders_values_and_its_own() {
    let mut options = Options::compress(false);
    options.params.merge(&[entry("Custom", MarkValue::Int(7))]);
    let (report, _, output) = run(
        "currentdistillerparams /CompressPages get =\n\
         currentdistillerparams /Custom get =\n\
         << /GrayImageResolution 72 >> setdistillerparams\n\
         currentdistillerparams /GrayImageResolution get =",
        &options,
    );
    assert_eq!(output, "false\n7\n72\n");
    assert_eq!(report.params.gray_image_resolution, 72);
    // The embedder's own extra key is not the job's doing and is not
    // reported against it.
    assert!(report.not_honoured.is_empty());
}

#[test]
fn a_params_mark_replayed_into_the_sink_applies_to_later_pages() {
    let mut page = Page::new(Bounds::new(0.0, 0.0, 612.0, 792.0));
    page.ops.push(
        IrOp::Fill {
            path: vec![Seg::Move(Point::new(1.0, 1.0)), Seg::Close],
            rule: ps_graphics::FillRule::NonZero,
        }
        .into(),
    );
    let mut sink = PdfSink::new(Vec::new(), Options::default()).unwrap();
    sink.page(page.clone());
    sink.document(DocMark::Params(vec![entry(
        "CompressPages",
        MarkValue::Bool(false),
    )]));
    sink.page(page);
    assert!(!sink.params().compress_pages);
    let pdf = check(&sink.finish().unwrap());
    assert!(contents(&pdf, 0).get("Filter").is_some());
    assert!(contents(&pdf, 1).get("Filter").is_none());
}

fn descriptor<'a>(pdf: &'a support::Pdf, font: &Value) -> &'a Value {
    pdf.resolve(font.get("FontDescriptor").unwrap().as_reference())
}

// embed-all-helvetica.ps, with Symbol beside it
#[test]
fn embed_all_embeds_the_faces_with_assets_and_reports_the_rest() {
    const TEXT: &str = "/Helvetica findfont 24 scalefont setfont 72 700 moveto (Hi) show \
                        /Symbol findfont 24 scalefont setfont 72 600 moveto (a) show showpage\n";
    let program = format!("<< /EmbedAllFonts true >> setdistillerparams {TEXT}");
    let (report, pdf, _) = run(&program, &Options::compress(false));
    assert_eq!(report.outcome, Outcome::Ok);
    assert!(report.params.embed_all_fonts);
    let pdf = check(&pdf);
    let helvetica = font(&pdf, 0, "F0");
    let symbol = font(&pdf, 0, "F1");
    assert_eq!(symbol.get("BaseFont").unwrap().as_name(), b"Symbol");
    assert!(descriptor(&pdf, symbol).get("FontFile2").is_none());
    if ps_fonts::has_resident_outlines() {
        assert_eq!(helvetica.get("Subtype").unwrap().as_name(), b"TrueType");
        assert!(
            helvetica
                .get("BaseFont")
                .unwrap()
                .as_name()
                .ends_with(b"+LiberationSans-Regular")
        );
        assert!(descriptor(&pdf, helvetica).get("FontFile2").is_some());
        assert_eq!(
            report.not_honoured,
            vec![refused("EmbedAllFonts", "true (Symbol: no outline asset)")]
        );
    } else {
        assert_eq!(helvetica.get("BaseFont").unwrap().as_name(), b"Helvetica");
        assert_eq!(
            report.not_honoured,
            vec![
                refused(
                    "EmbedAllFonts",
                    "true (Helvetica: outline assets absent from this build)"
                ),
                refused("EmbedAllFonts", "true (Symbol: no outline asset)"),
            ]
        );
    }

    // On through the options and switched off by the job: nothing
    // embedded, nothing reported.
    let mut options = Options::compress(false);
    options.params.embed_all_fonts = true;
    let program = format!("<< /EmbedAllFonts false >> setdistillerparams {TEXT}");
    let (report, pdf, _) = run(&program, &options);
    assert!(!report.params.embed_all_fonts);
    assert!(report.not_honoured.is_empty());
    let pdf = check(&pdf);
    assert_eq!(
        font(&pdf, 0, "F0").get("BaseFont").unwrap().as_name(),
        b"Helvetica"
    );
}

// downsample-gray-300-to-72.ps, reduced to four samples over a point
#[test]
fn gray_downsampling_averages_or_subsamples_as_the_type_says() {
    const IMAGE: &str = "72 72 translate 4 4 8 [4 0 0 -4 0 4] \
                         <000102030405060708090A0B0C0D0E0F> image showpage\n";
    let averaged = format!(
        "<< /DownsampleGrayImages true /GrayImageResolution 72 >> setdistillerparams {IMAGE}"
    );
    let (report, pdf, _) = run(&averaged, &Options::compress(false));
    assert_eq!(report.outcome, Outcome::Ok);
    assert_eq!(report.downsampled, 1);
    assert!(report.notes.is_empty());
    let pdf = check(&pdf);
    let image = xobject(&pdf, 0, "Im0");
    assert_eq!(image.get("Width").unwrap().as_int(), 1);
    assert_eq!(image.get("Height").unwrap().as_int(), 1);
    assert_eq!(decoded(image), [8], "the sixteen samples average 7.5");
    assert_eq!(
        decoded(contents(&pdf, 0)),
        b"q 1 0 0 1 72 72 cm /Im0 Do Q\n",
        "the matrix is unchanged"
    );

    let subsampled = format!(
        "<< /DownsampleGrayImages true /GrayImageResolution 72 \
         /GrayImageDownsampleType /Subsample >> setdistillerparams {IMAGE}"
    );
    let (report, pdf, _) = run(&subsampled, &Options::compress(false));
    assert_eq!(report.downsampled, 1);
    assert_eq!(decoded(xobject(&check(&pdf), 0, "Im0")), [0]);

    // Off by default, and left at a target the image does not exceed.
    let (report, pdf, _) = run(IMAGE, &Options::compress(false));
    assert_eq!(report.downsampled, 0);
    assert_eq!(
        xobject(&check(&pdf), 0, "Im0")
            .get("Width")
            .unwrap()
            .as_int(),
        4
    );
    let kept = format!(
        "<< /DownsampleGrayImages true /GrayImageResolution 300 >> setdistillerparams {IMAGE}"
    );
    let (report, _, _) = run(&kept, &Options::compress(false));
    assert_eq!(report.downsampled, 0, "288 per inch is under 300");

    // A depth the policy leaves alone is noted once per image.
    let two_bit = "<< /DownsampleGrayImages true /GrayImageResolution 72 >> setdistillerparams \
                   72 72 translate 4 4 2 [4 0 0 -4 0 4] <00000000> image showpage\n";
    let (report, _, _) = run(two_bit, &Options::compress(false));
    assert_eq!(report.downsampled, 0);
    assert_eq!(report.notes, ["page 1: image 0 (2-bit) is not downsampled"]);

    // An image carried as a DCT stream has no samples to reduce: it is
    // noted and written as it came, whatever its resolution.
    let dct = "<< /DownsampleGrayImages true /GrayImageResolution 72 >> setdistillerparams \
               72 72 translate << /ImageType 1 /Width 16 /Height 16 /BitsPerComponent 8 \
               /ImageMatrix [16 0 0 -16 0 16] \
               /DataSource currentfile /ASCIIHexDecode filter /DCTDecode filter >> image\n\
               FFD8FFDB0004AABBFFDA0003011234FF00FFD9>\nshowpage\n";
    let (report, pdf, _) = run(dct, &Options::compress(false));
    assert_eq!(report.downsampled, 0);
    assert_eq!(
        report.notes,
        ["page 1: image 0 (DCT-encoded) is not downsampled"]
    );
    let pdf = check(&pdf);
    let xobject = xobject(&pdf, 0, "Im0");
    assert_eq!(xobject.get("Filter").unwrap().as_name(), b"DCTDecode");
    assert_eq!(xobject.get("Width").unwrap().as_int(), 16);
    assert_eq!(xobject.stream_data().len(), 19);
}
