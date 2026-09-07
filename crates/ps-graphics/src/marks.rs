// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Turns a `pdfmark`'s kind and values into an annotation or a document
//! mark, following the pdfmark reference for the kinds honoured: `ANN`
//! (links only), `OUT`, `DEST`, `DOCINFO`, `DOCVIEW`, `PAGES`, and
//! `PAGE`. The values arrive as a flat list the operator did not pair;
//! pairing happens here, a name followed by its value, later keys
//! overriding earlier ones. Every other kind, every other annotation
//! subtype, and a mark missing what its kind requires is tolerated: it
//! is reported as ignored under its kind, never an error.

use ps_vm::{Bounds, MarkValue, Matrix, Point};

use crate::ir::{Annot, DocMark, LinkTarget, PageAttrs, Target, View};

/// What one mark amounts to.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Parsed {
    /// A link annotation for the page numbered `page`.
    Annot {
        page: usize,
        annot: Annot,
    },
    Doc(DocMark),
    /// Tolerated and dropped, under the key the count is kept by.
    Ignored(Vec<u8>),
}

/// The key–value view of a mark's entries.
struct Pairs<'a>(Vec<(&'a [u8], &'a MarkValue)>);

impl<'a> Pairs<'a> {
    fn of(entries: &'a [MarkValue]) -> Self {
        let mut pairs = Vec::new();
        let mut at = 0;
        while at + 1 < entries.len() {
            match entries[at].as_name() {
                Some(key) => {
                    pairs.push((key, &entries[at + 1]));
                    at += 2;
                }
                None => at += 1,
            }
        }
        Pairs(pairs)
    }

    /// The value of the last entry with `key`.
    fn get(&self, key: &str) -> Option<&'a MarkValue> {
        self.0
            .iter()
            .rev()
            .find(|(k, _)| *k == key.as_bytes())
            .map(|(_, v)| *v)
    }

    fn text(&self, key: &str) -> Option<&'a [u8]> {
        self.get(key).and_then(MarkValue::as_text)
    }

    fn name(&self, key: &str) -> Option<&'a [u8]> {
        self.get(key).and_then(MarkValue::as_name)
    }

    fn int(&self, key: &str) -> Option<i32> {
        self.get(key).and_then(MarkValue::as_int)
    }

    fn numbers(&self, key: &str) -> Option<Vec<f32>> {
        self.get(key)?
            .as_array()?
            .iter()
            .map(MarkValue::as_number)
            .collect()
    }

    /// The page a mark's `key` names, or the current page without it:
    /// a positive number is a page, `Next` and `Prev` are relative to
    /// the current page, and zero is the null destination.
    fn page(&self, key: &str, current: usize) -> Option<usize> {
        match self.get(key) {
            None => Some(current),
            Some(MarkValue::Int(n)) => usize::try_from(*n).ok().filter(|n| *n > 0),
            Some(MarkValue::Name(n)) if n == b"Next" => Some(current + 1),
            Some(MarkValue::Name(n)) if n == b"Prev" => Some(current.saturating_sub(1).max(1)),
            Some(_) => None,
        }
    }

    /// The `View` array as a view; anything the IR does not carry
    /// degrades to fitting the page.
    fn view(&self) -> View {
        let Some(items) = self.get("View").and_then(MarkValue::as_array) else {
            return View::Fit;
        };
        let component = |i: usize| items.get(i).and_then(MarkValue::as_number);
        match items.first().and_then(MarkValue::as_name) {
            Some(b"FitH") => View::FitH(component(1)),
            Some(b"XYZ") => View::Xyz {
                left: component(1),
                top: component(2),
                zoom: component(3).filter(|z| *z != 0.0),
            },
            _ => View::Fit,
        }
    }

    /// The destination of a bookmark or the open action: a name, or a
    /// page with a view; `None` for the null destination and for actions
    /// that lead outside the document.
    fn target(&self, current: usize) -> Option<Target> {
        if let Some(name) = self.text("Dest") {
            return Some(Target::Named(name.to_vec()));
        }
        match self.get("Action") {
            None => {}
            Some(MarkValue::Name(action)) if action == b"GoTo" => {}
            Some(_) => return None,
        }
        let index = self.page("Page", current)?;
        Some(Target::Page {
            index,
            view: self.view(),
        })
    }

    fn bounds(&self, key: &str) -> Option<Bounds> {
        let n = self.numbers(key)?;
        if n.len() != 4 {
            return None;
        }
        Some(normalised(Bounds::new(n[0], n[1], n[2], n[3])))
    }

    fn attrs(&self) -> PageAttrs {
        PageAttrs {
            crop_box: self.bounds("CropBox"),
            rotate: self.int("Rotate").filter(|r| r % 90 == 0),
        }
    }
}

