// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Compact Font Format programs: the header and the name, top-dictionary,
//! string, and global-subroutine indexes; top and private dictionaries;
//! charsets and encodings; local subroutines; and, for CID-keyed fonts,
//! the font dictionary array and the select table. Glyphs are found by
//! index, by name (through the charset's string ids), or by CID, and
//! interpreted by the Type 2 charstring interpreter ([`charstring`]).
//! [`write`] holds the low-level writers the test builder and the subset
//! writer share.

pub mod charstring;
pub mod write;

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use crate::encoding::STANDARD_ENCODING;
use crate::outline::Glyph;
use crate::program::FontError;

/// The standard strings every CFF program can refer to by string id
/// without carrying them; a program's own strings follow them.
pub const STANDARD_STRINGS: [&str; 391] = [
    ".notdef",
    "space",
    "exclam",
    "quotedbl",
    "numbersign",
    "dollar",
    "percent",
    "ampersand",
    "quoteright",
    "parenleft",
    "parenright",
    "asterisk",
    "plus",
    "comma",
    "hyphen",
    "period",
    "slash",
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "colon",
    "semicolon",
    "less",
    "equal",
    "greater",
    "question",
    "at",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "bracketleft",
    "backslash",
    "bracketright",
    "asciicircum",
    "underscore",
    "quoteleft",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "braceleft",
    "bar",
    "braceright",
    "asciitilde",
    "exclamdown",
    "cent",
    "sterling",
    "fraction",
    "yen",
    "florin",
    "section",
    "currency",
    "quotesingle",
    "quotedblleft",
    "guillemotleft",
    "guilsinglleft",
    "guilsinglright",
    "fi",
    "fl",
    "endash",
    "dagger",
    "daggerdbl",
    "periodcentered",
    "paragraph",
    "bullet",
    "quotesinglbase",
    "quotedblbase",
    "quotedblright",
    "guillemotright",
    "ellipsis",
    "perthousand",
    "questiondown",
    "grave",
    "acute",
    "circumflex",
    "tilde",
    "macron",
    "breve",
    "dotaccent",
    "dieresis",
    "ring",
    "cedilla",
    "hungarumlaut",
    "ogonek",
    "caron",
    "emdash",
    "AE",
    "ordfeminine",
    "Lslash",
    "Oslash",
    "OE",
    "ordmasculine",
    "ae",
    "dotlessi",
    "lslash",
    "oslash",
    "oe",
    "germandbls",
    "onesuperior",
    "logicalnot",
    "mu",
    "trademark",
    "Eth",
    "onehalf",
    "plusminus",
    "Thorn",
    "onequarter",
    "divide",
    "brokenbar",
    "degree",
    "thorn",
    "threequarters",
    "twosuperior",
    "registered",
    "minus",
    "eth",
    "multiply",
    "threesuperior",
    "copyright",
    "Aacute",
    "Acircumflex",
    "Adieresis",
    "Agrave",
    "Aring",
    "Atilde",
    "Ccedilla",
    "Eacute",
    "Ecircumflex",
    "Edieresis",
    "Egrave",
    "Iacute",
    "Icircumflex",
    "Idieresis",
    "Igrave",
    "Ntilde",
    "Oacute",
    "Ocircumflex",
    "Odieresis",
    "Ograve",
    "Otilde",
    "Scaron",
    "Uacute",
    "Ucircumflex",
    "Udieresis",
    "Ugrave",
    "Yacute",
    "Ydieresis",
    "Zcaron",
    "aacute",
    "acircumflex",
    "adieresis",
    "agrave",
    "aring",
    "atilde",
    "ccedilla",
    "eacute",
    "ecircumflex",
    "edieresis",
    "egrave",
    "iacute",
    "icircumflex",
    "idieresis",
    "igrave",
    "ntilde",
    "oacute",
    "ocircumflex",
    "odieresis",
    "ograve",
    "otilde",
    "scaron",
    "uacute",
    "ucircumflex",
    "udieresis",
    "ugrave",
    "yacute",
    "ydieresis",
    "zcaron",
    "exclamsmall",
    "Hungarumlautsmall",
    "dollaroldstyle",
    "dollarsuperior",
    "ampersandsmall",
    "Acutesmall",
    "parenleftsuperior",
    "parenrightsuperior",
    "twodotenleader",
    "onedotenleader",
    "zerooldstyle",
    "oneoldstyle",
    "twooldstyle",
    "threeoldstyle",
    "fouroldstyle",
    "fiveoldstyle",
    "sixoldstyle",
    "sevenoldstyle",
    "eightoldstyle",
    "nineoldstyle",
    "commasuperior",
    "threequartersemdash",
    "periodsuperior",
    "questionsmall",
    "asuperior",
    "bsuperior",
    "centsuperior",
    "dsuperior",
    "esuperior",
    "isuperior",
    "lsuperior",
    "msuperior",
    "nsuperior",
    "osuperior",
    "rsuperior",
    "ssuperior",
    "tsuperior",
    "ff",
    "ffi",
    "ffl",
    "parenleftinferior",
    "parenrightinferior",
    "Circumflexsmall",
    "hyphensuperior",
    "Gravesmall",
    "Asmall",
    "Bsmall",
    "Csmall",
    "Dsmall",
    "Esmall",
    "Fsmall",
    "Gsmall",
    "Hsmall",
    "Ismall",
    "Jsmall",
    "Ksmall",
    "Lsmall",
    "Msmall",
    "Nsmall",
    "Osmall",
    "Psmall",
    "Qsmall",
    "Rsmall",
    "Ssmall",
    "Tsmall",
    "Usmall",
    "Vsmall",
    "Wsmall",
    "Xsmall",
    "Ysmall",
    "Zsmall",
    "colonmonetary",
    "onefitted",
    "rupiah",
    "Tildesmall",
    "exclamdownsmall",
    "centoldstyle",
    "Lslashsmall",
    "Scaronsmall",
    "Zcaronsmall",
    "Dieresissmall",
    "Brevesmall",
    "Caronsmall",
    "Dotaccentsmall",
    "Macronsmall",
    "figuredash",
    "hypheninferior",
    "Ogoneksmall",
    "Ringsmall",
    "Cedillasmall",
    "questiondownsmall",
    "oneeighth",
    "threeeighths",
    "fiveeighths",
    "seveneighths",
    "onethird",
    "twothirds",
    "zerosuperior",
    "foursuperior",
    "fivesuperior",
    "sixsuperior",
    "sevensuperior",
    "eightsuperior",
    "ninesuperior",
    "zeroinferior",
    "oneinferior",
    "twoinferior",
    "threeinferior",
    "fourinferior",
    "fiveinferior",
    "sixinferior",
    "seveninferior",
    "eightinferior",
    "nineinferior",
    "centinferior",
    "dollarinferior",
    "periodinferior",
    "commainferior",
    "Agravesmall",
    "Aacutesmall",
    "Acircumflexsmall",
    "Atildesmall",
    "Adieresissmall",
    "Aringsmall",
    "AEsmall",
    "Ccedillasmall",
    "Egravesmall",
    "Eacutesmall",
    "Ecircumflexsmall",
    "Edieresissmall",
    "Igravesmall",
    "Iacutesmall",
    "Icircumflexsmall",
    "Idieresissmall",
    "Ethsmall",
    "Ntildesmall",
    "Ogravesmall",
    "Oacutesmall",
    "Ocircumflexsmall",
    "Otildesmall",
    "Odieresissmall",
    "OEsmall",
    "Oslashsmall",
    "Ugravesmall",
    "Uacutesmall",
    "Ucircumflexsmall",
    "Udieresissmall",
    "Yacutesmall",
    "Thornsmall",
    "Ydieresissmall",
    "001.000",
    "001.001",
    "001.002",
    "001.003",
    "Black",
    "Bold",
    "Book",
    "Light",
    "Medium",
    "Regular",
    "Roman",
    "Semibold",
];

