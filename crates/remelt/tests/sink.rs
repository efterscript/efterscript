// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The sink without an interpreter: pages built by hand, output checked
//! by the structural reader and by the uncompressed stream text.

mod support;

use std::io::{self, Write};

use ps_graphics::{FillRule, IrOp, Op, Page, PageSink};
use ps_vm::{Bounds, ImageSpec, LineCap, LineJoin, Matrix, Point, Seg, SpaceSpec};
use remelt::{Error, Options, PdfSink};
use support::{
    Value, array, check, color_space, content, decoded, distil_pages, kids, media_box, number,
    producer, resources, xobject,
};

const LETTER: Bounds = Bounds::new(0.0, 0.0, 612.0, 792.0);

fn uncompressed() -> Options {
    Options::compress(false)
}

fn page_with(ops: Vec<IrOp>) -> Page {
    let mut page = Page::new(LETTER);
    page.ops = ops.into_iter().map(Op::from).collect();
    page
}

fn p(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

fn line() -> Vec<Seg> {
    vec![Seg::Move(p(10.0, 10.0)), Seg::Line(p(100.0, 10.0))]
}

fn stroked_line() -> Page {
    page_with(vec![
        IrOp::LineWidth(2.0),
        IrOp::Stroke {
            path: line(),
            ctm: Matrix::IDENTITY,
        },
    ])
}

fn spot() -> SpaceSpec {
    SpaceSpec::Separation {
        name: b"Spot".to_vec(),
        alternate: Box::new(SpaceSpec::DeviceCMYK),
        tint_source: b"{0 0 0 4 -1 roll}".to_vec(),
    }
}

fn image_spec(space: Option<SpaceSpec>, bits: u8, decode: Vec<f32>) -> ImageSpec {
    ImageSpec {
        width: 2,
        height: 2,
        bits_per_component: bits,
        color_space: space.clone(),
        decode,
        matrix: Matrix::IDENTITY,
        interpolate: false,
        is_mask: space.is_none(),
    }
}

#[test]
fn a_hand_built_page_becomes_one_pdf_page() {
    let bytes = distil_pages(vec![stroked_line()], uncompressed());
    let pdf = check(&bytes);
    assert_eq!(kids(&pdf).len(), 1);
    assert_eq!(media_box(&pdf, 0), [0.0, 0.0, 612.0, 792.0]);
    assert_eq!(content(&pdf, 0), "2 w\n10 10 m\n100 10 l\nS\n");
    assert!(resources(&pdf, 0).get("ColorSpace").is_none());
    assert!(resources(&pdf, 0).get("XObject").is_none());
    // Content, page, pages node, catalog, info.
    assert_eq!(pdf.objects.len(), 5);
}

#[test]
fn the_media_box_is_the_pages_own() {
    let mut page = stroked_line();
    page.media_box = Bounds::new(10.0, 20.5, 300.0, 400.0);
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(media_box(&pdf, 0), [10.0, 20.5, 300.0, 400.0]);
}

#[test]
fn every_operation_maps_to_its_pdf_operator() {
    let path = vec![
        Seg::Move(p(0.0, 0.0)),
        Seg::Line(p(10.0, 0.0)),
        Seg::Curve(p(10.0, 5.0), p(5.0, 10.0), p(0.0, 10.0)),
        Seg::Close,
    ];
    let page = page_with(vec![
        IrOp::LineWidth(0.5),
        IrOp::LineCap(LineCap::Round),
        IrOp::LineJoin(LineJoin::Bevel),
        IrOp::MiterLimit(4.0),
        IrOp::Dash(vec![3.0, 1.0], 0.0),
        IrOp::Flatness(2.0),
        IrOp::Save,
        IrOp::Clip {
            path: path.clone(),
            rule: FillRule::EvenOdd,
        },
        IrOp::Fill {
            path: path.clone(),
            rule: FillRule::EvenOdd,
        },
        IrOp::Restore,
        IrOp::Clip {
            path: path.clone(),
            rule: FillRule::NonZero,
        },
        IrOp::Fill {
            path: path.clone(),
            rule: FillRule::NonZero,
        },
        IrOp::Stroke {
            path,
            ctm: Matrix::IDENTITY,
        },
    ]);
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    let segs = "0 0 m\n10 0 l\n10 5 5 10 0 10 c\nh\n";
    assert_eq!(
        content(&pdf, 0),
        format!(
            "0.5 w\n1 J\n2 j\n4 M\n[3 1] 0 d\n2 i\nq\n{segs}W* n\n{segs}f*\nQ\n{segs}W n\n{segs}f\n{segs}S\n"
        )
    );
}

#[test]
fn a_scaled_stroke_is_wrapped_in_its_matrix() {
    let page = page_with(vec![
        IrOp::LineWidth(1.0),
        IrOp::Stroke {
            path: vec![Seg::Move(p(10.0, 10.0)), Seg::Line(p(100.0, 10.0))],
            ctm: Matrix::scaling(2.0, 2.0),
        },
    ]);
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "1 w\nq\n2 0 0 2 0 0 cm\n5 5 m\n50 5 l\nS\nQ\n"
    );
}

#[test]
fn a_translated_stroke_keeps_its_width_inside_the_wrapper() {
    let page = page_with(vec![IrOp::Stroke {
        path: vec![Seg::Move(p(10.0, 20.0)), Seg::Line(p(40.0, 20.0))],
        ctm: Matrix::translation(10.0, 20.0),
    }]);
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "q\n1 0 0 1 10 20 cm\n0 0 m\n30 0 l\nS\nQ\n"
    );
}

#[test]
fn a_singular_stroke_ctm_is_written_unwrapped() {
    let page = page_with(vec![
        IrOp::LineWidth(3.0),
        IrOp::Stroke {
            path: line(),
            ctm: Matrix([1.0, 2.0, 2.0, 4.0, 5.0, 5.0]),
        },
    ]);
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(content(&pdf, 0), "3 w\n10 10 m\n100 10 l\nS\n");
}

#[test]
fn device_colour_uses_the_direct_operators_and_no_resource() {
    let mut page = Page::new(LETTER);
    let rgb = page.resources.intern_space(&SpaceSpec::DeviceRGB);
    let cmyk = page.resources.intern_space(&SpaceSpec::DeviceCMYK);
    let gray = page.resources.intern_space(&SpaceSpec::DeviceGray);
    page.ops = vec![
        IrOp::SetColorSpace(rgb),
        IrOp::SetColor(vec![0.2, 0.4, 0.6]),
        IrOp::SetColorSpace(cmyk),
        IrOp::SetColor(vec![0.0, 0.0, 0.0, 1.0]),
        IrOp::SetColorSpace(gray),
        IrOp::SetColor(vec![0.5]),
        IrOp::Fill {
            path: line(),
            rule: FillRule::NonZero,
        },
    ]
    .into_iter()
    .map(Op::from)
    .collect();
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "/DeviceRGB cs\n/DeviceRGB CS\n0.2 0.4 0.6 rg\n0.2 0.4 0.6 RG\n/DeviceCMYK cs\n/DeviceCMYK CS\n0 0 0 1 k\n0 0 0 1 K\n/DeviceGray cs\n/DeviceGray CS\n0.5 g\n0.5 G\n10 10 m\n100 10 l\nf\n"
    );
    assert!(resources(&pdf, 0).get("ColorSpace").is_none());
}

#[test]
fn a_separation_becomes_a_resource_with_a_calculator_function() {
    let mut page = Page::new(LETTER);
    let spot = page.resources.intern_space(&spot());
    page.ops = vec![
        IrOp::SetColorSpace(spot),
        IrOp::SetColor(vec![0.6]),
        IrOp::Fill {
            path: line(),
            rule: FillRule::NonZero,
        },
    ]
    .into_iter()
    .map(Op::from)
    .collect();
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "/CS0 cs\n/CS0 CS\n0.6 scn\n0.6 SCN\n10 10 m\n100 10 l\nf\n"
    );
    let space = array(color_space(&pdf, 0, "CS0"));
    assert_eq!(space.len(), 4);
    assert_eq!(space[0].as_name(), b"Separation");
    assert_eq!(space[1].as_name(), b"Spot");
    assert_eq!(space[2].as_name(), b"DeviceCMYK");
    let function = pdf.resolve(space[3].as_reference());
    assert_eq!(function.get("FunctionType").unwrap().as_int(), 4);
    let domain: Vec<f64> = array(function.get("Domain").unwrap())
        .iter()
        .map(number)
        .collect();
    assert_eq!(domain, [0.0, 1.0]);
    let range: Vec<f64> = array(function.get("Range").unwrap())
        .iter()
        .map(number)
        .collect();
    assert_eq!(range, [0.0, 1.0].repeat(4));
    assert!(function.get("Filter").is_none());
    assert_eq!(decoded(function), b"{0 0 0 4 -1 roll}");
}

