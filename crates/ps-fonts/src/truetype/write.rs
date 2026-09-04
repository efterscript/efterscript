// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Writing TrueType tables and assembling them into a program: what the
//! synthesised test fonts and the subsetter share.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    ARG_1_AND_2_ARE_WORDS, MORE_COMPONENTS, TrueTypeProgram, WE_HAVE_A_SCALE, WE_HAVE_A_TWO_BY_TWO,
    WE_HAVE_AN_X_AND_Y_SCALE,
};
use crate::mac_glyphs::MAC_GLYPH_NAMES;
use crate::program::FontError;

/// One table to assemble.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub tag: [u8; 4],
    pub data: Vec<u8>,
}

impl Table {
    pub fn new(tag: &[u8; 4], data: Vec<u8>) -> Self {
        Table { tag: *tag, data }
    }
}

fn pad4(data: &mut Vec<u8>) {
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
}

/// The table checksum: the sum of the big-endian words, zero-padded.
pub fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// The directory and the tables as separate four-byte-padded pieces, in
/// tag order, with checksums filled in and `head`'s adjustment set:
/// concatenated they are the program, and a Type 42 `sfnts` array may
/// hold them as its strings.
pub fn assemble_parts(tables: Vec<Table>) -> Vec<Vec<u8>> {
    let mut tables = tables;
    tables.sort_by_key(|table| table.tag);
    let count = u16::try_from(tables.len()).expect("table count");
    let mut entry_selector = 0u16;
    while (2u16 << entry_selector) <= count {
        entry_selector += 1;
    }
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = count * 16 - search_range;

    let mut directory = Vec::with_capacity(12 + 16 * tables.len());
    directory.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    directory.extend_from_slice(&count.to_be_bytes());
    directory.extend_from_slice(&search_range.to_be_bytes());
    directory.extend_from_slice(&entry_selector.to_be_bytes());
    directory.extend_from_slice(&range_shift.to_be_bytes());
    let mut offset = 12 + 16 * tables.len();
    let mut parts = Vec::with_capacity(tables.len() + 1);
    let mut head_index = None;
    for (k, table) in tables.into_iter().enumerate() {
        let length = table.data.len();
        let mut data = table.data;
        directory.extend_from_slice(&table.tag);
        directory.extend_from_slice(&checksum(&data).to_be_bytes());
        directory.extend_from_slice(&(offset as u32).to_be_bytes());
        directory.extend_from_slice(&(length as u32).to_be_bytes());
        pad4(&mut data);
        offset += data.len();
        if &table.tag == b"head" {
            head_index = Some(k);
        }
        parts.push(data);
    }
    if let Some(k) = head_index {
        let total = parts.iter().fold(checksum(&directory), |sum, part| {
            sum.wrapping_add(checksum(part))
        });
        let adjustment = 0xB1B0_AFBAu32.wrapping_sub(total);
        parts[k][8..12].copy_from_slice(&adjustment.to_be_bytes());
    }
    parts.insert(0, directory);
    parts
}

/// The assembled program.
pub fn assemble(tables: Vec<Table>) -> Vec<u8> {
    assemble_parts(tables).concat()
}

/// What `head` records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Head {
    pub units_per_em: u16,
    pub bbox: [i16; 4],
    pub long_loca: bool,
}

pub fn head(h: &Head) -> Vec<u8> {
    let mut out = Vec::with_capacity(54);
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    out.extend_from_slice(&0x000Bu16.to_be_bytes());
    out.extend_from_slice(&h.units_per_em.to_be_bytes());
    out.extend_from_slice(&[0u8; 16]);
    for v in h.bbox {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&8u16.to_be_bytes());
    out.extend_from_slice(&2i16.to_be_bytes());
    out.extend_from_slice(&i16::from(h.long_loca).to_be_bytes());
    out.extend_from_slice(&0i16.to_be_bytes());
    out
}

/// What `hhea` records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hhea {
    pub ascender: i16,
    pub descender: i16,
    pub line_gap: i16,
    pub advance_width_max: u16,
    pub min_left_sidebearing: i16,
    pub min_right_sidebearing: i16,
    pub x_max_extent: i16,
    pub num_hmetrics: u16,
}

