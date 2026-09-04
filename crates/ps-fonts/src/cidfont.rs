// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! CID-keyed fonts with Type 1 charstrings: the `CIDFontType 0` form a
//! `CIDInit` `StartData` section carries. The binary `GlyphData` holds a
//! CID map — per CID, the index of the font dictionary its charstring
//! runs under and the offset of that charstring, each CID's charstring
//! running to the next CID's offset — the subroutine maps of the font
//! dictionaries that keep their subroutines in the data, and the
//! charstrings themselves, encrypted as a Type 1 font's are. Each font
//! dictionary becomes its own Type 1 program with the CID as the glyph
//! key, so the existing charstring engine interprets every glyph.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use crate::outline::Glyph;
use crate::program::FontError;
use crate::type1::{CHARSTRING_KEY, Type1Program, decrypt};

/// Where a font dictionary's subroutines are.
#[derive(Clone, Debug, PartialEq)]
pub enum SubrSource {
    None,
    /// `SubrMapOffset`, `SDBytes`, `SubrCount`: a map in the glyph data
    /// of `count + 1` offsets of `bytes` bytes each, subroutine `k`
    /// running from its offset to the next.
    Map {
        offset: usize,
        bytes: u8,
        count: usize,
    },
    /// `Subrs` given as strings in the dictionary, still encrypted.
    Strings(Vec<Vec<u8>>),
}

/// One `FDArray` entry as the reader needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct FdLayout {
    pub len_iv: i32,
    pub subrs: SubrSource,
    /// The dictionary's own `FontMatrix`; glyphs are mapped into the
    /// space of the CIDFont's matrix when the two differ.
    pub font_matrix: Option<[f32; 6]>,
}

/// The numbers the CIDFont dictionary declares about its glyph data.
#[derive(Clone, Debug, PartialEq)]
pub struct CidLayout {
    pub cid_map_offset: usize,
    /// Bytes of font dictionary index per CID map entry; zero means
    /// every CID runs under dictionary 0.
    pub fd_bytes: u8,
    /// Bytes of charstring offset per CID map entry.
    pub gd_bytes: u8,
    pub cid_count: u32,
    /// The CIDFont's `FontMatrix`, the space every glyph is returned in.
    pub font_matrix: [f32; 6],
    pub fds: Vec<FdLayout>,
}

struct Fd {
    program: Type1Program,
    /// Maps the dictionary's glyph space into the CIDFont's, when the
    /// two matrices differ.
    map: Option<[f32; 6]>,
}

/// A CID-keyed font with Type 1 charstrings.
pub struct Type1CidProgram {
    fds: Vec<Fd>,
    /// The font dictionary of each CID that has a charstring.
    fd_of: Vec<Option<u8>>,
    cid_count: u32,
    cache: RefCell<HashMap<u16, Rc<Glyph>>>,
}

impl std::fmt::Debug for Type1CidProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Type1CidProgram")
            .field("fds", &self.fds.len())
            .field("cids", &self.fd_of.iter().flatten().count())
            .finish()
    }
}

/// A big-endian number of `bytes` bytes at `at`.
fn number(data: &[u8], at: usize, bytes: u8, what: &'static str) -> Result<usize, FontError> {
    let end = at
        .checked_add(usize::from(bytes))
        .ok_or(FontError::Truncated(what))?;
    let slice = data.get(at..end).ok_or(FontError::Truncated(what))?;
    Ok(slice
        .iter()
        .fold(0usize, |acc, &b| acc << 8 | usize::from(b)))
}

/// The `count + 1` offsets of a map whose entries are `entry` bytes with
/// the offset in the last `bytes` of each.
fn offsets(
    data: &[u8],
    at: usize,
    entry: usize,
    skip: usize,
    bytes: u8,
    count: usize,
    what: &'static str,
) -> Result<Vec<usize>, FontError> {
    let mut out = Vec::with_capacity(count + 1);
    for k in 0..=count {
        let base = at
            .checked_add(k.checked_mul(entry).ok_or(FontError::Truncated(what))?)
            .ok_or(FontError::Truncated(what))?;
        out.push(number(data, base + skip, bytes, what)?);
    }
    Ok(out)
}

fn slice_between<'a>(
    data: &'a [u8],
    start: usize,
    end: usize,
    what: &'static str,
) -> Result<&'a [u8], FontError> {
    if start > end {
        return Err(FontError::Malformed(what));
    }
    data.get(start..end).ok_or(FontError::Truncated(what))
}

/// The two-byte key a CID's charstring is filed under.
fn key(cid: u16) -> Vec<u8> {
    cid.to_be_bytes().to_vec()
}

fn invert(m: [f32; 6]) -> Option<[f32; 6]> {
    let [a, b, c, d, tx, ty] = m;
    let det = a * d - b * c;
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    Some([ia, ib, ic, id, -(tx * ia + ty * ic), -(tx * ib + ty * id)])
}

