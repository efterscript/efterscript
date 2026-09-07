// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Embedded fonts (ISO 32000-1 §9.6.2, §9.6.3, §9.8, §9.9). A Type 1,
//! FontType 2, or Type 42 font the job defined becomes a `Type1` or
//! `TrueType` font dictionary whose descriptor embeds the program
//! restricted to the glyphs the document showed: a Type 1 program
//! regenerated from the snapshot as `FontFile` with its three lengths,
//! a CFF program subset as `FontFile3` of subtype `Type1C`, a TrueType
//! program rewritten with a symbolic `(3,0)` cmap as `FontFile2`. The glyph set
//! is only known when the document ends, so the codes each font shows
//! are collected page by page and the objects are written at `finish`;
//! the font's object id is allocated at first use so pages can refer to
//! it. One font object serves one snapshot with one encoding, since the
//! TrueType cmap maps codes and two encodings could disagree on a code.
//!
//! Widths are in thousandths of text space: each glyph's displacement
//! taken through the font matrix, so a Type 1 program with an unusual
//! matrix and a Type 42 program with its unit-em glyph space both come
//! out right. Subset names carry a six-letter tag derived from the font
//! name and the glyph set, so the output stays deterministic. Composite
//! fonts share the table and its timing and are written by `composite`;
//! resident faces deferred for `EmbedAllFonts` by `embed_all`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use pdf_out::{Document, Filter, Ref};
use ps_fonts::type1::write::{Header, subset_names};
use ps_fonts::{
    Program, ProgramKind, STANDARD_ENCODING, TrueTypeProgram, Type1Program, cff, type1,
};
use ps_graphics::{FontSpec, GlyphNames, IrOp, Op, Page};
use ps_vm::Matrix;

use crate::composite::CompositeFont;
use crate::content::Recode;
use crate::fonts::{differences, put_bounds, widths, write_to_unicode};

/// `StemV` when the program does not say (TrueType always; Type 1
/// without `StdVW`).
pub(crate) const DEFAULT_STEM_V: f32 = 80.0;

/// Font descriptor flags (ISO 32000-1 Table 123).
pub(crate) const FIXED_PITCH: i64 = 1;
pub(crate) const SERIF: i64 = 1 << 1;
pub(crate) const SYMBOLIC: i64 = 1 << 2;
const NONSYMBOLIC: i64 = 1 << 5;
const ITALIC: i64 = 1 << 6;

/// A font written at the end: its resource, its object, and the codes
/// shown in it over every page.
pub(crate) struct EmbeddedFont {
    pub(crate) spec: FontSpec,
    pub(crate) font: Ref,
    pub(crate) codes: BTreeSet<u8>,
}

/// The embedded fonts used so far, one per snapshot and encoding, and
/// the composite fonts, one per CMap, writing mode, and descendant.
#[derive(Default)]
pub(crate) struct EmbeddedTable {
    fonts: Vec<EmbeddedFont>,
    composites: Vec<CompositeFont>,
}

/// The CIDs the page shows in font `index`, in its own operations and
/// in the glyph procedures of its Type 3 fonts; for a simple font the
/// CID is the code.
pub(crate) fn cids_used(page: &Page, index: usize) -> BTreeSet<u16> {
    fn collect(ops: &[Op], index: usize, into: &mut BTreeSet<u16>) {
        for op in ops {
            if let IrOp::Text { font, glyphs, .. } = &op.op
                && font.0 == index
            {
                into.extend(glyphs.iter().map(|g| g.cid));
            }
        }
    }
    let mut cids = BTreeSet::new();
    collect(&page.ops, index, &mut cids);
    for spec in &page.resources.fonts {
        if let FontSpec::Type3 { glyphs, .. } = spec {
            for glyph in glyphs.values() {
                collect(&glyph.ops, index, &mut cids);
            }
        }
    }
    cids
}

