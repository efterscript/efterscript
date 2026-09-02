// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Font objects (ISO 32000-1 §9.6). A resident font becomes an
//! unembedded Type 1 font dictionary naming the standard font, with the
//! codes that differ from the base font's built-in encoding as
//! `Differences`, the widths of the encoded glyphs from the metrics, a
//! font descriptor from the same metrics (the standard fourteen may omit
//! all of first char, last char, widths, and descriptor, or must carry
//! all four), and a ToUnicode CMap (§9.10.3) mapping each code whose
//! glyph name the glyph list knows. A Type 3 font (§9.6.5) carries its
//! own matrix and bounding box, one CharProc stream per captured glyph
//! written through the content writer and opening with `d0` or `d1`,
//! the encoding and widths of the captured glyphs in glyph space, and a
//! `Resources` dictionary for whatever the procedures refer to.
//!
//! Font objects are written once per document: a page whose font
//! resource is structurally equal to one already written — including the
//! values of the colour spaces, images, and fonts its glyphs name —
//! reuses the object. Embedded fonts are the exception: their objects
//! are allocated here and written when the document ends (see
//! `embedded`), since their subset depends on every page.

use std::collections::BTreeSet;
use std::io::Write;

use pdf_out::{ArrayBuilder, Document, Filter, Ref};
use ps_fonts::ResidentFace;
use ps_graphics::{FontSpec, GlyphNames, GlyphProc, Image, IrOp, Op, Page, Resources};
use ps_vm::{Bounds, Matrix, SpaceSpec};

use crate::content;
use crate::embedded::EmbeddedTable;
use crate::resources::Objects;

/// What a font's glyph procedures refer to on their page, by index.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Refs {
    pub spaces: BTreeSet<usize>,
    pub images: BTreeSet<usize>,
    pub fonts: BTreeSet<usize>,
}

fn collect(ops: &[Op], refs: &mut Refs) {
    for op in ops {
        match &op.op {
            IrOp::SetColorSpace(space) => {
                refs.spaces.insert(space.0);
            }
            IrOp::Image { image, .. } => {
                refs.images.insert(image.0);
            }
            IrOp::Text { font, .. } => {
                refs.fonts.insert(font.0);
            }
            _ => {}
        }
    }
}

/// The page resources `spec`'s glyph procedures name; none for a
/// resident font.
pub(crate) fn references(spec: &FontSpec) -> Refs {
    let mut refs = Refs::default();
    if let FontSpec::Type3 { glyphs, .. } = spec {
        for glyph in glyphs.values() {
            collect(&glyph.ops, &mut refs);
        }
    }
    refs
}

/// The identity of a font object: the spec and the values of what its
/// glyphs refer to, so two pages with different resource layouts share
/// an object only when it means the same thing on both.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    spec: FontSpec,
    spaces: Vec<SpaceSpec>,
    images: Vec<Image>,
    fonts: Vec<FontSpec>,
}

impl Key {
    fn of(spec: &FontSpec, resources: &Resources) -> Self {
        let refs = references(spec);
        Key {
            spec: spec.clone(),
            spaces: refs
                .spaces
                .iter()
                .filter_map(|&i| resources.color_spaces.get(i).cloned())
                .collect(),
            images: refs
                .images
                .iter()
                .filter_map(|&i| resources.images.get(i).cloned())
                .collect(),
            fonts: refs
                .fonts
                .iter()
                .filter_map(|&i| resources.fonts.get(i).cloned())
                .collect(),
        }
    }
}

/// The font objects written so far in the document, and the embedded
/// fonts still to be written at its end.
#[derive(Default)]
pub(crate) struct FontTable {
    written: Vec<(Key, Ref)>,
    pub(crate) embedded: EmbeddedTable,
}

impl FontTable {
    fn lookup(&self, key: &Key) -> Option<Ref> {
        self.written.iter().find(|(k, _)| k == key).map(|(_, r)| *r)
    }
}