fn normalised(b: Bounds) -> Bounds {
    Bounds::new(
        b.llx.min(b.urx),
        b.lly.min(b.ury),
        b.llx.max(b.urx),
        b.lly.max(b.ury),
    )
}

/// The rectangle's corners through `ctm`, boxed and normalised.
fn rect_through(ctm: Matrix, rect: Bounds) -> Bounds {
    let corners = [
        Point::new(rect.llx, rect.lly),
        Point::new(rect.urx, rect.lly),
        Point::new(rect.urx, rect.ury),
        Point::new(rect.llx, rect.ury),
    ]
    .map(|p| ctm.apply(p));
    let mut out = Bounds::new(
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for p in corners {
        out.llx = out.llx.min(p.x);
        out.lly = out.lly.min(p.y);
        out.urx = out.urx.max(p.x);
        out.ury = out.ury.max(p.y);
    }
    out
}

/// A link's target: a named destination, a URI action, or a page. The
/// action may be a dictionary naming the URI subtype, or the `Launch`
/// name with a `URI` entry beside it.
fn link_target(pairs: &Pairs<'_>, current: usize) -> LinkTarget {
    if let Some(name) = pairs.text("Dest") {
        return LinkTarget::Named(name.to_vec());
    }
    match pairs.get("Action") {
        Some(MarkValue::Dict(entries)) => {
            let entry = |key: &[u8]| entries.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v);
            let subtype = entry(b"S")
                .or_else(|| entry(b"Subtype"))
                .and_then(MarkValue::as_name);
            if subtype == Some(b"URI")
                && let Some(uri) = entry(b"URI").and_then(MarkValue::as_text)
            {
                return LinkTarget::Uri(uri.to_vec());
            }
        }
        Some(MarkValue::Name(action)) if action == b"Launch" => {
            if let Some(uri) = pairs.text("URI") {
                return LinkTarget::Uri(uri.to_vec());
            }
        }
        _ => {}
    }
    LinkTarget::Page {
        index: pairs.page("Page", current).unwrap_or(current),
        view: pairs.view(),
    }
}