/// The one-byte codes the page shows in a simple font: a CID a byte
/// cannot hold selects code 0, as the VM's show has it.
fn codes_used(page: &Page, index: usize) -> BTreeSet<u8> {
    cids_used(page, index)
        .into_iter()
        .map(|cid| u8::try_from(cid).unwrap_or(0))
        .collect()
}

impl EmbeddedTable {
    /// The font object for the embedded or composite font at `index` of
    /// `page`'s resources, allocated on first use; the codes (or CIDs)
    /// the page shows in it join the font's set. A composite font
    /// written as a Type 3 fallback enters its one-byte codes for this
    /// page in `recode`.
    pub(crate) fn use_font<W: Write>(
        &mut self,
        doc: &mut Document<W>,
        page: &Page,
        index: usize,
        recode: &mut Recode,
        notes: &mut Vec<String>,
    ) -> Ref {
        let spec = &page.resources.fonts[index];
        if let FontSpec::Composite { .. } = spec {
            let cids = cids_used(page, index);
            let font = match self.composites.iter_mut().find(|f| f.spec.same_font(spec)) {
                Some(font) => font,
                None => {
                    let font = doc.alloc();
                    self.composites.push(CompositeFont::new(spec, font));
                    self.composites.last_mut().expect("just pushed")
                }
            };
            font.record(spec, cids, notes);
            if let Some(codes) = font.recode() {
                recode.insert(index, codes);
            }
            return font.font;
        }
        let codes = codes_used(page, index);
        if let Some(font) = self.fonts.iter_mut().find(|f| f.spec == *spec) {
            font.codes.extend(codes);
            return font.font;
        }
        let font = doc.alloc();
        self.fonts.push(EmbeddedFont {
            spec: spec.clone(),
            font,
            codes,
        });
        font
    }

    /// Writes every font used: dictionary, descriptor, program stream,
    /// and ToUnicode CMap. Without `subset` a simple font's program is
    /// embedded whole under its own name; composite fonts are always
    /// subset, as the parameters reference has it for CID fonts. A
    /// resident face deferred here is embedded from its asset when
    /// `embed_all` still holds, else written unembedded as usual.
    pub(crate) fn write_all<W: Write>(
        self,
        doc: &mut Document<W>,
        filter: Filter,
        subset: bool,
        embed_all: bool,
    ) -> Result<(), pdf_out::Error> {
        for font in self.fonts {
            match &font.spec {
                FontSpec::Resident { .. } => {
                    crate::embed_all::write(doc, &font, filter, subset, embed_all)?;
                }
                _ => write_font(doc, &font, filter, subset)?,
            }
        }
        for font in self.composites {
            crate::composite::write_font(doc, &font, filter)?;
        }
        Ok(())
    }
}

// --- shared pieces --------------------------------------------------------------------

/// Six upper-case letters from a hash of the font name and the glyph
/// set, so the same subset of the same font gets the same tag.
pub(crate) fn subset_tag(font_name: &[u8], glyphs: &BTreeSet<Vec<u8>>) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes.iter().chain(std::iter::once(&0u8)) {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    feed(font_name);
    for glyph in glyphs {
        feed(glyph);
    }
    (0..6)
        .map(|_| {
            let letter = char::from(b'A' + (hash % 26) as u8);
            hash /= 26;
            letter
        })
        .collect()
}

pub(crate) fn tagged(tag: &str, font_name: &[u8]) -> Vec<u8> {
    let mut name = format!("{tag}+").into_bytes();
    name.extend_from_slice(font_name);
    name
}

fn is_standard(encoding: &GlyphNames) -> bool {
    encoding
        .iter()
        .zip(STANDARD_ENCODING.iter())
        .all(|(name, standard)| name.as_deref() == standard.map(str::as_bytes))
}

/// A width in thousandths of text space: the glyph-space advance through
/// the font matrix, rounded to a thousandth to shed single-precision
/// residue.
fn pdf_width(spec: &FontSpec, code: u8) -> f32 {
    let (wx, _) = spec.width(code);
    let a = spec.font_matrix().0[0];
    ((f64::from(wx) * f64::from(a) * 1_000_000.0).round() / 1000.0) as f32
}