/// The number of glyphs the ISOAdobe charset names: string ids 0 to 228
/// in order.
pub const ISO_ADOBE_COUNT: u16 = 229;

/// A dictionary operator: the byte of a one-byte operator, or
/// `0x0c00 | b` for the escaped operator `12 b`.
pub type DictOp = u16;

/// The escaped operator `12 b`.
pub const fn esc(b: u8) -> DictOp {
    0x0c00 | b as DictOp
}

/// The dictionary operators the reader and writer name.
pub mod op {
    use super::{DictOp, esc};

    pub const VERSION: DictOp = 0;
    pub const NOTICE: DictOp = 1;
    pub const FULL_NAME: DictOp = 2;
    pub const FAMILY_NAME: DictOp = 3;
    pub const WEIGHT: DictOp = 4;
    pub const FONT_BBOX: DictOp = 5;
    pub const BLUE_VALUES: DictOp = 6;
    pub const OTHER_BLUES: DictOp = 7;
    pub const FAMILY_BLUES: DictOp = 8;
    pub const FAMILY_OTHER_BLUES: DictOp = 9;
    pub const STD_HW: DictOp = 10;
    pub const STD_VW: DictOp = 11;
    pub const UNIQUE_ID: DictOp = 13;
    pub const XUID: DictOp = 14;
    pub const CHARSET: DictOp = 15;
    pub const ENCODING: DictOp = 16;
    pub const CHARSTRINGS: DictOp = 17;
    pub const PRIVATE: DictOp = 18;
    pub const SUBRS: DictOp = 19;
    pub const DEFAULT_WIDTH_X: DictOp = 20;
    pub const NOMINAL_WIDTH_X: DictOp = 21;
    pub const COPYRIGHT: DictOp = esc(0);
    pub const IS_FIXED_PITCH: DictOp = esc(1);
    pub const ITALIC_ANGLE: DictOp = esc(2);
    pub const UNDERLINE_POSITION: DictOp = esc(3);
    pub const UNDERLINE_THICKNESS: DictOp = esc(4);
    pub const PAINT_TYPE: DictOp = esc(5);
    pub const CHARSTRING_TYPE: DictOp = esc(6);
    pub const FONT_MATRIX: DictOp = esc(7);
    pub const STROKE_WIDTH: DictOp = esc(8);
    pub const BLUE_SCALE: DictOp = esc(9);
    pub const BLUE_SHIFT: DictOp = esc(10);
    pub const BLUE_FUZZ: DictOp = esc(11);
    pub const STEM_SNAP_H: DictOp = esc(12);
    pub const STEM_SNAP_V: DictOp = esc(13);
    pub const FORCE_BOLD: DictOp = esc(14);
    pub const LANGUAGE_GROUP: DictOp = esc(17);
    pub const EXPANSION_FACTOR: DictOp = esc(18);
    pub const INITIAL_RANDOM_SEED: DictOp = esc(19);
    pub const SYNTHETIC_BASE: DictOp = esc(20);
    pub const POSTSCRIPT: DictOp = esc(21);
    pub const BASE_FONT_NAME: DictOp = esc(22);
    pub const BASE_FONT_BLEND: DictOp = esc(23);
    pub const ROS: DictOp = esc(30);
    pub const CID_FONT_VERSION: DictOp = esc(31);
    pub const CID_FONT_REVISION: DictOp = esc(32);
    pub const CID_FONT_TYPE: DictOp = esc(33);
    pub const CID_COUNT: DictOp = esc(34);
    pub const UID_BASE: DictOp = esc(35);
    pub const FD_ARRAY: DictOp = esc(36);
    pub const FD_SELECT: DictOp = esc(37);
    pub const FONT_NAME: DictOp = esc(38);
}

