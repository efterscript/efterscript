// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Composite fonts in the PDF (ISO 32000-1 §9.7). A composite resource
//! becomes a Type 0 font over `Identity-H` (or `Identity-V` for writing
//! mode 1), whose content strings are two-byte CIDs, with one
//! descendant: a `CIDFontType0` whose `FontFile3` of subtype
//! `CIDFontType0C` holds the CID-keyed CFF subset, or a `CIDFontType2`
//! whose `FontFile2` holds the TrueType subset and whose `CIDToGIDMap`
//! stream maps each CID to the subset's glyph index (the subset
//! renumbers glyphs, so the map is never the identity). `W` carries the
//! used CIDs' advances in thousandths through the descendant's matrix,
//! grouped by runs of consecutive CIDs, under `DW 1000`; `CIDSystemInfo`
//! is the CFF program's, or `Adobe`/`Identity`/0 for TrueType. ToUnicode
//! maps each CID to the code bytes it came from read as UTF-16BE when
//! the CMap is Unicode-based, else through the TrueType program's own
//! Unicode cmap, else not at all.
//!
//! A descendant with Type 1 charstrings has no embedding form, so it is
//! written as a Type 3 font (§9.6.5) whose CharProcs are the used
//! glyphs' outlines from the engine, the CIDs re-encoded to consecutive
//! one-byte codes in order of first use across the document; in writing
//! mode 1 each glyph is drawn shifted to its vertical origin with zero
//! width, and the content writer moves the pen for every advance.
//!
//! Like embedded simple fonts, composite fonts are written when the
//! document finishes, over the CIDs every page used.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use efterscript_fonts::cff::{self, Privates};
use efterscript_fonts::{Outline, OutlineOp, Program, ProgramKind, TrueTypeProgram, truetype};
use efterscript_graphics::{FillRule, FontSpec, IrOp, Resources};
use efterscript_pdf::{ArrayBuilder, Document, Filter, Ref};
use efterscript_vm::{Matrix, Point, Seg};

use crate::content::{self, Recode};
use crate::embedded::{
    DEFAULT_STEM_V, Descriptor, FIXED_PITCH, SYMBOLIC, pdf_bbox, subset_tag, tagged,
    truetype_gid_unicode, write_descriptor,
};
use crate::fonts::{differences, put_bounds, write_to_unicode_codes};

/// The default vertical origin's height above the horizontal origin as
/// a fraction of the em: the `DW2` default's position (ISO 32000-1
/// §9.7.4.3), which the Type 3 fallback must apply itself.
const VERTICAL_ORIGIN_Y: f32 = 0.88;

/// A composite font used so far: its resource with the CID-to-code map
/// merged over every page, its object, the CIDs shown, and, for a Type
/// 3 fallback, the one-byte code each CID was given.
pub(crate) struct CompositeFont {
    pub(crate) spec: FontSpec,
    pub(crate) font: Ref,
    cids: BTreeSet<u16>,
    codes: Option<BTreeMap<u16, u8>>,
}

/// Whether the descendant has no embeddable program and so takes the
/// Type 3 form.
fn is_fallback(descendant: &FontSpec) -> bool {
    matches!(
        descendant,
        FontSpec::Embedded {
            kind: ProgramKind::Type1Cid | ProgramKind::Type1,
            ..
        }
    )
}

impl CompositeFont {
    pub(crate) fn new(spec: &FontSpec, font: Ref) -> Self {
        let fallback =
            matches!(spec, FontSpec::Composite { descendant, .. } if is_fallback(descendant));
        CompositeFont {
            spec: spec.clone(),
            font,
            cids: BTreeSet::new(),
            codes: fallback.then(BTreeMap::new),
        }
    }

    /// Records a page's use of the font: its CIDs and the codes they
    /// came from, and for a fallback the next one-byte codes for CIDs
    /// not seen before. A fallback holds 255 glyphs; a CID beyond them
    /// is noted and shown as the notdef.
    pub(crate) fn record(&mut self, spec: &FontSpec, cids: BTreeSet<u16>, notes: &mut Vec<String>) {
        if let (
            FontSpec::Composite { cid_to_code, .. },
            FontSpec::Composite {
                cid_to_code: page_codes,
                ..
            },
        ) = (&mut self.spec, spec)
        {
            for (cid, code) in page_codes {
                cid_to_code.entry(*cid).or_insert(*code);
            }
        }
        if let Some(codes) = &mut self.codes {
            for &cid in &cids {
                if codes.contains_key(&cid) {
                    continue;
                }
                match u8::try_from(codes.len() + 1) {
                    Ok(code) => {
                        codes.insert(cid, code);
                    }
                    Err(_) => notes.push(format!(
                        "CID {cid} is beyond the 255 glyphs a Type 3 fallback font holds; \
                         shown as its notdef"
                    )),
                }
            }
        }
        self.cids.extend(cids);
    }