/// A glyph-space box through the font matrix, in thousandths.
pub(crate) fn pdf_bbox(bbox: [f32; 4], matrix: Matrix) -> [f32; 4] {
    let [llx, lly, urx, ury] = bbox;
    let corners = [(llx, lly), (urx, lly), (llx, ury), (urx, ury)];
    let mut out = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for (x, y) in corners {
        let p = matrix.apply_delta(ps_vm::Point::new(x, y));
        out[0] = out[0].min(p.x * 1000.0);
        out[1] = out[1].min(p.y * 1000.0);
        out[2] = out[2].max(p.x * 1000.0);
        out[3] = out[3].max(p.y * 1000.0);
    }
    out.map(|v| (f64::from(v) * 1000.0).round() as f32 / 1000.0)
}

pub(crate) struct Descriptor {
    pub flags: i64,
    pub bbox: [f32; 4],
    pub italic_angle: f32,
    pub cap_height: Option<f32>,
    pub stem_v: f32,
}

pub(crate) fn write_descriptor<W: Write>(
    doc: &mut Document<W>,
    name: &[u8],
    d: &Descriptor,
    program: Option<(&str, Ref)>,
) -> Result<Ref, pdf_out::Error> {
    let r = doc.alloc();
    let flags = if d.italic_angle != 0.0 {
        d.flags | ITALIC
    } else {
        d.flags
    };
    doc.write_obj(r, |v| {
        v.dict(|dict| {
            dict.key("Type").name("FontDescriptor");
            dict.key("FontName").name_bytes(name);
            dict.key("Flags").int(flags);
            dict.key("FontBBox").array(|a| put_bounds(a, d.bbox));
            dict.key("ItalicAngle").real(d.italic_angle);
            dict.key("Ascent").real(d.bbox[3]);
            dict.key("Descent").real(d.bbox[1]);
            if let Some(cap_height) = d.cap_height {
                dict.key("CapHeight").real(cap_height);
            }
            dict.key("StemV").real(d.stem_v);
            if let Some((key, stream)) = program {
                dict.key(key).reference(stream);
            }
        });
    })?;
    Ok(r)
}

/// The names the font's used codes select; a code without a name in the
/// program draws `.notdef`, which the subset keeps anyway.
fn used_names<'a>(spec: &'a FontSpec, codes: &BTreeSet<u8>) -> Vec<&'a [u8]> {
    codes
        .iter()
        .filter_map(|&code| spec.glyph_name(code).map(Vec::as_slice))
        .collect()
}

/// The font's name in the document: tagged for a subset, its own for a
/// whole program.
pub(crate) fn embedded_name(subset: bool, font_name: &[u8], keep: &BTreeSet<Vec<u8>>) -> Vec<u8> {
    if subset {
        tagged(&subset_tag(font_name, keep), font_name)
    } else {
        font_name.to_vec()
    }
}

/// A TrueType program as a `FontFile2` stream with its `Length1`.
pub(crate) fn write_truetype_stream<W: Write>(
    doc: &mut Document<W>,
    bytes: &[u8],
    filter: Filter,
) -> Result<Ref, pdf_out::Error> {
    let stream = doc.alloc();
    doc.write_stream(stream, filter, bytes, |d| {
        d.key("Length1").int(bytes.len() as i64);
    })?;
    Ok(stream)
}

/// A TrueType program's `head` box in thousandths of the em, rounded to
/// a thousandth.
pub(crate) fn truetype_bbox(program: &TrueTypeProgram) -> [f32; 4] {
    let em = f64::from(program.units_per_em());
    program
        .bbox()
        .map(|v| ((f64::from(v) * 1000.0 / em) * 1000.0).round() as f32 / 1000.0)
}