/// Writes the font objects `page` needs that the document lacks and
/// returns one reference per font resource, in index order. `objects`
/// holds the page's colour spaces and images, already written, for the
/// `Resources` of a Type 3 font.
pub(crate) fn write_fonts<W: Write>(
    doc: &mut Document<W>,
    page: &Page,
    filter: Filter,
    table: &mut FontTable,
    objects: &Objects,
    notes: &mut Vec<String>,
) -> Result<Vec<Ref>, pdf_out::Error> {
    let resources = &page.resources;
    let mut refs = Vec::with_capacity(resources.fonts.len());
    let mut pending = Vec::new();
    for (index, spec) in resources.fonts.iter().enumerate() {
        if matches!(spec, FontSpec::Embedded { .. }) {
            refs.push(table.embedded.use_font(doc, page, index));
            continue;
        }
        let key = Key::of(spec, resources);
        match table.lookup(&key) {
            Some(r) => refs.push(r),
            None => {
                // Allocated before it is written, so a glyph's nested run
                // can refer to it and an equal font later on the page
                // finds it.
                let r = doc.alloc();
                table.written.push((key, r));
                refs.push(r);
                pending.push((spec, r));
            }
        }
    }
    for (spec, r) in pending {
        match spec {
            FontSpec::Resident { base, encoding } => {
                write_resident(doc, r, *base, encoding, filter)?;
            }
            FontSpec::Type3 {
                font_matrix,
                font_bbox,
                encoding,
                glyphs,
            } => {
                let type3 = Type3 {
                    font_matrix: *font_matrix,
                    font_bbox: *font_bbox,
                    encoding,
                    glyphs,
                    refs: references(spec),
                };
                let mut glyph_notes =
                    write_type3(doc, r, &type3, resources, objects, &refs, filter)?;
                notes.append(&mut glyph_notes);
            }
            FontSpec::Embedded { .. } => unreachable!("routed to the embedded table"),
        }
    }
    Ok(refs)
}

// --- shared pieces ---------------------------------------------------------------------

/// `Differences` entries: a code starts a run, consecutive codes follow
/// with their names alone.
pub(crate) fn differences<'a>(
    a: &mut ArrayBuilder<'_>,
    entries: impl Iterator<Item = (u8, &'a [u8])>,
) {
    let mut previous: Option<u8> = None;
    for (code, name) in entries {
        if previous.is_none_or(|p| p.checked_add(1) != Some(code)) {
            a.int(i64::from(code));
        }
        a.item().name_bytes(name);
        previous = Some(code);
    }
}

/// First code, last code, and the width of every code between, from the
/// codes that have a width; `None` when none has.
pub(crate) fn widths(width_of: impl Fn(u8) -> Option<f32>) -> Option<(u8, u8, Vec<f32>)> {
    let widths: Vec<Option<f32>> = (0..=255u8).map(width_of).collect();
    let first = widths.iter().position(Option::is_some)?;
    let last = widths.iter().rposition(Option::is_some)?;
    let values = widths[first..=last]
        .iter()
        .map(|w| w.unwrap_or(0.0))
        .collect();
    Some((first as u8, last as u8, values))
}

fn hex_utf16(chars: &[char]) -> String {
    let mut out = String::new();
    for c in chars {
        for unit in c.encode_utf16(&mut [0; 2]) {
            out.push_str(&format!("{unit:04X}"));
        }
    }
    out
}

