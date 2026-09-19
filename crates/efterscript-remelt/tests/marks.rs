// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The document objects marks become: outlines with the count rule,
//! named destinations, link annotations, information entries, the
//! catalog's viewer entries, page attributes — checked on hand-built
//! pages and marks through the structural reader — and the byte identity
//! of a document without marks.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use efterscript_graphics::{
    Annot, Collected, DocMark, Graphics, IrOp, LinkTarget, Op, Page, PageAttrs, PageSink, Target,
    View,
};
use efterscript_remelt::{Options, PdfSink};
use efterscript_vm::{Bounds, Config, Interp, Io, Matrix, Outcome, Point, Seg, SliceSource};
use support::{Pdf, Value, array, check, kids, number, page, producer};

const LETTER: Bounds = Bounds::new(0.0, 0.0, 612.0, 792.0);

fn uncompressed() -> Options {
    Options::compress(false)
}

fn blank() -> Page {
    Page::new(LETTER)
}

fn stroked_line() -> Page {
    let mut page = blank();
    page.ops = vec![
        Op::from(IrOp::LineWidth(2.0)),
        Op::from(IrOp::Stroke {
            path: vec![
                Seg::Move(Point::new(10.0, 10.0)),
                Seg::Line(Point::new(100.0, 10.0)),
            ],
            ctm: Matrix::IDENTITY,
        }),
    ];
    page
}

fn distil(collected: &Collected) -> Vec<u8> {
    let mut sink = PdfSink::new(Vec::new(), uncompressed()).unwrap();
    collected.replay(&mut sink);
    sink.finish().unwrap()
}

fn catalog(pdf: &Pdf) -> &Value {
    pdf.resolve(pdf.trailer_get("Root").unwrap().as_reference())
}

fn info(pdf: &Pdf) -> &Value {
    pdf.resolve(pdf.trailer_get("Info").unwrap().as_reference())
}

fn text(value: &Value) -> String {
    match value {
        Value::Str(s) => String::from_utf8_lossy(s).into_owned(),
        other => panic!("expected a string, got {other:?}"),
    }
}

fn name(value: &Value) -> String {
    String::from_utf8_lossy(value.as_name()).into_owned()
}

fn to_page(index: usize, view: View) -> Option<Target> {
    Some(Target::Page { index, view })
}

fn outline(title: &str, count: i32, target: Option<Target>) -> DocMark {
    DocMark::Outline {
        title: title.as_bytes().to_vec(),
        count,
        target,
    }
}

/// A destination array as (page id, fit name, remaining numbers or null).
fn dest(value: &Value) -> (u32, String, Vec<Option<f64>>) {
    let items = array(value);
    let rest = items[2..]
        .iter()
        .map(|v| match v {
            Value::Null => None,
            other => Some(number(other)),
        })
        .collect();
    (items[0].as_reference(), name(&items[1]), rest)
}

#[test]
fn outlines_follow_the_count_rule_with_a_closed_branch() {
    let mut collected = Collected::default();
    collected.document(outline("Chapter", 2, to_page(1, View::Fit)));
    collected.document(outline("First", 0, to_page(1, View::Fit)));
    collected.document(outline("Second", -1, Some(Target::Named(b"top".to_vec()))));
    collected.document(outline(
        "Hidden",
        0,
        to_page(
            2,
            View::Xyz {
                left: None,
                top: Some(700.0),
                zoom: None,
            },
        ),
    ));
    collected.document(outline("Elsewhere", 0, None));
    collected.page(blank());
    collected.page(blank());
    let pdf = check(&distil(&collected));
    let pages = kids(&pdf);
    let root = pdf.resolve(catalog(&pdf).get("Outlines").unwrap().as_reference());
    assert_eq!(root.get("Type").unwrap().as_name(), b"Outlines");
    assert_eq!(
        root.get("Count").unwrap().as_int(),
        4,
        "Hidden is closed away"
    );
    let chapter_id = root.get("First").unwrap().as_reference();
    let elsewhere_id = root.get("Last").unwrap().as_reference();
    let chapter = pdf.resolve(chapter_id);
    assert_eq!(text(chapter.get("Title").unwrap()), "Chapter");
    assert_eq!(chapter.get("Count").unwrap().as_int(), 2);
    assert_eq!(chapter.get("Next").unwrap().as_reference(), elsewhere_id);
    assert!(chapter.get("Prev").is_none());
    assert_eq!(
        dest(chapter.get("Dest").unwrap()),
        (pages[0], "Fit".to_string(), vec![])
    );
    let first_id = chapter.get("First").unwrap().as_reference();
    let second_id = chapter.get("Last").unwrap().as_reference();
    let first = pdf.resolve(first_id);
    assert_eq!(first.get("Parent").unwrap().as_reference(), chapter_id);
    assert_eq!(first.get("Next").unwrap().as_reference(), second_id);
    assert!(first.get("Count").is_none());
    assert!(first.get("First").is_none());
    let second = pdf.resolve(second_id);
    assert_eq!(second.get("Prev").unwrap().as_reference(), first_id);
    assert!(second.get("Next").is_none());
    assert_eq!(second.get("Count").unwrap().as_int(), -1, "closed");
    assert_eq!(second.get("Dest").unwrap().as_name(), b"top");
    let hidden = pdf.resolve(second.get("First").unwrap().as_reference());
    assert_eq!(text(hidden.get("Title").unwrap()), "Hidden");
    assert_eq!(hidden.get("Parent").unwrap().as_reference(), second_id);
    assert_eq!(
        dest(hidden.get("Dest").unwrap()),
        (pages[1], "XYZ".to_string(), vec![None, Some(700.0), None])
    );
    let elsewhere = pdf.resolve(elsewhere_id);
    assert_eq!(elsewhere.get("Prev").unwrap().as_reference(), chapter_id);
    assert!(
        elsewhere.get("Dest").is_none(),
        "an action outside the file"
    );
    assert!(catalog(&pdf).get("Dests").is_none());
}