#[test]
fn devicen_and_indexed_spaces_are_written_by_arity_and_inline() {
    let mut page = Page::new(LETTER);
    let duo = page.resources.intern_space(&SpaceSpec::DeviceN {
        names: vec![b"Orange".to_vec(), b"Green".to_vec()],
        alternate: Box::new(SpaceSpec::DeviceRGB),
        tint_source: b"{pop pop 0 0 0}".to_vec(),
    });
    let indexed = page.resources.intern_space(&SpaceSpec::Indexed {
        base: Box::new(spot()),
        hival: 1,
        lookup: vec![0x00, 0xFF],
    });
    page.ops = vec![
        IrOp::SetColorSpace(duo),
        IrOp::SetColor(vec![0.25, 0.75]),
        IrOp::SetColorSpace(indexed),
        IrOp::SetColor(vec![1.0]),
        IrOp::Fill {
            path: line(),
            rule: FillRule::NonZero,
        },
    ]
    .into_iter()
    .map(Op::from)
    .collect();
    let bytes = distil_pages(vec![page], uncompressed());
    let pdf = check(&bytes);
    assert_eq!(
        content(&pdf, 0),
        "/CS0 cs\n/CS0 CS\n0.25 0.75 scn\n0.25 0.75 SCN\n/CS1 cs\n/CS1 CS\n1 scn\n1 SCN\n10 10 m\n100 10 l\nf\n"
    );

    let duo = array(color_space(&pdf, 0, "CS0"));
    assert_eq!(duo[0].as_name(), b"DeviceN");
    let names: Vec<&[u8]> = array(&duo[1]).iter().map(Value::as_name).collect();
    assert_eq!(names, [b"Orange".as_slice(), b"Green"]);
    assert_eq!(duo[2].as_name(), b"DeviceRGB");
    let function = pdf.resolve(duo[3].as_reference());
    assert_eq!(array(function.get("Domain").unwrap()).len(), 4);
    assert_eq!(array(function.get("Range").unwrap()).len(), 6);
    assert_eq!(decoded(function), b"{pop pop 0 0 0}");

    let indexed = array(color_space(&pdf, 0, "CS1"));
    assert_eq!(indexed[0].as_name(), b"Indexed");
    assert_eq!(array(&indexed[1])[0].as_name(), b"Separation");
    assert_eq!(indexed[2].as_int(), 1);
    assert_eq!(indexed[3], Value::Str(vec![0x00, 0xFF]));
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/Indexed [ /Separation /Spot /DeviceCMYK "));
    assert!(text.contains(" 1 <00FF> ]"));
}

#[test]
fn a_gray_image_becomes_an_xobject_painted_through_its_matrix() {
    let mut page = Page::new(LETTER);
    let spec = image_spec(Some(SpaceSpec::DeviceGray), 8, vec![0.0, 1.0]);
    let image = page.resources.add_image(&spec, &[0x00, 0x55, 0xAA, 0xFF]);
    page.ops = vec![IrOp::Image {
        image,
        matrix: Matrix([50.0, 0.0, 0.0, 50.0, 100.0, 100.0]),
    }]
    .into_iter()
    .map(Op::from)
    .collect();
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(content(&pdf, 0), "q 50 0 0 50 100 100 cm /Im0 Do Q\n");
    let xobject = xobject(&pdf, 0, "Im0");
    assert_eq!(xobject.get("Type").unwrap().as_name(), b"XObject");
    assert_eq!(xobject.get("Subtype").unwrap().as_name(), b"Image");
    assert_eq!(xobject.get("Width").unwrap().as_int(), 2);
    assert_eq!(xobject.get("Height").unwrap().as_int(), 2);
    assert_eq!(xobject.get("BitsPerComponent").unwrap().as_int(), 8);
    assert_eq!(xobject.get("ColorSpace").unwrap().as_name(), b"DeviceGray");
    assert!(xobject.get("ImageMask").is_none());
    assert!(xobject.get("Decode").is_none());
    assert!(xobject.get("Interpolate").is_none());
    assert_eq!(
        xobject.get("Filter").unwrap().as_name(),
        b"FlateDecode",
        "image data is always in the Flate container"
    );
    assert_eq!(decoded(xobject), [0x00, 0x55, 0xAA, 0xFF]);
    // Device spaces interned for an image still declare no resource.
    assert!(resources(&pdf, 0).get("ColorSpace").is_none());
}

#[test]
fn an_image_mask_has_the_flag_and_no_colour_space() {
    let mut page = Page::new(LETTER);
    let spot = page.resources.intern_space(&spot());
    let mask = page
        .resources
        .add_image(&image_spec(None, 1, vec![1.0, 0.0]), &[0x40, 0x80]);
    page.ops = vec![
        IrOp::SetColorSpace(spot),
        IrOp::SetColor(vec![0.6]),
        IrOp::Image {
            image: mask,
            matrix: Matrix([1.0, 0.0, 0.0, -1.0, 0.0, 1.0]),
        },
    ]
    .into_iter()
    .map(Op::from)
    .collect();
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "/CS0 cs\n/CS0 CS\n0.6 scn\n0.6 SCN\nq 1 0 0 -1 0 1 cm /Im0 Do Q\n"
    );
    let xobject = xobject(&pdf, 0, "Im0");
    assert_eq!(xobject.get("ImageMask"), Some(&Value::Bool(true)));
    assert!(xobject.get("ColorSpace").is_none());
    assert_eq!(xobject.get("BitsPerComponent").unwrap().as_int(), 1);
    let decode: Vec<f64> = array(xobject.get("Decode").unwrap())
        .iter()
        .map(number)
        .collect();
    assert_eq!(decode, [1.0, 0.0]);
    assert_eq!(decoded(xobject), [0x40, 0x80]);
}

#[test]
fn decode_and_interpolate_appear_only_when_they_say_something() {
    let mut page = Page::new(LETTER);
    let mut inverted = image_spec(
        Some(SpaceSpec::DeviceRGB),
        8,
        vec![1.0, 0.0, 1.0, 0.0, 1.0, 0.0],
    );
    inverted.interpolate = true;
    let inverted = page.resources.add_image(&inverted, &[0; 12]);
    let indexed_space = SpaceSpec::Indexed {
        base: Box::new(SpaceSpec::DeviceRGB),
        hival: 3,
        lookup: vec![0; 12],
    };
    let plain = page
        .resources
        .add_image(&image_spec(Some(indexed_space), 2, vec![0.0, 3.0]), &[0; 2]);
    let default_mask = page
        .resources
        .add_image(&image_spec(None, 1, vec![0.0, 1.0]), &[0; 2]);
    for image in [inverted, plain, default_mask] {
        page.ops.push(
            IrOp::Image {
                image,
                matrix: Matrix::IDENTITY,
            }
            .into(),
        );
    }
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    let im0 = xobject(&pdf, 0, "Im0");
    assert_eq!(array(im0.get("Decode").unwrap()).len(), 6);
    assert_eq!(im0.get("Interpolate"), Some(&Value::Bool(true)));
    let im1 = xobject(&pdf, 0, "Im1");
    assert!(im1.get("Decode").is_none());
    assert_eq!(
        array(im1.get("ColorSpace").unwrap())[0].as_name(),
        b"Indexed"
    );
    assert!(xobject(&pdf, 0, "Im2").get("Decode").is_none());
    assert!(color_space(&pdf, 0, "CS1").get("Indexed").is_none());
}

#[test]
fn a_document_with_no_pages_is_well_formed() {
    let bytes = distil_pages(Vec::new(), uncompressed());
    let pdf = check(&bytes);
    assert!(kids(&pdf).is_empty());
    assert_eq!(
        producer(&pdf),
        format!("EfterScript {}", env!("CARGO_PKG_VERSION"))
    );
    let info = pdf.resolve(pdf.trailer_get("Info").unwrap().as_reference());
    match info {
        Value::Dict(entries) => assert_eq!(entries.len(), 1, "Producer only: {entries:?}"),
        other => panic!("Info must be a dictionary, got {other:?}"),
    }
    assert!(!String::from_utf8_lossy(&bytes).contains("Date"));
}

