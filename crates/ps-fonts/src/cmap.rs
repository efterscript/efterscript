// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! CMaps: the mapping from byte codes to CIDs a composite font selects
//! glyphs through (PLRM3 §5.11.4). A CMap is a set of codespace ranges,
//! which fix how many bytes each code takes, and the CID mappings over
//! them; it may chain to a parent through `usecmap`. The VM builds one
//! from a CMap program's operators with [`CMapBuilder`]; the shipped
//! Identity CMaps are the text of Adobe's resource files, run through
//! the same operators.

use std::collections::BTreeMap;
use std::rc::Rc;

/// The text of the predefined `Identity-H` CMap, byte-identical to the
/// upstream resource file.
pub const IDENTITY_H: &str = include_str!("../data/cmap/Identity-H");
/// The text of the predefined `Identity-V` CMap.
pub const IDENTITY_V: &str = include_str!("../data/cmap/Identity-V");

/// The predefined CMaps, in sorted order, with their program text.
pub const PREDEFINED: [(&str, &str); 2] = [("Identity-H", IDENTITY_H), ("Identity-V", IDENTITY_V)];

/// The program text of a predefined CMap, if `name` is one.
pub fn predefined(name: &[u8]) -> Option<&'static str> {
    PREDEFINED
        .iter()
        .find(|(n, _)| n.as_bytes() == name)
        .map(|(_, text)| *text)
}

/// The registry, ordering, and supplement a CMap declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CidSystemInfo {
    pub registry: Vec<u8>,
    pub ordering: Vec<u8>,
    pub supplement: i32,
}

/// A codespace range: codes of `len` bytes whose every byte lies within
/// the corresponding byte of `low` and `high`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Codespace {
    pub len: u8,
    pub low: u32,
    pub high: u32,
}

impl Codespace {
    /// How many leading bytes of `code` (taken as `len` bytes) lie within
    /// the range, byte by byte; `len` means the code is inside.
    fn matching_prefix(&self, code: u32, available: u8) -> u8 {
        let mut matched = 0;
        for k in 0..self.len.min(available) {
            let shift = 8 * u32::from(self.len - 1 - k);
            let byte = (code >> shift) & 0xff;
            let lo = (self.low >> shift) & 0xff;
            let hi = (self.high >> shift) & 0xff;
            if byte < lo || byte > hi {
                break;
            }
            matched += 1;
        }
        matched
    }
}

/// A range of codes of one byte length mapping to consecutive CIDs from
/// `cid`, selecting glyphs from descendant `font`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CidRange {
    pub len: u8,
    pub low: u32,
    pub high: u32,
    pub cid: u16,
    pub font: u8,
}

/// A `bfrange` or `bfchar` destination: the bytes the source codes map
/// to, kept for a ToUnicode CMap built from the job's own mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BfRange {
    pub len: u8,
    pub low: u32,
    pub high: u32,
    /// The destination of `low`; later codes add to its last byte, or
    /// take the next entry when several were given.
    pub dst: Vec<Vec<u8>>,
}

/// One decoded code: how many bytes it took, its value, the CID it maps
/// to (`None` is CID 0, the notdef), and the descendant font number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub len: u8,
    pub code: u32,
    pub cid: Option<u16>,
    pub font: u8,
}

/// A CMap.
#[derive(Clone, Debug, PartialEq)]
pub struct CMap {
    pub name: Vec<u8>,
    pub wmode: u8,
    pub system_info: Option<CidSystemInfo>,
    pub codespaces: Vec<Codespace>,
    /// `(len, code)` to `(cid, font)`.
    pub single: BTreeMap<(u8, u32), (u16, u8)>,
    pub ranges: Vec<CidRange>,
    pub notdef: Vec<CidRange>,
    pub bf_single: BTreeMap<(u8, u32), Vec<u8>>,
    pub bf_ranges: Vec<BfRange>,
    pub parent: Option<Rc<CMap>>,
    /// The name marks the CMap as Unicode-based (`UCS2` or `UTF16`): its
    /// codes are Unicode, which a ToUnicode CMap can be built from.
    pub unicode_based: bool,
}

