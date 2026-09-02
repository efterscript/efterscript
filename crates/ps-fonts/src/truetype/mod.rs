// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! TrueType programs as a Type 42 font carries them: the table directory,
//! the metrics and glyph tables, `post` names, and `cmap` subtables, with
//! quadratic contours converted to cubic outlines.

pub mod write;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::mac_glyphs::MAC_GLYPH_NAMES;
use crate::outline::{Glyph, Outline};
use crate::program::FontError;

/// Composite glyphs nested deeper than this are a fault.
const MAX_COMPONENT_DEPTH: usize = 8;

const ON_CURVE: u8 = 0x01;
const X_SHORT: u8 = 0x02;
const Y_SHORT: u8 = 0x04;
const REPEAT: u8 = 0x08;
const X_SAME_OR_POSITIVE: u8 = 0x10;
const Y_SAME_OR_POSITIVE: u8 = 0x20;

pub(crate) const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
const ARGS_ARE_XY_VALUES: u16 = 0x0002;
pub(crate) const WE_HAVE_A_SCALE: u16 = 0x0008;
pub(crate) const MORE_COMPONENTS: u16 = 0x0020;
pub(crate) const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
pub(crate) const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;

/// Big-endian reads with bounds checks; every failure is a truncation of
/// the named table.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    what: &'static str,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], what: &'static str) -> Self {
        Reader {
            bytes,
            pos: 0,
            what,
        }
    }

    fn at(bytes: &'a [u8], pos: usize, what: &'static str) -> Self {
        Reader { bytes, pos, what }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], FontError> {
        let slice = self
            .bytes
            .get(self.pos..self.pos + n)
            .ok_or(FontError::Truncated(self.what))?;
        self.pos += n;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, FontError> {
        Ok(self.take(1)?[0])
    }

    fn i8(&mut self) -> Result<i8, FontError> {
        Ok(self.u8()? as i8)
    }

    fn u16(&mut self) -> Result<u16, FontError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn i16(&mut self) -> Result<i16, FontError> {
        Ok(self.u16()? as i16)
    }

    fn u32(&mut self) -> Result<u32, FontError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i32(&mut self) -> Result<i32, FontError> {
        Ok(self.u32()? as i32)
    }

    /// A 2.14 fixed-point number.
    fn f2dot14(&mut self) -> Result<f32, FontError> {
        Ok(f32::from(self.i16()?) / 16384.0)
    }

    fn skip(&mut self, n: usize) -> Result<(), FontError> {
        self.take(n).map(|_| ())
    }
}

/// A parsed TrueType program with its glyph cache.
pub struct TrueTypeProgram {
    bytes: Vec<u8>,
    tables: BTreeMap<[u8; 4], (usize, usize)>,
    units_per_em: u16,
    bbox: [i16; 4],
    long_loca: bool,
    num_glyphs: u16,
    num_hmetrics: u16,
    ascender: i16,
    descender: i16,
    italic_angle: f32,
    fixed_pitch: bool,
    post_names: Vec<Vec<u8>>,
    names: BTreeMap<Vec<u8>, u16>,
    cache: RefCell<HashMap<Vec<u8>, Rc<Glyph>>>,
}

impl std::fmt::Debug for TrueTypeProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrueTypeProgram")
            .field("bytes", &self.bytes.len())
            .field("tables", &self.tables.len())
            .field("units_per_em", &self.units_per_em)
            .field("num_glyphs", &self.num_glyphs)
            .field("names", &self.names.len())
            .finish()
    }
}

