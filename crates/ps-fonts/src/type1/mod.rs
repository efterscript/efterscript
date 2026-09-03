// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Type 1 font programs: the encryption the format uses for its `eexec`
//! section and its charstrings, a program snapshot whose charstrings
//! are interpreted into outlines, a reader for font files ([`file`]),
//! and the writer that regenerates a program for embedding ([`write`]).

pub(crate) mod charstring;
pub mod file;
pub mod write;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

use crate::outline::Glyph;
use crate::program::FontError;

pub use file::{FileEncoding, ParsedFont, parse_file};

/// The key of an `eexec` section.
pub const EEXEC_KEY: u16 = 55665;
/// The key of a charstring.
pub const CHARSTRING_KEY: u16 = 4330;

const C1: u16 = 52845;
const C2: u16 = 22719;

/// The stream cipher, one byte at a time.
#[derive(Clone, Copy, Debug)]
pub struct Decryptor {
    r: u16,
}

impl Decryptor {
    pub fn new(key: u16) -> Self {
        Decryptor { r: key }
    }

    /// The plain byte for the cipher byte `c`.
    pub fn byte(&mut self, c: u8) -> u8 {
        let plain = c ^ (self.r >> 8) as u8;
        self.r = (u16::from(c).wrapping_add(self.r))
            .wrapping_mul(C1)
            .wrapping_add(C2);
        plain
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Encryptor {
    r: u16,
}

impl Encryptor {
    pub fn new(key: u16) -> Self {
        Encryptor { r: key }
    }

    /// The cipher byte for the plain byte `p`.
    pub fn byte(&mut self, p: u8) -> u8 {
        let c = p ^ (self.r >> 8) as u8;
        self.r = (u16::from(c).wrapping_add(self.r))
            .wrapping_mul(C1)
            .wrapping_add(C2);
        c
    }
}

/// Decrypts `data` with `key` and drops the first `skip` plain bytes.
pub fn decrypt(key: u16, data: &[u8], skip: usize) -> Vec<u8> {
    let mut d = Decryptor::new(key);
    data.iter().map(|&c| d.byte(c)).skip(skip).collect()
}

/// Encrypts `lead` zero bytes followed by `data` with `key`: the inverse
/// of [`decrypt`] with `skip = lead`.
pub fn encrypt(key: u16, data: &[u8], lead: usize) -> Vec<u8> {
    let mut e = Encryptor::new(key);
    std::iter::repeat_n(0u8, lead)
        .chain(data.iter().copied())
        .map(|p| e.byte(p))
        .collect()
}

pub fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Whether an `eexec` section is in hexadecimal form: its first four
/// bytes are all hexadecimal digits.
pub fn is_hex_section(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[..4].iter().all(|&b| hex_value(b).is_some())
}

/// Decodes hexadecimal digits, skipping whitespace, up to the first byte
/// that is neither; a dangling digit is dropped.
pub fn decode_hex(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() / 2);
    let mut high = None;
    for &b in text {
        match hex_value(b) {
            Some(v) => match high.take() {
                None => high = Some(v),
                Some(h) => out.push(h << 4 | v),
            },
            None if b.is_ascii_whitespace() => {}
            None => break,
        }
    }
    out
}

/// The plain text of a whole `eexec` section, hexadecimal or binary, with
/// the four leading bytes dropped.
pub fn decrypt_section(section: &[u8]) -> Vec<u8> {
    if is_hex_section(section) {
        decrypt(EEXEC_KEY, &decode_hex(section), 4)
    } else {
        decrypt(EEXEC_KEY, section, 4)
    }
}

/// What the writer and the font descriptor need from the font dictionary
/// besides the charstrings and subroutines. The `FontInfo` and `Private`
/// entries are carried as printed PostScript text, `(key, value)`, since
/// only the interpreter can print them and this crate cannot call it;
/// the numbers a descriptor needs are read back from that text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Type1Dict {
    pub font_bbox: [f32; 4],
    pub paint_type: i32,
    /// `FontInfo` entries in dictionary order.
    pub font_info: Vec<(Vec<u8>, Vec<u8>)>,
    /// `Private` entries other than `Subrs`, `lenIV`, and the reading
    /// procedures, in dictionary order.
    pub private: Vec<(Vec<u8>, Vec<u8>)>,
}

/// The first number in a printed value: `[80]` and `80` both give 80.
fn first_number(text: &[u8]) -> Option<f32> {
    let text = std::str::from_utf8(text).ok()?;
    text.split(|c: char| c.is_whitespace() || matches!(c, '[' | ']' | '{' | '}'))
        .find(|token| !token.is_empty())?
        .parse()
        .ok()
}