/// Why a builder rejected an entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CMapError {
    /// A code of zero or more than four bytes.
    CodeLength(usize),
    /// A range whose ends differ in length.
    RangeLength,
    /// A range whose low end is above its high end.
    RangeOrder,
    /// A CID outside the sixteen-bit range.
    Cid(i64),
}

impl std::fmt::Display for CMapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CMapError::CodeLength(n) => write!(f, "a code of {n} bytes"),
            CMapError::RangeLength => write!(f, "range ends of different lengths"),
            CMapError::RangeOrder => write!(f, "range low end above its high end"),
            CMapError::Cid(v) => write!(f, "CID {v} out of range"),
        }
    }
}

impl std::error::Error for CMapError {}

/// A code as its bytes: `(len, value)`.
fn code_of(bytes: &[u8]) -> Result<(u8, u32), CMapError> {
    if bytes.is_empty() || bytes.len() > 4 {
        return Err(CMapError::CodeLength(bytes.len()));
    }
    let value = bytes.iter().fold(0u32, |acc, &b| acc << 8 | u32::from(b));
    Ok((bytes.len() as u8, value))
}

fn range_of(low: &[u8], high: &[u8]) -> Result<(u8, u32, u32), CMapError> {
    let (len, lo) = code_of(low)?;
    let (len2, hi) = code_of(high)?;
    if len != len2 {
        return Err(CMapError::RangeLength);
    }
    if lo > hi {
        return Err(CMapError::RangeOrder);
    }
    Ok((len, lo, hi))
}

fn cid_of(cid: i64) -> Result<u16, CMapError> {
    u16::try_from(cid).map_err(|_| CMapError::Cid(cid))
}

fn is_unicode_based(name: &[u8]) -> bool {
    let text = String::from_utf8_lossy(name);
    text.contains("UCS2") || text.contains("UTF16")
}

impl CMap {
    /// The CID of a code the codespaces contain: a single mapping, a
    /// range, the parent's mapping, else a notdef range; `None` is CID 0.
    pub fn cid(&self, len: u8, code: u32) -> Option<(u16, u8)> {
        if let Some(&hit) = self.single.get(&(len, code)) {
            return Some(hit);
        }
        if let Some(hit) = Self::in_ranges(&self.ranges, len, code) {
            return Some(hit);
        }
        if let Some(parent) = &self.parent
            && let Some(hit) = parent.cid(len, code)
        {
            return Some(hit);
        }
        self.notdef
            .iter()
            .find(|r| r.len == len && (r.low..=r.high).contains(&code))
            .map(|r| (r.cid, r.font))
    }

    fn in_ranges(ranges: &[CidRange], len: u8, code: u32) -> Option<(u16, u8)> {
        ranges
            .iter()
            .find(|r| r.len == len && (r.low..=r.high).contains(&code))
            .map(|r| {
                let offset = (code - r.low).min(u32::from(u16::MAX)) as u16;
                (r.cid.saturating_add(offset), r.font)
            })
    }

    /// The codespaces, this CMap's own first, then the parent's.
    fn all_codespaces(&self) -> impl Iterator<Item = &Codespace> {
        let mut chain: Vec<&CMap> = vec![self];
        let mut next = self.parent.as_deref();
        while let Some(cmap) = next {
            chain.push(cmap);
            next = cmap.parent.as_deref();
        }
        chain.into_iter().flat_map(|c| c.codespaces.iter())
    }