/// The default `FontMatrix` of a program whose top dictionary has none.
pub const DEFAULT_FONT_MATRIX: [f32; 6] = [0.001, 0.0, 0.0, 0.001, 0.0, 0.0];

/// Big-endian reads with bounds checks; a read past the end is a
/// truncation of the named structure.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    what: &'static str,
}

impl<'a> Reader<'a> {
    fn at(bytes: &'a [u8], pos: usize, what: &'static str) -> Self {
        Reader { bytes, pos, what }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], FontError> {
        let slice = self
            .pos
            .checked_add(n)
            .and_then(|end| self.bytes.get(self.pos..end))
            .ok_or(FontError::Truncated(self.what))?;
        self.pos += n;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, FontError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, FontError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, FontError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    /// An offset of `size` bytes (one to four).
    fn offset(&mut self, size: u8) -> Result<usize, FontError> {
        if !(1..=4).contains(&size) {
            return Err(FontError::Malformed("offset size"));
        }
        let mut value = 0usize;
        for &b in self.take(usize::from(size))? {
            value = value << 8 | usize::from(b);
        }
        Ok(value)
    }
}

/// An INDEX: `count` items addressed by offsets, returned as slices of
/// the program along with the position just past the structure.
fn read_index<'a>(
    bytes: &'a [u8],
    pos: usize,
    what: &'static str,
) -> Result<(Vec<&'a [u8]>, usize), FontError> {
    let mut r = Reader::at(bytes, pos, what);
    let count = usize::from(r.u16()?);
    if count == 0 {
        return Ok((Vec::new(), r.pos));
    }
    let off_size = r.u8()?;
    let mut offsets = Vec::with_capacity(count + 1);
    for _ in 0..=count {
        offsets.push(r.offset(off_size)?);
    }
    // Offsets are relative to the byte before the data, so the data
    // starts at the first offset, which the format fixes at one.
    let base = r.pos - 1;
    let mut items = Vec::with_capacity(count);
    for pair in offsets.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start > end || start == 0 {
            return Err(FontError::Malformed(what));
        }
        let slice = base
            .checked_add(end)
            .and_then(|e| bytes.get(base + start..e))
            .ok_or(FontError::Truncated(what))?;
        items.push(slice);
    }
    let end = base + offsets[count];
    Ok((items, end))
}

/// A parsed dictionary: operators in the order they appear, each with
/// its operands as numbers. Offsets are exact in `f64`; real operands
/// keep the value the nibble encoding gave.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Dict {
    pub entries: Vec<(DictOp, Vec<f64>)>,
}

impl Dict {
    pub fn parse(bytes: &[u8]) -> Result<Dict, FontError> {
        let mut entries = Vec::new();
        let mut operands = Vec::new();
        let mut pos = 0;
        while pos < bytes.len() {
            let b = bytes[pos];
            pos += 1;
            let mut r = Reader::at(bytes, pos, "dictionary");
            match b {
                32..=246 => operands.push(f64::from(i32::from(b) - 139)),
                247..=250 => {
                    let w = r.u8()?;
                    operands.push(f64::from((i32::from(b) - 247) * 256 + i32::from(w) + 108));
                }
                251..=254 => {
                    let w = r.u8()?;
                    operands.push(f64::from(-(i32::from(b) - 251) * 256 - i32::from(w) - 108));
                }
                28 => operands.push(f64::from(r.u16()? as i16)),
                29 => operands.push(f64::from(r.u32()? as i32)),
                30 => operands.push(parse_real(&mut r)?),
                12 => {
                    let e = r.u8()?;
                    entries.push((esc(e), std::mem::take(&mut operands)));
                }
                0..=21 => entries.push((DictOp::from(b), std::mem::take(&mut operands))),
                _ => return Err(FontError::Malformed("dictionary operator")),
            }
            pos = r.pos;
        }
        if !operands.is_empty() {
            return Err(FontError::Malformed("dictionary operands"));
        }
        Ok(Dict { entries })
    }

    /// The operands of `op`, the last occurrence winning.
    pub fn get(&self, op: DictOp) -> Option<&[f64]> {
        self.entries
            .iter()
            .rev()
            .find(|(o, _)| *o == op)
            .map(|(_, v)| v.as_slice())
    }

    /// The single operand of `op`.
    pub fn number(&self, op: DictOp) -> Option<f64> {
        match self.get(op)? {
            &[v] => Some(v),
            _ => None,
        }
    }

    fn offset(&self, op: DictOp) -> Result<Option<usize>, FontError> {
        match self.number(op) {
            None => Ok(None),
            Some(v) if v >= 0.0 && v.fract() == 0.0 => Ok(Some(v as usize)),
            Some(_) => Err(FontError::Malformed("dictionary offset")),
        }
    }
}

/// A real operand: nibbles until the terminator.
fn parse_real(r: &mut Reader<'_>) -> Result<f64, FontError> {
    let mut text = String::new();
    loop {
        let b = r.u8()?;
        let mut done = false;
        for nibble in [b >> 4, b & 0x0f] {
            match nibble {
                0..=9 => text.push((b'0' + nibble) as char),
                0xa => text.push('.'),
                0xb => text.push('E'),
                0xc => text.push_str("E-"),
                0xe => text.push('-'),
                0xf => {
                    done = true;
                    break;
                }
                _ => return Err(FontError::Malformed("real operand")),
            }
        }
        if done {
            break;
        }
    }
    if text.is_empty() {
        return Ok(0.0);
    }
    text.parse()
        .map_err(|_| FontError::Malformed("real operand"))
}