pub fn hhea(h: &Hhea) -> Vec<u8> {
    let mut out = Vec::with_capacity(36);
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    out.extend_from_slice(&h.ascender.to_be_bytes());
    out.extend_from_slice(&h.descender.to_be_bytes());
    out.extend_from_slice(&h.line_gap.to_be_bytes());
    out.extend_from_slice(&h.advance_width_max.to_be_bytes());
    out.extend_from_slice(&h.min_left_sidebearing.to_be_bytes());
    out.extend_from_slice(&h.min_right_sidebearing.to_be_bytes());
    out.extend_from_slice(&h.x_max_extent.to_be_bytes());
    out.extend_from_slice(&1i16.to_be_bytes());
    out.extend_from_slice(&0i16.to_be_bytes());
    out.extend_from_slice(&0i16.to_be_bytes());
    out.extend_from_slice(&[0u8; 8]);
    out.extend_from_slice(&0i16.to_be_bytes());
    out.extend_from_slice(&h.num_hmetrics.to_be_bytes());
    out
}

/// What `maxp` (version 1.0) records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Maxp {
    pub num_glyphs: u16,
    pub max_points: u16,
    pub max_contours: u16,
    pub max_composite_points: u16,
    pub max_composite_contours: u16,
    pub max_component_elements: u16,
    pub max_component_depth: u16,
}

pub fn maxp(m: &Maxp) -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    for v in [
        m.num_glyphs,
        m.max_points,
        m.max_contours,
        m.max_composite_points,
        m.max_composite_contours,
        2,
        0,
        0,
        0,
        0,
        0,
        0,
        m.max_component_elements,
        m.max_component_depth,
    ] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    out
}

