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
    Options { compress: false }
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
        "/DeviceRGB cs\n0.2 0.4 0.6 rg\n/DeviceCMYK cs\n0 0 0 1 k\n/DeviceGray cs\n0.5 g\n10 10 m\n100 10 l\nf\n"
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
    assert_eq!(content(&pdf, 0), "/CS0 cs\n0.6 scn\n10 10 m\n100 10 l\nf\n");
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
        "/CS0 cs\n0.25 0.75 scn\n/CS1 cs\n1 scn\n10 10 m\n100 10 l\nf\n"
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
        "/CS0 cs\n0.6 scn\nq 1 0 0 -1 0 1 cm /Im0 Do Q\n"
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
    assert_eq!(Options::default(), Options { compress: true });
    let mut page = stroked_line();
    let spot = page.resources.intern_space(&spot());
    page.ops.insert(0, IrOp::SetColorSpace(spot).into());
    let pdf = check(&distil_pages(vec![page], Options::default()));
    let contents = pdf.resolve(page_contents(&pdf));
    assert_eq!(contents.get("Filter").unwrap().as_name(), b"FlateDecode");
    assert_eq!(content(&pdf, 0), "/CS0 cs\n2 w\n10 10 m\n100 10 l\nS\n");
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