/// A private dictionary with its local subroutines and the two width
/// values the charstrings are relative to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrivateDict {
    /// Every entry but `Subrs`, kept as numbers so a writer can copy the
    /// hinting values verbatim.
    pub dict: Dict,
    pub subrs: Vec<Vec<u8>>,
    pub default_width_x: f32,
    pub nominal_width_x: f32,
}

impl PrivateDict {
    fn parse(bytes: &[u8], offset: usize, size: usize) -> Result<PrivateDict, FontError> {
        let data = offset
            .checked_add(size)
            .and_then(|end| bytes.get(offset..end))
            .ok_or(FontError::Truncated("private dictionary"))?;
        let mut dict = Dict::parse(data)?;
        let subrs = match dict.offset(op::SUBRS)? {
            Some(relative) => {
                let at = offset
                    .checked_add(relative)
                    .ok_or(FontError::Truncated("local subroutines"))?;
                read_index(bytes, at, "local subroutines")?.0
            }
            None => Vec::new(),
        };
        dict.entries.retain(|(o, _)| *o != op::SUBRS);
        let default_width_x = dict.number(op::DEFAULT_WIDTH_X).unwrap_or(0.0) as f32;
        let nominal_width_x = dict.number(op::NOMINAL_WIDTH_X).unwrap_or(0.0) as f32;
        Ok(PrivateDict {
            dict,
            subrs: subrs.into_iter().map(<[u8]>::to_vec).collect(),
            default_width_x,
            nominal_width_x,
        })
    }

    /// The first operand of a numeric entry, `StdVW` say.
    pub fn number(&self, op: DictOp) -> Option<f32> {
        self.dict.get(op)?.first().map(|&v| v as f32)
    }
}

/// The code-to-glyph mapping of a name-keyed program.
#[derive(Clone, Debug, PartialEq)]
pub enum CffEncoding {
    /// The standard encoding, resolved through glyph names.
    Standard,
    /// A custom table, code to glyph index.
    Custom(Box<[Option<u16>; 256]>),
}

/// Where a program's private data lives.
#[derive(Clone, Debug, PartialEq)]
pub enum Privates {
    /// A name-keyed program: one private dictionary.
    Single(PrivateDict),
    /// A CID-keyed program: one per font dictionary, selected per glyph.
    Cid {
        dicts: Vec<PrivateDict>,
        /// The font dictionary index of each glyph.
        select: Vec<u8>,
    },
}

/// The `ROS` operator of a CID-keyed program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ros {
    pub registry: Vec<u8>,
    pub ordering: Vec<u8>,
    pub supplement: i32,
}

/// Where a charstring lives: a glyph's own, or a subroutine of either
/// kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Site {
    Glyph(u16),
    Local(usize),
    Global(usize),
}

/// The trace of a glyph's interpretation: which subroutines it ran, and
/// how many mask bytes each hint mask it executed took, by charstring
/// and offset of the mask operator. A mask's length depends on the stems
/// declared before it, which may lie in another charstring, so a writer
/// that must step over mask bytes reads it from here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reached {
    pub local: BTreeSet<usize>,
    pub global: BTreeSet<usize>,
    pub masks: BTreeSet<(Site, usize, usize)>,
}

/// A parsed CFF program with its glyph cache.
pub struct CffProgram {
    name: Vec<u8>,
    top: Dict,
    strings: Vec<Vec<u8>>,
    global_subrs: Vec<Vec<u8>>,
    charstrings: Vec<Vec<u8>>,
    /// Glyph index to string id (name-keyed) or CID (CID-keyed).
    charset: Vec<u16>,
    encoding: CffEncoding,
    privates: Privates,
    /// Name to glyph index (name-keyed) or CID to glyph index.
    lookup: HashMap<Vec<u8>, u16>,
    cids: HashMap<u16, u16>,
    cache: RefCell<HashMap<u16, Rc<Glyph>>>,
}

impl std::fmt::Debug for CffProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CffProgram")
            .field("name", &String::from_utf8_lossy(&self.name))
            .field("glyphs", &self.charstrings.len())
            .field("cid_keyed", &self.is_cid_keyed())
            .finish()
    }
}

/// Every font in the program, in name-index order.
pub fn parse_fonts(bytes: &[u8]) -> Result<Vec<CffProgram>, FontError> {
    let mut r = Reader::at(bytes, 0, "header");
    let major = r.u8()?;
    r.u8()?;
    let hdr_size = usize::from(r.u8()?);
    r.u8()?;
    if major != 1 {
        return Err(FontError::Unsupported("CFF major version"));
    }
    let (names, pos) = read_index(bytes, hdr_size, "name index")?;
    let (tops, pos) = read_index(bytes, pos, "top dictionary index")?;
    let (strings, pos) = read_index(bytes, pos, "string index")?;
    let (global_subrs, _) = read_index(bytes, pos, "global subroutine index")?;
    if names.len() != tops.len() {
        return Err(FontError::Malformed("top dictionary count"));
    }
    let strings: Vec<Vec<u8>> = strings.into_iter().map(<[u8]>::to_vec).collect();
    let global_subrs: Vec<Vec<u8>> = global_subrs.into_iter().map(<[u8]>::to_vec).collect();
    names
        .iter()
        .zip(&tops)
        .map(|(name, top)| CffProgram::from_top(bytes, name, top, &strings, &global_subrs))
        .collect()
}