#[test]
fn compression_wraps_text_streams_and_the_option_defaults_on() {
    assert!(Options::default().params.compress_pages);
    let mut page = stroked_line();
    let spot = page.resources.intern_space(&spot());
    page.ops.insert(0, IrOp::SetColorSpace(spot).into());
    let pdf = check(&distil_pages(vec![page], Options::default()));
    let contents = pdf.resolve(page_contents(&pdf));
    assert_eq!(contents.get("Filter").unwrap().as_name(), b"FlateDecode");
    assert_eq!(
        content(&pdf, 0),
        "/CS0 cs\n/CS0 CS\n2 w\n10 10 m\n100 10 l\nS\n"
    );
    let function = pdf.resolve(array(color_space(&pdf, 0, "CS0"))[3].as_reference());
    assert_eq!(function.get("Filter").unwrap().as_name(), b"FlateDecode");
    assert_eq!(decoded(function), b"{0 0 0 4 -1 roll}");
}

fn page_contents(pdf: &support::Pdf) -> u32 {
    support::page(pdf, 0)
        .get("Contents")
        .unwrap()
        .as_reference()
}

#[test]
fn pages_arrive_in_delivery_order() {
    let pages: Vec<Page> = (1..=3)
        .map(|k| {
            page_with(vec![IrOp::Fill {
                path: vec![Seg::Move(p(k as f32, 0.0)), Seg::Close],
                rule: FillRule::NonZero,
            }])
        })
        .collect();
    let pdf = check(&distil_pages(pages, uncompressed()));
    assert_eq!(kids(&pdf).len(), 3);
    for k in 0..3 {
        assert_eq!(content(&pdf, k), format!("{} 0 m\nh\nf\n", k + 1));
    }
}

#[test]
fn comments_land_between_objects() {
    let mut sink = PdfSink::new(Vec::new(), uncompressed()).unwrap();
    sink.comment("GENERATED-BY: a test").unwrap();
    sink.page(stroked_line());
    let bytes = sink.finish().unwrap();
    check(&bytes);
    assert!(String::from_utf8_lossy(&bytes).contains("\n% GENERATED-BY: a test\n2 0 obj\n"));
    let mut sink = PdfSink::new(Vec::new(), uncompressed()).unwrap();
    assert!(matches!(
        sink.comment("two\nlines"),
        Err(Error::Pdf(pdf_out::Error::InvalidComment))
    ));
}

#[test]
fn the_same_pages_give_the_same_bytes() {
    let build = || {
        let mut page = stroked_line();
        let spot = page.resources.intern_space(&spot());
        page.ops.push(IrOp::SetColorSpace(spot).into());
        vec![page, stroked_line()]
    };
    assert_eq!(
        distil_pages(build(), uncompressed()),
        distil_pages(build(), uncompressed())
    );
    assert_eq!(
        distil_pages(build(), Options::default()),
        distil_pages(build(), Options::default())
    );
}

/// Accepts `budget` bytes, then refuses everything.
#[derive(Debug)]
struct Failing {
    budget: usize,
}