pub(crate) fn write_font<W: Write>(
    doc: &mut Document<W>,
    font: &EmbeddedFont,
    filter: Filter,
    subset: bool,
) -> Result<(), pdf_out::Error> {
    let FontSpec::Embedded {
        kind,
        font_name,
        font_matrix,
        program,
        encoding,
        ..
    } = &font.spec
    else {
        return Ok(());
    };
    let metrics = widths(|code| {
        font.codes
            .contains(&code)
            .then(|| pdf_width(&font.spec, code))
    });
    let (base_font, descriptor) = match &*program.0 {
        Program::Type1(type1) => {
            let keep = if subset {
                subset_names(type1, used_names(&font.spec, &font.codes))
            } else {
                type1.charstrings().keys().cloned().collect()
            };
            let name = embedded_name(subset, font_name, &keep);
            let stream =
                write_type1_program(doc, type1, &name, *font_matrix, encoding, &keep, filter)?;
            let dict = type1.dict();
            let flags = if is_standard(encoding) {
                NONSYMBOLIC
            } else {
                SYMBOLIC
            } | if dict.font_info_bool("isFixedPitch") == Some(true) {
                FIXED_PITCH
            } else {
                0
            };
            let descriptor = Descriptor {
                flags,
                bbox: pdf_bbox(dict.font_bbox, *font_matrix),
                italic_angle: dict.font_info_number("ItalicAngle").unwrap_or(0.0),
                cap_height: dict.font_info_number("CapHeight"),
                stem_v: dict.private_number("StdVW").unwrap_or(DEFAULT_STEM_V),
            };
            let descriptor = write_descriptor(doc, &name, &descriptor, Some(("FontFile", stream)))?;
            (name, descriptor)
        }
        Program::TrueType(truetype) => {
            let (mut gids, cmap) = truetype_glyphs(&font.spec, truetype, &font.codes);
            if !subset {
                gids = (0..truetype.num_glyphs()).collect();
            }
            let keep: BTreeSet<Vec<u8>> = gids.iter().map(|g| g.to_be_bytes().to_vec()).collect();
            let name = embedded_name(subset, font_name, &keep);
            let bytes = ps_fonts::truetype::write::subset(truetype, &gids, &cmap)
                .unwrap_or_else(|_| truetype.bytes().to_vec());
            let stream = write_truetype_stream(doc, &bytes, filter)?;
            let descriptor = Descriptor {
                flags: SYMBOLIC
                    | if truetype.is_fixed_pitch() {
                        FIXED_PITCH
                    } else {
                        0
                    },
                bbox: truetype_bbox(truetype),
                italic_angle: truetype.italic_angle(),
                cap_height: None,
                stem_v: DEFAULT_STEM_V,
            };
            let descriptor =
                write_descriptor(doc, &name, &descriptor, Some(("FontFile2", stream)))?;
            (name, descriptor)
        }
        Program::Cff(cff) => {
            let keep = if subset {
                cff::write::subset_names(cff, used_names(&font.spec, &font.codes))
            } else {
                cff.glyph_names().into_iter().map(<[u8]>::to_vec).collect()
            };
            let name = embedded_name(subset, font_name, &keep);
            // Only a CID-keyed program cannot be subset, and the VM
            // defines no font over one; it would be described unembedded.
            let stream = match cff::write::subset(cff, &name, &keep) {
                Ok(bytes) => {
                    let stream = doc.alloc();
                    doc.write_stream(stream, filter, &bytes, |d| {
                        d.key("Subtype").name("Type1C");
                    })?;
                    Some(("FontFile3", stream))
                }
                Err(_) => None,
            };
            let flags = if is_standard(encoding) {
                NONSYMBOLIC
            } else {
                SYMBOLIC
            } | if cff.is_fixed_pitch() { FIXED_PITCH } else { 0 };
            let descriptor = Descriptor {
                flags,
                bbox: pdf_bbox(cff.font_bbox(), *font_matrix),
                italic_angle: cff.italic_angle(),
                cap_height: None,
                stem_v: cff
                    .private()
                    .and_then(|p| p.number(cff::op::STD_VW))
                    .unwrap_or(DEFAULT_STEM_V),
            };
            let descriptor = write_descriptor(doc, &name, &descriptor, stream)?;
            (name, descriptor)
        }
        // A CID-keyed Type 1 program has no simple-font embedding form,
        // and the VM defines no simple font over one; the composite
        // change writes it through its own path.
        Program::Type1Cid(_) => {
            let name = embedded_name(subset, font_name, &BTreeSet::new());
            let descriptor = Descriptor {
                flags: SYMBOLIC,
                bbox: [0.0; 4],
                italic_angle: 0.0,
                cap_height: None,
                stem_v: DEFAULT_STEM_V,
            };
            let descriptor = write_descriptor(doc, &name, &descriptor, None)?;
            (name, descriptor)
        }
    };
    let named: Vec<(u8, &[u8])> = font
        .codes
        .iter()
        .filter_map(|&code| spec_name(&font.spec, code).map(|n| (code, n)))
        .collect();
    let to_unicode = match &*program.0 {
        Program::TrueType(truetype) => write_to_unicode(
            doc,
            filter,
            named.iter().map(|&(code, name)| (code, name)),
            |name| truetype_unicode(truetype, name),
        )?,
        _ => write_to_unicode(
            doc,
            filter,
            named.iter().map(|&(code, name)| (code, name)),
            |_| None,
        )?,
    };
    let differing: Vec<(u8, &[u8])> = match kind {
        ProgramKind::Type1 | ProgramKind::Cff | ProgramKind::Type1Cid => font
            .codes
            .iter()
            .filter(|&&code| {
                encoding[usize::from(code)].as_deref()
                    != STANDARD_ENCODING[usize::from(code)].map(str::as_bytes)
            })
            .map(|&code| {
                (
                    code,
                    encoding[usize::from(code)].as_deref().unwrap_or(b".notdef"),
                )
            })
            .collect(),
        ProgramKind::TrueType => Vec::new(),
    };
    write_simple_font(
        doc,
        font.font,
        &SimpleFont {
            subtype: match kind {
                ProgramKind::Type1 | ProgramKind::Cff | ProgramKind::Type1Cid => "Type1",
                ProgramKind::TrueType => "TrueType",
            },
            base_font,
            metrics,
            differing,
            descriptor,
            to_unicode,
        },
    )
}