impl CffProgram {
    /// The first font of the program.
    pub fn parse(bytes: &[u8]) -> Result<CffProgram, FontError> {
        parse_fonts(bytes)?
            .into_iter()
            .next()
            .ok_or(FontError::Malformed("empty name index"))
    }

    fn from_top(
        bytes: &[u8],
        name: &[u8],
        top_bytes: &[u8],
        strings: &[Vec<u8>],
        global_subrs: &[Vec<u8>],
    ) -> Result<CffProgram, FontError> {
        let top = Dict::parse(top_bytes)?;
        if top.number(op::CHARSTRING_TYPE).is_some_and(|t| t != 2.0) {
            return Err(FontError::Unsupported("charstring type"));
        }
        let charstrings_at = top
            .offset(op::CHARSTRINGS)?
            .ok_or(FontError::Malformed("no charstrings"))?;
        let (charstrings, _) = read_index(bytes, charstrings_at, "charstrings")?;
        let charstrings: Vec<Vec<u8>> = charstrings.into_iter().map(<[u8]>::to_vec).collect();
        let n_glyphs =
            u16::try_from(charstrings.len()).map_err(|_| FontError::Malformed("glyph count"))?;
        if n_glyphs == 0 {
            return Err(FontError::Malformed("no glyphs"));
        }
        let cid_keyed = top.get(op::ROS).is_some();

        let charset = match top.offset(op::CHARSET)?.unwrap_or(0) {
            0 => (0..n_glyphs).collect(),
            1 | 2 => return Err(FontError::Unsupported("expert charset")),
            at => parse_charset(bytes, at, n_glyphs)?,
        };

        let privates = if cid_keyed {
            let fd_array_at = top
                .offset(op::FD_ARRAY)?
                .ok_or(FontError::Malformed("no font dictionary array"))?;
            let (fds, _) = read_index(bytes, fd_array_at, "font dictionary array")?;
            let mut dicts = Vec::with_capacity(fds.len());
            for fd in fds {
                let fd = Dict::parse(fd)?;
                dicts.push(private_of(bytes, &fd)?.unwrap_or_default());
            }
            if dicts.is_empty() {
                return Err(FontError::Malformed("empty font dictionary array"));
            }
            let select = match top.offset(op::FD_SELECT)? {
                Some(at) => parse_fd_select(bytes, at, n_glyphs)?,
                None => vec![0; usize::from(n_glyphs)],
            };
            if select.iter().any(|&fd| usize::from(fd) >= dicts.len()) {
                return Err(FontError::Malformed("font dictionary select"));
            }
            Privates::Cid { dicts, select }
        } else {
            Privates::Single(private_of(bytes, &top)?.unwrap_or_default())
        };

        let mut program = CffProgram {
            name: name.to_vec(),
            top,
            strings: strings.to_vec(),
            global_subrs: global_subrs.to_vec(),
            charstrings,
            charset,
            encoding: CffEncoding::Standard,
            privates,
            lookup: HashMap::new(),
            cids: HashMap::new(),
            cache: RefCell::new(HashMap::new()),
        };
        if cid_keyed {
            for (gid, &cid) in program.charset.iter().enumerate() {
                program.cids.entry(cid).or_insert(gid as u16);
            }
        } else {
            for gid in 0..n_glyphs {
                let name = program.sid_name(program.charset[usize::from(gid)]).to_vec();
                program.lookup.entry(name).or_insert(gid);
            }
            program.encoding = match program.top.offset(op::ENCODING)?.unwrap_or(0) {
                0 => CffEncoding::Standard,
                1 => return Err(FontError::Unsupported("expert encoding")),
                at => CffEncoding::Custom(Box::new(parse_encoding(bytes, at, &program)?)),
            };
        }
        Ok(program)
    }

    /// The font's name from the name index.
    pub fn name(&self) -> &[u8] {
        &self.name
    }

    pub fn top(&self) -> &Dict {
        &self.top
    }

    /// The program's own strings, string ids from 391 up.
    pub fn strings(&self) -> &[Vec<u8>] {
        &self.strings
    }

    /// The text of a string id: a standard string or one of the program's.
    pub fn sid_name(&self, sid: u16) -> &[u8] {
        match usize::from(sid).checked_sub(STANDARD_STRINGS.len()) {
            None => STANDARD_STRINGS[usize::from(sid)].as_bytes(),
            Some(k) => self.strings.get(k).map_or(b"", Vec::as_slice),
        }
    }

    /// The text a string-valued top entry names (`Notice`, `FullName`,
    /// `FamilyName`, `Weight`).
    pub fn top_string(&self, op: DictOp) -> Option<&[u8]> {
        let sid = self.top.number(op)?;
        u16::try_from(sid as i64).ok().map(|sid| self.sid_name(sid))
    }

    pub fn is_cid_keyed(&self) -> bool {
        matches!(self.privates, Privates::Cid { .. })
    }

    /// The registry, ordering, and supplement of a CID-keyed program.
    pub fn ros(&self) -> Option<Ros> {
        match self.top.get(op::ROS)? {
            &[registry, ordering, supplement] => Some(Ros {
                registry: self.sid_name(registry as u16).to_vec(),
                ordering: self.sid_name(ordering as u16).to_vec(),
                supplement: supplement as i32,
            }),
            _ => None,
        }
    }

    pub fn cid_count(&self) -> u32 {
        self.top.number(op::CID_COUNT).unwrap_or(8720.0) as u32
    }