impl Write for Failing {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.len() <= self.budget {
            self.budget -= buf.len();
            Ok(buf.len())
        } else {
            Err(io::Error::other("disk full"))
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn the_first_write_error_is_latched_and_reported_at_finish() {
    // Room for the header, not for the first object.
    let mut sink = PdfSink::new(Failing { budget: 15 }, uncompressed()).unwrap();
    sink.page(stroked_line());
    sink.page(stroked_line());
    assert_eq!(sink.pages(), 0);
    let error = sink.finish().unwrap_err();
    assert!(
        matches!(error, Error::Pdf(pdf_out::Error::Io(_))),
        "{error}"
    );
    assert!(error.to_string().contains("disk full"));

    assert!(matches!(
        PdfSink::new(Failing { budget: 0 }, uncompressed()),
        Err(Error::Pdf(pdf_out::Error::Io(_)))
    ));
}

// --- text ---------------------------------------------------------------------------------

use std::collections::BTreeMap;

use ps_fonts::{ResidentFace, STANDARD_ENCODING};
use ps_graphics::{FontIndex, FontSpec, GlyphProc, glyph_names};
use ps_vm::Glyph;
use support::{font, font_ref};

fn standard() -> ps_graphics::GlyphNames {
    let names: Vec<Option<Vec<u8>>> = STANDARD_ENCODING
        .iter()
        .map(|n| n.map(|n| n.as_bytes().to_vec()))
        .collect();
    glyph_names(&names)
}

fn helvetica() -> FontSpec {
    FontSpec::Resident {
        base: ResidentFace::Helvetica,
        encoding: standard(),
    }
}

fn glyph(code: u8, dx: f32, dy: f32) -> Glyph {
    Glyph::simple(code, dx, dy)
}

fn square_glyph(ops: Vec<IrOp>, bbox: Option<Bounds>) -> GlyphProc {
    GlyphProc {
        ops: ops.into_iter().map(Op::from).collect(),
        width: (1000.0, 0.0),
        bbox,
    }
}

fn square_path(size: f32) -> Vec<Seg> {
    vec![
        Seg::Move(p(0.0, 0.0)),
        Seg::Line(p(size, 0.0)),
        Seg::Line(p(size, size)),
        Seg::Line(p(0.0, size)),
        Seg::Close,
    ]
}

fn square_font(glyph: GlyphProc) -> FontSpec {
    let mut glyphs = BTreeMap::new();
    glyphs.insert(b"a".to_vec(), glyph);
    FontSpec::Type3 {
        font_matrix: Matrix::scaling(0.001, 0.001),
        font_bbox: Bounds::new(0.0, 0.0, 1000.0, 1000.0),
        encoding: standard(),
        glyphs,
    }
}

fn text_page(spec: FontSpec, matrix: Matrix, glyphs: Vec<Glyph>) -> Page {
    let mut page = Page::new(LETTER);
    let font = page.resources.add_font(spec);
    page.ops = vec![Op::from(IrOp::Text {
        font,
        matrix,
        glyphs,
        wmode: 0,
    })];
    page
}

fn numbers(value: &Value) -> Vec<f64> {
    array(value).iter().map(number).collect()
}

#[test]
fn a_resident_font_is_an_unembedded_type1_with_widths_and_tounicode() {
    let page = text_page(
        helvetica(),
        Matrix([0.012, 0.0, 0.0, 0.012, 100.0, 700.0]),
        vec![glyph(72, 722.0, 0.0), glyph(105, 222.0, 0.0)],
    );
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n12 0 0 12 100 700 Tm\n(Hi) Tj\nET\n"
    );
    let font = font(&pdf, 0, "F0");
    assert_eq!(font.get("Type").unwrap().as_name(), b"Font");
    assert_eq!(font.get("Subtype").unwrap().as_name(), b"Type1");
    assert_eq!(font.get("BaseFont").unwrap().as_name(), b"Helvetica");
    assert!(
        font.get("Encoding").is_none(),
        "no differences from the built-in encoding"
    );
    assert!(font.get("FontFile").is_none());
    let first = font.get("FirstChar").unwrap().as_int();
    assert_eq!(first, 32);
    assert_eq!(font.get("LastChar").unwrap().as_int(), 251);
    let widths = numbers(font.get("Widths").unwrap());
    assert_eq!(widths.len(), 251 - 32 + 1);
    assert_eq!(widths[(72 - first) as usize], 722.0);
    assert_eq!(widths[(105 - first) as usize], 222.0);
    // Code 127 is unassigned in the standard encoding.
    assert_eq!(widths[(127 - first) as usize], 0.0);
    let descriptor = pdf.resolve(font.get("FontDescriptor").unwrap().as_reference());
    assert_eq!(descriptor.get("FontName").unwrap().as_name(), b"Helvetica");
    assert_eq!(descriptor.get("Flags").unwrap().as_int(), 32);
    assert_eq!(number(descriptor.get("StemV").unwrap()), 88.0);
    assert_eq!(
        numbers(descriptor.get("FontBBox").unwrap()),
        [-166.0, -225.0, 1000.0, 931.0]
    );
    let cmap = String::from_utf8(decoded(
        pdf.resolve(font.get("ToUnicode").unwrap().as_reference()),
    ))
    .unwrap();
    assert!(cmap.contains("<48> <0048>\n"));
    assert!(cmap.contains("<69> <0069>\n"));
    assert!(
        cmap.contains("<27> <2019>\n"),
        "quoteright maps to the typographic quote"
    );
    assert!(cmap.contains("1 begincodespacerange\n<00> <FF>\nendcodespacerange\n"));
    assert!(!cmap.contains("<7F>"), "an unassigned code has no entry");
}

#[test]
fn encoding_differences_and_symbolic_fonts() {
    let mut encoding = standard();
    encoding[65] = Some(b"W".to_vec());
    encoding[66] = None;
    encoding[67] = Some(b"nosuchglyph".to_vec());
    let reencoded = FontSpec::Resident {
        base: ResidentFace::Helvetica,
        encoding,
    };
    let mut page = text_page(
        reencoded,
        Matrix([0.01, 0.0, 0.0, 0.01, 0.0, 0.0]),
        vec![glyph(65, 944.0, 0.0)],
    );
    let symbol = FontSpec::Resident {
        base: ResidentFace::Symbol,
        encoding: glyph_names(
            &ResidentFace::Symbol
                .builtin_encoding()
                .iter()
                .map(|n| n.map(|n| n.as_bytes().to_vec()))
                .collect::<Vec<_>>(),
        ),
    };
    let symbol = page.resources.add_font(symbol);
    page.ops.push(
        IrOp::Text {
            font: symbol,
            matrix: Matrix::scaling(0.01, 0.01),
            glyphs: vec![glyph(97, 631.0, 0.0)],
            wmode: 0,
        }
        .into(),
    );
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    let font = font(&pdf, 0, "F0");
    let differences = array(font.get("Encoding").unwrap().get("Differences").unwrap());
    let shown: Vec<String> = differences
        .iter()
        .map(|v| match v {
            Value::Int(i) => i.to_string(),
            Value::Name(n) => format!("/{}", String::from_utf8_lossy(n)),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(shown, ["65", "/W", "/.notdef", "/nosuchglyph"]);
    let widths = numbers(font.get("Widths").unwrap());
    let first = font.get("FirstChar").unwrap().as_int();
    assert_eq!(widths[(65 - first) as usize], 944.0);
    assert_eq!(widths[(66 - first) as usize], 0.0);
    assert_eq!(widths[(67 - first) as usize], 0.0);
    let symbol = support::font(&pdf, 0, "F1");
    assert_eq!(symbol.get("BaseFont").unwrap().as_name(), b"Symbol");
    assert!(symbol.get("Encoding").is_none());
    let descriptor = pdf.resolve(symbol.get("FontDescriptor").unwrap().as_reference());
    assert_eq!(descriptor.get("Flags").unwrap().as_int(), 4);
    assert!(descriptor.get("CapHeight").is_none());
    let cmap = String::from_utf8(decoded(
        pdf.resolve(symbol.get("ToUnicode").unwrap().as_reference()),
    ))
    .unwrap();
    assert!(cmap.contains("<61> <03B1>\n"), "alpha");
}

#[test]
fn a_type3_font_has_charprocs_written_through_the_content_writer() {
    let glyph_proc = square_glyph(
        vec![IrOp::Fill {
            path: square_path(1000.0),
            rule: FillRule::NonZero,
        }],
        Some(Bounds::new(0.0, 0.0, 1000.0, 1000.0)),
    );
    let page = text_page(
        square_font(glyph_proc),
        Matrix([0.02, 0.0, 0.0, 0.02, 10.0, 10.0]),
        vec![glyph(97, 1000.0, 0.0), glyph(97, 1000.0, 0.0)],
    );
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n20 0 0 20 10 10 Tm\n(aa) Tj\nET\n"
    );
    let font = font(&pdf, 0, "F0");
    assert_eq!(font.get("Subtype").unwrap().as_name(), b"Type3");
    assert_eq!(
        numbers(font.get("FontMatrix").unwrap()),
        [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]
    );
    assert_eq!(
        numbers(font.get("FontBBox").unwrap()),
        [0.0, 0.0, 1000.0, 1000.0]
    );
    let differences = array(font.get("Encoding").unwrap().get("Differences").unwrap());
    assert_eq!(differences[0].as_int(), 97);
    assert_eq!(differences[1].as_name(), b"a");
    assert_eq!(font.get("FirstChar").unwrap().as_int(), 97);
    assert_eq!(font.get("LastChar").unwrap().as_int(), 97);
    assert_eq!(numbers(font.get("Widths").unwrap()), [1000.0]);
    assert!(font.get("Resources").is_none());
    let charproc = pdf.resolve(
        font.get("CharProcs")
            .unwrap()
            .get("a")
            .unwrap()
            .as_reference(),
    );
    assert_eq!(
        String::from_utf8(decoded(charproc)).unwrap(),
        "1000 0 0 0 1000 1000 d1\n0 0 m\n1000 0 l\n1000 1000 l\n0 1000 l\nh\nf\n"
    );
    let cmap = String::from_utf8(decoded(
        pdf.resolve(font.get("ToUnicode").unwrap().as_reference()),
    ))
    .unwrap();
    assert!(cmap.contains("1 beginbfchar\n<61> <0061>\nendbfchar\n"));
}

#[test]
fn a_charwidth_glyph_opens_with_d0_and_carries_its_resources() {
    let mut page = Page::new(LETTER);
    let spot = page.resources.intern_space(&spot());
    let nested = page.resources.add_font(helvetica());
    let glyph_proc = GlyphProc {
        ops: vec![
            IrOp::SetColorSpace(spot).into(),
            IrOp::SetColor(vec![0.6]).into(),
            IrOp::Fill {
                path: square_path(500.0),
                rule: FillRule::NonZero,
            }
            .into(),
            IrOp::Text {
                font: nested,
                matrix: Matrix::scaling(0.4, 0.4),
                glyphs: vec![glyph(72, 722.0, 0.0)],
                wmode: 0,
            }
            .into(),
        ],
        width: (600.0, 0.0),
        bbox: None,
    };
    let font_index = page.resources.add_font(square_font(glyph_proc));
    page.ops = vec![Op::from(IrOp::Text {
        font: font_index,
        matrix: Matrix::scaling(0.01, 0.01),
        glyphs: vec![glyph(97, 600.0, 0.0)],
        wmode: 0,
    })];
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    let font = font(&pdf, 0, "F1");
    let charproc = pdf.resolve(
        font.get("CharProcs")
            .unwrap()
            .get("a")
            .unwrap()
            .as_reference(),
    );
    assert_eq!(
        String::from_utf8(decoded(charproc)).unwrap(),
        "600 0 d0\n/CS0 cs\n/CS0 CS\n0.6 scn\n0.6 SCN\n0 0 m\n500 0 l\n500 500 l\n0 500 l\nh\nf\n\
         BT\n/F0 1 Tf\n400 0 0 400 0 0 Tm\n(H) Tj\nET\n"
    );
    let resources = font.get("Resources").unwrap();
    assert_eq!(
        array(resources.get("ColorSpace").unwrap().get("CS0").unwrap())[0].as_name(),
        b"Separation"
    );
    assert_eq!(
        resources
            .get("Font")
            .unwrap()
            .get("F0")
            .unwrap()
            .as_reference(),
        font_ref(&pdf, 0, "F0")
    );
    assert!(resources.get("XObject").is_none());
    assert_eq!(numbers(font.get("Widths").unwrap()), [600.0]);
}

#[test]
fn displacements_become_adjustments_or_moves() {
    // xshow: 10, 20, 30 user units at size 10 are 1000, 2000, 3000 glyph
    // units; a, b, c are 556 wide.
    let page = text_page(
        helvetica(),
        Matrix::scaling(0.01, 0.01),
        vec![
            glyph(97, 1000.0, 0.0),
            glyph(98, 2000.0, 0.0),
            glyph(99, 3000.0, 0.0),
        ],
    );
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n10 0 0 10 0 0 Tm\n[(a) -444 (b) -1444 (c)] TJ\nET\n"
    );
    // A vertical displacement moves the line; the move is relative to
    // the line start, and the run continues after it.
    let page = text_page(
        helvetica(),
        Matrix::scaling(0.01, 0.01),
        vec![
            glyph(97, 556.0, 0.0),
            glyph(98, 556.0, 100.0),
            glyph(99, 500.0, 0.0),
            glyph(100, 556.0, 0.0),
        ],
    );
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n10 0 0 10 0 0 Tm\n(ab) Tj\n1.112 0.1 Td\n(cd) Tj\nET\n"
    );
    // Type 3 adjustments go through the font matrix's x scale.
    let glyph_proc = square_glyph(Vec::new(), None);
    let mut glyphs = BTreeMap::new();
    glyphs.insert(b"a".to_vec(), glyph_proc);
    let spec = FontSpec::Type3 {
        font_matrix: Matrix::scaling(0.01, 0.01),
        font_bbox: Bounds::new(0.0, 0.0, 100.0, 100.0),
        encoding: standard(),
        glyphs,
    };
    let page = text_page(
        spec,
        Matrix::scaling(0.02, 0.02),
        vec![
            glyph(97, 1000.0, 0.0),
            glyph(97, 1500.0, 0.0),
            glyph(97, 1000.0, 0.0),
        ],
    );
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n2 0 0 2 0 0 Tm\n[(aa) -5000 (a)] TJ\nET\n"
    );
}