/// A ToUnicode CMap over the codes whose glyph names the glyph list
/// maps — or `fallback` does, for a name the list lacks — in code
/// order; `None` when no code maps.
fn to_unicode<'a>(
    entries: impl Iterator<Item = (u8, &'a [u8])>,
    fallback: impl Fn(&[u8]) -> Option<Vec<char>>,
) -> Option<Vec<u8>> {
    let mapped: Vec<(u8, Vec<char>)> = entries
        .filter_map(|(code, name)| {
            ps_fonts::unicode(name)
                .or_else(|| fallback(name))
                .map(|chars| (code, chars))
        })
        .collect();
    if mapped.is_empty() {
        return None;
    }
    let mut text = String::from(
        "/CIDInit /ProcSet findresource begin\n\
         12 dict begin\n\
         begincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n\
         /CMapType 2 def\n\
         1 begincodespacerange\n\
         <00> <FF>\n\
         endcodespacerange\n",
    );
    // A bfchar block holds at most a hundred entries.
    for block in mapped.chunks(100) {
        text.push_str(&format!("{} beginbfchar\n", block.len()));
        for (code, chars) in block {
            text.push_str(&format!("<{code:02X}> <{}>\n", hex_utf16(chars)));
        }
        text.push_str("endbfchar\n");
    }
    text.push_str(
        "endcmap\n\
         CMapName currentdict /CMap defineresource pop\n\
         end\n\
         end\n",
    );
    Some(text.into_bytes())
}

pub(crate) fn write_to_unicode<'a, W: Write>(
    doc: &mut Document<W>,
    filter: Filter,
    entries: impl Iterator<Item = (u8, &'a [u8])>,
    fallback: impl Fn(&[u8]) -> Option<Vec<char>>,
) -> Result<Option<Ref>, pdf_out::Error> {
    let Some(cmap) = to_unicode(entries, fallback) else {
        return Ok(None);
    };
    let r = doc.alloc();
    doc.write_stream(r, filter, &cmap, |_| {})?;
    Ok(Some(r))
}

pub(crate) fn put_bounds(a: &mut ArrayBuilder<'_>, b: [f32; 4]) {
    for v in b {
        a.real(v);
    }
}

// --- resident fonts ------------------------------------------------------------------

/// Font descriptor flags (ISO 32000-1 Table 123) for a resident face.
fn flags(base: ResidentFace) -> i64 {
    const FIXED_PITCH: i64 = 1;
    const SERIF: i64 = 1 << 1;
    const SYMBOLIC: i64 = 1 << 2;
    const NONSYMBOLIC: i64 = 1 << 5;
    const ITALIC: i64 = 1 << 6;
    let metrics = base.metrics();
    let mut flags = if base.is_symbolic() {
        SYMBOLIC
    } else {
        NONSYMBOLIC
    };
    if metrics.is_fixed_pitch {
        flags |= FIXED_PITCH;
    }
    if base.family().is_serif() {
        flags |= SERIF;
    }
    if base.is_italic() {
        flags |= ITALIC;
    }
    flags
}

fn write_descriptor<W: Write>(
    doc: &mut Document<W>,
    base: ResidentFace,
) -> Result<Ref, pdf_out::Error> {
    let metrics = base.metrics();
    let [_, lly, _, ury] = metrics.font_bbox;
    let r = doc.alloc();
    doc.write_obj(r, |v| {
        v.dict(|d| {
            d.key("Type").name("FontDescriptor");
            d.key("FontName").name(base.postscript_name());
            d.key("Flags").int(flags(base));
            d.key("FontBBox")
                .array(|a| put_bounds(a, metrics.font_bbox));
            d.key("ItalicAngle").real(metrics.italic_angle);
            d.key("Ascent").real(metrics.ascender.unwrap_or(ury));
            d.key("Descent").real(metrics.descender.unwrap_or(lly));
            if let Some(cap_height) = metrics.cap_height {
                d.key("CapHeight").real(cap_height);
            }
            d.key("StemV").real(metrics.std_vw.unwrap_or(0.0));
        });
    })?;
    Ok(r)
}

