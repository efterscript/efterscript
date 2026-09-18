// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Test-side helpers over the PDF writer's own structural reader, which
//! lives with `efterscript-pdf`'s tests and is included from there so the job's
//! documents are read with the same reader as the engine's.

#![allow(dead_code)] // Each integration-test target uses a subset.

#[path = "../../../efterscript-pdf/tests/common/mod.rs"]
pub mod reader;

pub use reader::{Pdf, Value, check, inflate};

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

/// A stream's data after undoing its filter.
pub fn decoded(stream: &Value) -> Vec<u8> {
    let data = stream.stream_data();
    match stream.get("Filter") {
        None => data.to_vec(),
        Some(Value::Name(name)) if name == b"FlateDecode" => inflate(data),
        Some(other) => panic!("unexpected filter {other:?}"),
    }
}

/// The content stream of page `index` as text.
pub fn content(pdf: &Pdf, index: usize) -> String {
    let contents = page(pdf, index).get("Contents").unwrap().as_reference();
    String::from_utf8(decoded(pdf.resolve(contents))).unwrap()
}