#[test]
fn a_singular_type3_matrix_skips_the_run_with_a_note() {
    let mut glyphs = BTreeMap::new();
    glyphs.insert(b"a".to_vec(), square_glyph(Vec::new(), None));
    let spec = FontSpec::Type3 {
        font_matrix: Matrix::scaling(0.0, 0.0),
        font_bbox: Bounds::new(0.0, 0.0, 0.0, 0.0),
        encoding: standard(),
        glyphs,
    };
    let mut page = text_page(spec, Matrix::IDENTITY, vec![glyph(97, 0.0, 0.0)]);
    page.ops.push(
        IrOp::Fill {
            path: line(),
            rule: FillRule::NonZero,
        }
        .into(),
    );
    let mut sink = PdfSink::new(Vec::new(), uncompressed()).unwrap();
    sink.page(page);
    assert_eq!(
        sink.notes(),
        ["page 1: text in font F0 skipped: its font matrix is singular"]
    );
    let bytes = sink.finish().unwrap();
    let pdf = check(&bytes);
    assert_eq!(content(&pdf, 0), "10 10 m\n100 10 l\nf\n");
}

#[test]
fn fonts_are_written_once_per_document_and_shared_by_equal_pages() {
    let first = text_page(
        helvetica(),
        Matrix::scaling(0.012, 0.012),
        vec![glyph(72, 722.0, 0.0)],
    );
    let second = text_page(
        helvetica(),
        Matrix::scaling(0.024, 0.024),
        vec![glyph(105, 222.0, 0.0)],
    );
    let mut reencoded = standard();
    reencoded[65] = Some(b"W".to_vec());
    let third = text_page(
        FontSpec::Resident {
            base: ResidentFace::Helvetica,
            encoding: reencoded,
        },
        Matrix::scaling(0.012, 0.012),
        vec![glyph(65, 944.0, 0.0)],
    );
    let bytes = distil_pages(vec![first, second, third], uncompressed());
    let pdf = check(&bytes);
    assert_eq!(font_ref(&pdf, 0, "F0"), font_ref(&pdf, 1, "F0"));
    assert_ne!(font_ref(&pdf, 0, "F0"), font_ref(&pdf, 2, "F0"));
    let text = String::from_utf8_lossy(&bytes);
    assert_eq!(text.matches("/Subtype /Type1").count(), 2);
    let _ = FontIndex(0);
}

#[test]
fn a_type3_font_is_shared_only_when_its_references_mean_the_same() {
    let make = |space: SpaceSpec| {
        let mut page = Page::new(LETTER);
        let cs = page.resources.intern_space(&space);
        let glyph_proc = GlyphProc {
            ops: vec![
                IrOp::SetColorSpace(cs).into(),
                IrOp::SetColor(vec![0.5]).into(),
                IrOp::Fill {
                    path: square_path(100.0),
                    rule: FillRule::NonZero,
                }
                .into(),
            ],
            width: (1000.0, 0.0),
            bbox: None,
        };
        let font = page.resources.add_font(square_font(glyph_proc));
        page.ops = vec![Op::from(IrOp::Text {
            font,
            matrix: Matrix::scaling(0.01, 0.01),
            glyphs: vec![glyph(97, 1000.0, 0.0)],
            wmode: 0,
        })];
        page
    };
    let other = SpaceSpec::Separation {
        name: b"Other".to_vec(),
        alternate: Box::new(SpaceSpec::DeviceGray),
        tint_source: b"{}".to_vec(),
    };
    let pdf = check(&distil_pages(
        vec![make(spot()), make(spot()), make(other)],
        uncompressed(),
    ));
    assert_eq!(font_ref(&pdf, 0, "F0"), font_ref(&pdf, 1, "F0"));
    assert_ne!(font_ref(&pdf, 0, "F0"), font_ref(&pdf, 2, "F0"));
}

// --- embedded fonts -------------------------------------------------------------------

use std::rc::Rc;

use ps_fonts::testing::{corpus_truetype, corpus_type1};
use ps_fonts::type1::decrypt_section;
use ps_fonts::{ProgramKind, TrueTypeProgram};
use ps_graphics::ProgramRef;

fn names(pairs: &[(u8, &str)]) -> ps_graphics::GlyphNames {
    let mut names: Vec<Option<Vec<u8>>> = vec![None; 256];
    for &(code, name) in pairs {
        names[usize::from(code)] = Some(name.as_bytes().to_vec());
    }
    glyph_names(&names)
}

fn syn(program: &Rc<ps_fonts::Program>) -> FontSpec {
    FontSpec::Embedded {
        family: 3,
        kind: ProgramKind::Type1,
        font_name: b"Syn".to_vec(),
        font_matrix: Matrix::scaling(0.001, 0.001),
        program: ProgramRef(program.clone()),
        encoding: names(&[(97, "a"), (98, "b"), (101, "e"), (233, "eacute")]),
    }
}

/// The plain text of a FontFile's encrypted portion.
fn private_text(font_file: &Value) -> String {
    let data = decoded(font_file);
    let length1 = font_file.get("Length1").unwrap().as_int() as usize;
    let length2 = font_file.get("Length2").unwrap().as_int() as usize;
    let length3 = font_file.get("Length3").unwrap().as_int() as usize;
    assert_eq!(data.len(), length1 + length2 + length3);
    assert!(data.starts_with(b"%!FontType1-1.0: "));
    assert!(data.ends_with(b"cleartomark\n"));
    String::from_utf8_lossy(&decrypt_section(&data[length1..length1 + length2])).into_owned()
}

#[test]
fn an_embedded_type1_font_is_subset_and_written_at_finish() {
    let program = Rc::new(corpus_type1().program());
    let first = text_page(
        syn(&program),
        Matrix([0.01, 0.0, 0.0, 0.01, 100.0, 100.0]),
        vec![glyph(97, 600.0, 0.0), glyph(233, 500.0, 0.0)],
    );
    let second = text_page(
        syn(&program),
        Matrix([0.02, 0.0, 0.0, 0.02, 100.0, 200.0]),
        vec![glyph(101, 500.0, 0.0)],
    );
    let bytes = distil_pages(vec![first, second], uncompressed());
    let pdf = check(&bytes);
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n10 0 0 10 100 100 Tm\n<61E9> Tj\nET\n"
    );
    assert_eq!(font_ref(&pdf, 0, "F0"), font_ref(&pdf, 1, "F0"));
    let font = font(&pdf, 0, "F0");
    assert_eq!(font.get("Subtype").unwrap().as_name(), b"Type1");
    let base = font.get("BaseFont").unwrap().as_name().to_vec();
    assert_eq!(base.len(), 10);
    assert!(base.ends_with(b"+Syn"));
    assert!(base[..6].iter().all(u8::is_ascii_uppercase));
    assert_eq!(font.get("FirstChar").unwrap().as_int(), 97);
    assert_eq!(font.get("LastChar").unwrap().as_int(), 233);
    let widths = numbers(font.get("Widths").unwrap());
    assert_eq!(widths.len(), 233 - 97 + 1);
    assert_eq!(widths[0], 600.0);
    assert_eq!(widths[101 - 97], 500.0);
    assert_eq!(widths[98 - 97], 0.0, "an unused code has no width");
    assert_eq!(widths[233 - 97], 500.0);
    let differences = array(font.get("Encoding").unwrap().get("Differences").unwrap());
    assert_eq!(differences.len(), 2);
    assert_eq!(differences[0].as_int(), 233);
    assert_eq!(differences[1].as_name(), b"eacute");
    let descriptor = pdf.resolve(font.get("FontDescriptor").unwrap().as_reference());
    assert_eq!(
        descriptor.get("FontName").unwrap().as_name(),
        base.as_slice()
    );
    assert_eq!(descriptor.get("Flags").unwrap().as_int(), 4);
    assert_eq!(
        numbers(descriptor.get("FontBBox").unwrap()),
        [0.0, 0.0, 750.0, 750.0]
    );
    assert_eq!(number(descriptor.get("Ascent").unwrap()), 750.0);
    assert_eq!(number(descriptor.get("StemV").unwrap()), 80.0);
    let font_file = pdf.resolve(descriptor.get("FontFile").unwrap().as_reference());
    let plain = private_text(font_file);
    assert!(
        plain.contains("dup /CharStrings 5 dict dup begin\n"),
        "{plain}"
    );
    for name in [".notdef", "a", "e", "eacute", "acute"] {
        assert!(plain.contains(&format!("/{name} ")), "{name} kept");
    }
    assert!(!plain.contains("/b "), "b was never shown");
    let cmap = String::from_utf8(decoded(
        pdf.resolve(font.get("ToUnicode").unwrap().as_reference()),
    ))
    .unwrap();
    assert!(cmap.contains("<61> <0061>\n"));
    assert!(cmap.contains("<E9> <00E9>\n"));
    assert!(!cmap.contains("<62>"));
    // The font objects are written after both pages, whatever their ids.
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.rfind("/Type /Page ").unwrap() < text.find("/Subtype /Type1").unwrap());
}

