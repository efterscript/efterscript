// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Spec scenarios for file structure, allocation discipline, streams, and
//! determinism, verified by walking the writer's output with the test reader.

mod common;

use efterscript_pdf::{Document, Error, Filter, PageTree, Ref, write_info};
use proptest::prelude::*;

use common::Value;

/// A document exercising every value form: two streams (one per filter),
/// forward references, and a mixed-value dictionary.
fn build_sample() -> Vec<u8> {
    let mut doc = Document::new(Vec::new()).unwrap();
    doc.comment("sample for structural tests").unwrap();

    let pages = doc.alloc(); // written last: forward reference
    let content = doc.alloc();
    doc.write_stream(
        content,
        Filter::None,
        b"0 0 1 RG 10 w 0 0 m 100 100 l S\n",
        |_| {},
    )
    .unwrap();
    let page = doc.alloc();
    doc.write_obj(page, |v| {
        v.dict(|d| {
            d.key("Type").name("Page");
            d.key("Parent").reference(pages);
            d.key("MediaBox").array(|a| {
                a.int(0).int(0).real(612.0).real(792.0);
            });
            d.key("Resources").dict(|_| {});
            d.key("Contents").reference(content);
        });
    })
    .unwrap();

    let mixed = doc.alloc();
    doc.write_obj(mixed, |v| {
        v.dict(|d| {
            d.key("Literal").string(b"abc(1)");
            d.key("Hex").string(b"\xFF\x00");
            d.key("Odd name").name("A B#/x");
            d.key("Flags").array(|a| {
                a.boolean(true).boolean(false).null().real(0.5);
            });
            d.key("Nested").dict(|d| {
                d.key("Real").real(72.09);
            });
        });
    })
    .unwrap();

    let flate = doc.alloc();
    doc.write_stream(
        flate,
        Filter::Flate,
        b"compressible? stored, but valid.",
        |_| {},
    )
    .unwrap();

    doc.write_obj(pages, |v| {
        v.dict(|d| {
            d.key("Type").name("Pages");
            d.key("Kids").array(|a| {
                a.reference(page);
            });
            d.key("Count").int(1);
        });
    })
    .unwrap();

    let catalog = doc.alloc();
    doc.write_obj(catalog, |v| {
        v.dict(|d| {
            d.key("Type").name("Catalog");
            d.key("Pages").reference(pages);
        });
    })
    .unwrap();

    let info = write_info(&mut doc, |d| {
        d.key("Title").string(b"structural sample");
    })
    .unwrap();

    doc.finish(catalog, Some(info)).unwrap()
}

#[test]
fn two_runs_same_bytes() {
    assert_eq!(build_sample(), build_sample());
}

#[test]
fn structural_self_check() {
    let bytes = build_sample();
    let pdf = common::check(&bytes);
    assert_eq!(pdf.entries.len(), 7);
    assert!(pdf.trailer_get("Info").is_some());
}

#[test]
fn values_read_back_exactly() {
    let pdf = common::check(&build_sample());
    let mixed = pdf.resolve(4);
    assert_eq!(mixed.get("Literal"), Some(&Value::Str(b"abc(1)".to_vec())));
    assert_eq!(mixed.get("Hex"), Some(&Value::Str(b"\xFF\x00".to_vec())));
    assert!(mixed.get("Odd name").is_some(), "escaped name key survives");
    assert_eq!(
        mixed.get("Flags"),
        Some(&Value::Array(vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Null,
            Value::Real(0.5),
        ]))
    );
    assert_eq!(
        mixed.get("Nested").unwrap().get("Real"),
        Some(&Value::Real(72.09))
    );
}

#[test]
fn forward_reference_finishes_and_self_checks() {
    // Page objects reference a Pages node allocated first but written last.
    let bytes = build_sample();
    let pdf = common::check(&bytes);
    let page = pdf.resolve(3);
    assert_eq!(page.get("Parent").unwrap().as_reference(), 1);
    assert_eq!(pdf.resolve(1).get("Type").unwrap().as_name(), b"Pages");
}