    /// The fallback's codes by CID, for the content writer; `None` for a
    /// font written as Type 0.
    pub(crate) fn recode(&self) -> Option<BTreeMap<u16, u8>> {
        self.codes.clone()
    }
}

/// A CID's advance in thousandths of text space, rounded to a
/// thousandth to shed single-precision residue.
fn cid_width(spec: &FontSpec, cid: u16) -> f32 {
    let (wx, _) = spec.cid_width(cid);
    let a = spec.font_matrix().0[0];
    ((f64::from(wx) * f64::from(a) * 1_000_000.0).round() / 1000.0) as f32
}

/// The characters a CID stands for: the code bytes it came from as
/// UTF-16BE under a Unicode-based CMap, else the TrueType program's
/// Unicode cmap read backwards from the CID's glyph, else none.
fn unicode_of(spec: &FontSpec, cid: u16) -> Option<Vec<char>> {
    let FontSpec::Composite {
        unicode_based,
        descendant,
        cid_to_code,
        ..
    } = spec
    else {
        return None;
    };
    if *unicode_based {
        let &(code, len) = cid_to_code.get(&cid)?;
        let mut bytes = code.to_be_bytes()[4 - usize::from(len.clamp(1, 4))..].to_vec();
        if !bytes.len().is_multiple_of(2) {
            bytes.insert(0, 0);
        }
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        return char::decode_utf16(units)
            .collect::<Result<Vec<char>, _>>()
            .ok()
            .filter(|chars| !chars.is_empty());
    }
    if let FontSpec::Embedded { program, .. } = &**descendant
        && let Program::TrueType(truetype) = &*program.0
    {
        let gid = truetype.cid_map()?.gid(cid)?;
        return truetype_gid_unicode(truetype, gid);
    }
    None
}

/// The `W` array: runs of consecutive CIDs as `c [w …]`.
fn put_widths(a: &mut ArrayBuilder<'_>, widths: &[(u16, f32)]) {
    let mut at = 0;
    while at < widths.len() {
        let mut end = at + 1;
        while end < widths.len() && widths[end].0 == widths[end - 1].0 + 1 {
            end += 1;
        }
        a.int(i64::from(widths[at].0));
        a.array(|run| {
            for &(_, w) in &widths[at..end] {
                run.real(w);
            }
        });
        at = end;
    }
}

struct SystemInfo {
    registry: Vec<u8>,
    ordering: Vec<u8>,
    supplement: i32,
}

impl SystemInfo {
    fn identity() -> Self {
        SystemInfo {
            registry: b"Adobe".to_vec(),
            ordering: b"Identity".to_vec(),
            supplement: 0,
        }
    }
}

/// What the descendant's program became: its dictionary's subtype, the
/// descriptor holding the subset, its character collection, and the
/// CID-to-glyph map stream of a TrueType descendant.
struct Descendant {
    subtype: &'static str,
    descriptor: Ref,
    system_info: SystemInfo,
    cid_to_gid: Option<Ref>,
}

fn write_cff_descendant<W: Write>(
    doc: &mut Document<W>,
    program: &cff::CffProgram,
    name: &[u8],
    font_matrix: Matrix,
    cids: &BTreeSet<u16>,
    filter: Filter,
) -> Result<Descendant, efterscript_pdf::Error> {
    // A program that is not CID-keyed cannot take the CID form; it is
    // described without a stream, as an unembeddable simple font is.
    let stream = match cff::write::subset_cid(program, name, cids) {
        Ok(bytes) => {
            let stream = doc.alloc();
            doc.write_stream(stream, filter, &bytes, |d| {
                d.key("Subtype").name("CIDFontType0C");
            })?;
            Some(("FontFile3", stream))
        }
        Err(_) => None,
    };
    let stem_v = match program.privates() {
        Privates::Cid { dicts, .. } => dicts.first().and_then(|d| d.number(cff::op::STD_VW)),
        Privates::Single(dict) => dict.number(cff::op::STD_VW),
    };
    let descriptor = Descriptor {
        flags: SYMBOLIC
            | if program.is_fixed_pitch() {
                FIXED_PITCH
            } else {
                0
            },
        bbox: pdf_bbox(program.font_bbox(), font_matrix),
        italic_angle: program.italic_angle(),
        cap_height: None,
        stem_v: stem_v.unwrap_or(DEFAULT_STEM_V),
    };
    let descriptor = write_descriptor(doc, name, &descriptor, stream)?;
    let system_info = program
        .ros()
        .map_or_else(SystemInfo::identity, |ros| SystemInfo {
            registry: ros.registry,
            ordering: ros.ordering,
            supplement: ros.supplement,
        });
    Ok(Descendant {
        subtype: "CIDFontType0",
        descriptor,
        system_info,
        cid_to_gid: None,
    })
}