impl TrueTypeProgram {
    /// Parses the table directory and the tables every glyph lookup
    /// needs (`head`, `hhea`, `maxp`, `hmtx`, `loca`, `glyf`), plus `post`
    /// when present. Glyph names come from `post` alone until
    /// [`TrueTypeProgram::with_names`] supplies the font's `CharStrings`.
    pub fn parse(bytes: Vec<u8>) -> Result<Self, FontError> {
        let mut r = Reader::new(&bytes, "table directory");
        let tag = r.take(4)?;
        if tag == b"ttcf" {
            return Err(FontError::Malformed("font collection"));
        }
        let num_tables = r.u16()?;
        r.skip(6)?;
        let mut tables = BTreeMap::new();
        for _ in 0..num_tables {
            let tag: [u8; 4] = r.take(4)?.try_into().expect("four bytes");
            r.u32()?;
            let offset = r.u32()? as usize;
            let length = r.u32()? as usize;
            if offset
                .checked_add(length)
                .is_none_or(|end| end > bytes.len())
            {
                return Err(FontError::Truncated("sfnt"));
            }
            tables.insert(tag, (offset, length));
        }
        let table = |tag: &[u8; 4]| -> Result<&[u8], FontError> {
            let &(offset, length) = tables.get(tag).ok_or(FontError::MissingTable(*tag))?;
            Ok(&bytes[offset..offset + length])
        };

        let mut head = Reader::new(table(b"head")?, "head");
        head.skip(18)?;
        let units_per_em = head.u16()?;
        head.skip(16)?;
        let bbox = [head.i16()?, head.i16()?, head.i16()?, head.i16()?];
        head.skip(6)?;
        let long_loca = match head.i16()? {
            0 => false,
            1 => true,
            _ => return Err(FontError::Malformed("head")),
        };

        let mut hhea = Reader::new(table(b"hhea")?, "hhea");
        hhea.skip(4)?;
        let ascender = hhea.i16()?;
        let descender = hhea.i16()?;
        hhea.skip(26)?;
        let num_hmetrics = hhea.u16()?;

        let mut maxp = Reader::new(table(b"maxp")?, "maxp");
        maxp.skip(4)?;
        let num_glyphs = maxp.u16()?;

        let loca = table(b"loca")?;
        let entry = if long_loca { 4 } else { 2 };
        if loca.len() < (usize::from(num_glyphs) + 1) * entry {
            return Err(FontError::Truncated("loca"));
        }
        table(b"glyf")?;
        let hmtx = table(b"hmtx")?;
        if num_hmetrics == 0 || hmtx.len() < usize::from(num_hmetrics) * 4 {
            return Err(FontError::Truncated("hmtx"));
        }

        let mut italic_angle = 0.0;
        let mut fixed_pitch = false;
        let mut post_names = Vec::new();
        if let Some(post) = tables.get(b"post").map(|&(o, l)| &bytes[o..o + l]) {
            let mut r = Reader::new(post, "post");
            let version = r.u32()?;
            italic_angle = r.i32()? as f32 / 65536.0;
            r.skip(4)?;
            fixed_pitch = r.u32()? != 0;
            if version == 0x0002_0000 {
                r.skip(16)?;
                let count = r.u16()?;
                let mut indices = Vec::with_capacity(usize::from(count));
                for _ in 0..count {
                    indices.push(r.u16()?);
                }
                let mut strings: Vec<&[u8]> = Vec::new();
                while r.pos < post.len() {
                    let len = usize::from(r.u8()?);
                    strings.push(r.take(len)?);
                }
                for index in indices {
                    let name = match usize::from(index) {
                        k if k < MAC_GLYPH_NAMES.len() => MAC_GLYPH_NAMES[k].as_bytes(),
                        k => strings
                            .get(k - MAC_GLYPH_NAMES.len())
                            .copied()
                            .ok_or(FontError::Malformed("post"))?,
                    };
                    post_names.push(name.to_vec());
                }
            }
        }

        Ok(TrueTypeProgram {
            tables,
            units_per_em,
            bbox,
            long_loca,
            num_glyphs,
            num_hmetrics,
            ascender,
            descender,
            italic_angle,
            fixed_pitch,
            post_names,
            names: BTreeMap::new(),
            cache: RefCell::new(HashMap::new()),
            bytes,
        })
    }

    /// The glyph names the Type 42 dictionary's `CharStrings` maps to
    /// indices; consulted before the `post` names.
    pub fn with_names(mut self, names: BTreeMap<Vec<u8>, u16>) -> Self {
        self.names = names;
        self.cache.borrow_mut().clear();
        self
    }