#[test]
fn dangling_allocation_is_an_error_naming_the_id() {
    let mut doc = Document::new(Vec::new()).unwrap();
    let catalog = doc.alloc();
    doc.write_obj(catalog, |v| {
        v.dict(|d| {
            d.key("Type").name("Catalog");
        });
    })
    .unwrap();
    let dangling = doc.alloc();
    assert_eq!(dangling.id(), 2);
    match doc.finish(catalog, None) {
        Err(Error::UnwrittenObjects(ids)) => {
            assert_eq!(ids, [2]);
            let shown = Error::UnwrittenObjects(ids).to_string();
            assert!(shown.contains('2'), "error must name the id: {shown}");
        }
        other => panic!("expected UnwrittenObjects, got {other:?}"),
    }
}

#[test]
fn writing_an_object_twice_is_an_error() {
    let mut doc = Document::new(Vec::new()).unwrap();
    let r = doc.alloc();
    doc.write_obj(r, |v| v.int(1)).unwrap();
    match doc.write_obj(r, |v| v.int(2)) {
        Err(Error::ObjectAlreadyWritten(1)) => {}
        other => panic!("expected ObjectAlreadyWritten, got {other:?}"),
    }
}

#[test]
fn flate_stream_round_trips() {
    let pdf = common::check(&build_sample());
    let flate = pdf.resolve(5);
    assert_eq!(flate.get("Filter").unwrap().as_name(), b"FlateDecode");
    assert_eq!(
        common::inflate(flate.stream_data()),
        b"compressible? stored, but valid."
    );
    // And a stream long enough to split into two stored blocks.
    let mut doc = Document::new(Vec::new()).unwrap();
    let catalog = doc.alloc();
    doc.write_obj(catalog, |v| {
        v.dict(|d| {
            d.key("Type").name("Catalog");
        });
    })
    .unwrap();
    let big = doc.alloc();
    let long_data: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
    doc.write_stream(big, Filter::Flate, &long_data, |_| {})
        .unwrap();
    let bytes = doc.finish(catalog, None).unwrap();
    let pdf = common::check(&bytes);
    assert_eq!(common::inflate(pdf.resolve(2).stream_data()), long_data);
}

#[test]
fn stream_length_is_exact() {
    let pdf = common::check(&build_sample());
    // The reader slices data by Length and then requires `endstream`, so a
    // parsed stream already proves exactness; double-check the numbers.
    for id in [2u32, 5] {
        let stream = pdf.resolve(id);
        assert_eq!(
            stream.get("Length").unwrap().as_int() as usize,
            stream.stream_data().len()
        );
    }
}

fn catalog_only<W: std::io::Write>(mut doc: Document<W>) -> W {
    let catalog = doc.alloc();
    doc.write_obj(catalog, |v| {
        v.dict(|d| {
            d.key("Type").name("Catalog");
        });
    })
    .unwrap();
    doc.finish(catalog, None).unwrap()
}

#[test]
fn the_version_is_patched_at_finish_on_a_seekable_sink() {
    let mut doc = Document::new_seekable(std::io::Cursor::new(Vec::new())).unwrap();
    assert!(doc.version_patchable());
    doc.set_version(1, 4).unwrap();
    let bytes = catalog_only(doc).into_inner();
    assert!(bytes.starts_with(b"%PDF-1.4\n%"));
    let pdf = common::check(&bytes);
    assert_eq!(pdf.objects.len(), 1);

    // Setting it back to 1.7 leaves the header as written.
    let mut doc = Document::new_seekable(std::io::Cursor::new(Vec::new())).unwrap();
    doc.set_version(1, 4).unwrap();
    doc.set_version(1, 7).unwrap();
    assert!(catalog_only(doc).into_inner().starts_with(b"%PDF-1.7\n"));
}

#[test]
fn a_non_seekable_sink_keeps_the_written_version() {
    let mut doc = Document::new(Vec::new()).unwrap();
    assert!(!doc.version_patchable());
    doc.set_version(1, 4).unwrap();
    assert!(catalog_only(doc).starts_with(b"%PDF-1.7\n"));
}