    pub fn font_matrix(&self) -> [f32; 6] {
        match self.top.get(op::FONT_MATRIX) {
            Some(&[a, b, c, d, e, f]) => {
                [a as f32, b as f32, c as f32, d as f32, e as f32, f as f32]
            }
            _ => DEFAULT_FONT_MATRIX,
        }
    }

    /// Whether the top dictionary carries a `FontMatrix` of its own.
    pub fn has_font_matrix(&self) -> bool {
        self.top.get(op::FONT_MATRIX).is_some()
    }

    pub fn font_bbox(&self) -> [f32; 4] {
        match self.top.get(op::FONT_BBOX) {
            Some(&[a, b, c, d]) => [a as f32, b as f32, c as f32, d as f32],
            _ => [0.0; 4],
        }
    }

    pub fn italic_angle(&self) -> f32 {
        self.top.number(op::ITALIC_ANGLE).unwrap_or(0.0) as f32
    }

    pub fn is_fixed_pitch(&self) -> bool {
        self.top.number(op::IS_FIXED_PITCH).unwrap_or(0.0) != 0.0
    }

    pub fn paint_type(&self) -> i32 {
        self.top.number(op::PAINT_TYPE).unwrap_or(0.0) as i32
    }

    pub fn glyph_count(&self) -> u16 {
        self.charstrings.len() as u16
    }

    /// The charstring of a glyph index.
    pub fn charstring(&self, gid: u16) -> Result<&[u8], FontError> {
        self.charstrings
            .get(usize::from(gid))
            .map(Vec::as_slice)
            .ok_or(FontError::GlyphIndex(gid))
    }

    pub fn global_subrs(&self) -> &[Vec<u8>] {
        &self.global_subrs
    }

    /// The private data: the one dictionary of a name-keyed program, or
    /// the font dictionary array of a CID-keyed one.
    pub fn privates(&self) -> &Privates {
        &self.privates
    }

    /// The private dictionary of a name-keyed program.
    pub fn private(&self) -> Option<&PrivateDict> {
        match &self.privates {
            Privates::Single(dict) => Some(dict),
            Privates::Cid { .. } => None,
        }
    }

    /// The private dictionary a glyph's charstring runs under.
    pub fn private_for(&self, gid: u16) -> Result<&PrivateDict, FontError> {
        match &self.privates {
            Privates::Single(dict) => Ok(dict),
            Privates::Cid { dicts, select } => {
                let fd = select
                    .get(usize::from(gid))
                    .ok_or(FontError::GlyphIndex(gid))?;
                Ok(&dicts[usize::from(*fd)])
            }
        }
    }

    /// The font dictionary index of a glyph of a CID-keyed program.
    pub fn fd_index(&self, gid: u16) -> Option<u8> {
        match &self.privates {
            Privates::Single(_) => None,
            Privates::Cid { select, .. } => select.get(usize::from(gid)).copied(),
        }
    }

    /// Glyph index to string id or CID.
    pub fn charset(&self) -> &[u16] {
        &self.charset
    }

    /// The glyph names of a name-keyed program, in glyph-index order.
    pub fn charset_names(&self) -> Vec<&[u8]> {
        if self.is_cid_keyed() {
            return Vec::new();
        }
        self.charset.iter().map(|&sid| self.sid_name(sid)).collect()
    }

    /// The name of a glyph index of a name-keyed program.
    pub fn glyph_name(&self, gid: u16) -> Option<&[u8]> {
        if self.is_cid_keyed() {
            return None;
        }
        self.charset
            .get(usize::from(gid))
            .map(|&sid| self.sid_name(sid))
    }

    pub fn gid(&self, name: &[u8]) -> Option<u16> {
        self.lookup.get(name).copied()
    }

    pub fn gid_of_cid(&self, cid: u16) -> Option<u16> {
        self.cids.get(&cid).copied()
    }

    /// The names the program defines, in sorted order.
    pub fn glyph_names(&self) -> Vec<&[u8]> {
        let mut names: Vec<&[u8]> = self.lookup.keys().map(Vec::as_slice).collect();
        names.sort_unstable();
        names
    }

    pub fn cff_encoding(&self) -> &CffEncoding {
        &self.encoding
    }

    /// Whether the program uses the standard encoding rather than its own.
    pub fn has_standard_encoding(&self) -> bool {
        matches!(self.encoding, CffEncoding::Standard)
    }

    /// Code to glyph index, the standard encoding resolved through the
    /// glyph names; empty for a CID-keyed program.
    pub fn encoding(&self) -> [Option<u16>; 256] {
        match &self.encoding {
            CffEncoding::Custom(table) => **table,
            CffEncoding::Standard if self.is_cid_keyed() => [None; 256],
            CffEncoding::Standard => {
                let mut table = [None; 256];
                for (code, name) in STANDARD_ENCODING.iter().enumerate() {
                    table[code] = name.and_then(|n| self.gid(n.as_bytes()));
                }
                table
            }
        }
    }

    /// The glyph at `gid`, interpreted on first request.
    pub fn glyph_by_index(&self, gid: u16) -> Result<Rc<Glyph>, FontError> {
        if let Some(glyph) = self.cache.borrow().get(&gid) {
            return Ok(glyph.clone());
        }
        let interpreted = charstring::interpret(self, gid)?;
        let glyph = Rc::new(interpreted.glyph);
        self.cache.borrow_mut().insert(gid, glyph.clone());
        Ok(glyph)
    }