/// The parts of a simple font dictionary (ISO 32000-1 §9.6.2) once its
/// descriptor and streams are written.
pub(crate) struct SimpleFont<'a> {
    pub subtype: &'static str,
    pub base_font: Vec<u8>,
    /// First code, last code, and the widths between; none when no code
    /// has one.
    pub metrics: Option<(u8, u8, Vec<f32>)>,
    /// `Differences` entries; none leaves the encoding out.
    pub differing: Vec<(u8, &'a [u8])>,
    pub descriptor: Ref,
    pub to_unicode: Option<Ref>,
}

pub(crate) fn write_simple_font<W: Write>(
    doc: &mut Document<W>,
    r: Ref,
    font: &SimpleFont<'_>,
) -> Result<(), pdf_out::Error> {
    doc.write_obj(r, |v| {
        v.dict(|d| {
            d.key("Type").name("Font");
            d.key("Subtype").name(font.subtype);
            d.key("BaseFont").name_bytes(&font.base_font);
            let (first, last, values) = font.metrics.clone().unwrap_or((0, 0, vec![0.0]));
            d.key("FirstChar").int(i64::from(first));
            d.key("LastChar").int(i64::from(last));
            d.key("Widths").array(|a| {
                for w in values {
                    a.real(w);
                }
            });
            if !font.differing.is_empty() {
                d.key("Encoding").dict(|e| {
                    e.key("Type").name("Encoding");
                    e.key("Differences")
                        .array(|a| differences(a, font.differing.iter().copied()));
                });
            }
            d.key("FontDescriptor").reference(font.descriptor);
            if let Some(to_unicode) = font.to_unicode {
                d.key("ToUnicode").reference(to_unicode);
            }
        });
    })
}

/// The encoding's name for `code`, none for a code without one.
pub(crate) fn spec_name(spec: &FontSpec, code: u8) -> Option<&[u8]> {
    spec.glyph_name(code).map(Vec::as_slice)
}