#[test]
fn version_digits_are_checked() {
    let mut doc = Document::new_seekable(std::io::Cursor::new(Vec::new())).unwrap();
    assert!(matches!(doc.set_version(1, 10), Err(Error::InvalidVersion)));
    assert!(matches!(doc.set_version(10, 0), Err(Error::InvalidVersion)));
    doc.set_version(2, 0).unwrap();
    assert!(catalog_only(doc).into_inner().starts_with(b"%PDF-2.0\n"));
}

#[test]
fn comments_are_validated() {
    let mut doc = Document::new(Vec::new()).unwrap();
    assert!(matches!(
        doc.comment("no\nnewlines"),
        Err(Error::InvalidComment)
    ));
    doc.comment("fine").unwrap();
}

proptest! {
    #[test]
    fn arbitrary_bytes_survive_flate_streams(data in proptest::collection::vec(any::<u8>(), 0..2000)) {
        let mut doc = Document::new(Vec::new()).unwrap();
        let catalog = doc.alloc();
        doc.write_obj(catalog, |v| {
            v.dict(|d| {
                d.key("Type").name("Catalog");
            });
        }).unwrap();
        let r = doc.alloc();
        doc.write_stream(r, Filter::Flate, &data, |_| {}).unwrap();
        let bytes = doc.finish(catalog, None).unwrap();
        let pdf = common::check(&bytes);
        prop_assert_eq!(common::inflate(pdf.resolve(2).stream_data()), data);
    }

    #[test]
    fn arbitrary_strings_and_names_read_back(
        s in proptest::collection::vec(any::<u8>(), 0..64),
        // NUL is excluded: no PDF name can carry it, and the writer refuses
        // one (covered by `nul_in_names_is_rejected`).
        n in proptest::collection::vec(1u8..=255, 0..32),
    ) {
        let mut doc = Document::new(Vec::new()).unwrap();
        let catalog = doc.alloc();
        doc.write_obj(catalog, |v| {
            v.dict(|d| {
                d.key("Type").name("Catalog");
            });
        }).unwrap();
        let r = doc.alloc();
        let (s2, n2) = (s.clone(), n.clone());
        doc.write_obj(r, |v| {
            v.dict(|d| {
                d.key("S").string(&s2);
                d.key("N").name_bytes(&n2);
            });
        }).unwrap();
        let bytes = doc.finish(catalog, None).unwrap();
        let pdf = common::check(&bytes);
        let obj = pdf.resolve(2);
        prop_assert_eq!(obj.get("S"), Some(&Value::Str(s)));
        prop_assert_eq!(obj.get("N"), Some(&Value::Name(n)));
    }

    #[test]
    fn documents_are_deterministic(pages in 1usize..4, content in proptest::collection::vec(any::<u8>(), 0..256)) {
        let build = || {
            let mut doc = Document::new(Vec::new()).unwrap();
            let mut tree = PageTree::new(&mut doc);
            for _ in 0..pages {
                tree.add_page(&mut doc, [0.0, 0.0, 612.0, 792.0], Filter::None, &content, |_| {}).unwrap();
            }
            let root = tree.finish(&mut doc).unwrap();
            doc.finish(root, None).unwrap()
        };
        prop_assert_eq!(build(), build());
    }
}

// Keep `Ref` in the public surface under test.
#[allow(dead_code)]
fn takes_ref(r: Ref) -> u32 {
    r.id()
}

#[test]
fn nul_in_names_is_rejected() {
    let mut out = Vec::new();
    let mut doc = efterscript_pdf::Document::new(&mut out).expect("header");
    let r = doc.alloc();
    assert!(matches!(
        doc.write_obj(r, |v| v.name_bytes(b"bad\0name")),
        Err(efterscript_pdf::Error::NulInName)
    ));
    let r2 = doc.alloc();
    assert!(matches!(
        doc.write_obj(r2, |v| v.dict(|d| d.key_bytes(b"\0").null())),
        Err(efterscript_pdf::Error::NulInName)
    ));
    let r3 = doc.alloc();
    assert!(matches!(
        doc.write_stream(r3, efterscript_pdf::Filter::None, b"x", |d| {
            d.key_bytes(b"a\0b").boolean(true);
        }),
        Err(efterscript_pdf::Error::NulInName)
    ));
}