/// `hmtx` with one full metric per glyph.
pub fn hmtx(metrics: &[(u16, i16)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(metrics.len() * 4);
    for &(advance, lsb) in metrics {
        out.extend_from_slice(&advance.to_be_bytes());
        out.extend_from_slice(&lsb.to_be_bytes());
    }
    out
}

/// `loca` and `glyf` for the glyph records in index order, each padded
/// to four bytes; the short `loca` format when the offsets allow it.
/// Returns `(loca, glyf, long_loca)`.
pub fn loca_and_glyf(glyphs: &[Vec<u8>]) -> (Vec<u8>, Vec<u8>, bool) {
    let mut glyf = Vec::new();
    let mut offsets = Vec::with_capacity(glyphs.len() + 1);
    for glyph in glyphs {
        offsets.push(glyf.len());
        glyf.extend_from_slice(glyph);
        pad4(&mut glyf);
    }
    offsets.push(glyf.len());
    let long = glyf.len() / 2 > usize::from(u16::MAX);
    let mut loca = Vec::new();
    for offset in offsets {
        if long {
            loca.extend_from_slice(&(offset as u32).to_be_bytes());
        } else {
            loca.extend_from_slice(&((offset / 2) as u16).to_be_bytes());
        }
    }
    (loca, glyf, long)
}

/// The control box of `(x, y, on_curve)` contours in font units.
pub fn contours_bbox(contours: &[Vec<(i16, i16, bool)>]) -> [i16; 4] {
    let mut bbox: Option<[i16; 4]> = None;
    for &(x, y, _) in contours.iter().flatten() {
        bbox = Some(match bbox {
            None => [x, y, x, y],
            Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
        });
    }
    bbox.unwrap_or([0; 4])
}

/// A simple glyph record from `(x, y, on_curve)` contours; empty for no
/// contours. Coordinates are written as words throughout.
pub fn simple_glyph(contours: &[Vec<(i16, i16, bool)>]) -> Vec<u8> {
    let contours: Vec<&Vec<(i16, i16, bool)>> = contours.iter().filter(|c| !c.is_empty()).collect();
    if contours.is_empty() {
        return Vec::new();
    }
    let bbox = contours_bbox(&contours.iter().map(|c| (*c).clone()).collect::<Vec<_>>());
    let mut out = Vec::new();
    out.extend_from_slice(&(contours.len() as i16).to_be_bytes());
    for v in bbox {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let mut end = 0usize;
    for contour in &contours {
        end += contour.len();
        out.extend_from_slice(&((end - 1) as u16).to_be_bytes());
    }
    out.extend_from_slice(&0u16.to_be_bytes());
    let points: Vec<(i16, i16, bool)> = contours.iter().flat_map(|c| c.iter().copied()).collect();
    for &(_, _, on) in &points {
        out.push(if on { 0x01 } else { 0x00 });
    }
    let mut prev = 0i16;
    for &(x, _, _) in &points {
        out.extend_from_slice(&x.wrapping_sub(prev).to_be_bytes());
        prev = x;
    }
    prev = 0;
    for &(_, y, _) in &points {
        out.extend_from_slice(&y.wrapping_sub(prev).to_be_bytes());
        prev = y;
    }
    out
}

fn post_header(version: u32, italic_angle: f32, fixed_pitch: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    out.extend_from_slice(&version.to_be_bytes());
    out.extend_from_slice(&((italic_angle * 65536.0) as i32).to_be_bytes());
    out.extend_from_slice(&0i16.to_be_bytes());
    out.extend_from_slice(&0i16.to_be_bytes());
    out.extend_from_slice(&u32::from(fixed_pitch).to_be_bytes());
    out.extend_from_slice(&[0u8; 16]);
    out
}

/// A `post` table without glyph names.
pub fn post_format3(italic_angle: f32, fixed_pitch: bool) -> Vec<u8> {
    post_header(0x0003_0000, italic_angle, fixed_pitch)
}

/// A `post` table naming every glyph, standard names by index and the
/// rest as strings.
pub fn post_format2(names: &[&[u8]], italic_angle: f32, fixed_pitch: bool) -> Vec<u8> {
    let mut out = post_header(0x0002_0000, italic_angle, fixed_pitch);
    out.extend_from_slice(&(names.len() as u16).to_be_bytes());
    let mut strings = Vec::new();
    let mut extra = 0u16;
    for name in names {
        let index = match MAC_GLYPH_NAMES.iter().position(|n| n.as_bytes() == *name) {
            Some(k) => k as u16,
            None => {
                strings.push(*name);
                extra += 1;
                258 + extra - 1
            }
        };
        out.extend_from_slice(&index.to_be_bytes());
    }
    for name in strings {
        let name = &name[..name.len().min(255)];
        out.push(name.len() as u8);
        out.extend_from_slice(name);
    }
    out
}

/// A format 4 subtable body for codes up to `0xFFFE`.
fn cmap_format4(map: &BTreeMap<u32, u16>) -> Vec<u8> {
    let mut runs: Vec<(u16, u16, Vec<u16>)> = Vec::new();
    for (&code, &gid) in map {
        let Ok(code) = u16::try_from(code) else {
            continue;
        };
        if code == 0xFFFF {
            continue;
        }
        match runs.last_mut() {
            Some((_, end, gids)) if *end + 1 == code => {
                *end = code;
                gids.push(gid);
            }
            _ => runs.push((code, code, vec![gid])),
        }
    }
    let seg_count = runs.len() + 1;
    let mut entry_selector = 0u16;
    while (2usize << entry_selector) <= seg_count {
        entry_selector += 1;
    }
    let search_range = (1u16 << entry_selector) * 2;
    let range_shift = seg_count as u16 * 2 - search_range;
    let mut ends = Vec::new();
    let mut starts = Vec::new();
    let mut deltas = Vec::new();
    let mut range_offsets = Vec::new();
    let mut glyph_ids: Vec<u16> = Vec::new();
    for (k, (start, end, gids)) in runs.iter().enumerate() {
        ends.push(*end);
        starts.push(*start);
        deltas.push(0u16);
        let offset = 2 * (seg_count - k) + 2 * glyph_ids.len();
        range_offsets.push(offset as u16);
        glyph_ids.extend_from_slice(gids);
    }
    ends.push(0xFFFF);
    starts.push(0xFFFF);
    deltas.push(1);
    range_offsets.push(0);
    let length = 16 + seg_count * 8 + glyph_ids.len() * 2;
    let mut out = Vec::with_capacity(length);
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&(length as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&((seg_count * 2) as u16).to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&entry_selector.to_be_bytes());
    out.extend_from_slice(&range_shift.to_be_bytes());
    let words = |out: &mut Vec<u8>, list: &[u16]| {
        for v in list {
            out.extend_from_slice(&v.to_be_bytes());
        }
    };
    words(&mut out, &ends);
    out.extend_from_slice(&0u16.to_be_bytes());
    words(&mut out, &starts);
    words(&mut out, &deltas);
    words(&mut out, &range_offsets);
    for gid in glyph_ids {
        out.extend_from_slice(&gid.to_be_bytes());
    }
    out
}

/// A `cmap` table with one format 4 subtable per `(platform, encoding,
/// map)`.
pub fn cmap(subtables: &[(u16, u16, &BTreeMap<u32, u16>)]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(subtables.len() as u16).to_be_bytes());
    let mut bodies = Vec::new();
    let mut offset = 4 + 8 * subtables.len();
    for &(platform, encoding, map) in subtables {
        let body = cmap_format4(map);
        out.extend_from_slice(&platform.to_be_bytes());
        out.extend_from_slice(&encoding.to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        offset += body.len();
        bodies.push(body);
    }
    for body in bodies {
        out.extend_from_slice(&body);
    }
    out
}

/// A `name` table with the family, subfamily, full, and PostScript names
/// as platform 3 (UTF-16) records.
pub fn name(family: &str, subfamily: &str, postscript_name: &str) -> Vec<u8> {
    let full = format!("{family} {subfamily}");
    let records: [(u16, &str); 4] = [
        (1, family),
        (2, subfamily),
        (4, &full),
        (6, postscript_name),
    ];
    let mut strings = Vec::new();
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&(records.len() as u16).to_be_bytes());
    out.extend_from_slice(&((6 + 12 * records.len()) as u16).to_be_bytes());
    for (id, text) in records {
        let encoded: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for v in [
            3u16,
            1,
            0x0409,
            id,
            encoded.len() as u16,
            strings.len() as u16,
        ] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        strings.extend_from_slice(&encoded);
    }
    out.extend_from_slice(&strings);
    out
}

// --- subsetting ---------------------------------------------------------------------

/// The glyphs a subset keeps: glyph 0, `used`, and every component a
/// kept composite glyph refers to.
fn closure(program: &TrueTypeProgram, used: &BTreeSet<u16>) -> Result<BTreeSet<u16>, FontError> {
    let mut keep = BTreeSet::new();
    let mut pending: Vec<u16> = used.iter().copied().collect();
    pending.push(0);
    while let Some(gid) = pending.pop() {
        if gid >= program.num_glyphs() || !keep.insert(gid) {
            continue;
        }
        pending.extend(program.components(gid)?);
    }
    Ok(keep)
}

/// A composite glyph record with its component indices renumbered.
fn renumbered_composite(data: &[u8], map: &BTreeMap<u16, u16>) -> Result<Vec<u8>, FontError> {
    let mut out = data.to_vec();
    let mut at = 10;
    loop {
        let word = |at: usize| -> Result<u16, FontError> {
            data.get(at..at + 2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .ok_or(FontError::Truncated("glyf"))
        };
        let flags = word(at)?;
        let component = word(at + 2)?;
        let new = *map
            .get(&component)
            .ok_or(FontError::GlyphIndex(component))?;
        out[at + 2..at + 4].copy_from_slice(&new.to_be_bytes());
        at += 4;
        at += if flags & ARG_1_AND_2_ARE_WORDS != 0 {
            4
        } else {
            2
        };
        if flags & WE_HAVE_A_SCALE != 0 {
            at += 2;
        } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            at += 4;
        } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
            at += 8;
        }
        if flags & MORE_COMPONENTS == 0 {
            return Ok(out);
        }
    }
}