    /// The glyph named `name` (name-keyed programs): `Ok(None)` when
    /// there is none.
    pub fn glyph(&self, name: &[u8]) -> Result<Option<Rc<Glyph>>, FontError> {
        match self.gid(name) {
            Some(gid) => self.glyph_by_index(gid).map(Some),
            None => Ok(None),
        }
    }

    /// The glyph of a CID (CID-keyed programs): `Ok(None)` when there is
    /// none.
    pub fn glyph_by_cid(&self, cid: u16) -> Result<Option<Rc<Glyph>>, FontError> {
        match self.gid_of_cid(cid) {
            Some(gid) => self.glyph_by_index(gid).map(Some),
            None => Ok(None),
        }
    }

    /// The subroutines the charstring at `gid` runs, transitively and
    /// through the components of an accented glyph, with the hint masks
    /// it executes.
    pub fn reached_subrs(&self, gid: u16) -> Result<Reached, FontError> {
        Ok(charstring::interpret(self, gid)?.reached)
    }

    /// The glyph indices an accented glyph composes, empty otherwise.
    pub fn components(&self, gid: u16) -> Result<Vec<u16>, FontError> {
        Ok(charstring::interpret(self, gid)?.components)
    }
}

fn private_of(bytes: &[u8], dict: &Dict) -> Result<Option<PrivateDict>, FontError> {
    match dict.get(op::PRIVATE) {
        None => Ok(None),
        Some(&[size, offset]) if size >= 0.0 && offset >= 0.0 => Ok(Some(PrivateDict::parse(
            bytes,
            offset as usize,
            size as usize,
        )?)),
        Some(_) => Err(FontError::Malformed("private entry")),
    }
}

fn parse_charset(bytes: &[u8], at: usize, n_glyphs: u16) -> Result<Vec<u16>, FontError> {
    let mut r = Reader::at(bytes, at, "charset");
    let format = r.u8()?;
    let mut charset = Vec::with_capacity(usize::from(n_glyphs));
    charset.push(0);
    match format {
        0 => {
            while charset.len() < usize::from(n_glyphs) {
                charset.push(r.u16()?);
            }
        }
        1 | 2 => {
            while charset.len() < usize::from(n_glyphs) {
                let first = r.u16()?;
                let left = if format == 1 {
                    u16::from(r.u8()?)
                } else {
                    r.u16()?
                };
                for k in 0..=left {
                    if charset.len() == usize::from(n_glyphs) {
                        break;
                    }
                    charset.push(
                        first
                            .checked_add(k)
                            .ok_or(FontError::Malformed("charset range"))?,
                    );
                }
            }
        }
        _ => return Err(FontError::Malformed("charset format")),
    }
    Ok(charset)
}

fn parse_encoding(
    bytes: &[u8],
    at: usize,
    program: &CffProgram,
) -> Result<[Option<u16>; 256], FontError> {
    let mut r = Reader::at(bytes, at, "encoding");
    let format = r.u8()?;
    let mut table = [None; 256];
    let n_glyphs = program.glyph_count();
    match format & 0x7f {
        0 => {
            let n_codes = r.u8()?;
            for k in 0..u16::from(n_codes) {
                let code = r.u8()?;
                let gid = k + 1;
                if gid < n_glyphs {
                    table[usize::from(code)] = Some(gid);
                }
            }
        }
        1 => {
            let n_ranges = r.u8()?;
            let mut gid: u16 = 1;
            for _ in 0..n_ranges {
                let first = r.u8()?;
                let left = r.u8()?;
                for k in 0..=u16::from(left) {
                    let code = u16::from(first) + k;
                    if code < 256 && gid < n_glyphs {
                        table[usize::from(code)] = Some(gid);
                    }
                    gid = gid.saturating_add(1);
                }
            }
        }
        _ => return Err(FontError::Malformed("encoding format")),
    }
    if format & 0x80 != 0 {
        let n_sups = r.u8()?;
        for _ in 0..n_sups {
            let code = r.u8()?;
            let sid = r.u16()?;
            if let Some(gid) = program.charset.iter().position(|&s| s == sid) {
                table[usize::from(code)] = Some(gid as u16);
            }
        }
    }
    Ok(table)
}

