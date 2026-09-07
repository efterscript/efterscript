// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Marks through the backend and through the interpreter: where a link
//! lands, when a document mark reaches the sink and with which page,
//! what is tolerated, and how the dump shows it. The interpreter
//! scenarios each have a corpus file under `corpus/unit/pdfmark/`.

use std::cell::RefCell;
use std::rc::Rc;

use ps_graphics::{
    Annot, Collected, DocMark, Graphics, IrOp, LinkTarget, PageAttrs, Target, View, dump,
};
use ps_vm::{
    Bounds, Config, GraphicsBackend, Interp, Io, MarkValue, Matrix, Outcome, Point, Seg,
    SliceSource,
};

type Sink = Rc<RefCell<Collected>>;

fn backend() -> (Graphics<Sink>, Sink) {
    let sink: Sink = Rc::new(RefCell::new(Collected::default()));
    (Graphics::new(sink.clone()), sink)
}

fn name(text: &str) -> MarkValue {
    MarkValue::Name(text.as_bytes().to_vec())
}

fn string(text: &str) -> MarkValue {
    MarkValue::String(text.as_bytes().to_vec())
}

fn numbers(values: &[f32]) -> MarkValue {
    MarkValue::Array(values.iter().map(|v| MarkValue::Real(*v)).collect())
}

fn link(rect: &[f32], extra: Vec<MarkValue>) -> Vec<MarkValue> {
    let mut entries = vec![name("Rect"), numbers(rect), name("Subtype"), name("Link")];
    entries.extend(extra);
    entries
}

struct Run {
    outcome: Outcome,
    collected: Collected,
}