#[test]
fn a_short_document_ends_a_branch_early_and_counts_what_it_has() {
    let mut collected = Collected::default();
    collected.document(outline("Part", 3, to_page(1, View::Fit)));
    collected.document(outline("Only", 0, to_page(1, View::Fit)));
    collected.page(blank());
    let pdf = check(&distil(&collected));
    let root = pdf.resolve(catalog(&pdf).get("Outlines").unwrap().as_reference());
    assert_eq!(root.get("Count").unwrap().as_int(), 2);
    let part = pdf.resolve(root.get("First").unwrap().as_reference());
    assert_eq!(part.get("Count").unwrap().as_int(), 1);
    assert_eq!(
        part.get("First").unwrap().as_reference(),
        part.get("Last").unwrap().as_reference()
    );
}

#[test]
fn named_destinations_go_into_the_catalog_and_skip_missing_pages() {
    let mut collected = Collected::default();
    collected.document(DocMark::Dest {
        name: b"top".to_vec(),
        page: 1,
        view: View::FitH(Some(700.0)),
    });
    collected.document(DocMark::Dest {
        name: b"gone".to_vec(),
        page: 5,
        view: View::Fit,
    });
    collected.document(DocMark::Dest {
        name: b"top".to_vec(),
        page: 2,
        view: View::FitH(None),
    });
    collected.page(blank());
    collected.page(blank());
    let pdf = check(&distil(&collected));
    let pages = kids(&pdf);
    let dests = pdf.resolve(catalog(&pdf).get("Dests").unwrap().as_reference());
    assert_eq!(
        dest(dests.get("top").unwrap()),
        (pages[1], "FitH".to_string(), vec![None]),
        "the later definition wins"
    );
    assert!(dests.get("gone").is_none());
    assert!(catalog(&pdf).get("Outlines").is_none());
}