    pub fn names(&self) -> &BTreeMap<Vec<u8>, u16> {
        &self.names
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The raw bytes of the table tagged `tag`, if present.
    pub fn table(&self, tag: &[u8; 4]) -> Option<&[u8]> {
        self.tables.get(tag).map(|&(o, l)| &self.bytes[o..o + l])
    }

    pub fn table_tags(&self) -> Vec<[u8; 4]> {
        self.tables.keys().copied().collect()
    }

    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// `head`'s bounding box in font units.
    pub fn bbox(&self) -> [i16; 4] {
        self.bbox
    }

    pub fn num_glyphs(&self) -> u16 {
        self.num_glyphs
    }

    pub fn ascender(&self) -> i16 {
        self.ascender
    }

    pub fn descender(&self) -> i16 {
        self.descender
    }

    pub fn italic_angle(&self) -> f32 {
        self.italic_angle
    }

    pub fn is_fixed_pitch(&self) -> bool {
        self.fixed_pitch
    }

    /// The `post` table's name for `gid`, when the table has names.
    pub fn post_name(&self, gid: u16) -> Option<&[u8]> {
        self.post_names.get(usize::from(gid)).map(Vec::as_slice)
    }

    /// The glyph index of `name`: through the supplied names, else the
    /// `post` table.
    pub fn gid(&self, name: &[u8]) -> Option<u16> {
        if let Some(&gid) = self.names.get(name) {
            return Some(gid);
        }
        self.post_names
            .iter()
            .position(|n| n == name)
            .and_then(|k| u16::try_from(k).ok())
    }

    /// The advance width of `gid` in font units; glyphs past the metrics
    /// count share the last advance.
    pub fn advance(&self, gid: u16) -> Result<u16, FontError> {
        if gid >= self.num_glyphs {
            return Err(FontError::GlyphIndex(gid));
        }
        let hmtx = self
            .table(b"hmtx")
            .ok_or(FontError::MissingTable(*b"hmtx"))?;
        let k = gid.min(self.num_hmetrics - 1);
        Reader::at(hmtx, usize::from(k) * 4, "hmtx").u16()
    }

    /// The left sidebearing of `gid` in font units.
    pub fn left_sidebearing(&self, gid: u16) -> Result<i16, FontError> {
        if gid >= self.num_glyphs {
            return Err(FontError::GlyphIndex(gid));
        }
        let hmtx = self
            .table(b"hmtx")
            .ok_or(FontError::MissingTable(*b"hmtx"))?;
        let n = usize::from(self.num_hmetrics);
        let k = usize::from(gid);
        if k < n {
            Reader::at(hmtx, k * 4 + 2, "hmtx").i16()
        } else {
            Reader::at(hmtx, n * 4 + (k - n) * 2, "hmtx").i16()
        }
    }

    /// The `glyf` bytes of `gid`; empty for a glyph without an outline.
    pub fn glyph_data(&self, gid: u16) -> Result<&[u8], FontError> {
        if gid >= self.num_glyphs {
            return Err(FontError::GlyphIndex(gid));
        }
        let loca = self
            .table(b"loca")
            .ok_or(FontError::MissingTable(*b"loca"))?;
        let k = usize::from(gid);
        let (start, end) = if self.long_loca {
            let mut r = Reader::at(loca, k * 4, "loca");
            (r.u32()? as usize, r.u32()? as usize)
        } else {
            let mut r = Reader::at(loca, k * 2, "loca");
            (usize::from(r.u16()?) * 2, usize::from(r.u16()?) * 2)
        };
        let glyf = self
            .table(b"glyf")
            .ok_or(FontError::MissingTable(*b"glyf"))?;
        if start > end || end > glyf.len() {
            return Err(FontError::Truncated("glyf"));
        }
        Ok(&glyf[start..end])
    }

    /// The glyphs a composite glyph refers to directly; empty for a
    /// simple glyph.
    pub fn components(&self, gid: u16) -> Result<Vec<u16>, FontError> {
        let data = self.glyph_data(gid)?;
        if data.len() < 10 || Reader::new(data, "glyf").i16()? >= 0 {
            return Ok(Vec::new());
        }
        let mut r = Reader::at(data, 10, "glyf");
        let mut components = Vec::new();
        loop {
            let flags = r.u16()?;
            components.push(r.u16()?);
            r.skip(if flags & ARG_1_AND_2_ARE_WORDS != 0 {
                4
            } else {
                2
            })?;
            if flags & WE_HAVE_A_SCALE != 0 {
                r.skip(2)?;
            } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
                r.skip(4)?;
            } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
                r.skip(8)?;
            }
            if flags & MORE_COMPONENTS == 0 {
                return Ok(components);
            }
        }
    }

    /// The outline of `gid` in font units.
    pub fn outline(&self, gid: u16) -> Result<Outline, FontError> {
        let mut outline = Outline::new();
        self.append_outline(gid, &mut outline, 0)?;
        Ok(outline)
    }

    fn append_outline(&self, gid: u16, out: &mut Outline, depth: usize) -> Result<(), FontError> {
        let data = self.glyph_data(gid)?;
        if data.is_empty() {
            return Ok(());
        }
        let mut r = Reader::new(data, "glyf");
        let contours = r.i16()?;
        r.skip(8)?;
        if contours >= 0 {
            let points = simple_points(&mut r, contours as usize)?;
            for contour in points {
                contour_to_outline(&contour, out);
            }
            return Ok(());
        }
        if depth >= MAX_COMPONENT_DEPTH {
            return Err(FontError::Malformed("composite glyph nesting"));
        }
        loop {
            let flags = r.u16()?;
            let component = r.u16()?;
            let (dx, dy) = if flags & ARG_1_AND_2_ARE_WORDS != 0 {
                (r.i16()?, r.i16()?)
            } else {
                (i16::from(r.i8()?), i16::from(r.i8()?))
            };
            // Point-matching placement (offsets naming points in the two
            // glyphs) is not resolved; such components land unshifted.
            let (dx, dy) = if flags & ARGS_ARE_XY_VALUES != 0 {
                (f32::from(dx), f32::from(dy))
            } else {
                (0.0, 0.0)
            };
            let [a, b, c, d] = if flags & WE_HAVE_A_SCALE != 0 {
                let s = r.f2dot14()?;
                [s, 0.0, 0.0, s]
            } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
                let a = r.f2dot14()?;
                let d = r.f2dot14()?;
                [a, 0.0, 0.0, d]
            } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
                [r.f2dot14()?, r.f2dot14()?, r.f2dot14()?, r.f2dot14()?]
            } else {
                [1.0, 0.0, 0.0, 1.0]
            };
            let mut part = Outline::new();
            self.append_outline(component, &mut part, depth + 1)?;
            out.ops.extend(
                part.map(|x, y| (a * x + c * y + dx, b * x + d * y + dy))
                    .ops,
            );
            if flags & MORE_COMPONENTS == 0 {
                return Ok(());
            }
        }
    }

    /// The glyph at `gid`: its advance and outline in font units.
    pub fn glyph_by_index(&self, gid: u16) -> Result<Glyph, FontError> {
        Ok(Glyph {
            advance: (f32::from(self.advance(gid)?), 0.0),
            outline: self.outline(gid)?,
        })
    }

    /// The glyph named `name`, interpreted on first request.
    pub fn glyph(&self, name: &[u8]) -> Result<Option<Rc<Glyph>>, FontError> {
        if let Some(glyph) = self.cache.borrow().get(name) {
            return Ok(Some(glyph.clone()));
        }
        let Some(gid) = self.gid(name) else {
            return Ok(None);
        };
        let glyph = Rc::new(self.glyph_by_index(gid)?);
        self.cache.borrow_mut().insert(name.to_vec(), glyph.clone());
        Ok(Some(glyph))
    }

    /// The code-to-glyph mapping of the `cmap` subtable for the platform
    /// and encoding, when the table has one in format 4 or 0; entries
    /// mapping to glyph 0 are omitted.
    pub fn cmap(
        &self,
        platform: u16,
        encoding: u16,
    ) -> Result<Option<BTreeMap<u32, u16>>, FontError> {
        let Some(cmap) = self.table(b"cmap") else {
            return Ok(None);
        };
        let mut r = Reader::new(cmap, "cmap");
        r.u16()?;
        let count = r.u16()?;
        let mut offset = None;
        for _ in 0..count {
            let p = r.u16()?;
            let e = r.u16()?;
            let o = r.u32()? as usize;
            if (p, e) == (platform, encoding) {
                offset = Some(o);
            }
        }
        let Some(offset) = offset else {
            return Ok(None);
        };
        let mut r = Reader::at(cmap, offset, "cmap");
        let mut map = BTreeMap::new();
        match r.u16()? {
            0 => {
                r.skip(4)?;
                for code in 0..256u32 {
                    let gid = u16::from(r.u8()?);
                    if gid != 0 {
                        map.insert(code, gid);
                    }
                }
            }
            4 => {
                r.skip(4)?;
                let seg_count = usize::from(r.u16()? / 2);
                r.skip(6)?;
                let mut ends = Vec::with_capacity(seg_count);
                for _ in 0..seg_count {
                    ends.push(r.u16()?);
                }
                r.skip(2)?;
                let mut starts = Vec::with_capacity(seg_count);
                for _ in 0..seg_count {
                    starts.push(r.u16()?);
                }
                let mut deltas = Vec::with_capacity(seg_count);
                for _ in 0..seg_count {
                    deltas.push(r.u16()?);
                }
                let range_offsets_at = r.pos;
                let mut range_offsets = Vec::with_capacity(seg_count);
                for _ in 0..seg_count {
                    range_offsets.push(r.u16()?);
                }
                for k in 0..seg_count {
                    let (start, end) = (starts[k], ends[k]);
                    if start > end {
                        return Err(FontError::Malformed("cmap"));
                    }
                    for code in start..=end {
                        if code == 0xFFFF {
                            break;
                        }
                        let gid = if range_offsets[k] == 0 {
                            code.wrapping_add(deltas[k])
                        } else {
                            let at = range_offsets_at
                                + k * 2
                                + usize::from(range_offsets[k])
                                + usize::from(code - start) * 2;
                            let gid = Reader::at(cmap, at, "cmap").u16()?;
                            if gid == 0 {
                                0
                            } else {
                                gid.wrapping_add(deltas[k])
                            }
                        };
                        if gid != 0 {
                            map.insert(u32::from(code), gid);
                        }
                    }
                }
            }
            _ => return Ok(None),
        }
        Ok(Some(map))
    }
}