fn exec(program: &str) -> Run {
    let (io, _, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let sink: Sink = Rc::new(RefCell::new(Collected::default()));
    interp.set_graphics_backend(Box::new(Graphics::new(sink.clone())));
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    Run {
        outcome,
        collected: sink.take(),
    }
}

fn marks(run: &Run) -> Vec<DocMark> {
    run.collected.marks().cloned().collect()
}

// --- the backend ---------------------------------------------------------------

#[test]
fn a_link_lands_on_the_current_page_with_its_rectangle_through_the_ctm() {
    let (mut g, sink) = backend();
    g.concat(Matrix::scaling(2.0, 2.0)).unwrap();
    g.pdfmark(
        b"ANN",
        &link(&[50.0, 20.0, 10.0, 10.0], vec![name("Dest"), name("top")]),
    )
    .unwrap();
    assert_eq!(
        g.annots(),
        [Annot {
            rect: Bounds::new(20.0, 20.0, 100.0, 40.0),
            target: LinkTarget::Named(b"top".to_vec()),
            border: None,
            color: None,
            contents: None,
        }]
    );
    assert!(g.ignored_marks().is_empty());
    g.showpage().unwrap();
    assert!(g.annots().is_empty());
    let collected = sink.borrow();
    assert_eq!(collected.pages.len(), 1);
    assert_eq!(collected.pages[0].annots.len(), 1);
    assert!(collected.marks.is_empty());
}

#[test]
fn a_link_for_a_later_page_waits_for_it_and_an_earlier_one_is_dropped() {
    let (mut g, sink) = backend();
    g.pdfmark(
        b"ANN",
        &link(
            &[0.0, 0.0, 1.0, 1.0],
            vec![name("SrcPg"), MarkValue::Int(2)],
        ),
    )
    .unwrap();
    assert!(g.annots().is_empty());
    g.showpage().unwrap();
    assert_eq!(g.current_page(), 2);
    assert_eq!(g.annots().len(), 0, "attached at delivery, not before");
    g.pdfmark(
        b"ANN",
        &link(
            &[0.0, 0.0, 1.0, 1.0],
            vec![name("SrcPg"), MarkValue::Int(1)],
        ),
    )
    .unwrap();
    assert_eq!(g.ignored_marks().get(&b"ANN"[..]), Some(&1));
    g.showpage().unwrap();
    let collected = sink.borrow();
    assert_eq!(collected.pages[0].annots.len(), 0);
    assert_eq!(collected.pages[1].annots.len(), 1);
    assert_eq!(
        collected.marks().collect::<Vec<_>>(),
        [&DocMark::Ignored {
            kind: b"ANN".to_vec()
        }]
    );
}

#[test]
fn document_marks_reach_the_sink_at_once_with_the_page_resolved() {
    let (mut g, sink) = backend();
    g.pdfmark(b"OUT", &[name("Title"), string("One")]).unwrap();
    assert_eq!(
        sink.borrow().marks,
        [(
            0,
            DocMark::Outline {
                title: b"One".to_vec(),
                count: 0,
                target: Some(Target::Page {
                    index: 1,
                    view: View::Fit
                }),
            }
        )]
    );
    g.showpage().unwrap();
    g.copypage().unwrap();
    assert_eq!(g.current_page(), 3, "a copied page counts as delivered");
    g.pdfmark(b"DEST", &[name("Dest"), name("here")]).unwrap();
    g.pdfmark(b"PAGE", &[name("Rotate"), MarkValue::Int(90)])
        .unwrap();
    g.showpage().unwrap();
    g.pdfmark(b"DOCINFO", &[name("Title"), string("T")])
        .unwrap();
    let collected = sink.borrow();
    assert_eq!(collected.pages.len(), 3);
    let positions: Vec<usize> = collected.marks.iter().map(|(at, _)| *at).collect();
    assert_eq!(positions, [0, 2, 2, 3]);
    assert_eq!(
        collected.marks[1].1,
        DocMark::Dest {
            name: b"here".to_vec(),
            page: 3,
            view: View::Fit
        }
    );
    assert_eq!(
        collected.marks[2].1,
        DocMark::PageAttr {
            page: 3,
            attrs: PageAttrs {
                crop_box: None,
                rotate: Some(90)
            }
        }
    );
}

#[test]
fn unknown_kinds_and_subtypes_are_counted_and_reported() {
    let (mut g, sink) = backend();
    g.pdfmark(b"NOSUCH", &[name("Foo"), MarkValue::Int(1)])
        .unwrap();
    g.pdfmark(b"NOSUCH", &[]).unwrap();
    g.pdfmark(b"ANN", &[name("Rect"), numbers(&[0.0, 0.0, 1.0, 1.0])])
        .unwrap();
    g.pdfmark(b"LNK", &[]).unwrap();
    let ignored: Vec<(&[u8], usize)> = g
        .ignored_marks()
        .iter()
        .map(|(k, n)| (k.as_slice(), *n))
        .collect();
    assert_eq!(
        ignored,
        [(&b"ANN/Text"[..], 1), (&b"LNK"[..], 1), (&b"NOSUCH"[..], 2)]
    );
    assert_eq!(sink.borrow().marks.len(), 4);
    assert!(g.annots().is_empty());
}

#[test]
fn erasepage_keeps_annotations_and_the_null_device_still_resolves_pages() {
    let (mut g, sink) = backend();
    g.pdfmark(b"ANN", &link(&[0.0, 0.0, 1.0, 1.0], Vec::new()))
        .unwrap();
    g.erasepage().unwrap();
    assert_eq!(g.annots().len(), 1);
    g.gsave().unwrap();
    g.nulldevice().unwrap();
    g.showpage().unwrap();
    assert_eq!(g.current_page(), 1, "nothing was delivered");
    g.grestore().unwrap();
    g.showpage().unwrap();
    assert_eq!(sink.borrow().pages[0].annots.len(), 1);
}

// --- the dump -------------------------------------------------------------------

#[test]
fn the_dump_lists_annotations_after_the_ops_and_marks_in_a_doc_section() {
    let run = exec(
        "[ /Title (One) /Count 0 /OUT pdfmark \
         0 0 10 10 rectfill \
         [ /Rect [10 10 50 20] /Border [0 0 1] /Color [1 0 0] /Dest /top /Subtype /Link /ANN pdfmark \
         [ /Rect [0 0 5 5] /Subtype /Link /Action << /Subtype /URI /URI (https://example.org/) >> /ANN pdfmark \
         [ /Rect [0 0 5 5] /Subtype /Link /Page 2 /View [/XYZ null 700 0] /Contents (next) /ANN pdfmark \
         [ /Dest /top /View [/FitH 700] /DEST pdfmark \
         [ /Title (Report) /Author (Someone) /DOCINFO pdfmark \
         [ /PageMode /UseOutlines /Page 1 /DOCVIEW pdfmark \
         [ /CropBox [0 0 288 288] /PAGES pdfmark \
         [ /Rotate 90 /PAGE pdfmark \
         [ /Foo 1 /NOSUCH pdfmark \
         showpage",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let text = run.collected.dump();
    assert_eq!(
        text,
        "ir/1\n\
         page 612 792\n\
         resources:\n\
         ops:\n\
         m 0 0\n\
         l 10 0\n\
         l 10 10\n\
         l 0 10\n\
         h\n\
         f\n\
         annot link 10 10 50 20 dest=/top border=0 0 1 color=1 0 0\n\
         annot link 0 0 5 5 uri=(https://example.org/)\n\
         annot link 0 0 5 5 page=2 xyz null 700 null contents=(next)\n\
         \n\
         doc:\n\
         out 0 (One) -> page=1 fit\n\
         dest /top -> page 1 fith 700\n\
         info /Title (Report)\n\
         info /Author (Someone)\n\
         view mode=/UseOutlines open=page=1 fit\n\
         pages cropbox 0 0 288 288\n\
         page 1 rotate 90\n\
         ignored /NOSUCH\n"
    );
    assert_eq!(
        dump::pages(&run.collected.pages),
        text.split("\n\ndoc:").next().unwrap().to_owned() + "\n"
    );
}

#[test]
fn a_page_without_marks_dumps_as_before() {
    let run = exec("0 0 10 10 rectfill showpage");
    assert_eq!(run.collected.dump(), dump::pages(&run.collected.pages));
    assert!(!run.collected.dump().contains("doc:"));
}

// --- the corpus scenarios ---------------------------------------------------------

// pdfmark/nested-bookmarks.ps
#[test]
fn nested_bookmarks_hold_three_entries_on_page_one() {
    let run = exec(
        "[ /Title (Chapter) /Count 2 /OUT pdfmark \
         [ /Title (First) /Count 0 /OUT pdfmark \
         [ /Title (Second) /Count 0 /OUT pdfmark \
         showpage",
    );
    let to_page_one = Some(Target::Page {
        index: 1,
        view: View::Fit,
    });
    assert_eq!(
        marks(&run),
        [
            DocMark::Outline {
                title: b"Chapter".to_vec(),
                count: 2,
                target: to_page_one.clone(),
            },
            DocMark::Outline {
                title: b"First".to_vec(),
                count: 0,
                target: to_page_one.clone(),
            },
            DocMark::Outline {
                title: b"Second".to_vec(),
                count: 0,
                target: to_page_one,
            },
        ]
    );
    assert!(run.collected.marks.iter().all(|(at, _)| *at == 0));
}

// pdfmark/cross-page-link.ps
#[test]
fn a_link_by_name_across_pages_is_scaled_and_resolves_to_page_one() {
    let run = exec(
        "[ /Dest /top /DEST pdfmark showpage \
         2 2 scale \
         [ /Rect [10 10 50 20] /Dest /top /Subtype /Link /ANN pdfmark showpage",
    );
    assert_eq!(run.collected.pages.len(), 2);
    assert!(run.collected.pages[0].annots.is_empty());
    assert_eq!(
        run.collected.pages[1].annots,
        [Annot {
            rect: Bounds::new(20.0, 20.0, 100.0, 40.0),
            target: LinkTarget::Named(b"top".to_vec()),
            border: None,
            color: None,
            contents: None,
        }]
    );
    assert_eq!(
        marks(&run),
        [DocMark::Dest {
            name: b"top".to_vec(),
            page: 1,
            view: View::Fit,
        }]
    );
}

// pdfmark/document-info.ps
#[test]
fn document_information_holds_both_entries() {
    let run = exec("[ /Title (Report) /Author (Someone) /DOCINFO pdfmark showpage");
    assert_eq!(
        marks(&run),
        [DocMark::Info(vec![
            (b"Title".to_vec(), b"Report".to_vec()),
            (b"Author".to_vec(), b"Someone".to_vec()),
        ])]
    );
}

// pdfmark/unknown-kind.ps
#[test]
fn an_unknown_kind_raises_nothing_and_is_counted() {
    let run = exec("[ /Foo 1 /NOSUCH pdfmark showpage");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        marks(&run),
        [DocMark::Ignored {
            kind: b"NOSUCH".to_vec()
        }]
    );
}

// pdfmark/guarded-idiom.ps
#[test]
fn the_guarded_idiom_honours_the_mark_with_a_backend() {
    let run = exec(
        "/pdfmark where { pop } { userdict /pdfmark /cleartomark load put } ifelse \
         [ /Title (Guarded) /DOCINFO pdfmark showpage",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(marks(&run).len(), 1);
}

// graphics/moveto-replaces-moveto.ps
#[test]
fn a_second_moveto_replaces_the_first() {
    let run = exec("10 10 moveto 20 20 moveto 30 30 lineto stroke showpage");
    let ops: Vec<IrOp> = run.collected.pages[0]
        .ops
        .iter()
        .map(|o| o.op.clone())
        .collect();
    assert_eq!(
        ops,
        [IrOp::Stroke {
            path: vec![
                Seg::Move(Point::new(20.0, 20.0)),
                Seg::Line(Point::new(30.0, 30.0)),
            ],
            ctm: Matrix::IDENTITY,
        }]
    );
}