impl Type1Dict {
    fn lookup<'a>(entries: &'a [(Vec<u8>, Vec<u8>)], key: &str) -> Option<&'a [u8]> {
        entries
            .iter()
            .find(|(k, _)| k == key.as_bytes())
            .map(|(_, v)| v.as_slice())
    }

    /// The first number of a `FontInfo` entry.
    pub fn font_info_number(&self, key: &str) -> Option<f32> {
        Self::lookup(&self.font_info, key).and_then(first_number)
    }

    /// A boolean `FontInfo` entry.
    pub fn font_info_bool(&self, key: &str) -> Option<bool> {
        match Self::lookup(&self.font_info, key)? {
            b"true" => Some(true),
            b"false" => Some(false),
            _ => None,
        }
    }

    /// The first number of a `Private` entry: `StdVW [80]` gives 80.
    pub fn private_number(&self, key: &str) -> Option<f32> {
        Self::lookup(&self.private, key).and_then(first_number)
    }
}

/// The charstrings and subroutines of a Type 1 font, decrypted once, with
/// a cache of the glyphs interpreted so far.
pub struct Type1Program {
    len_iv: i32,
    subrs: Vec<Vec<u8>>,
    charstrings: BTreeMap<Vec<u8>, Vec<u8>>,
    dict: Type1Dict,
    cache: RefCell<HashMap<Vec<u8>, Rc<Glyph>>>,
}

impl std::fmt::Debug for Type1Program {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Type1Program")
            .field("len_iv", &self.len_iv)
            .field("subrs", &self.subrs.len())
            .field("charstrings", &self.charstrings.len())
            .finish()
    }
}

impl Type1Program {
    /// A program from the encrypted strings a font dictionary holds:
    /// `Private/lenIV` (`-1` for unencrypted charstrings), `Private/Subrs`
    /// in index order, and `CharStrings` by name.
    pub fn new(len_iv: i32, subrs: Vec<Vec<u8>>, charstrings: Vec<(Vec<u8>, Vec<u8>)>) -> Self {
        let plain = |bytes: &[u8]| match usize::try_from(len_iv) {
            Ok(skip) => decrypt(CHARSTRING_KEY, bytes, skip),
            Err(_) => bytes.to_vec(),
        };
        Self::from_decrypted(
            len_iv,
            subrs.iter().map(|s| plain(s)).collect(),
            charstrings
                .into_iter()
                .map(|(name, cs)| (name, plain(&cs)))
                .collect(),
        )
    }