/// A quadratic contour's points: `(x, y, on_curve)`.
type Contour = Vec<(f32, f32, bool)>;

/// The contours of a simple glyph.
fn simple_points(r: &mut Reader<'_>, contours: usize) -> Result<Vec<Contour>, FontError> {
    let mut ends = Vec::with_capacity(contours);
    for _ in 0..contours {
        ends.push(usize::from(r.u16()?));
    }
    let total = ends.last().map_or(0, |&e| e + 1);
    let instructions = usize::from(r.u16()?);
    r.skip(instructions)?;
    let mut flags = Vec::with_capacity(total);
    while flags.len() < total {
        let flag = r.u8()?;
        flags.push(flag);
        if flag & REPEAT != 0 {
            let n = usize::from(r.u8()?);
            for _ in 0..n {
                flags.push(flag);
            }
        }
    }
    flags.truncate(total);
    let mut xs = Vec::with_capacity(total);
    let mut x = 0i32;
    for &flag in &flags {
        x += if flag & X_SHORT != 0 {
            let d = i32::from(r.u8()?);
            if flag & X_SAME_OR_POSITIVE != 0 {
                d
            } else {
                -d
            }
        } else if flag & X_SAME_OR_POSITIVE != 0 {
            0
        } else {
            i32::from(r.i16()?)
        };
        xs.push(x);
    }
    let mut ys = Vec::with_capacity(total);
    let mut y = 0i32;
    for &flag in &flags {
        y += if flag & Y_SHORT != 0 {
            let d = i32::from(r.u8()?);
            if flag & Y_SAME_OR_POSITIVE != 0 {
                d
            } else {
                -d
            }
        } else if flag & Y_SAME_OR_POSITIVE != 0 {
            0
        } else {
            i32::from(r.i16()?)
        };
        ys.push(y);
    }
    let mut out = Vec::with_capacity(contours);
    let mut start = 0;
    for end in ends {
        if end + 1 < start || end >= total {
            return Err(FontError::Malformed("glyf contour"));
        }
        out.push(
            (start..=end)
                .map(|k| (xs[k] as f32, ys[k] as f32, flags[k] & ON_CURVE != 0))
                .collect(),
        );
        start = end + 1;
    }
    Ok(out)
}

