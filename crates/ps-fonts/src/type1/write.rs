// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Regenerating a Type 1 font program from a snapshot: the cleartext
//! that builds the font dictionary, an `eexec` section in binary form
//! holding the private dictionary, the subroutines, and the kept
//! charstrings re-encrypted with `lenIV` 4, and the zero trailer. What a
//! PDF embeds as `FontFile`, restricted to the glyphs a document used.

use std::collections::BTreeSet;

use super::{CHARSTRING_KEY, EEXEC_KEY, Type1Program, encrypt, is_hex_section};
use crate::encoding::STANDARD_ENCODING;

/// `0 0 hsbw endchar`: the blank `.notdef` written when the program has
/// none, since a Type 1 program must define one.
const NOTDEF: [u8; 4] = [139, 139, 13, 14];

/// The `eexec` trailer: 512 zeros in eight lines and `cleartomark`.
pub const TRAILER: &str = "0000000000000000000000000000000000000000000000000000000000000000\n\
0000000000000000000000000000000000000000000000000000000000000000\n\
0000000000000000000000000000000000000000000000000000000000000000\n\
0000000000000000000000000000000000000000000000000000000000000000\n\
0000000000000000000000000000000000000000000000000000000000000000\n\
0000000000000000000000000000000000000000000000000000000000000000\n\
0000000000000000000000000000000000000000000000000000000000000000\n\
0000000000000000000000000000000000000000000000000000000000000000\n\
cleartomark\n";

/// Encrypts an `eexec` section in binary form. The four plain lead bytes
/// are chosen so that the cipher's first four bytes are not all
/// hexadecimal digits (which would mark a hexadecimal section) and its
/// first byte is not whitespace (which a lenient reader skips after
/// `eexec`).
pub fn encrypt_section_binary(plain: &[u8]) -> Vec<u8> {
    let mut lead = 0u8;
    loop {
        let mut text = vec![lead, 0, 0, 0];
        text.extend_from_slice(plain);
        let cipher = encrypt(EEXEC_KEY, &text, 0);
        if !is_hex_section(&cipher) && !cipher[0].is_ascii_whitespace() {
            return cipher;
        }
        lead = lead.wrapping_add(1);
    }
}

/// The dictionary entries the cleartext takes from the caller: the name
/// the font is defined under, its matrix, and the encoding in effect.
#[derive(Clone, Debug, PartialEq)]
pub struct Header<'a> {
    pub font_name: &'a [u8],
    pub font_matrix: [f32; 6],
    /// 256 entries; `None` is `.notdef`.
    pub encoding: &'a [Option<Vec<u8>>],
}

/// A regenerated program and the lengths of its three portions: the
/// cleartext through the `eexec` line, the binary encrypted section, and
/// the trailer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub bytes: Vec<u8>,
    pub length1: usize,
    pub length2: usize,
    pub length3: usize,
}

/// The glyphs a subset keeps for the names in `used`: those the program
/// has, `.notdef`, and the components of every `seac` glyph among them.
pub fn subset_names<'a>(
    program: &Type1Program,
    used: impl IntoIterator<Item = &'a [u8]>,
) -> BTreeSet<Vec<u8>> {
    let mut keep = BTreeSet::new();
    keep.insert(b".notdef".to_vec());
    for name in used {
        if program.charstring(name).is_none() {
            continue;
        }
        keep.insert(name.to_vec());
        for component in program.seac_components(name).unwrap_or_default() {
            if program.charstring(&component).is_some() {
                keep.insert(component);
            }
        }
    }
    keep
}

/// Private keys the writer supplies itself or that must not follow the
/// program: the reading procedures, `lenIV`, the subroutines, and the
/// identifiers a subset no longer matches.
const OWN_PRIVATE_KEYS: [&[u8]; 9] = [
    b"RD",
    b"ND",
    b"NP",
    b"-|",
    b"|-",
    b"|",
    b"lenIV",
    b"Subrs",
    b"UniqueID",
];