#[test]
fn an_embedded_truetype_font_carries_a_symbolic_subset_with_a_cmap() {
    let program = Rc::new(corpus_truetype().program().unwrap());
    let spec = FontSpec::Embedded {
        family: 4,
        kind: ProgramKind::TrueType,
        font_name: b"SynTT".to_vec(),
        font_matrix: Matrix::IDENTITY,
        program: ProgramRef(program),
        encoding: names(&[(65, "a"), (66, "o"), (67, "zz")]),
    };
    let page = text_page(
        spec,
        Matrix([20.0, 0.0, 0.0, 20.0, 100.0, 100.0]),
        vec![
            glyph(65, 0.5, 0.0),
            glyph(66, 1200.0 / 2048.0, 0.0),
            glyph(67, 0.5, 0.0),
        ],
    );
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n20 0 0 20 100 100 Tm\n(ABC) Tj\nET\n"
    );
    let font = font(&pdf, 0, "F0");
    assert_eq!(font.get("Subtype").unwrap().as_name(), b"TrueType");
    assert!(font.get("BaseFont").unwrap().as_name().ends_with(b"+SynTT"));
    assert!(
        font.get("Encoding").is_none(),
        "symbolic: the cmap maps the codes"
    );
    assert_eq!(font.get("FirstChar").unwrap().as_int(), 65);
    assert_eq!(
        numbers(font.get("Widths").unwrap()),
        [500.0, 585.938, 500.0]
    );
    let descriptor = pdf.resolve(font.get("FontDescriptor").unwrap().as_reference());
    assert_eq!(descriptor.get("Flags").unwrap().as_int(), 4);
    assert_eq!(
        numbers(descriptor.get("FontBBox").unwrap()),
        [0.0, 0.0, 537.109, 488.281]
    );
    let font_file = pdf.resolve(descriptor.get("FontFile2").unwrap().as_reference());
    let data = decoded(font_file);
    assert_eq!(
        font_file.get("Length1").unwrap().as_int() as usize,
        data.len()
    );
    let subset = TrueTypeProgram::parse(data).unwrap();
    assert_eq!(subset.num_glyphs(), 3);
    let cmap = subset.cmap(3, 0).unwrap().unwrap();
    assert_eq!(cmap.get(&65), Some(&1));
    assert_eq!(cmap.get(&66), Some(&2));
    assert_eq!(cmap.get(&0xF041), Some(&1));
    assert_eq!(cmap.get(&67), None, "a code drawing glyph 0 is not mapped");
    assert_eq!(subset.advance(1).unwrap(), 1024);
    assert_eq!(subset.advance(2).unwrap(), 1200);
    let to_unicode = String::from_utf8(decoded(
        pdf.resolve(font.get("ToUnicode").unwrap().as_reference()),
    ))
    .unwrap();
    assert!(to_unicode.contains("<41> <0061>\n"), "{to_unicode}");
    assert!(to_unicode.contains("<42> <006F>\n"));
}

#[test]
fn glyphs_shown_inside_a_type3_procedure_count_for_the_embedded_font() {
    let program = Rc::new(corpus_type1().program());
    let mut page = Page::new(LETTER);
    let type3 = page.resources.add_font(square_font(GlyphProc {
        ops: vec![
            IrOp::Text {
                font: FontIndex(1),
                matrix: Matrix::scaling(0.001, 0.001),
                glyphs: vec![glyph(101, 500.0, 0.0)],
                wmode: 0,
            }
            .into(),
        ],
        width: (1000.0, 0.0),
        bbox: None,
    }));
    let embedded = page.resources.add_font(syn(&program));
    assert_eq!(embedded, FontIndex(1));
    page.ops = vec![Op::from(IrOp::Text {
        font: type3,
        matrix: Matrix::scaling(0.01, 0.01),
        glyphs: vec![glyph(97, 1000.0, 0.0)],
        wmode: 0,
    })];
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    let font = font(&pdf, 0, "F1");
    assert_eq!(font.get("FirstChar").unwrap().as_int(), 101);
    assert_eq!(font.get("LastChar").unwrap().as_int(), 101);
    assert_eq!(numbers(font.get("Widths").unwrap()), [500.0]);
}

#[test]
fn embedded_fonts_distil_deterministically() {
    let program = Rc::new(corpus_type1().program());
    let make = || {
        text_page(
            syn(&program),
            Matrix([0.01, 0.0, 0.0, 0.01, 100.0, 100.0]),
            vec![glyph(97, 600.0, 0.0)],
        )
    };
    let first = distil_pages(vec![make()], uncompressed());
    let second = distil_pages(vec![make()], uncompressed());
    assert_eq!(first, second);
    let program_again = Rc::new(corpus_type1().program());
    let third = distil_pages(
        vec![text_page(
            syn(&program_again),
            Matrix([0.01, 0.0, 0.0, 0.01, 100.0, 100.0]),
            vec![glyph(97, 600.0, 0.0)],
        )],
        uncompressed(),
    );
    assert_eq!(first, third, "the snapshot's identity leaves no trace");
}

// --- composite fonts ------------------------------------------------------------------

fn cid_glyph(cid: u16, dx: f32, dy: f32) -> Glyph {
    Glyph {
        code: u32::from(cid),
        len: 2,
        cid,
        dx,
        dy,
    }
}

fn composite(wmode: u8, program: &Rc<ps_fonts::Program>, name: &[u8]) -> FontSpec {
    FontSpec::Composite {
        cmap_name: if wmode == 1 {
            b"Identity-V".to_vec()
        } else {
            b"Identity-H".to_vec()
        },
        wmode,
        unicode_based: false,
        descendant: Box::new(FontSpec::Embedded {
            family: 5,
            kind: program.kind(),
            font_name: name.to_vec(),
            font_matrix: Matrix::scaling(0.001, 0.001),
            program: ps_graphics::ProgramRef(program.clone()),
            encoding: glyph_names(&[]),
        }),
        cid_to_code: BTreeMap::new(),
    }
}

fn composite_page(spec: FontSpec, wmode: u8, glyphs: Vec<Glyph>) -> Page {
    let mut page = Page::new(LETTER);
    let font = page.resources.add_font(spec);
    page.ops = vec![Op::from(IrOp::Text {
        font,
        matrix: Matrix([0.01, 0.0, 0.0, 0.01, 100.0, 100.0]),
        glyphs,
        wmode,
    })];
    page
}

#[test]
fn composite_adjustments_run_along_the_writing_direction() {
    use ps_fonts::testing::corpus_cid_cff;
    let program = Rc::new(corpus_cid_cff().program().unwrap());
    // Horizontal: CID 1 pushed to 1000 units, then CID 2 at its width.
    let page = composite_page(
        composite(0, &program, b"SynCID"),
        0,
        vec![cid_glyph(1, 1000.0, 0.0), cid_glyph(2, 700.0, 0.0)],
    );
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n10 0 0 10 100 100 Tm\n[<0001> -500 <0002>] TJ\nET\n"
    );
    // Vertical: a longer drop is an adjustment, a sideways step a move.
    let page = composite_page(
        composite(1, &program, b"SynCID"),
        1,
        vec![
            cid_glyph(1, 0.0, -1500.0),
            cid_glyph(2, 100.0, -1000.0),
            cid_glyph(1, 0.0, -1000.0),
        ],
    );
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n10 0 0 10 100 100 Tm\n[<0001> 500 <0002>] TJ\n0.1 -2.5 Td\n<0001> Tj\nET\n"
    );
    let font = font(&pdf, 0, "F0");
    assert_eq!(font.get("Encoding").unwrap().as_name(), b"Identity-V");
}