/// A program restricted to the glyphs in `used` (plus glyph 0 and every
/// component they need), densely renumbered: `glyf`, `loca`, `hmtx`,
/// `hhea`, `maxp`, and `head` rewritten, `post` without names, a `cmap`
/// with one `(3,0)` subtable mapping each code in `codes` — by its bare
/// value and in the `0xF0xx` range — to its glyph, and `cvt `, `fpgm`,
/// and `prep` copied when present.
pub fn subset(
    program: &TrueTypeProgram,
    used: &BTreeSet<u16>,
    codes: &BTreeMap<u8, u16>,
) -> Result<Vec<u8>, FontError> {
    subset_with_map(program, used, codes).map(|(bytes, _)| bytes)
}

/// [`subset`] together with the old-to-new glyph index map of every
/// kept glyph, which a CID-keyed embedding needs for its CID-to-glyph
/// map.
pub fn subset_with_map(
    program: &TrueTypeProgram,
    used: &BTreeSet<u16>,
    codes: &BTreeMap<u8, u16>,
) -> Result<(Vec<u8>, BTreeMap<u16, u16>), FontError> {
    let keep = closure(program, used)?;
    let map: BTreeMap<u16, u16> = keep
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new as u16))
        .collect();
    let mut records = Vec::with_capacity(keep.len());
    let mut metrics = Vec::with_capacity(keep.len());
    for &old in &keep {
        let data = program.glyph_data(old)?;
        let composite = data.len() >= 10 && i16::from_be_bytes([data[0], data[1]]) < 0;
        records.push(if composite {
            renumbered_composite(data, &map)?
        } else {
            data.to_vec()
        });
        metrics.push((program.advance(old)?, program.left_sidebearing(old)?));
    }
    let (loca, glyf, long_loca) = loca_and_glyf(&records);
    let required = |tag: &[u8; 4]| program.table(tag).ok_or(FontError::MissingTable(*tag));
    let mut head = required(b"head")?.to_vec();
    if head.len() < 54 {
        return Err(FontError::Truncated("head"));
    }
    head[50..52].copy_from_slice(&i16::from(long_loca).to_be_bytes());
    let mut hhea = required(b"hhea")?.to_vec();
    if hhea.len() < 36 {
        return Err(FontError::Truncated("hhea"));
    }
    hhea[34..36].copy_from_slice(&(keep.len() as u16).to_be_bytes());
    let mut maxp = required(b"maxp")?.to_vec();
    if maxp.len() < 6 {
        return Err(FontError::Truncated("maxp"));
    }
    maxp[4..6].copy_from_slice(&(keep.len() as u16).to_be_bytes());
    let mut cmap_entries = BTreeMap::new();
    for (&code, old) in codes {
        if let Some(&new) = map.get(old) {
            cmap_entries.insert(u32::from(code), new);
            cmap_entries.insert(0xF000 + u32::from(code), new);
        }
    }
    let mut tables = vec![
        Table::new(b"head", head),
        Table::new(b"hhea", hhea),
        Table::new(b"maxp", maxp),
        Table::new(b"hmtx", hmtx(&metrics)),
        Table::new(b"loca", loca),
        Table::new(b"glyf", glyf),
        Table::new(
            b"post",
            post_format3(program.italic_angle(), program.is_fixed_pitch()),
        ),
        Table::new(b"cmap", cmap(&[(3, 0, &cmap_entries)])),
    ];
    for tag in [b"cvt ", b"fpgm", b"prep"] {
        if let Some(data) = program.table(tag) {
            tables.push(Table::new(tag, data.to_vec()));
        }
    }
    Ok((assemble(tables), map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TrueTypeFont;

    fn font() -> TrueTypeFont {
        TrueTypeFont::new(1000)
            .glyph(
                "A",
                600,
                vec![vec![(0, 0, true), (500, 0, true), (250, 700, true)]],
            )
            .glyph(
                "B",
                550,
                vec![vec![(0, 0, true), (0, 700, true), (400, 350, true)]],
            )
            .glyph(
                "C",
                500,
                vec![vec![(0, 0, true), (100, 0, true), (100, 100, true)]],
            )
            .map(65, 1)
            .map(66, 2)
            .map(67, 3)
    }

    /// A composite of `component` offset by `(dx, dy)`, as a glyph record.
    fn composite(component: u16, dx: i16, dy: i16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(-1i16).to_be_bytes());
        out.extend_from_slice(&[0u8; 8]);
        out.extend_from_slice(&(ARG_1_AND_2_ARE_WORDS | 0x0002).to_be_bytes());
        out.extend_from_slice(&component.to_be_bytes());
        out.extend_from_slice(&dx.to_be_bytes());
        out.extend_from_slice(&dy.to_be_bytes());
        out
    }

    #[test]
    fn a_subset_keeps_the_used_glyphs_renumbered_with_a_symbolic_cmap() {
        let crate::Program::TrueType(program) = font().program().unwrap() else {
            unreachable!()
        };
        let used: BTreeSet<u16> = [3u16].into_iter().collect();
        let codes: BTreeMap<u8, u16> = [(67u8, 3u16), (68, 9)].into_iter().collect();
        let bytes = subset(&program, &used, &codes).unwrap();
        let sub = TrueTypeProgram::parse(bytes).unwrap();
        assert_eq!(sub.num_glyphs(), 2);
        assert_eq!(sub.advance(0).unwrap(), 500);
        assert_eq!(sub.advance(1).unwrap(), 500);
        assert_eq!(sub.left_sidebearing(1).unwrap(), 0);
        assert_eq!(sub.glyph_data(1).unwrap(), program.glyph_data(3).unwrap());
        assert_eq!(sub.units_per_em(), 1000);
        assert_eq!(sub.bbox(), program.bbox());
        let cmap = sub.cmap(3, 0).unwrap().unwrap();
        assert_eq!(
            cmap.into_iter().collect::<Vec<_>>(),
            vec![(67, 1), (0xF043, 1)]
        );
        assert_eq!(sub.post_name(1), None);
        assert_eq!(sub.table_tags().len(), 8);
        assert_eq!(sub.glyph_by_index(1).unwrap().outline.ops.len(), 5);
    }

    #[test]
    fn composites_bring_their_components_and_are_renumbered() {
        let mut font = font();
        font.glyphs.push(crate::testing::TtGlyph {
            name: "D".to_string(),
            advance: 600,
            contours: Vec::new(),
        });
        let mut tables = font.tables();
        // Replace the empty record of glyph 4 by a composite of glyph 3.
        let mut records: Vec<Vec<u8>> = (0..4)
            .map(|k| {
                let crate::Program::TrueType(p) = font.program().unwrap() else {
                    unreachable!()
                };
                p.glyph_data(k).unwrap().to_vec()
            })
            .collect();
        records.push(composite(3, 20, 30));
        let (loca, glyf, _) = loca_and_glyf(&records);
        for table in &mut tables {
            match &table.tag {
                b"loca" => table.data = loca.clone(),
                b"glyf" => table.data = glyf.clone(),
                _ => {}
            }
        }
        let program = TrueTypeProgram::parse(assemble(tables)).unwrap();
        assert_eq!(program.components(4).unwrap(), vec![3]);
        let used: BTreeSet<u16> = [4u16].into_iter().collect();
        let codes: BTreeMap<u8, u16> = [(68u8, 4u16)].into_iter().collect();
        let sub = TrueTypeProgram::parse(subset(&program, &used, &codes).unwrap()).unwrap();
        assert_eq!(sub.num_glyphs(), 3);
        assert_eq!(sub.components(2).unwrap(), vec![1]);
        assert_eq!(sub.advance(2).unwrap(), 600);
        let original = program.outline(4).unwrap();
        assert_eq!(sub.outline(2).unwrap(), original);
        assert_eq!(sub.cmap(3, 0).unwrap().unwrap().get(&68), Some(&2));
        assert_eq!(
            subset(&program, &[9u16].into_iter().collect(), &BTreeMap::new())
                .map(|b| TrueTypeProgram::parse(b).unwrap().num_glyphs()),
            Ok(1)
        );
        assert_eq!(
            renumbered_composite(&composite(7, 0, 0), &BTreeMap::new()),
            Err(FontError::GlyphIndex(7))
        );
    }

    #[test]
    fn checksums_pad_and_wrap() {
        assert_eq!(checksum(&[0, 0, 0, 1, 0, 0, 0, 2]), 3);
        assert_eq!(checksum(&[1]), 0x0100_0000);
        assert_eq!(checksum(&[0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 2]), 1);
    }

    #[test]
    fn assembled_directory_is_sorted_and_padded() {
        let parts = assemble_parts(vec![
            Table::new(b"zzzz", vec![1, 2, 3]),
            Table::new(b"aaaa", vec![9]),
        ]);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].len(), 12 + 32);
        assert_eq!(&parts[0][12..16], b"aaaa");
        assert_eq!(&parts[0][28..32], b"zzzz");
        assert_eq!(parts[1], vec![9, 0, 0, 0]);
        assert_eq!(parts[2], vec![1, 2, 3, 0]);
        let bytes = assemble(vec![Table::new(b"aaaa", vec![9])]);
        assert_eq!(bytes.len(), 12 + 16 + 4);
        assert_eq!(&bytes[4..6], &[0, 1]);
    }

    #[test]
    fn head_adjustment_makes_the_whole_sum_to_the_magic() {
        let h = head(&Head {
            units_per_em: 1000,
            bbox: [0, 0, 1, 1],
            long_loca: false,
        });
        assert_eq!(h.len(), 54);
        let bytes = assemble(vec![
            Table::new(b"head", h),
            Table::new(b"maxp", vec![0; 6]),
        ]);
        assert_eq!(checksum(&bytes), 0xB1B0_AFBA);
    }

    #[test]
    fn simple_glyph_records_and_loca() {
        let square = simple_glyph(&[vec![(0, 0, true), (10, 0, true), (10, 10, true)]]);
        assert_eq!(&square[..2], &[0, 1]);
        assert_eq!(square.len(), 10 + 2 + 2 + 3 + 6 + 6);
        assert!(simple_glyph(&[]).is_empty());
        let (loca, glyf, long) = loca_and_glyf(&[Vec::new(), square.clone()]);
        assert!(!long);
        assert_eq!(loca, vec![0, 0, 0, 0, 0, 16]);
        assert_eq!(glyf.len(), 32);
        assert_eq!(
            contours_bbox(&[vec![(3, -1, true), (-2, 5, false)]]),
            [-2, -1, 3, 5]
        );
    }

    #[test]
    fn post_names_use_the_standard_indices() {
        let post = post_format2(&[b".notdef", b"A", b"custom"], 0.0, false);
        assert_eq!(&post[..4], &[0, 2, 0, 0]);
        assert_eq!(&post[32..34], &[0, 3]);
        assert_eq!(&post[34..40], &[0, 0, 0, 36, 1, 2]);
        assert_eq!(&post[40..], b"\x06custom");
        assert_eq!(post_format3(-12.0, true).len(), 32);
        assert_eq!(
            &post_format3(-12.0, true)[4..8],
            &(-12i32 * 65536).to_be_bytes()
        );
    }

    #[test]
    fn cmap_format4_round_trips_through_the_parser() {
        let map: BTreeMap<u32, u16> = [(65u32, 1u16), (66, 2), (70, 7), (0x10000, 9)]
            .into_iter()
            .collect();
        let table = cmap(&[(3, 0, &map), (1, 0, &map)]);
        assert_eq!(&table[..4], &[0, 0, 0, 2]);
        let mut r = crate::truetype::Reader::at(&table, 4 + 8 * 2, "cmap");
        let format = r.u16().unwrap();
        assert_eq!(format, 4);
        let program = crate::testing::TrueTypeFont::new(1000)
            .glyph("A", 500, vec![])
            .map(65, 1)
            .map(66, 1)
            .map(70, 1)
            .program()
            .unwrap();
        let crate::Program::TrueType(tt) = program else {
            unreachable!()
        };
        let parsed = tt.cmap(3, 0).unwrap().unwrap();
        assert_eq!(
            parsed.into_iter().collect::<Vec<_>>(),
            vec![(65, 1), (66, 1), (70, 1)]
        );
        assert_eq!(tt.cmap(3, 1).unwrap(), None);
    }

    #[test]
    fn name_table_lists_four_records() {
        let n = name("Syn", "Regular", "Syn-Regular");
        assert_eq!(&n[..6], &[0, 0, 0, 4, 0, 54]);
        assert_eq!(n.len(), 54 + 2 * (3 + 7 + 11 + 11));
    }
}