#[test]
fn link_annotations_are_written_with_their_page_and_target() {
    let mut first = blank();
    first.annots = vec![
        Annot {
            rect: Bounds::new(20.0, 20.0, 100.0, 40.0),
            target: LinkTarget::Named(b"top".to_vec()),
            border: None,
            color: None,
            contents: None,
        },
        Annot {
            rect: Bounds::new(0.0, 0.0, 5.0, 5.0),
            target: LinkTarget::Uri(b"https://example.org/".to_vec()),
            border: Some([0.0, 0.0, 1.0]),
            color: Some(vec![0.0, 0.0, 1.0]),
            contents: Some(b"site".to_vec()),
        },
        Annot {
            rect: Bounds::new(0.0, 0.0, 5.0, 5.0),
            target: LinkTarget::Page {
                index: 2,
                view: View::Fit,
            },
            border: None,
            color: None,
            contents: None,
        },
        Annot {
            rect: Bounds::new(0.0, 0.0, 5.0, 5.0),
            target: LinkTarget::Page {
                index: 9,
                view: View::Fit,
            },
            border: None,
            color: None,
            contents: None,
        },
    ];
    let mut collected = Collected::default();
    collected.page(first);
    collected.page(blank());
    let pdf = check(&distil(&collected));
    let pages = kids(&pdf);
    let annots: Vec<&Value> = array(page(&pdf, 0).get("Annots").unwrap())
        .iter()
        .map(|r| pdf.resolve(r.as_reference()))
        .collect();
    assert_eq!(annots.len(), 4);
    assert!(page(&pdf, 1).get("Annots").is_none());
    for a in &annots {
        assert_eq!(a.get("Type").unwrap().as_name(), b"Annot");
        assert_eq!(a.get("Subtype").unwrap().as_name(), b"Link");
    }
    let rect: Vec<f64> = array(annots[0].get("Rect").unwrap())
        .iter()
        .map(number)
        .collect();
    assert_eq!(rect, [20.0, 20.0, 100.0, 40.0]);
    let border: Vec<f64> = array(annots[0].get("Border").unwrap())
        .iter()
        .map(number)
        .collect();
    assert_eq!(
        border,
        [0.0, 0.0, 0.0],
        "no border unless the mark gave one"
    );
    assert!(annots[0].get("C").is_none());
    assert_eq!(annots[0].get("Dest").unwrap().as_name(), b"top");
    let border: Vec<f64> = array(annots[1].get("Border").unwrap())
        .iter()
        .map(number)
        .collect();
    assert_eq!(border, [0.0, 0.0, 1.0]);
    let color: Vec<f64> = array(annots[1].get("C").unwrap())
        .iter()
        .map(number)
        .collect();
    assert_eq!(color, [0.0, 0.0, 1.0]);
    let action = annots[1].get("A").unwrap();
    assert_eq!(action.get("S").unwrap().as_name(), b"URI");
    assert_eq!(text(action.get("URI").unwrap()), "https://example.org/");
    assert_eq!(text(annots[1].get("Contents").unwrap()), "site");
    assert_eq!(
        dest(annots[2].get("Dest").unwrap()),
        (pages[1], "Fit".to_string(), vec![]),
        "a link to a page written later still resolves"
    );
    assert!(annots[3].get("Dest").is_none(), "a page the document lacks");
}

#[test]
fn information_entries_merge_and_the_producer_stays_ours() {
    let mut collected = Collected::default();
    collected.document(DocMark::Info(vec![
        (b"Title".to_vec(), b"Report".to_vec()),
        (b"Author".to_vec(), b"Someone".to_vec()),
        (b"Producer".to_vec(), b"Theirs".to_vec()),
    ]));
    collected.page(blank());
    collected.document(DocMark::Info(vec![(b"Title".to_vec(), b"Final".to_vec())]));
    let pdf = check(&distil(&collected));
    assert_eq!(text(info(&pdf).get("Title").unwrap()), "Final");
    assert_eq!(text(info(&pdf).get("Author").unwrap()), "Someone");
    assert!(producer(&pdf).starts_with("EfterScript "));
    assert!(info(&pdf).get("CreationDate").is_none());
}

#[test]
fn viewer_entries_and_page_attributes_reach_the_catalog_and_pages() {
    let mut collected = Collected::default();
    collected.document(DocMark::PagesDefault(PageAttrs {
        crop_box: Some(Bounds::new(36.0, 36.0, 576.0, 756.0)),
        rotate: None,
    }));
    collected.document(DocMark::View {
        page_mode: Some(b"UseOutlines".to_vec()),
        page_layout: Some(b"OneColumn".to_vec()),
        open: to_page(
            2,
            View::Xyz {
                left: None,
                top: None,
                zoom: None,
            },
        ),
    });
    collected.document(DocMark::PageAttr {
        page: 1,
        attrs: PageAttrs {
            crop_box: None,
            rotate: Some(90),
        },
    });
    collected.page(blank());
    collected.document(DocMark::PageAttr {
        page: 2,
        attrs: PageAttrs {
            crop_box: Some(Bounds::new(0.0, 0.0, 288.0, 288.0)),
            rotate: None,
        },
    });
    collected.page(blank());
    let pdf = check(&distil(&collected));
    let pages = kids(&pdf);
    let root = catalog(&pdf);
    assert_eq!(root.get("PageMode").unwrap().as_name(), b"UseOutlines");
    assert_eq!(root.get("PageLayout").unwrap().as_name(), b"OneColumn");
    assert_eq!(
        dest(root.get("OpenAction").unwrap()),
        (pages[1], "XYZ".to_string(), vec![None, None, None])
    );
    let crop = |index: usize| -> Vec<f64> {
        array(page(&pdf, index).get("CropBox").unwrap())
            .iter()
            .map(number)
            .collect()
    };
    assert_eq!(crop(0), [36.0, 36.0, 576.0, 756.0], "the document default");
    assert_eq!(page(&pdf, 0).get("Rotate").unwrap().as_int(), 90);
    assert_eq!(crop(1), [0.0, 0.0, 288.0, 288.0], "the page's own");
    assert!(page(&pdf, 1).get("Rotate").is_none());
}