fn write_resident<W: Write>(
    doc: &mut Document<W>,
    r: Ref,
    base: ResidentFace,
    encoding: &GlyphNames,
    filter: Filter,
) -> Result<(), pdf_out::Error> {
    let builtin = base.builtin_encoding();
    let named = || {
        encoding
            .iter()
            .enumerate()
            .filter_map(|(code, name)| name.as_deref().map(|n| (code as u8, n)))
    };
    let differing: Vec<(u8, &[u8])> = encoding
        .iter()
        .enumerate()
        .filter(|(code, name)| name.as_deref() != builtin[*code].map(str::as_bytes))
        .map(|(code, name)| (code as u8, name.as_deref().unwrap_or(b".notdef")))
        .collect();
    let metrics = widths(|code| {
        let name = encoding[usize::from(code)].as_deref()?;
        Some(
            std::str::from_utf8(name)
                .ok()
                .and_then(|name| base.width(name))
                .map_or(0.0, f32::from),
        )
    });
    let descriptor = match metrics {
        Some(_) => Some(write_descriptor(doc, base)?),
        None => None,
    };
    let to_unicode = write_to_unicode(doc, filter, named(), |_| None)?;
    doc.write_obj(r, |v| {
        v.dict(|d| {
            d.key("Type").name("Font");
            d.key("Subtype").name("Type1");
            d.key("BaseFont").name(base.postscript_name());
            if let Some((first, last, values)) = &metrics {
                d.key("FirstChar").int(i64::from(*first));
                d.key("LastChar").int(i64::from(*last));
                d.key("Widths").array(|a| {
                    for &w in values {
                        a.real(w);
                    }
                });
            }
            if !differing.is_empty() {
                d.key("Encoding").dict(|e| {
                    e.key("Type").name("Encoding");
                    e.key("Differences")
                        .array(|a| differences(a, differing.iter().copied()));
                });
            }
            if let Some(descriptor) = descriptor {
                d.key("FontDescriptor").reference(descriptor);
            }
            if let Some(to_unicode) = to_unicode {
                d.key("ToUnicode").reference(to_unicode);
            }
        });
    })
}

// --- Type 3 fonts ----------------------------------------------------------------------

struct Type3<'a> {
    font_matrix: Matrix,
    font_bbox: Bounds,
    encoding: &'a GlyphNames,
    glyphs: &'a std::collections::BTreeMap<Vec<u8>, GlyphProc>,
    refs: Refs,
}

/// The CharProc's opening line: the glyph's width and, for a
/// colour-independent glyph, its box (`d1`); a glyph that may set its
/// own colour declares the width alone (`d0`).
fn glyph_prefix(glyph: &GlyphProc) -> String {
    let (wx, wy) = glyph.width;
    match glyph.bbox {
        Some(b) => format!(
            "{} d1\n",
            pdf_out_reals(&[wx, wy, b.llx, b.lly, b.urx, b.ury])
        ),
        None => format!("{} d0\n", pdf_out_reals(&[wx, wy])),
    }
}

fn pdf_out_reals(values: &[f32]) -> String {
    values
        .iter()
        .map(|&v| pdf_out::fmt_real(v))
        .collect::<Vec<_>>()
        .join(" ")
}