fn parse_fd_select(bytes: &[u8], at: usize, n_glyphs: u16) -> Result<Vec<u8>, FontError> {
    let mut r = Reader::at(bytes, at, "font dictionary select");
    let format = r.u8()?;
    let n = usize::from(n_glyphs);
    match format {
        0 => Ok(r.take(n)?.to_vec()),
        3 => {
            let n_ranges = r.u16()?;
            let mut select = vec![0u8; n];
            let mut first = r.u16()?;
            for _ in 0..n_ranges {
                let fd = r.u8()?;
                let next = r.u16()?;
                if next < first {
                    return Err(FontError::Malformed("font dictionary select range"));
                }
                for slot in select
                    .iter_mut()
                    .take(usize::from(next))
                    .skip(usize::from(first))
                {
                    *slot = fd;
                }
                first = next;
            }
            Ok(select)
        }
        _ => Err(FontError::Malformed("font dictionary select format")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_strings_are_complete() {
        assert_eq!(STANDARD_STRINGS.len(), 391);
        assert_eq!(STANDARD_STRINGS[0], ".notdef");
        assert_eq!(STANDARD_STRINGS[1], "space");
        assert_eq!(STANDARD_STRINGS[34], "A");
        assert_eq!(STANDARD_STRINGS[66], "a");
        assert_eq!(STANDARD_STRINGS[95], "asciitilde");
        assert_eq!(STANDARD_STRINGS[150], "onesuperior");
        assert_eq!(STANDARD_STRINGS[228], "zcaron");
        assert_eq!(STANDARD_STRINGS[229], "exclamsmall");
        assert_eq!(STANDARD_STRINGS[378], "Ydieresissmall");
        assert_eq!(STANDARD_STRINGS[390], "Semibold");
        let mut sorted = STANDARD_STRINGS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 391, "no duplicates");
        // Every standard-encoding name is a standard string within the
        // ISOAdobe range, which the standard encoding resolves through.
        for name in STANDARD_ENCODING.iter().flatten() {
            let sid = STANDARD_STRINGS.iter().position(|s| s == name).unwrap();
            assert!(sid < usize::from(ISO_ADOBE_COUNT), "{name}");
        }
    }

    #[test]
    fn dictionaries_decode_every_operand_form() {
        // 1000 (247-form), -1000, 28 with i16, 29 with i32, a real, then
        // a one-byte and a two-byte operator.
        let mut bytes = vec![];
        write::dict_number(1000.0, &mut bytes);
        write::dict_number(-1000.0, &mut bytes);
        bytes.extend_from_slice(&[28, 0x80, 0x00]);
        bytes.extend_from_slice(&[29, 0x00, 0x01, 0x00, 0x00]);
        bytes.extend_from_slice(&[30, 0xe2, 0xa5, 0xc3, 0xff]);
        bytes.push(op::FONT_BBOX as u8);
        write::dict_number(0.5, &mut bytes);
        bytes.extend_from_slice(&[12, 2]);
        let dict = Dict::parse(&bytes).unwrap();
        assert_eq!(
            dict.get(op::FONT_BBOX).unwrap(),
            &[1000.0, -1000.0, -32768.0, 65536.0, -2.5e-3]
        );
        assert_eq!(dict.number(op::ITALIC_ANGLE), Some(0.5));
        assert_eq!(dict.number(op::FONT_BBOX), None);
        assert_eq!(dict.get(op::CHARSET), None);
        assert_eq!(
            Dict::parse(&[139]),
            Err(FontError::Malformed("dictionary operands"))
        );
        assert_eq!(
            Dict::parse(&[28, 0]),
            Err(FontError::Truncated("dictionary"))
        );
        assert_eq!(
            Dict::parse(&[30, 0xd0]),
            Err(FontError::Malformed("real operand"))
        );
        assert_eq!(
            Dict::parse(&[22]),
            Err(FontError::Malformed("dictionary operator"))
        );
    }

    #[test]
    fn indexes_read_back_what_the_writer_produces() {
        let items = vec![b"ab".to_vec(), Vec::new(), vec![0u8; 300]];
        let bytes = write::index(&items);
        let (read, end) = read_index(&bytes, 0, "index").unwrap();
        assert_eq!(read.len(), 3);
        assert_eq!(read[0], b"ab");
        assert!(read[1].is_empty());
        assert_eq!(read[2].len(), 300);
        assert_eq!(end, bytes.len());
        let none = write::index(&[]);
        let (empty, end) = read_index(&none, 0, "index").unwrap();
        assert!(empty.is_empty());
        assert_eq!(end, 2);
        assert_eq!(
            read_index(&[0, 1, 1, 1], 0, "index"),
            Err(FontError::Truncated("index"))
        );
        assert_eq!(
            read_index(&[0, 1, 1, 2, 1, 0], 0, "index"),
            Err(FontError::Malformed("index"))
        );
        assert_eq!(
            read_index(&[0, 1, 5, 1], 0, "index"),
            Err(FontError::Malformed("offset size"))
        );
    }

    #[test]
    fn charsets_and_select_tables_decode_in_every_format() {
        // Format 0: explicit string ids.
        let bytes = [0u8, 0, 5, 0, 7];
        assert_eq!(parse_charset(&bytes, 0, 3).unwrap(), vec![0, 5, 7]);
        // Format 1: one range of three from 10.
        let bytes = [1u8, 0, 10, 2];
        assert_eq!(parse_charset(&bytes, 0, 4).unwrap(), vec![0, 10, 11, 12]);
        // Format 2: a range wider than a byte, cut at the glyph count.
        let bytes = [2u8, 1, 0, 1, 0];
        assert_eq!(parse_charset(&bytes, 0, 3).unwrap(), vec![0, 256, 257]);
        assert_eq!(
            parse_charset(&[3u8], 0, 2),
            Err(FontError::Malformed("charset format"))
        );
        let select = parse_fd_select(&[0u8, 0, 0, 1, 1], 0, 4).unwrap();
        assert_eq!(select, vec![0, 0, 1, 1]);
        let bytes = [3u8, 0, 2, 0, 0, 0, 0, 2, 1, 0, 5];
        assert_eq!(parse_fd_select(&bytes, 0, 5).unwrap(), vec![0, 0, 1, 1, 1]);
        assert_eq!(
            parse_fd_select(&[1u8], 0, 1),
            Err(FontError::Malformed("font dictionary select format"))
        );
    }

    #[test]
    fn headers_and_structure_are_checked() {
        assert!(matches!(
            parse_fonts(&[1, 0]),
            Err(FontError::Truncated("header"))
        ));
        assert!(matches!(
            parse_fonts(&[2, 0, 4, 1, 0, 0, 0, 0, 0, 0, 0, 0]),
            Err(FontError::Unsupported("CFF major version"))
        ));
        let mut bytes = vec![1, 0, 4, 1];
        bytes.extend(write::index(&[b"F".to_vec()]));
        bytes.extend(write::index(&[Vec::new()]));
        bytes.extend(write::index(&[]));
        bytes.extend(write::index(&[]));
        assert!(matches!(
            parse_fonts(&bytes),
            Err(FontError::Malformed("no charstrings"))
        ));
    }
}