/// `first` followed by `second`.
fn compose(first: [f32; 6], second: [f32; 6]) -> [f32; 6] {
    let [a, b, c, d, tx, ty] = first;
    let [a2, b2, c2, d2, tx2, ty2] = second;
    [
        a * a2 + b * c2,
        a * b2 + b * d2,
        c * a2 + d * c2,
        c * b2 + d * d2,
        tx * a2 + ty * c2 + tx2,
        tx * b2 + ty * d2 + ty2,
    ]
}

impl Type1CidProgram {
    /// Parses `data` (the bytes `StartData` read) as `layout` describes.
    pub fn parse(data: &[u8], layout: &CidLayout) -> Result<Self, FontError> {
        if layout.fds.is_empty() {
            return Err(FontError::Malformed("font dictionary array"));
        }
        if layout.fd_bytes > 4 || layout.gd_bytes == 0 || layout.gd_bytes > 4 {
            return Err(FontError::Malformed("CID map entry size"));
        }
        let count =
            usize::try_from(layout.cid_count).map_err(|_| FontError::Malformed("CIDCount"))?;
        if count > usize::from(u16::MAX) + 1 {
            return Err(FontError::Malformed("CIDCount"));
        }
        let entry = usize::from(layout.fd_bytes) + usize::from(layout.gd_bytes);
        let glyph_offsets = offsets(
            data,
            layout.cid_map_offset,
            entry,
            usize::from(layout.fd_bytes),
            layout.gd_bytes,
            count,
            "CID map",
        )?;
        let mut charstrings: Vec<BTreeMap<Vec<u8>, Vec<u8>>> =
            (0..layout.fds.len()).map(|_| BTreeMap::new()).collect();
        let mut fd_of = vec![None; count];
        for cid in 0..count {
            let (start, end) = (glyph_offsets[cid], glyph_offsets[cid + 1]);
            if start == end {
                continue;
            }
            let fd = if layout.fd_bytes == 0 {
                0
            } else {
                number(
                    data,
                    layout.cid_map_offset + cid * entry,
                    layout.fd_bytes,
                    "CID map",
                )?
            };
            if fd >= layout.fds.len() {
                return Err(FontError::Malformed("font dictionary index"));
            }
            let cipher = slice_between(data, start, end, "charstring")?;
            let plain = match usize::try_from(layout.fds[fd].len_iv) {
                Ok(skip) => decrypt(CHARSTRING_KEY, cipher, skip),
                Err(_) => cipher.to_vec(),
            };
            charstrings[fd].insert(key(cid as u16), plain);
            fd_of[cid] = Some(fd as u8);
        }
        let mut fds = Vec::with_capacity(layout.fds.len());
        for (fd, glyphs) in layout.fds.iter().zip(charstrings) {
            let plain = |cipher: &[u8]| match usize::try_from(fd.len_iv) {
                Ok(skip) => decrypt(CHARSTRING_KEY, cipher, skip),
                Err(_) => cipher.to_vec(),
            };
            let subrs = match &fd.subrs {
                SubrSource::None => Vec::new(),
                SubrSource::Strings(strings) => strings.iter().map(|s| plain(s)).collect(),
                SubrSource::Map {
                    offset,
                    bytes,
                    count,
                } => {
                    if *bytes == 0 || *bytes > 4 {
                        return Err(FontError::Malformed("subroutine map entry size"));
                    }
                    let subr_offsets = offsets(
                        data,
                        *offset,
                        usize::from(*bytes),
                        0,
                        *bytes,
                        *count,
                        "subroutine map",
                    )?;
                    subr_offsets
                        .windows(2)
                        .map(|w| slice_between(data, w[0], w[1], "subroutine").map(plain))
                        .collect::<Result<Vec<_>, _>>()?
                }
            };
            let map = match fd.font_matrix {
                Some(matrix) if matrix != layout.font_matrix => {
                    let inverse =
                        invert(layout.font_matrix).ok_or(FontError::Malformed("FontMatrix"))?;
                    Some(compose(matrix, inverse))
                }
                _ => None,
            };
            fds.push(Fd {
                program: Type1Program::from_decrypted(fd.len_iv, subrs, glyphs),
                map,
            });
        }
        Ok(Type1CidProgram {
            fds,
            fd_of,
            cid_count: layout.cid_count,
            cache: RefCell::new(HashMap::new()),
        })
    }

    pub fn cid_count(&self) -> u32 {
        self.cid_count
    }

    pub fn fd_count(&self) -> usize {
        self.fds.len()
    }

    /// The font dictionary index of a CID with a charstring.
    pub fn fd_index(&self, cid: u16) -> Option<u8> {
        self.fd_of.get(usize::from(cid)).copied().flatten()
    }

    /// The CIDs that have charstrings, in order.
    pub fn cids(&self) -> Vec<u16> {
        self.fd_of
            .iter()
            .enumerate()
            .filter_map(|(cid, fd)| fd.map(|_| cid as u16))
            .collect()
    }

    /// The decrypted charstring of a CID.
    pub fn charstring(&self, cid: u16) -> Option<&[u8]> {
        let fd = self.fd_index(cid)?;
        self.fds[usize::from(fd)].program.charstring(&key(cid))
    }

    /// The decrypted subroutines of font dictionary `fd`.
    pub fn subrs(&self, fd: u8) -> Option<&[Vec<u8>]> {
        self.fds.get(usize::from(fd)).map(|fd| fd.program.subrs())
    }