fn write_truetype_descendant<W: Write>(
    doc: &mut Document<W>,
    program: &TrueTypeProgram,
    name: &[u8],
    cids: &BTreeSet<u16>,
    filter: Filter,
) -> Result<Descendant, efterscript_pdf::Error> {
    let gid_of = |cid: u16| program.cid_map().and_then(|map| map.gid(cid)).unwrap_or(0);
    let gids: BTreeSet<u16> = cids.iter().map(|&cid| gid_of(cid)).collect();
    // Should the subset fail (it cannot for glyphs the VM interpreted),
    // the whole program goes in with its own glyph numbering.
    let (bytes, map) = truetype::write::subset_with_map(program, &gids, &BTreeMap::new())
        .unwrap_or_else(|_| {
            (
                program.bytes().to_vec(),
                (0..program.num_glyphs()).map(|g| (g, g)).collect(),
            )
        });
    let stream = doc.alloc();
    doc.write_stream(stream, filter, &bytes, |d| {
        d.key("Length1").int(bytes.len() as i64);
    })?;
    let em = f32::from(program.units_per_em());
    let bbox = program.bbox().map(|v| f32::from(v) * 1000.0 / em);
    let descriptor = Descriptor {
        flags: SYMBOLIC
            | if program.is_fixed_pitch() {
                FIXED_PITCH
            } else {
                0
            },
        bbox: bbox.map(|v| (f64::from(v) * 1000.0).round() as f32 / 1000.0),
        italic_angle: program.italic_angle(),
        cap_height: None,
        stem_v: DEFAULT_STEM_V,
    };
    let descriptor = write_descriptor(doc, name, &descriptor, Some(("FontFile2", stream)))?;
    // Two bytes per CID up to the highest used one: the subset's index
    // of the CID's glyph, glyph 0 for a CID the document did not show.
    let highest = cids.iter().next_back().copied().unwrap_or(0);
    let mut table = Vec::with_capacity((usize::from(highest) + 1) * 2);
    for cid in 0..=highest {
        let new = if cids.contains(&cid) {
            map.get(&gid_of(cid)).copied().unwrap_or(0)
        } else {
            0
        };
        table.extend_from_slice(&new.to_be_bytes());
    }
    let cid_to_gid = doc.alloc();
    doc.write_stream(cid_to_gid, filter, &table, |_| {})?;
    Ok(Descendant {
        subtype: "CIDFontType2",
        descriptor,
        system_info: SystemInfo::identity(),
        cid_to_gid: Some(cid_to_gid),
    })
}

/// Writes a composite font: the Type 0 dictionary, its descendant with
/// the subset program, and the ToUnicode CMap; or the Type 3 fallback.
pub(crate) fn write_font<W: Write>(
    doc: &mut Document<W>,
    font: &CompositeFont,
    filter: Filter,
) -> Result<(), efterscript_pdf::Error> {
    let FontSpec::Composite {
        wmode, descendant, ..
    } = &font.spec
    else {
        return Ok(());
    };
    let FontSpec::Embedded {
        font_name,
        font_matrix,
        program,
        ..
    } = &**descendant
    else {
        return Ok(());
    };
    if let Some(codes) = &font.codes {
        return write_type3_fallback(doc, font, codes, filter);
    }
    let keep: BTreeSet<Vec<u8>> = font.cids.iter().map(|c| c.to_be_bytes().to_vec()).collect();
    let name = tagged(&subset_tag(font_name, &keep), font_name);
    let descendant = match &*program.0 {
        Program::Cff(cff) => {
            write_cff_descendant(doc, cff, &name, *font_matrix, &font.cids, filter)?
        }
        Program::TrueType(truetype) => {
            write_truetype_descendant(doc, truetype, &name, &font.cids, filter)?
        }
        Program::Type1(_) | Program::Type1Cid(_) => unreachable!("written as a Type 3 fallback"),
    };
    let widths: Vec<(u16, f32)> = font
        .cids
        .iter()
        .map(|&cid| (cid, cid_width(&font.spec, cid)))
        .collect();
    let mapped: Vec<(u32, Vec<char>)> = font
        .cids
        .iter()
        .filter_map(|&cid| unicode_of(&font.spec, cid).map(|chars| (u32::from(cid), chars)))
        .collect();
    let to_unicode = write_to_unicode_codes(doc, filter, 2, &mapped)?;
    let descendant_ref = doc.alloc();
    doc.write_obj(descendant_ref, |v| {
        v.dict(|d| {
            d.key("Type").name("Font");
            d.key("Subtype").name(descendant.subtype);
            d.key("BaseFont").name_bytes(&name);
            d.key("CIDSystemInfo").dict(|info| {
                info.key("Registry")
                    .string(&descendant.system_info.registry);
                info.key("Ordering")
                    .string(&descendant.system_info.ordering);
                info.key("Supplement")
                    .int(i64::from(descendant.system_info.supplement));
            });
            d.key("FontDescriptor").reference(descendant.descriptor);
            d.key("DW").int(1000);
            if !widths.is_empty() {
                d.key("W").array(|a| put_widths(a, &widths));
            }
            if let Some(map) = descendant.cid_to_gid {
                d.key("CIDToGIDMap").reference(map);
            }
        });
    })?;
    doc.write_obj(font.font, |v| {
        v.dict(|d| {
            d.key("Type").name("Font");
            d.key("Subtype").name("Type0");
            d.key("BaseFont").name_bytes(&name);
            d.key("Encoding").name(if *wmode == 1 {
                "Identity-V"
            } else {
                "Identity-H"
            });
            d.key("DescendantFonts").array(|a| {
                a.reference(descendant_ref);
            });
            if let Some(to_unicode) = to_unicode {
                d.key("ToUnicode").reference(to_unicode);
            }
        });
    })
}