/// Interprets a mark of `kind` made under `ctm` while page `current` is
/// under construction.
pub(crate) fn parse(kind: &[u8], entries: &[MarkValue], ctm: Matrix, current: usize) -> Parsed {
    let pairs = Pairs::of(entries);
    let ignored = || Parsed::Ignored(kind.to_vec());
    match kind {
        b"ANN" => {
            let subtype = pairs.name("Subtype").unwrap_or(b"Text");
            if subtype != b"Link" {
                let mut key = b"ANN/".to_vec();
                key.extend_from_slice(subtype);
                return Parsed::Ignored(key);
            }
            let Some(rect) = pairs.bounds("Rect") else {
                return ignored();
            };
            let page = match pairs.get("SrcPg") {
                None => current,
                Some(value) => match value.as_int().and_then(|n| usize::try_from(n).ok()) {
                    Some(n) if n > 0 => n,
                    _ => return ignored(),
                },
            };
            let border = pairs.numbers("Border").filter(|b| b.len() >= 3);
            let color = pairs
                .numbers("Color")
                .or_else(|| pairs.numbers("C"))
                .filter(|c| c.len() == 3);
            Parsed::Annot {
                page,
                annot: Annot {
                    rect: rect_through(ctm, rect),
                    target: link_target(&pairs, current),
                    border: border.map(|b| [b[0], b[1], b[2]]),
                    color,
                    contents: pairs.text("Contents").map(<[u8]>::to_vec),
                },
            }
        }
        b"OUT" => {
            let Some(title) = pairs.text("Title") else {
                return ignored();
            };
            Parsed::Doc(DocMark::Outline {
                title: title.to_vec(),
                count: pairs.int("Count").unwrap_or(0),
                target: pairs.target(current),
            })
        }
        b"DEST" => {
            let (Some(name), Some(page)) = (pairs.text("Dest"), pairs.page("Page", current)) else {
                return ignored();
            };
            Parsed::Doc(DocMark::Dest {
                name: name.to_vec(),
                page,
                view: pairs.view(),
            })
        }
        b"DOCINFO" => Parsed::Doc(DocMark::Info(
            pairs
                .0
                .iter()
                .filter_map(|(key, value)| Some((key.to_vec(), value.as_string()?.to_vec())))
                .collect(),
        )),
        b"DOCVIEW" => {
            let open = if pairs.get("Dest").is_some() || pairs.get("Page").is_some() {
                pairs.target(current)
            } else {
                None
            };
            Parsed::Doc(DocMark::View {
                page_mode: pairs.name("PageMode").map(<[u8]>::to_vec),
                page_layout: pairs.name("PageLayout").map(<[u8]>::to_vec),
                open,
            })
        }
        b"PAGES" => Parsed::Doc(DocMark::PagesDefault(pairs.attrs())),
        b"PAGE" => Parsed::Doc(DocMark::PageAttr {
            page: current,
            attrs: pairs.attrs(),
        }),
        _ => ignored(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> MarkValue {
        MarkValue::Name(text.as_bytes().to_vec())
    }

    fn string(text: &str) -> MarkValue {
        MarkValue::String(text.as_bytes().to_vec())
    }

    fn numbers(values: &[f32]) -> MarkValue {
        MarkValue::Array(values.iter().map(|v| MarkValue::Real(*v)).collect())
    }

    fn view(items: Vec<MarkValue>) -> MarkValue {
        MarkValue::Array(items)
    }

    #[test]
    fn pairs_follow_names_and_later_keys_win() {
        let entries = [
            name("A"),
            MarkValue::Int(1),
            MarkValue::Int(9),
            name("B"),
            name("X"),
            name("A"),
            MarkValue::Int(2),
            name("C"),
        ];
        let pairs = Pairs::of(&entries);
        assert_eq!(pairs.int("A"), Some(2));
        assert_eq!(pairs.get("B"), Some(&name("X")), "a name can be a value");
        assert_eq!(pairs.get("C"), None, "a trailing key has no value");
        assert_eq!(pairs.get("X"), None, "a value is not a key");
    }

    #[test]
    fn pages_resolve_relative_to_the_current_one() {
        let absent = Pairs::of(&[]);
        assert_eq!(absent.page("Page", 3), Some(3));
        let zero = [name("Page"), MarkValue::Int(0)];
        assert_eq!(Pairs::of(&zero).page("Page", 3), None);
        let next = [name("Page"), name("Next")];
        assert_eq!(Pairs::of(&next).page("Page", 3), Some(4));
        let prev = [name("Page"), name("Prev")];
        assert_eq!(Pairs::of(&prev).page("Page", 1), Some(1));
        let real = [name("Page"), MarkValue::Real(2.0)];
        assert_eq!(Pairs::of(&real).page("Page", 1), None);
    }

    #[test]
    fn views_take_three_forms_and_degrade_to_fit() {
        let fit = [name("View"), view(vec![name("Fit")])];
        assert_eq!(Pairs::of(&fit).view(), View::Fit);
        let fith = [name("View"), view(vec![name("FitH"), MarkValue::Int(700)])];
        assert_eq!(Pairs::of(&fith).view(), View::FitH(Some(700.0)));
        let bare = [name("View"), view(vec![name("FitH")])];
        assert_eq!(Pairs::of(&bare).view(), View::FitH(None));
        let xyz = [
            name("View"),
            view(vec![
                name("XYZ"),
                MarkValue::Null,
                MarkValue::Int(700),
                MarkValue::Int(0),
            ]),
        ];
        assert_eq!(
            Pairs::of(&xyz).view(),
            View::Xyz {
                left: None,
                top: Some(700.0),
                zoom: None
            }
        );
        let fitr = [name("View"), view(vec![name("FitR"), MarkValue::Int(1)])];
        assert_eq!(Pairs::of(&fitr).view(), View::Fit);
        assert_eq!(Pairs::of(&[]).view(), View::Fit);
    }

    #[test]
    fn rectangles_go_through_the_ctm_and_normalise() {
        let rect = Bounds::new(10.0, 20.0, 0.0, 0.0);
        assert_eq!(
            rect_through(Matrix::IDENTITY, rect),
            Bounds::new(0.0, 0.0, 10.0, 20.0)
        );
        assert_eq!(
            rect_through(Matrix::scaling(2.0, 2.0), Bounds::new(1.0, 2.0, 3.0, 4.0)),
            Bounds::new(2.0, 4.0, 6.0, 8.0)
        );
        let rotated = rect_through(Matrix::rotation(90.0), Bounds::new(0.0, 0.0, 10.0, 5.0));
        assert!((rotated.llx + 5.0).abs() < 1e-4 && rotated.lly.abs() < 1e-4);
        assert!(rotated.urx.abs() < 1e-4 && (rotated.ury - 10.0).abs() < 1e-4);
    }

    #[test]
    fn links_take_names_uris_and_pages() {
        let ctm = Matrix::IDENTITY;
        let named = [
            name("Rect"),
            numbers(&[0.0, 0.0, 1.0, 1.0]),
            name("Dest"),
            name("top"),
            name("Subtype"),
            name("Link"),
        ];
        let Parsed::Annot { page, annot } = parse(b"ANN", &named, ctm, 2) else {
            panic!("a link");
        };
        assert_eq!(page, 2);
        assert_eq!(annot.target, LinkTarget::Named(b"top".to_vec()));
        assert_eq!(annot.border, None);
        let uri = [
            name("Rect"),
            numbers(&[0.0, 0.0, 1.0, 1.0]),
            name("Subtype"),
            name("Link"),
            name("Action"),
            MarkValue::Dict(vec![
                (b"Subtype".to_vec(), name("URI")),
                (b"URI".to_vec(), string("https://example.org/")),
            ]),
            name("Border"),
            numbers(&[0.0, 0.0, 1.0]),
            name("Color"),
            numbers(&[1.0, 0.0, 0.0]),
        ];
        let Parsed::Annot { annot, .. } = parse(b"ANN", &uri, ctm, 1) else {
            panic!("a link");
        };
        assert_eq!(
            annot.target,
            LinkTarget::Uri(b"https://example.org/".to_vec())
        );
        assert_eq!(annot.border, Some([0.0, 0.0, 1.0]));
        assert_eq!(annot.color, Some(vec![1.0, 0.0, 0.0]));
        let launch = [
            name("Rect"),
            numbers(&[0.0, 0.0, 1.0, 1.0]),
            name("Subtype"),
            name("Link"),
            name("Action"),
            name("Launch"),
            name("URI"),
            string("u"),
        ];
        let Parsed::Annot { annot, .. } = parse(b"ANN", &launch, ctm, 1) else {
            panic!("a link");
        };
        assert_eq!(annot.target, LinkTarget::Uri(b"u".to_vec()));
        let paged = [
            name("Subtype"),
            name("Link"),
            name("Rect"),
            numbers(&[0.0, 0.0, 1.0, 1.0]),
            name("Page"),
            MarkValue::Int(3),
            name("SrcPg"),
            MarkValue::Int(4),
            name("Contents"),
            string("go"),
        ];
        let Parsed::Annot { page, annot } = parse(b"ANN", &paged, ctm, 1) else {
            panic!("a link");
        };
        assert_eq!(page, 4);
        assert_eq!(
            annot.target,
            LinkTarget::Page {
                index: 3,
                view: View::Fit
            }
        );
        assert_eq!(annot.contents.as_deref(), Some(&b"go"[..]));
    }

    #[test]
    fn other_annotations_and_malformed_marks_are_ignored_by_key() {
        let note = [name("Rect"), numbers(&[0.0, 0.0, 1.0, 1.0])];
        assert_eq!(
            parse(b"ANN", &note, Matrix::IDENTITY, 1),
            Parsed::Ignored(b"ANN/Text".to_vec())
        );
        let widget = [name("Subtype"), name("Widget")];
        assert_eq!(
            parse(b"ANN", &widget, Matrix::IDENTITY, 1),
            Parsed::Ignored(b"ANN/Widget".to_vec())
        );
        let no_rect = [name("Subtype"), name("Link")];
        assert_eq!(
            parse(b"ANN", &no_rect, Matrix::IDENTITY, 1),
            Parsed::Ignored(b"ANN".to_vec())
        );
        let no_title = [name("Count"), MarkValue::Int(1)];
        assert_eq!(
            parse(b"OUT", &no_title, Matrix::IDENTITY, 1),
            Parsed::Ignored(b"OUT".to_vec())
        );
        let no_name = [name("Page"), MarkValue::Int(1)];
        assert_eq!(
            parse(b"DEST", &no_name, Matrix::IDENTITY, 1),
            Parsed::Ignored(b"DEST".to_vec())
        );
        assert_eq!(
            parse(
                b"NOSUCH",
                &[name("Foo"), MarkValue::Int(1)],
                Matrix::IDENTITY,
                1
            ),
            Parsed::Ignored(b"NOSUCH".to_vec())
        );
    }

    #[test]
    fn document_kinds_become_marks() {
        let ctm = Matrix::IDENTITY;
        let out = [
            name("Title"),
            string("Chapter"),
            name("Count"),
            MarkValue::Int(-2),
        ];
        assert_eq!(
            parse(b"OUT", &out, ctm, 2),
            Parsed::Doc(DocMark::Outline {
                title: b"Chapter".to_vec(),
                count: -2,
                target: Some(Target::Page {
                    index: 2,
                    view: View::Fit
                }),
            })
        );
        let launch = [name("Title"), string("Doc"), name("Action"), name("Launch")];
        assert_eq!(
            parse(b"OUT", &launch, ctm, 1),
            Parsed::Doc(DocMark::Outline {
                title: b"Doc".to_vec(),
                count: 0,
                target: None,
            })
        );
        let null_dest = [name("Title"), string("N"), name("Page"), MarkValue::Int(0)];
        assert!(matches!(
            parse(b"OUT", &null_dest, ctm, 1),
            Parsed::Doc(DocMark::Outline { target: None, .. })
        ));
        let dest = [
            name("Dest"),
            name("top"),
            name("View"),
            view(vec![name("FitH"), MarkValue::Int(5)]),
        ];
        assert_eq!(
            parse(b"DEST", &dest, ctm, 1),
            Parsed::Doc(DocMark::Dest {
                name: b"top".to_vec(),
                page: 1,
                view: View::FitH(Some(5.0)),
            })
        );
        let info = [
            name("Title"),
            string("Report"),
            name("Pages"),
            MarkValue::Int(3),
            name("Author"),
            string("Someone"),
        ];
        assert_eq!(
            parse(b"DOCINFO", &info, ctm, 1),
            Parsed::Doc(DocMark::Info(vec![
                (b"Title".to_vec(), b"Report".to_vec()),
                (b"Author".to_vec(), b"Someone".to_vec()),
            ]))
        );
        let docview = [
            name("PageMode"),
            name("UseOutlines"),
            name("Page"),
            MarkValue::Int(2),
            name("View"),
            view(vec![
                name("XYZ"),
                MarkValue::Null,
                MarkValue::Null,
                MarkValue::Null,
            ]),
        ];
        assert_eq!(
            parse(b"DOCVIEW", &docview, ctm, 1),
            Parsed::Doc(DocMark::View {
                page_mode: Some(b"UseOutlines".to_vec()),
                page_layout: None,
                open: Some(Target::Page {
                    index: 2,
                    view: View::Xyz {
                        left: None,
                        top: None,
                        zoom: None
                    }
                }),
            })
        );
        let mode_only = [name("PageLayout"), name("TwoColumnLeft")];
        assert_eq!(
            parse(b"DOCVIEW", &mode_only, ctm, 1),
            Parsed::Doc(DocMark::View {
                page_mode: None,
                page_layout: Some(b"TwoColumnLeft".to_vec()),
                open: None,
            })
        );
        let crop = [
            name("CropBox"),
            numbers(&[288.0, 288.0, 0.0, 0.0]),
            name("Rotate"),
            MarkValue::Int(90),
        ];
        let attrs = PageAttrs {
            crop_box: Some(Bounds::new(0.0, 0.0, 288.0, 288.0)),
            rotate: Some(90),
        };
        assert_eq!(
            parse(b"PAGES", &crop, ctm, 1),
            Parsed::Doc(DocMark::PagesDefault(attrs.clone()))
        );
        assert_eq!(
            parse(b"PAGE", &crop, ctm, 3),
            Parsed::Doc(DocMark::PageAttr { page: 3, attrs })
        );
        let odd = [name("Rotate"), MarkValue::Int(45)];
        assert_eq!(
            parse(b"PAGE", &odd, ctm, 1),
            Parsed::Doc(DocMark::PageAttr {
                page: 1,
                attrs: PageAttrs::default()
            })
        );
    }
}