    /// A program whose subroutines and charstrings are already plain.
    pub fn from_decrypted(
        len_iv: i32,
        subrs: Vec<Vec<u8>>,
        charstrings: BTreeMap<Vec<u8>, Vec<u8>>,
    ) -> Self {
        Type1Program {
            len_iv,
            subrs,
            charstrings,
            dict: Type1Dict::default(),
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// Attaches the dictionary entries the writer and descriptor need.
    pub fn with_dict(mut self, dict: Type1Dict) -> Self {
        self.dict = dict;
        self
    }

    pub fn dict(&self) -> &Type1Dict {
        &self.dict
    }

    pub fn len_iv(&self) -> i32 {
        self.len_iv
    }

    /// The decrypted subroutines, in index order.
    pub fn subrs(&self) -> &[Vec<u8>] {
        &self.subrs
    }

    /// The decrypted charstrings by name.
    pub fn charstrings(&self) -> &BTreeMap<Vec<u8>, Vec<u8>> {
        &self.charstrings
    }

    /// The decrypted charstring of `name`.
    pub fn charstring(&self, name: &[u8]) -> Option<&[u8]> {
        self.charstrings.get(name).map(Vec::as_slice)
    }

    /// The glyph of `name`, interpreted on first request.
    pub fn glyph(&self, name: &[u8]) -> Result<Option<Rc<Glyph>>, FontError> {
        if let Some(glyph) = self.cache.borrow().get(name) {
            return Ok(Some(glyph.clone()));
        }
        let Some(interpreted) = charstring::interpret(self, name)? else {
            return Ok(None);
        };
        let glyph = Rc::new(interpreted.glyph);
        self.cache.borrow_mut().insert(name.to_vec(), glyph.clone());
        Ok(Some(glyph))
    }

    /// The names of the glyphs `name` composes with `seac`, empty for a
    /// plain glyph; what a subset must keep alongside it.
    pub fn seac_components(&self, name: &[u8]) -> Result<Vec<Vec<u8>>, FontError> {
        Ok(charstring::interpret(self, name)?
            .map(|interpreted| interpreted.components)
            .unwrap_or_default())
    }

    /// The indices of every subroutine the charstring of `name` runs,
    /// transitively — through nested calls, through the hint-replacement
    /// other-subroutine, and inside the components of a `seac` glyph;
    /// empty for a name the program lacks.
    pub fn reached_subrs(&self, name: &[u8]) -> Result<BTreeSet<usize>, FontError> {
        Ok(charstring::interpret(self, name)?
            .map(|interpreted| interpreted.subrs)
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cipher_round_trips_with_the_lead_bytes_dropped() {
        let plain = b"/x 42 def";
        let cipher = encrypt(EEXEC_KEY, plain, 4);
        assert_eq!(cipher.len(), plain.len() + 4);
        assert_eq!(decrypt(EEXEC_KEY, &cipher, 4), plain);
        let cs = encrypt(CHARSTRING_KEY, &[139, 14], 4);
        assert_eq!(decrypt(CHARSTRING_KEY, &cs, 4), vec![139, 14]);
        assert_eq!(decrypt(CHARSTRING_KEY, &cs, 0).len(), 6);
    }

    #[test]
    fn hex_sections_are_detected_and_decoded() {
        assert!(is_hex_section(b"0aF9zz"));
        assert!(!is_hex_section(b"0aF"));
        assert!(!is_hex_section(b"0a\x80F"));
        assert_eq!(decode_hex(b"4 1\n42 4"), vec![0x41, 0x42]);
        assert_eq!(decode_hex(b"41zz42"), vec![0x41]);
        let cipher = encrypt(EEXEC_KEY, b"(hi) print", 4);
        let hex: String = cipher.iter().map(|b| format!("{b:02X}")).collect();
        assert_eq!(decrypt_section(hex.as_bytes()), b"(hi) print");
    }

    #[test]
    fn binary_sections_decrypt_when_not_all_hex() {
        let mut lead = 0u8;
        let cipher = loop {
            let mut plain = vec![lead, 0, 0, 0];
            plain.extend_from_slice(b"(hi) print");
            let cipher = encrypt(EEXEC_KEY, &plain, 0);
            if !is_hex_section(&cipher) {
                break cipher;
            }
            lead += 1;
        };
        assert_eq!(decrypt_section(&cipher), b"(hi) print");
    }

    #[test]
    fn unencrypted_charstrings_are_taken_as_they_are() {
        let program = Type1Program::new(-1, vec![vec![11]], vec![(b"a".to_vec(), vec![139, 14])]);
        assert_eq!(program.charstring(b"a"), Some(&[139u8, 14][..]));
        assert_eq!(program.subrs(), &[vec![11]]);
        assert_eq!(program.len_iv(), -1);
        assert!(format!("{program:?}").contains("charstrings: 1"));
        let program = Type1Program::new(
            2,
            Vec::new(),
            vec![(b"a".to_vec(), encrypt(CHARSTRING_KEY, &[139, 14], 2))],
        );
        assert_eq!(program.charstring(b"a"), Some(&[139u8, 14][..]));
        assert_eq!(program.charstring(b"b"), None);
    }

    #[test]
    fn dictionary_numbers_are_read_back_from_their_printed_form() {
        let dict = Type1Dict {
            font_bbox: [0.0, -200.0, 1000.0, 900.0],
            paint_type: 0,
            font_info: vec![
                (b"ItalicAngle".to_vec(), b"-12.5".to_vec()),
                (b"isFixedPitch".to_vec(), b"true".to_vec()),
                (b"Notice".to_vec(), b"(text)".to_vec()),
            ],
            private: vec![
                (b"StdVW".to_vec(), b"[ 80 ]".to_vec()),
                (b"BlueValues".to_vec(), b"[]".to_vec()),
            ],
        };
        assert_eq!(dict.font_info_number("ItalicAngle"), Some(-12.5));
        assert_eq!(dict.font_info_bool("isFixedPitch"), Some(true));
        assert_eq!(dict.font_info_bool("Notice"), None);
        assert_eq!(dict.font_info_number("Notice"), None);
        assert_eq!(dict.private_number("StdVW"), Some(80.0));
        assert_eq!(dict.private_number("BlueValues"), None);
        assert_eq!(dict.private_number("StdHW"), None);
        let program =
            Type1Program::from_decrypted(4, Vec::new(), BTreeMap::new()).with_dict(dict.clone());
        assert_eq!(program.dict(), &dict);
        assert_eq!(
            Type1Program::from_decrypted(4, Vec::new(), BTreeMap::new())
                .dict()
                .paint_type,
            0
        );
    }
}