fn real(v: f32) -> String {
    if v == v.trunc() && v.abs() < 1e9 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn is_standard(encoding: &[Option<Vec<u8>>]) -> bool {
    encoding.len() == 256
        && encoding
            .iter()
            .zip(STANDARD_ENCODING.iter())
            .all(|(name, standard)| name.as_deref() == standard.map(str::as_bytes))
}

fn cleartext(program: &Type1Program, header: &Header<'_>) -> Vec<u8> {
    let dict = program.dict();
    let name = String::from_utf8_lossy(header.font_name);
    let mut out = format!("%!FontType1-1.0: {name}\n11 dict begin\n");
    if !dict.font_info.is_empty() {
        out.push_str(&format!(
            "/FontInfo {} dict dup begin\n",
            dict.font_info.len()
        ));
        for (key, value) in &dict.font_info {
            out.push_str(&format!(
                "/{} {} def\n",
                String::from_utf8_lossy(key),
                String::from_utf8_lossy(value)
            ));
        }
        out.push_str("end readonly def\n");
    }
    out.push_str(&format!(
        "/FontName /{name} def\n/PaintType {} def\n/FontType 1 def\n",
        dict.paint_type
    ));
    let m: Vec<String> = header.font_matrix.iter().map(|&v| real(v)).collect();
    out.push_str(&format!("/FontMatrix [{}] readonly def\n", m.join(" ")));
    let b: Vec<String> = dict.font_bbox.iter().map(|&v| real(v)).collect();
    out.push_str(&format!("/FontBBox {{{}}} readonly def\n", b.join(" ")));
    if is_standard(header.encoding) {
        out.push_str("/Encoding StandardEncoding def\n");
    } else {
        out.push_str("/Encoding 256 array\n0 1 255 {1 index exch /.notdef put} for\n");
        for (code, name) in header.encoding.iter().enumerate().take(256) {
            if let Some(name) = name
                && name != b".notdef"
            {
                out.push_str(&format!(
                    "dup {code} /{} put\n",
                    String::from_utf8_lossy(name)
                ));
            }
        }
        out.push_str("readonly def\n");
    }
    out.push_str("currentdict end\ncurrentfile eexec\n");
    out.into_bytes()
}

/// The plain text of the encrypted section: the private dictionary with
/// the program's own entries, the subroutines, and the kept charstrings.
fn private_text(program: &Type1Program, glyphs: &BTreeSet<Vec<u8>>) -> Vec<u8> {
    let dict = program.dict();
    let entries: Vec<&(Vec<u8>, Vec<u8>)> = dict
        .private
        .iter()
        .filter(|(key, _)| !OWN_PRIVATE_KEYS.contains(&key.as_slice()))
        .collect();
    let count = 4 + entries.len() + usize::from(!program.subrs().is_empty());
    let mut out = format!(
        "dup /Private {count} dict dup begin\n\
         /RD {{string currentfile exch readstring pop}} executeonly def\n\
         /ND {{noaccess def}} executeonly def\n\
         /NP {{noaccess put}} executeonly def\n\
         /lenIV 4 def\n"
    )
    .into_bytes();
    for (key, value) in entries {
        out.extend_from_slice(b"/");
        out.extend_from_slice(key);
        out.push(b' ');
        out.extend_from_slice(value);
        out.extend_from_slice(b" def\n");
    }
    if !program.subrs().is_empty() {
        out.extend_from_slice(format!("/Subrs {} array\n", program.subrs().len()).as_bytes());
        for (k, code) in program.subrs().iter().enumerate() {
            let cipher = encrypt(CHARSTRING_KEY, code, 4);
            out.extend_from_slice(format!("dup {k} {} RD ", cipher.len()).as_bytes());
            out.extend_from_slice(&cipher);
            out.extend_from_slice(b" NP\n");
        }
        out.extend_from_slice(b"ND\n");
    }
    out.extend_from_slice(b"noaccess put\n");
    let charstrings: Vec<(&[u8], &[u8])> = glyphs
        .iter()
        .filter_map(|name| {
            let code = program
                .charstring(name)
                .or((name == b".notdef").then_some(&NOTDEF[..]))?;
            Some((name.as_slice(), code))
        })
        .collect();
    out.extend_from_slice(
        format!("dup /CharStrings {} dict dup begin\n", charstrings.len()).as_bytes(),
    );
    for (name, code) in charstrings {
        let cipher = encrypt(CHARSTRING_KEY, code, 4);
        out.extend_from_slice(b"/");
        out.extend_from_slice(name);
        out.extend_from_slice(format!(" {} RD ", cipher.len()).as_bytes());
        out.extend_from_slice(&cipher);
        out.extend_from_slice(b" ND\n");
    }
    out.extend_from_slice(
        b"end\nreadonly put\nend\ndup /FontName get exch definefont pop\nmark currentfile closefile\n",
    );
    out
}

/// The complete program defining `glyphs` (see [`subset_names`]) under
/// the header's name, matrix, and encoding.
pub fn write(program: &Type1Program, header: &Header<'_>, glyphs: &BTreeSet<Vec<u8>>) -> Written {
    let mut bytes = cleartext(program, header);
    let length1 = bytes.len();
    let cipher = encrypt_section_binary(&private_text(program, glyphs));
    let length2 = cipher.len();
    bytes.extend_from_slice(&cipher);
    let trailer = format!("\n{TRAILER}");
    bytes.extend_from_slice(trailer.as_bytes());
    Written {
        bytes,
        length1,
        length2,
        length3: trailer.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{CharstringBuilder, Type1Font, rectangle};
    use crate::type1::{Type1Dict, decrypt, decrypt_section};

    fn font() -> Type1Font {
        let e = CharstringBuilder::new()
            .hsbw(20, 500)
            .rmoveto(0, 0)
            .rlineto(400, 0)
            .closepath()
            .endchar()
            .bytes();
        let acute = CharstringBuilder::new()
            .hsbw(30, 300)
            .rmoveto(0, 500)
            .closepath()
            .endchar()
            .bytes();
        let eacute = CharstringBuilder::new()
            .hsbw(20, 500)
            .seac(30, 150, 20, 101, 194)
            .bytes();
        Type1Font::new("Syn")
            .glyph("a", 600, &rectangle(50.0, 0.0, 550.0, 500.0))
            .glyph("b", 400, &rectangle(0.0, 0.0, 100.0, 100.0))
            .charstring("e", e)
            .charstring("acute", acute)
            .charstring("eacute", eacute)
            .standard_subrs()
            .encode(97, "a")
            .encode(233, "eacute")
    }

    fn names(list: &[&str]) -> BTreeSet<Vec<u8>> {
        list.iter().map(|n| n.as_bytes().to_vec()).collect()
    }

    fn encoding(font: &Type1Font) -> Vec<Option<Vec<u8>>> {
        let mut out = vec![None; 256];
        for (code, name) in &font.encoding {
            out[usize::from(*code)] = Some(name.as_bytes().to_vec());
        }
        out
    }

    /// The charstring of `name` as the written program carries it,
    /// decrypted.
    fn charstring_in(plain: &[u8], name: &str) -> Option<Vec<u8>> {
        let key = format!("/{name} ");
        let at = plain.windows(key.len()).position(|w| w == key.as_bytes())?;
        let rest = &plain[at + key.len()..];
        let space = rest.iter().position(|&b| b == b' ')?;
        let len: usize = std::str::from_utf8(&rest[..space]).ok()?.parse().ok()?;
        let start = space + 4;
        Some(decrypt(CHARSTRING_KEY, &rest[start..start + len], 4))
    }

    #[test]
    fn subsets_keep_notdef_and_seac_components() {
        let crate::Program::Type1(program) = font().program() else {
            unreachable!()
        };
        assert_eq!(
            subset_names(&program, [&b"a"[..], b"zz"]),
            names(&[".notdef", "a"])
        );
        assert_eq!(
            subset_names(&program, [&b"eacute"[..]]),
            names(&[".notdef", "acute", "e", "eacute"])
        );
        assert_eq!(subset_names(&program, []), names(&[".notdef"]));
    }

    #[test]
    fn the_written_program_has_the_three_portions_and_the_kept_glyphs() {
        let font = font();
        let crate::Program::Type1(program) = font.program() else {
            unreachable!()
        };
        let program = program.with_dict(Type1Dict {
            font_bbox: [0.0, -10.0, 750.0, 750.0],
            paint_type: 0,
            font_info: vec![(b"ItalicAngle".to_vec(), b"0".to_vec())],
            private: vec![
                (b"BlueValues".to_vec(), b"[-10 0]".to_vec()),
                (b"lenIV".to_vec(), b"1".to_vec()),
                (b"UniqueID".to_vec(), b"5".to_vec()),
                (b"RD".to_vec(), b"--nostringval--".to_vec()),
                (b"OtherSubrs".to_vec(), b"[{} {} {} {}]".to_vec()),
            ],
        });
        let encoding = encoding(&font);
        let header = Header {
            font_name: b"ABCDEF+Syn",
            font_matrix: [0.001, 0.0, 0.0, 0.001, 0.0, 0.0],
            encoding: &encoding,
        };
        let keep = subset_names(&program, [&b"a"[..]]);
        let written = write(&program, &header, &keep);
        assert_eq!(
            written.bytes.len(),
            written.length1 + written.length2 + written.length3
        );
        let clear = std::str::from_utf8(&written.bytes[..written.length1]).unwrap();
        assert!(clear.starts_with("%!FontType1-1.0: ABCDEF+Syn\n"));
        assert!(
            clear.contains("/FontInfo 1 dict dup begin\n/ItalicAngle 0 def\nend readonly def\n")
        );
        assert!(clear.contains("/FontName /ABCDEF+Syn def\n"));
        assert!(clear.contains("/FontMatrix [0.001 0 0 0.001 0 0] readonly def\n"));
        assert!(clear.contains("/FontBBox {0 -10 750 750} readonly def\n"));
        assert!(clear.contains("dup 97 /a put\ndup 233 /eacute put\nreadonly def\n"));
        assert!(clear.ends_with("currentdict end\ncurrentfile eexec\n"));
        let cipher = &written.bytes[written.length1..written.length1 + written.length2];
        assert!(!is_hex_section(cipher));
        let plain = decrypt_section(cipher);
        let text = String::from_utf8_lossy(&plain).into_owned();
        assert!(text.starts_with("dup /Private 7 dict dup begin\n"));
        assert!(text.contains(
            "/lenIV 4 def\n/BlueValues [-10 0] def\n/OtherSubrs [{} {} {} {}] def\n/Subrs 4 array\n"
        ));
        assert!(!text.contains("UniqueID"));
        assert!(!text.contains("nostringval"));
        assert!(text.contains("dup /CharStrings 2 dict dup begin\n"));
        assert!(text.ends_with("end\nreadonly put\nend\ndup /FontName get exch definefont pop\nmark currentfile closefile\n"));
        assert_eq!(
            charstring_in(&plain, "a").as_deref(),
            program.charstring(b"a")
        );
        assert_eq!(
            charstring_in(&plain, ".notdef").as_deref(),
            program.charstring(b".notdef")
        );
        assert_eq!(charstring_in(&plain, "b"), None);
        let trailer = &written.bytes[written.length1 + written.length2..];
        assert_eq!(trailer, format!("\n{TRAILER}").as_bytes());
    }

    #[test]
    fn a_standard_encoding_is_named_and_a_missing_notdef_is_synthesised() {
        let standard: Vec<Option<Vec<u8>>> = STANDARD_ENCODING
            .iter()
            .map(|n| n.map(|n| n.as_bytes().to_vec()))
            .collect();
        let mut font = font();
        font.glyphs.retain(|(name, _)| name != ".notdef");
        let crate::Program::Type1(program) = font.program() else {
            unreachable!()
        };
        let header = Header {
            font_name: b"Syn",
            font_matrix: [0.0005, 0.0, 0.0, 0.0005, 0.0, 0.0],
            encoding: &standard,
        };
        let written = write(&program, &header, &subset_names(&program, [&b"b"[..]]));
        let clear = std::str::from_utf8(&written.bytes[..written.length1]).unwrap();
        assert!(clear.contains("/Encoding StandardEncoding def\n"));
        assert!(clear.contains("/FontMatrix [0.0005 0 0 0.0005 0 0] readonly def\n"));
        assert!(clear.contains("/FontInfo 2 dict dup begin\n"));
        let plain =
            decrypt_section(&written.bytes[written.length1..written.length1 + written.length2]);
        assert_eq!(charstring_in(&plain, ".notdef"), Some(NOTDEF.to_vec()));
        assert!(charstring_in(&plain, "b").is_some());
    }

    #[test]
    fn binary_sections_never_start_with_whitespace_or_four_hex_digits() {
        for k in 0..64u8 {
            let plain = vec![k; 8];
            let cipher = encrypt_section_binary(&plain);
            assert!(!is_hex_section(&cipher));
            assert!(!cipher[0].is_ascii_whitespace());
            assert_eq!(decrypt_section(&cipher), plain);
        }
    }
}
