// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Embed-all: with `EmbedAllFonts` in force, a resident face backed by
//! an outline asset is written as an embedded TrueType font (ISO
//! 32000-1 §9.6.3, §9.9) rather than as the unembedded standard font.
//! The glyphs come from the asset — each used code through the encoding
//! to a glyph name, then to a glyph index by `post` name or by the
//! name's Unicode value through the asset's cmap — subset and given a
//! symbolic `(3,0)` cmap as for a job's own TrueType font; the widths
//! stay the metrics' (the AFM), the one authority for advances the
//! resident set keeps. `BaseFont` is the asset's own name with a subset
//! tag, or untagged when the whole program is embedded. A face deferred
//! here while `EmbedAllFonts` no longer holds at the end is written
//! unembedded as usual.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use pdf_out::{Document, Filter};
use ps_fonts::{Program, ResidentFace, ResidentOutlines, TrueTypeProgram};
use ps_graphics::{FontSpec, GlyphNames, ProgramRef};
use ps_vm::Matrix;

use crate::embedded::{
    DEFAULT_STEM_V, Descriptor, EmbeddedFont, FIXED_PITCH, SERIF, SYMBOLIC, SimpleFont,
    embedded_name, spec_name, truetype_bbox, write_descriptor, write_font, write_simple_font,
    write_truetype_stream,
};
use crate::fonts::{widths, write_resident, write_to_unicode};

/// A face with its parsed TrueType asset.
struct Asset<'a> {
    base: ResidentFace,
    encoding: &'a GlyphNames,
    outlines: &'a ResidentOutlines,
    program: &'a TrueTypeProgram,
}

/// Writes a deferred resident font: from its asset when `embed_all`
/// holds and the asset is at hand, else unembedded.
pub(crate) fn write<W: Write>(
    doc: &mut Document<W>,
    font: &EmbeddedFont,
    filter: Filter,
    subset: bool,
    embed_all: bool,
) -> Result<(), pdf_out::Error> {
    let FontSpec::Resident { base, encoding } = &font.spec else {
        return Ok(());
    };
    let outlines = if embed_all { base.outlines() } else { None };
    let Some(outlines) = outlines else {
        return write_resident(doc, font.font, *base, encoding, filter);
    };
    match &**outlines.program() {
        Program::TrueType(program) => {
            let asset = Asset {
                base: *base,
                encoding,
                outlines: &outlines,
                program,
            };
            write_truetype(doc, font, &asset, filter, subset)
        }
        // A charstring asset is what the VM already describes as an
        // embedded font; the same path serves a build that did not.
        _ => write_font(
            doc,
            &EmbeddedFont {
                spec: FontSpec::Embedded {
                    family: 0,
                    kind: outlines.program().kind(),
                    font_name: outlines.font_name().to_vec(),
                    font_matrix: Matrix::scaling(0.001, 0.001),
                    program: ProgramRef(outlines.program().clone()),
                    encoding: encoding.clone(),
                },
                font: font.font,
                codes: font.codes.clone(),
            },
            filter,
            subset,
        ),
    }
}

/// The glyph indices the used codes draw and the code-to-index map for
/// the subset's cmap; a code without a glyph in the asset draws glyph 0.
fn glyphs(asset: &Asset<'_>, codes: &BTreeSet<u8>) -> (BTreeSet<u16>, BTreeMap<u8, u16>) {
    let mut gids = BTreeSet::new();
    let mut cmap = BTreeMap::new();
    for &code in codes {
        let gid = asset.encoding[usize::from(code)]
            .as_deref()
            .and_then(|name| asset.outlines.glyph_index(name))
            .unwrap_or(0);
        gids.insert(gid);
        cmap.insert(code, gid);
    }
    (gids, cmap)
}

fn write_truetype<W: Write>(
    doc: &mut Document<W>,
    font: &EmbeddedFont,
    asset: &Asset<'_>,
    filter: Filter,
    subset: bool,
) -> Result<(), pdf_out::Error> {
    let (mut gids, cmap) = glyphs(asset, &font.codes);
    if !subset {
        gids = (0..asset.program.num_glyphs()).collect();
    }
    let keep: BTreeSet<Vec<u8>> = gids.iter().map(|g| g.to_be_bytes().to_vec()).collect();
    let name = embedded_name(subset, asset.outlines.font_name(), &keep);
    let bytes = ps_fonts::truetype::write::subset(asset.program, &gids, &cmap)
        .unwrap_or_else(|_| asset.program.bytes().to_vec());
    let stream = write_truetype_stream(doc, &bytes, filter)?;
    let afm = asset.base.metrics().afm();
    let fixed = asset.program.is_fixed_pitch() || afm.is_some_and(|afm| afm.is_fixed_pitch);
    let descriptor = Descriptor {
        flags: SYMBOLIC
            | if fixed { FIXED_PITCH } else { 0 }
            | if asset.base.family().is_serif() {
                SERIF
            } else {
                0
            },
        bbox: truetype_bbox(asset.program),
        italic_angle: asset.program.italic_angle(),
        cap_height: afm.and_then(|afm| afm.cap_height),
        stem_v: afm.and_then(|afm| afm.std_vw).unwrap_or(DEFAULT_STEM_V),
    };
    let descriptor = write_descriptor(doc, &name, &descriptor, Some(("FontFile2", stream)))?;
    let metrics = widths(|code| {
        font.codes.contains(&code).then(|| {
            asset.encoding[usize::from(code)]
                .as_deref()
                .and_then(|name| std::str::from_utf8(name).ok())
                .and_then(|name| asset.base.width(name))
                .map_or(0.0, f32::from)
        })
    });
    let named = font
        .codes
        .iter()
        .filter_map(|&code| spec_name(&font.spec, code).map(|n| (code, n)));
    let to_unicode = write_to_unicode(doc, filter, named, |_| None)?;
    write_simple_font(
        doc,
        font.font,
        &SimpleFont {
            subtype: "TrueType",
            base_font: name,
            metrics,
            differing: Vec::new(),
            descriptor,
            to_unicode,
        },
    )
}