#[test]
fn a_type3_fallback_assigns_codes_in_order_of_first_use_across_pages() {
    use ps_fonts::Program;
    use ps_fonts::testing::CidType1Font;
    let program = Rc::new(Program::Type1Cid(CidType1Font::corpus().program().unwrap()));
    let first = composite_page(
        composite(0, &program, b"SynCIDT1"),
        0,
        vec![cid_glyph(2, 700.0, 0.0)],
    );
    let second = composite_page(
        composite(0, &program, b"SynCIDT1"),
        0,
        vec![cid_glyph(1, 500.0, 0.0), cid_glyph(2, 700.0, 0.0)],
    );
    let pdf = check(&distil_pages(vec![first, second], uncompressed()));
    assert_eq!(font_ref(&pdf, 0, "F0"), font_ref(&pdf, 1, "F0"));
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n10 0 0 10 100 100 Tm\n<01> Tj\nET\n"
    );
    assert_eq!(
        content(&pdf, 1),
        "BT\n/F0 1 Tf\n10 0 0 10 100 100 Tm\n<0201> Tj\nET\n",
        "CID 2 took code 1 on the first page, CID 1 code 2 on the second"
    );
    let font = font(&pdf, 0, "F0");
    assert_eq!(font.get("Subtype").unwrap().as_name(), b"Type3");
    let differences = array(font.get("Encoding").unwrap().get("Differences").unwrap());
    assert_eq!(differences.len(), 3);
    assert_eq!(differences[0].as_int(), 1);
    assert_eq!(differences[1].as_name(), b"cid2");
    assert_eq!(differences[2].as_name(), b"cid1");
    assert_eq!(numbers(font.get("Widths").unwrap()), [700.0, 500.0]);
    assert_eq!(
        numbers(font.get("FontBBox").unwrap()),
        [0.0, 0.0, 600.0, 600.0]
    );
    let procs = font.get("CharProcs").unwrap();
    let cid1 = String::from_utf8(decoded(
        pdf.resolve(procs.get("cid1").unwrap().as_reference()),
    ))
    .unwrap();
    assert!(
        cid1.starts_with("500 0 50 0 450 400 d1\n50 0 m\n"),
        "{cid1}"
    );
}

#[test]
fn a_vertical_type3_fallback_draws_from_the_vertical_origin_with_zero_widths() {
    use ps_fonts::Program;
    use ps_fonts::testing::CidType1Font;
    let program = Rc::new(Program::Type1Cid(CidType1Font::corpus().program().unwrap()));
    let page = composite_page(
        composite(1, &program, b"SynCIDT1"),
        1,
        vec![cid_glyph(2, 0.0, -1000.0), cid_glyph(1, 0.0, -1000.0)],
    );
    let pdf = check(&distil_pages(vec![page], uncompressed()));
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n10 0 0 10 100 100 Tm\n<02> Tj\n0 -1 Td\n<01> Tj\nET\n",
        "codes follow CID order within a page; every advance is a move"
    );
    let font = font(&pdf, 0, "F0");
    assert_eq!(numbers(font.get("Widths").unwrap()), [0.0, 0.0]);
    let procs = font.get("CharProcs").unwrap();
    let cid2 = String::from_utf8(decoded(
        pdf.resolve(procs.get("cid2").unwrap().as_reference()),
    ))
    .unwrap();
    assert_eq!(
        cid2,
        "0 0 -350 -880 250 -280 d1\n-350 -880 m\n250 -880 l\n250 -280 l\n-350 -280 l\nh\nf\n"
    );
}

// --- embed-all and downsampling ---------------------------------------------------------

use ps_graphics::DocMark;
use ps_vm::MarkValue;

fn builtin(face: ResidentFace) -> ps_graphics::GlyphNames {
    let names: Vec<Option<Vec<u8>>> = face
        .builtin_encoding()
        .iter()
        .map(|n| n.map(|n| n.as_bytes().to_vec()))
        .collect();
    glyph_names(&names)
}

fn resident(face: ResidentFace) -> FontSpec {
    FontSpec::Resident {
        base: face,
        encoding: builtin(face),
    }
}

fn embed_all() -> Options {
    let mut options = uncompressed();
    options.params.embed_all_fonts = true;
    options
}

const TEXT_AT_24: Matrix = Matrix([0.024, 0.0, 0.0, 0.024, 72.0, 700.0]);

/// The `FontFile2` stream behind a font dictionary, when it embeds one.
fn font_file2<'a>(pdf: &'a support::Pdf, font: &Value) -> Option<&'a Value> {
    let descriptor = pdf.resolve(font.get("FontDescriptor")?.as_reference());
    Some(pdf.resolve(descriptor.get("FontFile2")?.as_reference()))
}

fn refused(key: &str, text: &str) -> (String, String) {
    (key.to_string(), text.to_string())
}

// embed-all-helvetica.ps
#[test]
fn embed_all_writes_a_resident_face_from_its_asset_with_the_metrics_widths() {
    let page = text_page(
        helvetica(),
        TEXT_AT_24,
        vec![glyph(72, 722.0, 0.0), glyph(105, 222.0, 0.0)],
    );
    let mut sink = PdfSink::new(Vec::new(), embed_all()).unwrap();
    sink.page(page);
    let not_honoured = sink.not_honoured();
    let pdf = check(&sink.finish().unwrap());
    assert_eq!(
        content(&pdf, 0),
        "BT\n/F0 1 Tf\n24 0 0 24 72 700 Tm\n(Hi) Tj\nET\n"
    );
    let font = font(&pdf, 0, "F0");
    if !ps_fonts::has_resident_outlines() {
        assert_eq!(font.get("BaseFont").unwrap().as_name(), b"Helvetica");
        assert!(font_file2(&pdf, font).is_none());
        assert_eq!(
            not_honoured,
            vec![refused(
                "EmbedAllFonts",
                "true (Helvetica: outline assets absent from this build)"
            )]
        );
        return;
    }
    assert!(not_honoured.is_empty(), "{not_honoured:?}");
    assert_eq!(font.get("Subtype").unwrap().as_name(), b"TrueType");
    let base_font = font.get("BaseFont").unwrap().as_name();
    assert!(
        base_font.ends_with(b"+LiberationSans-Regular"),
        "{}",
        String::from_utf8_lossy(base_font)
    );
    assert_eq!(base_font.len(), "ABCDEF+LiberationSans-Regular".len());
    assert!(
        font.get("Encoding").is_none(),
        "symbolic: the cmap maps the codes"
    );
    assert_eq!(font.get("FirstChar").unwrap().as_int(), 72);
    assert_eq!(font.get("LastChar").unwrap().as_int(), 105);
    let widths = numbers(font.get("Widths").unwrap());
    assert_eq!(widths.len(), 34);
    assert_eq!((widths[0], widths[33]), (722.0, 222.0), "the AFM's H and i");
    let descriptor = pdf.resolve(font.get("FontDescriptor").unwrap().as_reference());
    assert_eq!(descriptor.get("Flags").unwrap().as_int(), 4);
    assert_eq!(descriptor.get("FontName").unwrap().as_name(), base_font);
    assert_eq!(number(descriptor.get("CapHeight").unwrap()), 718.0);
    assert_eq!(number(descriptor.get("StemV").unwrap()), 88.0);
    let font_file = font_file2(&pdf, font).unwrap();
    let data = decoded(font_file);
    assert_eq!(
        font_file.get("Length1").unwrap().as_int() as usize,
        data.len()
    );
    let subset = TrueTypeProgram::parse(data).unwrap();
    assert_eq!(subset.num_glyphs(), 3, "notdef, H, i");
    let cmap = subset.cmap(3, 0).unwrap().unwrap();
    let (h, i) = (cmap[&72u32], cmap[&105u32]);
    assert!(h != 0 && i != 0 && h != i, "{cmap:?}");
    assert_eq!(cmap.get(&0xF048), Some(&h));
    // The asset is metric-compatible: its advances agree with the AFM.
    let em = f64::from(subset.units_per_em());
    let advance = |gid: u16| (f64::from(subset.advance(gid).unwrap()) * 1000.0 / em).round();
    assert_eq!((advance(h), advance(i)), (722.0, 222.0));
    let to_unicode = String::from_utf8(decoded(
        pdf.resolve(font.get("ToUnicode").unwrap().as_reference()),
    ))
    .unwrap();
    assert!(to_unicode.contains("<48> <0048>\n"), "{to_unicode}");
    assert!(to_unicode.contains("<69> <0069>\n"));
}