#[test]
fn an_open_action_by_name_is_the_name() {
    let mut collected = Collected::default();
    collected.document(DocMark::View {
        page_mode: None,
        page_layout: None,
        open: Some(Target::Named(b"start".to_vec())),
    });
    collected.page(blank());
    let pdf = check(&distil(&collected));
    assert_eq!(catalog(&pdf).get("OpenAction").unwrap().as_name(), b"start");
    assert!(catalog(&pdf).get("PageMode").is_none());
}

#[test]
fn marks_are_counted_for_the_report() {
    let mut sink = PdfSink::new(Vec::new(), uncompressed()).unwrap();
    sink.document(DocMark::Info(Vec::new()));
    sink.document(DocMark::Ignored {
        kind: b"NOSUCH".to_vec(),
    });
    sink.document(DocMark::Ignored {
        kind: b"ANN/Widget".to_vec(),
    });
    sink.document(DocMark::Ignored {
        kind: b"NOSUCH".to_vec(),
    });
    let mut page = blank();
    page.annots.push(Annot {
        rect: Bounds::new(0.0, 0.0, 1.0, 1.0),
        target: LinkTarget::Named(b"x".to_vec()),
        border: None,
        color: None,
        contents: None,
    });
    sink.page(page);
    assert_eq!(sink.marks_written(), 2);
    let ignored: Vec<(&str, usize)> = sink
        .marks_ignored()
        .iter()
        .map(|(k, n)| (k.as_str(), *n))
        .collect();
    assert_eq!(ignored, [("ANN/Widget", 1), ("NOSUCH", 2)]);
    check(&sink.finish().unwrap());
}

#[test]
fn a_document_without_marks_is_byte_identical_to_its_golden() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let program = std::fs::read(format!("{root}/corpus/unit/graphics/stroked-line.ps")).unwrap();
    let golden = std::fs::read(format!(
        "{root}/corpus/golden/pdf/graphics/stroked-line.pdf"
    ))
    .unwrap();
    let (io, _, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let collected = Rc::new(RefCell::new(Collected::default()));
    interp.set_graphics_backend(Box::new(Graphics::new(collected.clone())));
    assert_eq!(interp.run(&mut SliceSource::new(&program)), Outcome::Ok);
    let collected = collected.take();
    assert!(collected.marks.is_empty());
    let mut sink = PdfSink::new(Vec::new(), uncompressed().unversioned_producer()).unwrap();
    for comment in [
        "SPDX-FileCopyrightText: 2026 EfterScript contributors",
        "SPDX-License-Identifier: MIT",
        "GENERATED-BY: difftest --update-pdf",
    ] {
        sink.comment(comment).unwrap();
    }
    collected.replay(&mut sink);
    assert_eq!(sink.finish().unwrap(), golden);
    let by_hand = Collected {
        pages: vec![stroked_line()],
        marks: Vec::new(),
    };
    let pdf = check(&distil(&by_hand));
    assert_eq!(pdf.objects.len(), 5, "content, page, pages, catalog, info");
}

#[test]
fn distill_reports_the_marks_it_ignored() {
    let (io, _, _) = Io::capture();
    let config = Config {
        io,
        ..Default::default()
    };
    let program = b"[ /Foo 1 /NOSUCH pdfmark [ /Title (T) /DOCINFO pdfmark \
                    [ /Rect [0 0 1 1] /Dest /x /Subtype /Link /ANN pdfmark showpage";
    let (report, pdf) =
        efterscript_remelt::distill(program, config, &uncompressed(), Vec::new()).unwrap();
    assert_eq!(report.outcome, Outcome::Ok);
    assert_eq!(report.marks_written, 2);
    assert_eq!(report.marks_ignored.get("NOSUCH"), Some(&1));
    assert_eq!(report.marks_ignored.len(), 1);
    let pdf = check(&pdf);
    assert_eq!(text(info(&pdf).get("Title").unwrap()), "T");
    assert_eq!(array(page(&pdf, 0).get("Annots").unwrap()).len(), 1);
}