fn write_type3<W: Write>(
    doc: &mut Document<W>,
    r: Ref,
    font: &Type3<'_>,
    resources: &Resources,
    objects: &Objects,
    fonts: &[Ref],
    filter: Filter,
) -> Result<Vec<String>, pdf_out::Error> {
    let mut notes = Vec::new();
    let mut procs: Vec<(&[u8], Ref)> = Vec::with_capacity(font.glyphs.len());
    for (name, glyph) in font.glyphs {
        let rendered = content::render(&glyph.ops, resources);
        let mut body = glyph_prefix(glyph).into_bytes();
        body.extend_from_slice(&rendered.bytes);
        notes.extend(rendered.notes);
        let stream = doc.alloc();
        doc.write_stream(stream, filter, &body, |_| {})?;
        procs.push((name, stream));
    }
    // The codes whose glyph was captured, in code order.
    let captured: Vec<(u8, &[u8])> = font
        .encoding
        .iter()
        .enumerate()
        .filter_map(|(code, name)| {
            let name = name.as_deref()?;
            font.glyphs.contains_key(name).then_some((code as u8, name))
        })
        .collect();
    let metrics = widths(|code| {
        let name = font.encoding[usize::from(code)].as_deref()?;
        font.glyphs.get(name).map(|g| g.width.0)
    })
    .unwrap_or((0, 0, vec![0.0]));
    let to_unicode = write_to_unicode(doc, filter, captured.iter().copied(), |_| None)?;
    let b = font.font_bbox;
    doc.write_obj(r, |v| {
        v.dict(|d| {
            d.key("Type").name("Font");
            d.key("Subtype").name("Type3");
            d.key("FontBBox")
                .array(|a| put_bounds(a, [b.llx, b.lly, b.urx, b.ury]));
            d.key("FontMatrix").array(|a| {
                for value in font.font_matrix.0 {
                    a.real(value);
                }
            });
            d.key("CharProcs").dict(|c| {
                for (name, stream) in &procs {
                    c.key_bytes(name).reference(*stream);
                }
            });
            d.key("Encoding").dict(|e| {
                e.key("Type").name("Encoding");
                e.key("Differences")
                    .array(|a| differences(a, captured.iter().copied()));
            });
            let (first, last, values) = &metrics;
            d.key("FirstChar").int(i64::from(*first));
            d.key("LastChar").int(i64::from(*last));
            d.key("Widths").array(|a| {
                for &w in values {
                    a.real(w);
                }
            });
            if objects.names_anything(&font.refs, fonts) {
                d.key("Resources")
                    .dict(|res| objects.resources_dict(res, fonts, Some(&font.refs)));
            }
            if let Some(to_unicode) = to_unicode {
                d.key("ToUnicode").reference(to_unicode);
            }
        });
    })?;
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_unicode_maps_known_names_in_code_order_and_skips_the_rest() {
        let entries: [(u8, &[u8]); 4] = [(72, b"H"), (0, b".notdef"), (105, b"i"), (200, b"fi")];
        let cmap = String::from_utf8(to_unicode(entries.into_iter(), |_| None).unwrap()).unwrap();
        assert!(cmap.contains("3 beginbfchar\n<48> <0048>\n<69> <0069>\n<C8> <FB01>\nendbfchar\n"));
        assert!(cmap.starts_with("/CIDInit /ProcSet findresource begin\n"));
        assert!(
            cmap.ends_with("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n")
        );
        assert_eq!(
            to_unicode([(0u8, b".notdef".as_slice())].into_iter(), |_| None),
            None
        );
        let fallback = to_unicode([(1u8, b"g1".as_slice())].into_iter(), |name| {
            (name == b"g1").then(|| vec!['x'])
        })
        .unwrap();
        assert!(
            String::from_utf8(fallback)
                .unwrap()
                .contains("<01> <0078>\n")
        );
        assert_eq!(hex_utf16(&['😀']), "D83DDE00");
    }

    #[test]
    fn widths_span_the_first_to_the_last_encoded_code() {
        let (first, last, values) = widths(|code| match code {
            65 => Some(1.0),
            67 => Some(3.0),
            _ => None,
        })
        .unwrap();
        assert_eq!((first, last), (65, 67));
        assert_eq!(values, [1.0, 0.0, 3.0]);
        assert!(widths(|_| None).is_none());
    }

    #[test]
    fn flags_follow_the_family() {
        assert_eq!(flags(ResidentFace::Helvetica), 32);
        assert_eq!(flags(ResidentFace::TimesItalic), 2 | 32 | 64);
        assert_eq!(flags(ResidentFace::CourierBold), 1 | 2 | 32);
        assert_eq!(flags(ResidentFace::Symbol), 4);
        assert_eq!(flags(ResidentFace::PalatinoItalic), 2 | 32 | 64);
        assert_eq!(flags(ResidentFace::AvantGardeBook), 32);
    }

    #[test]
    fn a_glyph_procedure_opens_with_its_width_operator() {
        let cached = GlyphProc {
            ops: Vec::new(),
            width: (1000.0, 0.0),
            bbox: Some(Bounds::new(0.0, 0.0, 750.0, 750.0)),
        };
        assert_eq!(glyph_prefix(&cached), "1000 0 0 0 750 750 d1\n");
        let free = GlyphProc {
            ops: Vec::new(),
            width: (500.0, 0.0),
            bbox: None,
        };
        assert_eq!(glyph_prefix(&free), "500 0 d0\n");
    }
}
