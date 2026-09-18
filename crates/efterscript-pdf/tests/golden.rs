// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The one-page golden: regenerates the document and byte-compares it with
//! the committed file. The output contains no dates and no ids beyond
//! allocation order, so the bytes are stable by construction.

mod common;

use efterscript_pdf::{Document, Filter, PageTree};

const GOLDEN_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../corpus/golden/pdf/one-page.pdf"
);

const MARKER: &str = "GENERATED-BY: pdf-out golden test";

fn build_one_page() -> Vec<u8> {
    let mut doc = Document::new(Vec::new()).unwrap();
    doc.comment(MARKER).unwrap();
    let mut tree = PageTree::new(&mut doc);
    tree.add_page(
        &mut doc,
        [0.0, 0.0, 612.0, 792.0],
        Filter::None,
        b"0 0 1 RG 4 w 72 72 468 648 re S\n",
        |_| {},
    )
    .unwrap();
    let root = tree.finish(&mut doc).unwrap();
    doc.finish(root, None).unwrap()
}

#[test]
fn golden_bytes_match() {
    let committed =
        std::fs::read(GOLDEN_PATH).unwrap_or_else(|e| panic!("cannot read {GOLDEN_PATH}: {e}"));
    let regenerated = build_one_page();
    assert!(
        committed == regenerated,
        "regenerated output differs from the committed golden file"
    );
}

#[test]
fn golden_is_self_describing() {
    let bytes = std::fs::read(GOLDEN_PATH).unwrap();
    let marker_line = format!("% {MARKER}\n");
    assert!(
        bytes
            .windows(marker_line.len())
            .any(|w| w == marker_line.as_bytes()),
        "golden file must carry its provenance comment"
    );
}

#[test]
fn golden_self_checks() {
    let pdf = common::check(&build_one_page());
    assert_eq!(pdf.objects.len(), 4);
    let root = pdf.trailer_get("Root").unwrap().as_reference();
    let pages = pdf.resolve(root).get("Pages").unwrap().as_reference();
    let kids = match pdf.resolve(pages).get("Kids").unwrap() {
        common::Value::Array(kids) => kids,
        other => panic!("Kids must be an array, got {other:?}"),
    };
    assert_eq!(kids.len(), 1);
}

/// If `EFTERSCRIPT_PDF_CHECK` names a command, run it with the golden path
/// as its argument and require exit status 0. Skipped silently when unset.
/// The value is treated as a program name, not a shell command line.
#[test]
fn golden_passes_external_checker() {
    let Some(checker) = std::env::var_os("EFTERSCRIPT_PDF_CHECK") else {
        return;
    };
    let status = std::process::Command::new(&checker)
        .arg(GOLDEN_PATH)
        .status()
        .unwrap_or_else(|e| panic!("cannot run {checker:?}: {e}"));
    assert!(status.success(), "{checker:?} rejected the golden file");
}
