// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Test-side helpers over the PDF writer's own structural reader, which
//! lives with `pdf-out`'s tests and is included from there so the two
//! crates check output with one reader.

#![allow(dead_code)] // Each integration-test target uses a subset.

#[path = "../../../pdf-out/tests/common/mod.rs"]
pub mod reader;

pub use reader::{Pdf, Value, check, inflate};

use ps_graphics::{Page, PageSink};
use remelt::{Options, PdfSink};

/// Writes `pages` through the sink and returns the finished bytes.
pub fn distil_pages(pages: Vec<Page>, options: Options) -> Vec<u8> {
    let mut sink = PdfSink::new(Vec::new(), options).unwrap();
    for page in pages {
        sink.page(page);
    }
    sink.finish().unwrap()
}

/// The page ids in tree order.
pub fn kids(pdf: &Pdf) -> Vec<u32> {
    let root = pdf.trailer_get("Root").unwrap().as_reference();
    let pages = pdf.resolve(root).get("Pages").unwrap().as_reference();
    let node = pdf.resolve(pages);
    assert_eq!(node.get("Type").unwrap().as_name(), b"Pages");
    let kids = match node.get("Kids").unwrap() {
        Value::Array(kids) => kids.iter().map(Value::as_reference).collect::<Vec<_>>(),
        other => panic!("Kids must be an array, got {other:?}"),
    };
    assert_eq!(node.get("Count").unwrap().as_int(), kids.len() as i64);
    kids
}

pub fn page(pdf: &Pdf, index: usize) -> &Value {
    let page = pdf.resolve(kids(pdf)[index]);
    assert_eq!(page.get("Type").unwrap().as_name(), b"Page");
    page
}

pub fn media_box(pdf: &Pdf, index: usize) -> Vec<f64> {
    match page(pdf, index).get("MediaBox").unwrap() {
        Value::Array(items) => items
            .iter()
            .map(|v| match v {
                Value::Int(i) => *i as f64,
                Value::Real(r) => *r,
                other => panic!("MediaBox holds numbers, got {other:?}"),
            })
            .collect(),
        other => panic!("MediaBox must be an array, got {other:?}"),
    }
}

/// A stream's data after undoing its filter; a DCT stream is its own
/// data, passed through as it was.
pub fn decoded(stream: &Value) -> Vec<u8> {
    let data = stream.stream_data();
    match stream.get("Filter") {
        None => data.to_vec(),
        Some(Value::Name(name)) if name == b"FlateDecode" => inflate(data),
        Some(Value::Name(name)) if name == b"DCTDecode" => data.to_vec(),
        Some(other) => panic!("unexpected filter {other:?}"),
    }
}

/// The content stream of page `index` as text.
pub fn content(pdf: &Pdf, index: usize) -> String {
    let contents = page(pdf, index).get("Contents").unwrap().as_reference();
    String::from_utf8(decoded(pdf.resolve(contents))).unwrap()
}

pub fn resources(pdf: &Pdf, index: usize) -> &Value {
    page(pdf, index).get("Resources").unwrap()
}

/// The named colour-space resource of page `index`.
pub fn color_space<'a>(pdf: &'a Pdf, index: usize, name: &str) -> &'a Value {
    resources(pdf, index)
        .get("ColorSpace")
        .unwrap_or_else(|| panic!("page {index} declares no colour-space resources"))
        .get(name)
        .unwrap_or_else(|| panic!("page {index} has no colour space {name}"))
}

/// The named image XObject of page `index`.
pub fn xobject<'a>(pdf: &'a Pdf, index: usize, name: &str) -> &'a Value {
    let r = resources(pdf, index)
        .get("XObject")
        .unwrap_or_else(|| panic!("page {index} declares no XObjects"))
        .get(name)
        .unwrap_or_else(|| panic!("page {index} has no XObject {name}"))
        .as_reference();
    pdf.resolve(r)
}

/// The named tiling pattern of page `index`.
pub fn pattern<'a>(pdf: &'a Pdf, index: usize, name: &str) -> &'a Value {
    let r = resources(pdf, index)
        .get("Pattern")
        .unwrap_or_else(|| panic!("page {index} declares no patterns"))
        .get(name)
        .unwrap_or_else(|| panic!("page {index} has no pattern {name}"))
        .as_reference();
    pdf.resolve(r)
}

pub fn producer(pdf: &Pdf) -> String {
    let info = pdf.trailer_get("Info").unwrap().as_reference();
    match pdf.resolve(info).get("Producer").unwrap() {
        Value::Str(s) => String::from_utf8(s.clone()).unwrap(),
        other => panic!("Producer must be a string, got {other:?}"),
    }
}

pub fn array(value: &Value) -> &[Value] {
    match value {
        Value::Array(items) => items,
        other => panic!("expected an array, got {other:?}"),
    }
}

pub fn number(value: &Value) -> f64 {
    match value {
        Value::Int(i) => *i as f64,
        Value::Real(r) => *r,
        other => panic!("expected a number, got {other:?}"),
    }
}

/// The reference of the named font resource of page `index`.
pub fn font_ref(pdf: &Pdf, index: usize, name: &str) -> u32 {
    resources(pdf, index)
        .get("Font")
        .unwrap_or_else(|| panic!("page {index} declares no fonts"))
        .get(name)
        .unwrap_or_else(|| panic!("page {index} has no font {name}"))
        .as_reference()
}

/// The named font dictionary of page `index`.
pub fn font<'a>(pdf: &'a Pdf, index: usize, name: &str) -> &'a Value {
    pdf.resolve(font_ref(pdf, index, name))
}