    /// Decodes the first code of `bytes`, which must not be empty. The
    /// byte length is that of the codespace range containing the bytes;
    /// bytes in no range take the length of the shortest range whose
    /// leading bytes they partially match, else one byte, and map to CID
    /// 0 (`cid` is `None`). The length never exceeds what is left.
    pub fn decode(&self, bytes: &[u8]) -> Decoded {
        let available = bytes.len().min(4) as u8;
        let mut best_partial: Option<u8> = None;
        for space in self.all_codespaces() {
            let value = bytes
                .iter()
                .take(usize::from(space.len))
                .fold(0u32, |acc, &b| acc << 8 | u32::from(b));
            let matched = space.matching_prefix(value, available);
            if matched == space.len && available >= space.len {
                let cid = self.cid(space.len, value);
                return Decoded {
                    len: space.len,
                    code: value,
                    cid: cid.map(|(cid, _)| cid),
                    font: cid.map_or(0, |(_, font)| font),
                };
            }
            if matched > 0 && best_partial.is_none_or(|len| space.len < len) {
                best_partial = Some(space.len);
            }
        }
        let len = best_partial.unwrap_or(1).min(available).max(1);
        let code = bytes
            .iter()
            .take(usize::from(len))
            .fold(0u32, |acc, &b| acc << 8 | u32::from(b));
        Decoded {
            len,
            code,
            cid: None,
            font: 0,
        }
    }

    /// Every code of `bytes` in order.
    pub fn decode_all(&self, bytes: &[u8]) -> Vec<Decoded> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            let decoded = self.decode(&bytes[at..]);
            at += usize::from(decoded.len);
            out.push(decoded);
        }
        out
    }

    /// The bytes a code maps to through the `bf` entries, this CMap's
    /// own first, then the parent's.
    pub fn bf(&self, len: u8, code: u32) -> Option<Vec<u8>> {
        if let Some(dst) = self.bf_single.get(&(len, code)) {
            return Some(dst.clone());
        }
        for range in &self.bf_ranges {
            if range.len != len || !(range.low..=range.high).contains(&code) {
                continue;
            }
            let offset = (code - range.low) as usize;
            if range.dst.len() > 1 {
                return range.dst.get(offset).cloned();
            }
            let mut dst = range.dst.first()?.clone();
            let mut carry = offset;
            for byte in dst.iter_mut().rev() {
                let sum = usize::from(*byte) + carry;
                *byte = (sum & 0xff) as u8;
                carry = sum >> 8;
                if carry == 0 {
                    break;
                }
            }
            return Some(dst);
        }
        self.parent.as_ref().and_then(|p| p.bf(len, code))
    }
}

/// Builds a [`CMap`] from the entries a CMap program declares.
#[derive(Clone, Debug, Default)]
pub struct CMapBuilder {
    name: Vec<u8>,
    wmode: u8,
    system_info: Option<CidSystemInfo>,
    codespaces: Vec<Codespace>,
    single: BTreeMap<(u8, u32), (u16, u8)>,
    ranges: Vec<CidRange>,
    notdef: Vec<CidRange>,
    bf_single: BTreeMap<(u8, u32), Vec<u8>>,
    bf_ranges: Vec<BfRange>,
    parent: Option<Rc<CMap>>,
    font: u8,
}