/// One closed quadratic contour as cubic segments: implied on-curve
/// points between consecutive off-curve ones, each quadratic raised to
/// the cubic with the same shape.
pub fn contour_to_outline(points: &[(f32, f32, bool)], out: &mut Outline) {
    if points.is_empty() {
        return;
    }
    let n = points.len();
    let (start, order): ((f32, f32), Vec<usize>) = if points[0].2 {
        ((points[0].0, points[0].1), (1..n).collect())
    } else if points[n - 1].2 {
        ((points[n - 1].0, points[n - 1].1), (0..n - 1).collect())
    } else {
        let mid = (
            (points[0].0 + points[n - 1].0) / 2.0,
            (points[0].1 + points[n - 1].1) / 2.0,
        );
        (mid, (0..n).collect())
    };
    out.move_to(start.0, start.1);
    let mut current = start;
    let mut control: Option<(f32, f32)> = None;
    let quad = |out: &mut Outline, from: (f32, f32), q: (f32, f32), to: (f32, f32)| {
        let c1 = (
            from.0 + 2.0 / 3.0 * (q.0 - from.0),
            from.1 + 2.0 / 3.0 * (q.1 - from.1),
        );
        let c2 = (
            to.0 + 2.0 / 3.0 * (q.0 - to.0),
            to.1 + 2.0 / 3.0 * (q.1 - to.1),
        );
        out.curve_to(c1.0, c1.1, c2.0, c2.1, to.0, to.1);
    };
    for k in order {
        let (x, y, on) = points[k];
        match (on, control) {
            (true, None) => {
                out.line_to(x, y);
                current = (x, y);
            }
            (true, Some(q)) => {
                quad(out, current, q, (x, y));
                current = (x, y);
                control = None;
            }
            (false, None) => control = Some((x, y)),
            (false, Some(q)) => {
                let mid = ((q.0 + x) / 2.0, (q.1 + y) / 2.0);
                quad(out, current, q, mid);
                current = mid;
                control = Some((x, y));
            }
        }
    }
    match control {
        Some(q) => quad(out, current, q, start),
        None if current != start => out.line_to(start.0, start.1),
        None => {}
    }
    out.close();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::OutlineOp::{Close, CurveTo, LineTo, MoveTo};

    #[test]
    fn on_curve_polygons_become_lines() {
        let mut o = Outline::new();
        contour_to_outline(
            &[(0.0, 0.0, true), (10.0, 0.0, true), (10.0, 10.0, true)],
            &mut o,
        );
        assert_eq!(
            o.ops,
            vec![
                MoveTo(0.0, 0.0),
                LineTo(10.0, 0.0),
                LineTo(10.0, 10.0),
                LineTo(0.0, 0.0),
                Close
            ]
        );
        let mut empty = Outline::new();
        contour_to_outline(&[], &mut empty);
        assert!(empty.is_empty());
    }

    #[test]
    fn off_curve_points_become_cubics_with_implied_midpoints() {
        let mut o = Outline::new();
        contour_to_outline(
            &[
                (0.0, 0.0, true),
                (30.0, 0.0, false),
                (30.0, 30.0, false),
                (0.0, 30.0, true),
            ],
            &mut o,
        );
        assert_eq!(
            o.ops,
            vec![
                MoveTo(0.0, 0.0),
                CurveTo(20.0, 0.0, 30.0, 5.0, 30.0, 15.0),
                CurveTo(30.0, 25.0, 20.0, 30.0, 0.0, 30.0),
                LineTo(0.0, 0.0),
                Close
            ]
        );
        let mut o = Outline::new();
        contour_to_outline(&[(30.0, 0.0, false), (0.0, 0.0, true)], &mut o);
        assert_eq!(
            o.ops,
            vec![
                MoveTo(0.0, 0.0),
                CurveTo(20.0, 0.0, 20.0, 0.0, 0.0, 0.0),
                Close
            ]
        );
        let mut o = Outline::new();
        contour_to_outline(&[(0.0, 0.0, false), (10.0, 10.0, false)], &mut o);
        assert_eq!(o.ops[0], MoveTo(5.0, 5.0));
        assert_eq!(o.ops.len(), 4);
    }
}