fn write_type1_program<W: Write>(
    doc: &mut Document<W>,
    program: &Type1Program,
    name: &[u8],
    font_matrix: Matrix,
    encoding: &GlyphNames,
    keep: &BTreeSet<Vec<u8>>,
    filter: Filter,
) -> Result<Ref, pdf_out::Error> {
    let header = Header {
        font_name: name,
        font_matrix: font_matrix.0,
        encoding: &encoding[..],
    };
    let written = type1::write::write(program, &header, keep);
    let stream = doc.alloc();
    doc.write_stream(stream, filter, &written.bytes, |d| {
        d.key("Length1").int(written.length1 as i64);
        d.key("Length2").int(written.length2 as i64);
        d.key("Length3").int(written.length3 as i64);
    })?;
    Ok(stream)
}

/// The glyph indices the used codes draw and the code-to-index map for
/// the subset's cmap; a code whose name the program lacks draws its
/// `.notdef` glyph.
fn truetype_glyphs(
    spec: &FontSpec,
    program: &TrueTypeProgram,
    codes: &BTreeSet<u8>,
) -> (BTreeSet<u16>, BTreeMap<u8, u16>) {
    let mut gids = BTreeSet::new();
    let mut cmap = BTreeMap::new();
    for &code in codes {
        let gid = spec_name(spec, code)
            .and_then(|name| program.gid(name))
            .or_else(|| program.gid(b".notdef"))
            .unwrap_or(0);
        gids.insert(gid);
        cmap.insert(code, gid);
    }
    (gids, cmap)
}

/// The character a TrueType glyph name stands for when the glyph list
/// does not know the name: the program's own Unicode cmap, read
/// backwards from the glyph the name selects.
fn truetype_unicode(program: &TrueTypeProgram, name: &[u8]) -> Option<Vec<char>> {
    truetype_gid_unicode(program, program.gid(name)?)
}

/// The character a TrueType glyph index stands for in the program's own
/// Unicode cmap, read backwards.
pub(crate) fn truetype_gid_unicode(program: &TrueTypeProgram, gid: u16) -> Option<Vec<char>> {
    let cmap = program.cmap(3, 1).ok().flatten()?;
    cmap.iter()
        .find(|(_, g)| **g == gid)
        .and_then(|(code, _)| char::from_u32(*code))
        .map(|c| vec![c])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_tags_are_six_letters_and_follow_the_glyph_set() {
        let a: BTreeSet<Vec<u8>> = [b"a".to_vec()].into_iter().collect();
        let ab: BTreeSet<Vec<u8>> = [b"a".to_vec(), b"b".to_vec()].into_iter().collect();
        let tag = subset_tag(b"Syn", &a);
        assert_eq!(tag.len(), 6);
        assert!(tag.bytes().all(|b| b.is_ascii_uppercase()));
        assert_eq!(tag, subset_tag(b"Syn", &a));
        assert_ne!(tag, subset_tag(b"Syn", &ab));
        assert_ne!(tag, subset_tag(b"Other", &a));
        assert_eq!(tagged("ABCDEF", b"Syn"), b"ABCDEF+Syn");
    }

    #[test]
    fn boxes_and_widths_go_through_the_font_matrix_in_thousandths() {
        let m = Matrix::scaling(0.001, 0.001);
        assert_eq!(
            pdf_bbox([0.0, -200.0, 1000.0, 900.0], m),
            [0.0, -200.0, 1000.0, 900.0]
        );
        let half = Matrix::scaling(0.0005, 0.0005);
        assert_eq!(
            pdf_bbox([0.0, -200.0, 2000.0, 1800.0], half),
            [0.0, -100.0, 1000.0, 900.0]
        );
        let flipped = Matrix([0.0, 0.001, -0.001, 0.0, 0.0, 0.0]);
        assert_eq!(
            pdf_bbox([0.0, 0.0, 100.0, 50.0], flipped),
            [-50.0, 0.0, 0.0, 100.0]
        );
    }
}