impl CMapBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn name(&mut self, name: &[u8]) -> &mut Self {
        self.name = name.to_vec();
        self
    }

    pub fn wmode(&mut self, wmode: u8) -> &mut Self {
        self.wmode = wmode;
        self
    }

    pub fn system_info(&mut self, info: CidSystemInfo) -> &mut Self {
        self.system_info = Some(info);
        self
    }

    /// `usecmap`: the parent whose mappings apply where this CMap has
    /// none. Its writing mode is inherited unless set here.
    pub fn use_cmap(&mut self, parent: Rc<CMap>) -> &mut Self {
        self.wmode = parent.wmode;
        self.parent = Some(parent);
        self
    }

    /// `usefont`: the descendant font number later mappings select.
    pub fn use_font(&mut self, font: u8) -> &mut Self {
        self.font = font;
        self
    }

    pub fn codespace(&mut self, low: &[u8], high: &[u8]) -> Result<&mut Self, CMapError> {
        let (len, low, high) = range_of(low, high)?;
        self.codespaces.push(Codespace { len, low, high });
        Ok(self)
    }

    pub fn cid_range(&mut self, low: &[u8], high: &[u8], cid: i64) -> Result<&mut Self, CMapError> {
        let (len, low, high) = range_of(low, high)?;
        self.ranges.push(CidRange {
            len,
            low,
            high,
            cid: cid_of(cid)?,
            font: self.font,
        });
        Ok(self)
    }

    pub fn cid_char(&mut self, code: &[u8], cid: i64) -> Result<&mut Self, CMapError> {
        let (len, code) = code_of(code)?;
        self.single.insert((len, code), (cid_of(cid)?, self.font));
        Ok(self)
    }

    pub fn notdef_range(
        &mut self,
        low: &[u8],
        high: &[u8],
        cid: i64,
    ) -> Result<&mut Self, CMapError> {
        let (len, low, high) = range_of(low, high)?;
        self.notdef.push(CidRange {
            len,
            low,
            high,
            cid: cid_of(cid)?,
            font: self.font,
        });
        Ok(self)
    }

    pub fn bf_char(&mut self, code: &[u8], dst: &[u8]) -> Result<&mut Self, CMapError> {
        let (len, code) = code_of(code)?;
        self.bf_single.insert((len, code), dst.to_vec());
        Ok(self)
    }

    pub fn bf_range(
        &mut self,
        low: &[u8],
        high: &[u8],
        dst: Vec<Vec<u8>>,
    ) -> Result<&mut Self, CMapError> {
        let (len, low, high) = range_of(low, high)?;
        self.bf_ranges.push(BfRange {
            len,
            low,
            high,
            dst,
        });
        Ok(self)
    }

    pub fn build(self) -> CMap {
        let unicode_based = is_unicode_based(&self.name);
        CMap {
            name: self.name,
            wmode: self.wmode,
            system_info: self.system_info,
            codespaces: self.codespaces,
            single: self.single,
            ranges: self.ranges,
            notdef: self.notdef,
            bf_single: self.bf_single,
            bf_ranges: self.bf_ranges,
            parent: self.parent,
            unicode_based,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One-byte codes 0x20..0x7E from CID 1, two-byte codes 0x8140..0x817E
    /// from CID 200, a notdef range over the rest of the two-byte space.
    fn mixed() -> CMap {
        let mut b = CMapBuilder::new();
        b.name(b"Syn-H")
            .codespace(&[0x20], &[0x7e])
            .unwrap()
            .codespace(&[0x81, 0x40], &[0x81, 0xfe])
            .unwrap()
            .cid_range(&[0x20], &[0x7e], 1)
            .unwrap()
            .cid_range(&[0x81, 0x40], &[0x81, 0x7e], 200)
            .unwrap()
            .notdef_range(&[0x81, 0x80], &[0x81, 0xfe], 7)
            .unwrap()
            .cid_char(&[0x7e], 999)
            .unwrap();
        b.build()
    }

    #[test]
    fn codes_inside_a_codespace_take_its_length_and_mapping() {
        let cmap = mixed();
        assert_eq!(
            cmap.decode(&[0x41, 0x42]),
            Decoded {
                len: 1,
                code: 0x41,
                cid: Some(34),
                font: 0
            }
        );
        assert_eq!(cmap.decode(&[0x81, 0x41]).cid, Some(201));
        assert_eq!(cmap.decode(&[0x81, 0x41]).len, 2);
        // A single mapping wins over the range it lies in.
        assert_eq!(cmap.decode(&[0x7e]).cid, Some(999));
        // Inside the codespace, outside every mapping: the notdef range.
        assert_eq!(cmap.decode(&[0x81, 0x90]).cid, Some(7));
        // Inside the codespace with no mapping of any kind: CID 0.
        assert_eq!(cmap.decode(&[0x81, 0x7f]).cid, None);
        assert_eq!(cmap.decode(&[0x81, 0x7f]).len, 2);
        let all = cmap.decode_all(&[0x41, 0x81, 0x40, 0x42]);
        assert_eq!(
            all.iter().map(|d| (d.len, d.cid)).collect::<Vec<_>>(),
            vec![(1, Some(34)), (2, Some(200)), (1, Some(35))]
        );
    }

    #[test]
    fn partial_matches_take_the_shortest_partially_matching_length() {
        let cmap = mixed();
        // First byte in the two-byte range, second outside: two bytes of
        // notdef.
        assert_eq!(
            cmap.decode(&[0x81, 0x20, 0x41]),
            Decoded {
                len: 2,
                code: 0x8120,
                cid: None,
                font: 0
            }
        );
        // No codespace starts with 0x00: one byte.
        assert_eq!(cmap.decode(&[0x00, 0x41]).len, 1);
        assert_eq!(cmap.decode(&[0x00, 0x41]).cid, None);
        // A lead byte with nothing after it: what is left.
        assert_eq!(cmap.decode(&[0x81]).len, 1);
        assert_eq!(cmap.decode(&[0x81]).code, 0x81);
        // Among several partially matching codespaces the shortest wins.
        let mut b = CMapBuilder::new();
        b.codespace(&[0x85, 0x00, 0x00], &[0x8f, 0xff, 0xff])
            .unwrap()
            .codespace(&[0x80, 0x00], &[0x80, 0x3f])
            .unwrap()
            .codespace(&[0x00, 0x00, 0x00, 0x00], &[0x00, 0x00, 0xff, 0xff])
            .unwrap();
        let cmap = b.build();
        assert_eq!(cmap.decode(&[0x80, 0x40, 0x00, 0x00]).len, 2);
        assert_eq!(cmap.decode(&[0x85, 0x40, 0x00, 0x00]).len, 3);
        assert_eq!(cmap.decode(&[0x85, 0x40, 0x00, 0x00]).code, 0x854000);
        assert_eq!(cmap.decode(&[0x00, 0x00, 0x12, 0x34]).len, 4);
        assert_eq!(cmap.decode(&[0x00, 0x00, 0x12, 0x34]).code, 0x1234);
        assert_eq!(cmap.decode(&[0x00, 0x01, 0x12, 0x34]).len, 4);
        assert_eq!(cmap.decode(&[0x00, 0x01, 0x12, 0x34]).cid, None);
    }

    #[test]
    fn byte_ranges_are_checked_per_byte() {
        let mut b = CMapBuilder::new();
        b.codespace(&[0x81, 0x40], &[0xfe, 0xfe]).unwrap();
        let cmap = b.build();
        // Numerically between the ends, but the second byte is outside.
        assert_eq!(cmap.decode(&[0x90, 0xff]).len, 2);
        assert_eq!(cmap.decode(&[0x90, 0xff]).cid, None);
        assert_eq!(cmap.decode(&[0x90, 0x40]).len, 2);
    }

    #[test]
    fn usecmap_chains_mappings_and_writing_mode() {
        let mut b = CMapBuilder::new();
        b.name(b"Identity-H")
            .codespace(&[0, 0], &[0xff, 0xff])
            .unwrap()
            .cid_range(&[0, 0], &[0xff, 0xff], 0)
            .unwrap();
        let parent = Rc::new(b.build());
        let mut child = CMapBuilder::new();
        child.name(b"Child-V");
        child.use_cmap(parent.clone());
        child.cid_char(&[0x00, 0x05], 77).unwrap();
        let mut vertical = child.clone();
        vertical.wmode(1);
        let child = child.build();
        let vertical = vertical.build();
        assert_eq!(child.wmode, 0);
        assert_eq!(vertical.wmode, 1);
        assert_eq!(child.decode(&[0x12, 0x34]).cid, Some(0x1234));
        assert_eq!(child.decode(&[0x00, 0x05]).cid, Some(77));
        assert_eq!(child.decode(&[0x12, 0x34]).len, 2);
        assert!(Rc::ptr_eq(child.parent.as_ref().unwrap(), &parent));
    }

    #[test]
    fn identity_files_are_the_shipped_text() {
        assert!(IDENTITY_H.starts_with("%!PS-Adobe-3.0 Resource-CMap"));
        assert!(IDENTITY_H.contains("/CMapName /Identity-H def"));
        assert!(IDENTITY_V.contains("/Identity-H usecmap"));
        assert_eq!(predefined(b"Identity-V"), Some(IDENTITY_V));
        assert_eq!(predefined(b"Identity-X"), None);
        assert!(PREDEFINED.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn names_mark_unicode_based_cmaps_and_fonts_are_recorded() {
        let mut b = CMapBuilder::new();
        b.name(b"UniJIS-UCS2-H");
        assert!(b.build().unicode_based);
        let mut b = CMapBuilder::new();
        b.name(b"90ms-RKSJ-H");
        assert!(!b.build().unicode_based);
        let mut b = CMapBuilder::new();
        b.name(b"UniJIS-UTF16-V").wmode(1);
        b.use_font(2);
        b.codespace(&[0, 0], &[0xff, 0xff]).unwrap();
        b.cid_range(&[0, 0], &[0, 0xff], 5).unwrap();
        let cmap = b.build();
        assert!(cmap.unicode_based);
        assert_eq!(
            cmap.decode(&[0, 3]),
            Decoded {
                len: 2,
                code: 3,
                cid: Some(8),
                font: 2
            }
        );
    }

    #[test]
    fn bf_entries_map_codes_to_bytes() {
        let mut b = CMapBuilder::new();
        b.codespace(&[0], &[0xff]).unwrap();
        b.bf_char(&[0x41], &[0x00, 0x41]).unwrap();
        b.bf_range(&[0x61], &[0x63], vec![vec![0x00, 0xfe]])
            .unwrap();
        b.bf_range(
            &[0x70],
            &[0x71],
            vec![vec![0x00, 0x10], vec![0x00, 0x20, 0x00, 0x21]],
        )
        .unwrap();
        let cmap = b.build();
        assert_eq!(cmap.bf(1, 0x41), Some(vec![0x00, 0x41]));
        assert_eq!(cmap.bf(1, 0x62), Some(vec![0x00, 0xff]));
        assert_eq!(cmap.bf(1, 0x63), Some(vec![0x01, 0x00]), "carry");
        assert_eq!(cmap.bf(1, 0x71), Some(vec![0x00, 0x20, 0x00, 0x21]));
        assert_eq!(cmap.bf(1, 0x50), None);
    }

    #[test]
    fn malformed_entries_are_rejected() {
        let mut b = CMapBuilder::new();
        assert_eq!(b.codespace(&[], &[]).unwrap_err(), CMapError::CodeLength(0));
        assert_eq!(
            b.codespace(&[0; 5], &[0; 5]).unwrap_err(),
            CMapError::CodeLength(5)
        );
        assert_eq!(
            b.cid_range(&[0], &[0, 0], 1).unwrap_err(),
            CMapError::RangeLength
        );
        assert_eq!(
            b.cid_range(&[5], &[4], 1).unwrap_err(),
            CMapError::RangeOrder
        );
        assert_eq!(b.cid_char(&[1], 70000).unwrap_err(), CMapError::Cid(70000));
        assert_eq!(
            b.notdef_range(&[1], &[2], -1).unwrap_err(),
            CMapError::Cid(-1)
        );
        assert_eq!(
            CMapError::RangeOrder.to_string(),
            "range low end above its high end"
        );
    }
}