#[test]
fn embed_all_reports_faces_without_an_asset_and_faces_written_before_the_request() {
    let mut sink = PdfSink::new(Vec::new(), uncompressed()).unwrap();
    sink.page(text_page(
        helvetica(),
        TEXT_AT_24,
        vec![glyph(72, 722.0, 0.0)],
    ));
    sink.document(DocMark::Params(vec![(
        b"EmbedAllFonts".to_vec(),
        MarkValue::Bool(true),
    )]));
    let mut page = Page::new(LETTER);
    let symbol = page.resources.add_font(resident(ResidentFace::Symbol));
    let again = page.resources.add_font(helvetica());
    page.ops = vec![
        IrOp::Text {
            font: symbol,
            matrix: TEXT_AT_24,
            glyphs: vec![glyph(97, 631.0, 0.0)],
            wmode: 0,
        },
        IrOp::Text {
            font: again,
            matrix: TEXT_AT_24,
            glyphs: vec![glyph(105, 222.0, 0.0)],
            wmode: 0,
        },
    ]
    .into_iter()
    .map(Op::from)
    .collect();
    sink.page(page);
    assert!(sink.params().embed_all_fonts);
    let not_honoured = sink.not_honoured();
    let pdf = check(&sink.finish().unwrap());
    let first = font(&pdf, 0, "F0");
    assert_eq!(first.get("BaseFont").unwrap().as_name(), b"Helvetica");
    assert!(font_file2(&pdf, first).is_none());
    let symbol = font(&pdf, 1, "F0");
    assert_eq!(symbol.get("BaseFont").unwrap().as_name(), b"Symbol");
    assert!(font_file2(&pdf, symbol).is_none());
    let second = font(&pdf, 1, "F1");
    if ps_fonts::has_resident_outlines() {
        assert_eq!(second.get("Subtype").unwrap().as_name(), b"TrueType");
        assert!(font_file2(&pdf, second).is_some());
        assert_ne!(font_ref(&pdf, 0, "F0"), font_ref(&pdf, 1, "F1"));
        assert_eq!(
            not_honoured,
            vec![
                refused(
                    "EmbedAllFonts",
                    "true (Helvetica: written before the request)"
                ),
                refused("EmbedAllFonts", "true (Symbol: no outline asset)"),
            ]
        );
    } else {
        assert_eq!(
            font_ref(&pdf, 0, "F0"),
            font_ref(&pdf, 1, "F1"),
            "one unembedded object serves both pages"
        );
        assert_eq!(
            not_honoured,
            vec![
                refused(
                    "EmbedAllFonts",
                    "true (Helvetica: outline assets absent from this build)"
                ),
                refused("EmbedAllFonts", "true (Symbol: no outline asset)"),
            ]
        );
    }
}

#[test]
fn subset_fonts_off_embeds_the_whole_asset_under_its_own_name() {
    if !ps_fonts::has_resident_outlines() {
        return;
    }
    let mut options = embed_all();
    options.params.subset_fonts = false;
    let page = text_page(helvetica(), TEXT_AT_24, vec![glyph(72, 722.0, 0.0)]);
    let pdf = check(&distil_pages(vec![page], options));
    let font = font(&pdf, 0, "F0");
    assert_eq!(
        font.get("BaseFont").unwrap().as_name(),
        b"LiberationSans-Regular"
    );
    let whole = TrueTypeProgram::parse(decoded(font_file2(&pdf, font).unwrap())).unwrap();
    let asset = ResidentFace::Helvetica.outlines().unwrap();
    let ps_fonts::Program::TrueType(program) = &**asset.program() else {
        panic!("a TrueType asset")
    };
    assert_eq!(whole.num_glyphs(), program.num_glyphs());
    assert_eq!(
        whole.cmap(3, 0).unwrap().unwrap().len(),
        2,
        "the used code, bare and in the F0 range"
    );
}

#[test]
fn embed_all_finds_glyphs_by_unicode_when_the_post_table_lacks_the_name() {
    if !ps_fonts::has_resident_outlines() {
        return;
    }
    let mut names = standard();
    names[128] = Some(b"Euro".to_vec());
    names[129] = Some(b"nosuchglyph".to_vec());
    let spec = FontSpec::Resident {
        base: ResidentFace::Helvetica,
        encoding: names,
    };
    let page = text_page(
        spec,
        TEXT_AT_24,
        vec![glyph(128, 556.0, 0.0), glyph(129, 0.0, 0.0)],
    );
    let pdf = check(&distil_pages(vec![page], embed_all()));
    let font = font(&pdf, 0, "F0");
    assert_eq!(numbers(font.get("Widths").unwrap()), [556.0, 0.0]);
    let subset = TrueTypeProgram::parse(decoded(font_file2(&pdf, font).unwrap())).unwrap();
    assert_eq!(subset.num_glyphs(), 2, "notdef and Euro");
    let cmap = subset.cmap(3, 0).unwrap().unwrap();
    assert_eq!(cmap.get(&128), Some(&1));
    assert_eq!(
        cmap.get(&129),
        None,
        "a missing glyph draws notdef, unmapped"
    );
}

#[test]
fn colour_images_are_averaged_per_component_and_masks_are_subsampled() {
    let mut page = Page::new(LETTER);
    let mut rgb = image_spec(
        Some(SpaceSpec::DeviceRGB),
        8,
        vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
    );
    (rgb.width, rgb.height) = (4, 2);
    let rgb_data: Vec<u8> = (0..24).collect();
    let colour = page.resources.add_image(&rgb, &rgb_data);
    let mut bits = image_spec(None, 1, vec![0.0, 1.0]);
    (bits.width, bits.height) = (4, 2);
    let mask = page.resources.add_image(&bits, &[0b1010_0000, 0b0101_0000]);
    let indexed = SpaceSpec::Indexed {
        base: Box::new(SpaceSpec::DeviceRGB),
        hival: 1,
        lookup: vec![0; 6],
    };
    let mut table = image_spec(Some(indexed), 8, vec![0.0, 255.0]);
    (table.width, table.height) = (4, 2);
    let lookup = page.resources.add_image(&table, &[0; 8]);
    let gray = page.resources.add_image(
        &image_spec(Some(SpaceSpec::DeviceGray), 8, vec![0.0, 1.0]),
        &[1, 2, 3, 4],
    );
    // Four samples across two points and two down one: 144 per inch.
    let at = Matrix([2.0, 0.0, 0.0, 1.0, 100.0, 100.0]);
    page.ops = [colour, mask, lookup, gray]
        .into_iter()
        .map(|image| Op::from(IrOp::Image { image, matrix: at }))
        .collect();
    let mut options = uncompressed();
    options.params.downsample_color_images = true;
    options.params.color_image_resolution = 72;
    options.params.downsample_mono_images = true;
    options.params.mono_image_resolution = 72;
    let mut sink = PdfSink::new(Vec::new(), options).unwrap();
    sink.page(page);
    assert_eq!(sink.downsampled(), 2);
    assert_eq!(
        sink.notes(),
        ["page 1: image 2 (Indexed) is not downsampled"]
    );
    assert_eq!(
        sink.not_honoured(),
        vec![refused(
            "MonoImageDownsampleType",
            "/Average (one-bit images are subsampled)"
        )]
    );
    let pdf = check(&sink.finish().unwrap());
    assert!(
        content(&pdf, 0).starts_with("q 2 0 0 1 100 100 cm /Im0 Do Q\n"),
        "the matrix maps the unit square and stays"
    );
    let size = |name: &str| {
        let x = xobject(&pdf, 0, name);
        (
            x.get("Width").unwrap().as_int(),
            x.get("Height").unwrap().as_int(),
        )
    };
    assert_eq!(size("Im0"), (2, 1));
    assert_eq!(decoded(xobject(&pdf, 0, "Im0")), [8, 9, 10, 14, 15, 16]);
    assert_eq!(size("Im1"), (2, 1));
    let mask = xobject(&pdf, 0, "Im1");
    assert_eq!(mask.get("ImageMask"), Some(&Value::Bool(true)));
    assert_eq!(decoded(mask), [0b1100_0000]);
    assert_eq!(size("Im2"), (4, 2), "Indexed stays");
    assert_eq!(size("Im3"), (2, 2), "gray downsampling is off");
    assert_eq!(decoded(xobject(&pdf, 0, "Im3")), [1, 2, 3, 4]);
}