    /// The glyph of a CID in the CIDFont's glyph space: `Ok(None)` when
    /// the CID has no charstring.
    pub fn glyph_by_cid(&self, cid: u16) -> Result<Option<Rc<Glyph>>, FontError> {
        if let Some(glyph) = self.cache.borrow().get(&cid) {
            return Ok(Some(glyph.clone()));
        }
        let Some(fd) = self.fd_index(cid) else {
            return Ok(None);
        };
        let fd = &self.fds[usize::from(fd)];
        let Some(glyph) = fd.program.glyph(&key(cid))? else {
            return Ok(None);
        };
        let glyph = match fd.map {
            None => glyph,
            Some(m) => {
                let [a, b, c, d, tx, ty] = m;
                let (ax, ay) = glyph.advance;
                Rc::new(Glyph {
                    advance: (a * ax + c * ay, b * ax + d * ay),
                    outline: glyph
                        .outline
                        .map(|x, y| (a * x + c * y + tx, b * x + d * y + ty)),
                })
            }
        };
        self.cache.borrow_mut().insert(cid, glyph.clone());
        Ok(Some(glyph))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outline::OutlineOp;
    use crate::testing::{CharstringBuilder, CidType1Font, rectangle};

    #[test]
    fn a_synthesised_font_parses_with_per_dictionary_private_data() {
        let font = CidType1Font::corpus();
        let program = font.program().unwrap();
        assert_eq!(program.fd_count(), 2);
        assert_eq!(program.cid_count(), 4);
        assert_eq!(program.cids(), vec![0, 1, 2, 3]);
        assert_eq!(program.fd_index(1), Some(0));
        assert_eq!(program.fd_index(2), Some(1));
        assert_eq!(program.fd_index(3), Some(0));
        assert_eq!(program.fd_index(7), None);
        let one = program.glyph_by_cid(1).unwrap().unwrap();
        assert_eq!(one.advance, (500.0, 0.0));
        assert_eq!(one.outline.control_box(), Some([50.0, 0.0, 450.0, 400.0]));
        // CID 2 draws through font dictionary 1's subroutine, which is
        // encrypted with that dictionary's own lenIV.
        let two = program.glyph_by_cid(2).unwrap().unwrap();
        assert_eq!(two.advance, (700.0, 0.0));
        assert_eq!(two.outline.control_box(), Some([0.0, 0.0, 600.0, 600.0]));
        let three = program.glyph_by_cid(3).unwrap().unwrap();
        assert_eq!(three.advance, (300.0, 0.0));
        assert!(program.glyph_by_cid(9).unwrap().is_none());
        assert_eq!(program.subrs(1).map(<[_]>::len), Some(1));
        assert!(program.charstring(2).is_some());
        assert!(format!("{program:?}").contains("fds: 2"));
    }

    #[test]
    fn a_dictionary_matrix_maps_into_the_font_matrix_space() {
        let font = CidType1Font::new("M")
            .fd(4, Vec::new())
            .fd_matrix(0, [0.002, 0.0, 0.0, 0.002, 0.0, 0.0])
            .glyph(1, 0, 400, &rectangle(0.0, 0.0, 100.0, 50.0));
        let program = font.program().unwrap();
        let glyph = program.glyph_by_cid(1).unwrap().unwrap();
        assert_eq!(glyph.advance, (800.0, 0.0));
        assert_eq!(glyph.outline.control_box(), Some([0.0, 0.0, 200.0, 100.0]));
        assert!(matches!(glyph.outline.ops[0], OutlineOp::MoveTo(0.0, 0.0)));
    }

    #[test]
    fn faults_in_the_layout_and_data_are_reported() {
        let font = CidType1Font::corpus();
        let (data, layout) = font.glyph_data();
        let mut short = layout.clone();
        short.cid_count = 40;
        assert!(matches!(
            Type1CidProgram::parse(&data, &short),
            Err(FontError::Truncated("CID map"))
        ));
        let mut bad = layout.clone();
        bad.gd_bytes = 0;
        assert!(matches!(
            Type1CidProgram::parse(&data, &bad),
            Err(FontError::Malformed("CID map entry size"))
        ));
        let mut none = layout.clone();
        none.fds.clear();
        assert!(matches!(
            Type1CidProgram::parse(&data, &none),
            Err(FontError::Malformed("font dictionary array"))
        ));
        let mut one = layout.clone();
        one.fds.truncate(1);
        assert!(matches!(
            Type1CidProgram::parse(&data, &one),
            Err(FontError::Malformed("font dictionary index"))
        ));
        assert!(matches!(
            Type1CidProgram::parse(&data[..data.len() - 3], &layout),
            Err(FontError::Truncated("charstring"))
        ));
        // A charstring that cannot be interpreted is the operator's fault.
        let font = CidType1Font::new("Bad").fd(4, Vec::new()).charstring(
            1,
            0,
            CharstringBuilder::new().hsbw(0, 400).num(1).bytes(),
        );
        let program = font.program().unwrap();
        assert!(program.glyph_by_cid(1).is_err());
    }
}