fn segments(outline: &Outline) -> Vec<Seg> {
    outline
        .ops
        .iter()
        .map(|op| match *op {
            OutlineOp::MoveTo(x, y) => Seg::Move(Point::new(x, y)),
            OutlineOp::LineTo(x, y) => Seg::Line(Point::new(x, y)),
            OutlineOp::CurveTo(x1, y1, x2, y2, x, y) => {
                Seg::Curve(Point::new(x1, y1), Point::new(x2, y2), Point::new(x, y))
            }
            OutlineOp::Close => Seg::Close,
        })
        .collect()
}

/// The Type 3 form of a composite font whose descendant has no
/// embedding form: one CharProc per used CID from the engine's outline,
/// opening with `d1` over the outline's box, then the path and a fill;
/// the encoding maps the assigned codes to glyphs named `cid<N>`.
fn write_type3_fallback<W: Write>(
    doc: &mut Document<W>,
    font: &CompositeFont,
    codes: &BTreeMap<u16, u8>,
    filter: Filter,
) -> Result<(), efterscript_pdf::Error> {
    let FontSpec::Composite {
        wmode, descendant, ..
    } = &font.spec
    else {
        return Ok(());
    };
    let vertical = *wmode == 1;
    let mut by_code: Vec<(u8, u16)> = codes.iter().map(|(&cid, &code)| (code, cid)).collect();
    by_code.sort_unstable();
    let mut procs: Vec<(Vec<u8>, Ref)> = Vec::with_capacity(by_code.len());
    let mut widths: Vec<f32> = Vec::with_capacity(by_code.len());
    let mut font_bbox = [0.0f32; 4];
    let mut mapped: Vec<(u32, Vec<char>)> = Vec::new();
    for &(code, cid) in &by_code {
        let glyph = font.spec.cid_glyph(cid);
        let wx = glyph.as_ref().map_or(0.0, |g| g.advance.0);
        let outline = glyph.as_ref().map(|g| &g.outline);
        // In vertical mode the glyph hangs from its vertical origin at
        // the pen, and PDF's own advance is zero: the writer moves the
        // pen by the recorded vertical displacement.
        let (shift, width) = if vertical {
            ((-wx / 2.0, -VERTICAL_ORIGIN_Y * 1000.0), 0.0)
        } else {
            ((0.0, 0.0), wx)
        };
        let outline = outline.map(|o| o.translated(shift.0, shift.1));
        let bbox = outline
            .as_ref()
            .and_then(Outline::control_box)
            .unwrap_or([0.0; 4]);
        if procs.is_empty() {
            font_bbox = bbox;
        } else {
            font_bbox = [
                font_bbox[0].min(bbox[0]),
                font_bbox[1].min(bbox[1]),
                font_bbox[2].max(bbox[2]),
                font_bbox[3].max(bbox[3]),
            ];
        }
        let mut body = format!(
            "{} d1\n",
            [width, 0.0, bbox[0], bbox[1], bbox[2], bbox[3]]
                .iter()
                .map(|&v| efterscript_pdf::fmt_real(v))
                .collect::<Vec<_>>()
                .join(" ")
        )
        .into_bytes();
        let path = outline.as_ref().map(segments).unwrap_or_default();
        if !path.is_empty() {
            let fill = IrOp::Fill {
                path,
                rule: FillRule::NonZero,
            };
            let rendered = content::render(&[fill.into()], &Resources::default(), &Recode::new());
            body.extend_from_slice(&rendered.bytes);
        }
        let stream = doc.alloc();
        doc.write_stream(stream, filter, &body, |_| {})?;
        procs.push((format!("cid{cid}").into_bytes(), stream));
        widths.push(width);
        if let Some(chars) = unicode_of(&font.spec, cid) {
            mapped.push((u32::from(code), chars));
        }
    }
    let to_unicode = write_to_unicode_codes(doc, filter, 1, &mapped)?;
    let (first, last) = match (by_code.first(), by_code.last()) {
        (Some(first), Some(last)) => (first.0, last.0),
        _ => (0, 0),
    };
    if widths.is_empty() {
        widths.push(0.0);
    }
    let font_matrix = descendant.font_matrix();
    doc.write_obj(font.font, |v| {
        v.dict(|d| {
            d.key("Type").name("Font");
            d.key("Subtype").name("Type3");
            d.key("FontBBox").array(|a| put_bounds(a, font_bbox));
            d.key("FontMatrix").array(|a| {
                for value in font_matrix.0 {
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
                e.key("Differences").array(|a| {
                    differences(
                        a,
                        by_code
                            .iter()
                            .zip(&procs)
                            .map(|(&(code, _), (name, _))| (code, name.as_slice())),
                    );
                });
            });
            d.key("FirstChar").int(i64::from(first));
            d.key("LastChar").int(i64::from(last));
            d.key("Widths").array(|a| {
                for &w in &widths {
                    a.real(w);
                }
            });
            d.key("Resources").dict(|_| {});
            if let Some(to_unicode) = to_unicode {
                d.key("ToUnicode").reference(to_unicode);
            }
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_group_consecutive_cids() {
        let mut doc = Document::new(Vec::new()).unwrap();
        let r = doc.alloc();
        doc.write_obj(r, |v| {
            v.array(|a| {
                put_widths(
                    a,
                    &[
                        (1, 500.0),
                        (2, 700.0),
                        (34, 500.0),
                        (35, 700.0),
                        (200, 300.0),
                    ],
                );
            });
        })
        .unwrap();
        let root = doc.alloc();
        doc.write_obj(root, |v| {
            v.dict(|d| {
                d.key("Type").name("Catalog");
            });
        })
        .unwrap();
        let bytes = doc.finish(root, None).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.contains("[ 1 [ 500 700 ] 34 [ 500 700 ] 200 [ 300 ] ]"),
            "{text}"
        );
    }

    #[test]
    fn unicode_comes_from_the_code_bytes_of_a_unicode_based_cmap() {
        use efterscript_fonts::testing::corpus_cid_cff;
        use std::rc::Rc;
        let program = Rc::new(corpus_cid_cff().program().unwrap());
        let spec = |unicode_based: bool| FontSpec::Composite {
            cmap_name: b"Syn-UCS2-H".to_vec(),
            wmode: 0,
            unicode_based,
            descendant: Box::new(FontSpec::Embedded {
                family: 1,
                kind: ProgramKind::Cff,
                font_name: b"SynCID".to_vec(),
                font_matrix: Matrix::scaling(0.001, 0.001),
                program: efterscript_graphics::ProgramRef(program.clone()),
                encoding: efterscript_graphics::glyph_names(&[]),
            }),
            cid_to_code: [
                (1, (0x41, 2)),
                (2, (0xD83D_DE00, 4)),
                (34, (0x42, 1)),
                (35, (0xD800, 2)),
            ]
            .into_iter()
            .collect(),
        };
        let unicode = spec(true);
        assert_eq!(unicode_of(&unicode, 1), Some(vec!['A']));
        assert_eq!(unicode_of(&unicode, 2), Some(vec!['\u{1F600}']));
        assert_eq!(unicode_of(&unicode, 34), Some(vec!['B']), "one byte padded");
        assert_eq!(unicode_of(&unicode, 35), None, "a lone surrogate");
        assert_eq!(unicode_of(&unicode, 200), None, "never shown");
        assert_eq!(unicode_of(&spec(false), 1), None, "no source for CFF");
        assert_eq!(cid_width(&unicode, 1), 500.0);
        assert_eq!(cid_width(&unicode, 2), 700.0);
    }
}
