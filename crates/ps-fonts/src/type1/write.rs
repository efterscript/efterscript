// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Regenerating a Type 1 font program from a snapshot: the cleartext
//! that builds the font dictionary, an `eexec` section in binary form
//! holding the private dictionary, the subroutines, and the kept
//! charstrings re-encrypted with `lenIV` 4, and the zero trailer. What a
//! PDF embeds as `FontFile`, restricted to the glyphs a document used.
//! The subroutines are pruned to those the kept charstrings reach and
//! renumbered densely, every call rewritten; where a call's operand is
//! not a literal the numbering stays and the unreached subroutines are
//! written as `return` stubs instead.

use std::collections::{BTreeMap, BTreeSet};

use super::charstring::{Token, encode, tokens};
use super::{CHARSTRING_KEY, EEXEC_KEY, Type1Program, encrypt, is_hex_section};
use crate::encoding::STANDARD_ENCODING;

/// `0 0 hsbw endchar`: the blank `.notdef` written when the program has
/// none, since a Type 1 program must define one.
const NOTDEF: [u8; 4] = [139, 139, 13, 14];

/// The first four subroutines are the flex and hint-replacement entries
/// the other-subroutine convention names; a subset keeps them whether or
/// not a charstring reaches them.
const CONVENTIONAL_SUBRS: usize = 4;

/// `return`: the stub written for a subroutine no kept charstring reaches.
const RETURN: [u8; 1] = [11];

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

/// The subroutine indices a subset of `glyphs` keeps: every one a kept
/// charstring reaches, transitively, and the first four, bounded by the
/// array. `None` when a kept charstring cannot be interpreted, since its
/// reach is then unknown and every subroutine must stay.
pub fn reachable_subrs(
    program: &Type1Program,
    glyphs: &BTreeSet<Vec<u8>>,
) -> Option<BTreeSet<usize>> {
    let count = program.subrs().len();
    let mut keep: BTreeSet<usize> = (0..CONVENTIONAL_SUBRS.min(count)).collect();
    for name in glyphs {
        if program.charstring(name).is_none() {
            continue;
        }
        keep.extend(program.reached_subrs(name).ok()?);
    }
    Some(keep)
}

/// The program's subroutines with every index outside `keep` replaced by
/// the `return` stub; the array's length and every index are unchanged.
pub fn pruned_subrs(program: &Type1Program, keep: &BTreeSet<usize>) -> Vec<Vec<u8>> {
    program
        .subrs()
        .iter()
        .enumerate()
        .map(|(k, code)| {
            if keep.contains(&k) {
                code.clone()
            } else {
                RETURN.to_vec()
            }
        })
        .collect()
}

/// The charstrings of `glyphs` as the program has them, a blank
/// `.notdef` supplied when the program lacks one.
fn kept_charstrings(
    program: &Type1Program,
    glyphs: &BTreeSet<Vec<u8>>,
) -> BTreeMap<Vec<u8>, Vec<u8>> {
    glyphs
        .iter()
        .filter_map(|name| {
            let code = program
                .charstring(name)
                .or((name == b".notdef").then_some(&NOTDEF[..]))?;
            Some((name.clone(), code.to_vec()))
        })
        .collect()
}

/// A subset's subroutines and charstrings with the subroutine indices
/// renumbered densely (see [`renumber`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Renumbered {
    pub subrs: Vec<Vec<u8>>,
    pub charstrings: BTreeMap<Vec<u8>, Vec<u8>>,
}

/// `1 3 callothersubr pop callsubr return`: a subroutine that performs
/// hint replacement for its caller, which pushes the subroutine number
/// before calling it.
fn is_hint_helper(tokens: &[Token]) -> bool {
    tokens
        == [
            Token::Num(1),
            Token::Num(3),
            Token::Esc(16),
            Token::Esc(17),
            Token::Op(10),
            Token::Op(11),
        ]
}

/// Rewrites the subroutine indices of one charstring through `map`: the
/// literal operand of each `callsubr`, the subroutine number handed to
/// the hint-replacement other-subroutine (which `pop` then feeds to
/// `callsubr`), and the number pushed before a call to one of the
/// `helpers` that does the same on the caller's behalf. `None` when an
/// index is not a literal the rewrite can see, or names a subroutine
/// outside the map.
fn renumber_charstring(
    code: &[u8],
    map: &BTreeMap<usize, usize>,
    helpers: &BTreeSet<usize>,
) -> Option<Vec<u8>> {
    let mut tokens = tokens(code).ok()?;
    if is_hint_helper(&tokens) {
        return Some(code.to_vec());
    }
    let renumbered = |k: i32| -> Option<Token> {
        let new = *map.get(&usize::try_from(k).ok()?)?;
        Some(Token::Num(i32::try_from(new).ok()?))
    };
    let mut last_other_subr = None;
    for at in 0..tokens.len() {
        match tokens[at] {
            Token::Op(10) => match *tokens.get(at.wrapping_sub(1))? {
                Token::Num(k) => {
                    if usize::try_from(k).is_ok_and(|k| helpers.contains(&k)) {
                        let Token::Num(subr) = *tokens.get(at.wrapping_sub(2))? else {
                            return None;
                        };
                        tokens[at - 2] = renumbered(subr)?;
                    }
                    tokens[at - 1] = renumbered(k)?;
                }
                Token::Esc(17) if last_other_subr == Some(3) => {}
                _ => return None,
            },
            Token::Esc(16) => {
                let Token::Num(which) = *tokens.get(at.wrapping_sub(1))? else {
                    return None;
                };
                if which == 3 {
                    let count = tokens.get(at.wrapping_sub(2)).copied();
                    match (tokens.get(at.wrapping_sub(3)).copied(), count) {
                        (Some(Token::Num(k)), Some(Token::Num(1))) => {
                            tokens[at - 3] = renumbered(k)?;
                        }
                        _ => return None,
                    }
                }
                last_other_subr = Some(which);
            }
            _ => {}
        }
    }
    Some(encode(&tokens))
}

/// The subroutines in `keep` (see [`reachable_subrs`]) renumbered
/// densely in index order — the first four keep their indices — with
/// every call in the kept charstrings and subroutines rewritten. `None`
/// when a call's operand is not a literal, in which case the numbering
/// must stay and [`pruned_subrs`] applies.
pub fn renumber(
    program: &Type1Program,
    keep: &BTreeSet<usize>,
    glyphs: &BTreeSet<Vec<u8>>,
) -> Option<Renumbered> {
    let map: BTreeMap<usize, usize> = keep
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect();
    let helpers: BTreeSet<usize> = keep
        .iter()
        .copied()
        .filter(|&k| {
            program
                .subrs()
                .get(k)
                .is_some_and(|code| tokens(code).is_ok_and(|t| is_hint_helper(&t)))
        })
        .collect();
    let subrs = keep
        .iter()
        .map(|&k| renumber_charstring(program.subrs().get(k)?, &map, &helpers))
        .collect::<Option<Vec<_>>>()?;
    let charstrings = kept_charstrings(program, glyphs)
        .into_iter()
        .map(|(name, code)| Some((name, renumber_charstring(&code, &map, &helpers)?)))
        .collect::<Option<BTreeMap<_, _>>>()?;
    Some(Renumbered { subrs, charstrings })
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
/// the program's own entries, then the subroutines and charstrings as
/// given.
fn private_text(
    program: &Type1Program,
    subrs: &[Vec<u8>],
    charstrings: &BTreeMap<Vec<u8>, Vec<u8>>,
) -> Vec<u8> {
    let dict = program.dict();
    let entries: Vec<&(Vec<u8>, Vec<u8>)> = dict
        .private
        .iter()
        .filter(|(key, _)| !OWN_PRIVATE_KEYS.contains(&key.as_slice()))
        .collect();
    let count = 4 + entries.len() + usize::from(!subrs.is_empty());
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
    if !subrs.is_empty() {
        out.extend_from_slice(format!("/Subrs {} array\n", subrs.len()).as_bytes());
        for (k, code) in subrs.iter().enumerate() {
            let cipher = encrypt(CHARSTRING_KEY, code, 4);
            out.extend_from_slice(format!("dup {k} {} RD ", cipher.len()).as_bytes());
            out.extend_from_slice(&cipher);
            out.extend_from_slice(b" NP\n");
        }
        out.extend_from_slice(b"ND\n");
    }
    out.extend_from_slice(b"noaccess put\n");
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
/// the header's name, matrix, and encoding, its subroutines pruned to
/// those the glyphs reach and renumbered (see [`reachable_subrs`] and
/// [`renumber`]).
pub fn write(program: &Type1Program, header: &Header<'_>, glyphs: &BTreeSet<Vec<u8>>) -> Written {
    let mut bytes = cleartext(program, header);
    let length1 = bytes.len();
    let (subrs, charstrings) = match reachable_subrs(program, glyphs) {
        Some(keep) => match renumber(program, &keep, glyphs) {
            Some(dense) => (dense.subrs, dense.charstrings),
            None => (
                pruned_subrs(program, &keep),
                kept_charstrings(program, glyphs),
            ),
        },
        None => (program.subrs().to_vec(), kept_charstrings(program, glyphs)),
    };
    let cipher = encrypt_section_binary(&private_text(program, &subrs, &charstrings));
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

    fn set(list: &[usize]) -> BTreeSet<usize> {
        list.iter().copied().collect()
    }

    /// Four conventional subroutines, then: 4 unused, 5 a hint
    /// subroutine reached only through hint replacement, 6 unused, 7 a
    /// line, 8 calling 7, 9 the hint-replacement helper. `h` reaches 5
    /// inline and 8; `k` reaches 5 through the helper and 8; `acute`
    /// reaches 4 and 6.
    fn pruning_font() -> Type1Font {
        let unused = |d: i32| CharstringBuilder::new().rlineto(d, d).r#return().bytes();
        let hint = CharstringBuilder::new().hstem(0, 10).r#return().bytes();
        let inner = CharstringBuilder::new().rlineto(5, 5).r#return().bytes();
        let outer = CharstringBuilder::new().callsubr(7).r#return().bytes();
        let helper = CharstringBuilder::new()
            .num(1)
            .num(3)
            .callothersubr()
            .pop()
            .op(10)
            .r#return()
            .bytes();
        let h = CharstringBuilder::new()
            .hsbw(0, 300)
            .num(5)
            .num(1)
            .num(3)
            .callothersubr()
            .pop()
            .op(10)
            .rmoveto(0, 0)
            .callsubr(8)
            .endchar()
            .bytes();
        let k = CharstringBuilder::new()
            .hsbw(0, 300)
            .num(5)
            .callsubr(9)
            .rmoveto(0, 0)
            .callsubr(8)
            .endchar()
            .bytes();
        let acute = CharstringBuilder::new()
            .hsbw(0, 300)
            .rmoveto(0, 500)
            .callsubr(4)
            .callsubr(6)
            .endchar()
            .bytes();
        Type1Font::new("Syn")
            .standard_subrs()
            .subr(unused(1))
            .subr(hint)
            .subr(unused(2))
            .subr(inner)
            .subr(outer)
            .subr(helper)
            .charstring("h", h)
            .charstring("k", k)
            .charstring("acute", acute)
            .encode(104, "h")
    }

    #[test]
    fn subsets_renumber_the_subroutines_they_reach() {
        use crate::type1::parse_file;
        let font = pruning_font();
        let parsed = parse_file(&font.pfb()).unwrap();
        let program = &parsed.program;
        let keep = subset_names(program, [&b"h"[..]]);
        let reached = set(&[0, 1, 2, 3, 5, 7, 8]);
        assert_eq!(reachable_subrs(program, &keep), Some(reached.clone()));

        let stubbed = pruned_subrs(program, &reached);
        assert_eq!(stubbed.len(), 10);
        assert_eq!(stubbed[5], program.subrs()[5]);
        assert_eq!(stubbed[4], vec![11]);
        assert_eq!(stubbed[6], vec![11]);
        assert_eq!(stubbed[9], vec![11]);

        // Inline hint replacement: `5 1 3 callothersubr pop callsubr`
        // becomes `4 1 3 …`; the nested chain 8 → 7 becomes 6 → 5.
        let dense = renumber(program, &reached, &keep).unwrap();
        assert_eq!(dense.subrs.len(), 7);
        assert_eq!(&dense.subrs[..4], &program.subrs()[..4]);
        assert_eq!(dense.subrs[4], program.subrs()[5]);
        assert_eq!(dense.subrs[5], program.subrs()[7]);
        assert_eq!(
            dense.subrs[6],
            CharstringBuilder::new().callsubr(5).r#return().bytes()
        );
        assert_eq!(
            dense.charstrings[&b"h"[..]],
            CharstringBuilder::new()
                .hsbw(0, 300)
                .num(4)
                .num(1)
                .num(3)
                .callothersubr()
                .pop()
                .op(10)
                .rmoveto(0, 0)
                .callsubr(6)
                .endchar()
                .bytes()
        );
        assert_eq!(dense.charstrings.len(), 2);

        let encoding = parsed.encoding.names();
        let header = Header {
            font_name: b"ABCDEF+Syn",
            font_matrix: parsed.font_matrix,
            encoding: &encoding,
        };
        let written = write(program, &header, &keep);
        let again = parse_file(&written.bytes).unwrap().program;
        assert_eq!(again.subrs(), dense.subrs);
        assert_eq!(again.glyph(b"h"), program.glyph(b"h"));
        assert_eq!(again.reached_subrs(b"h").unwrap(), set(&[4, 5, 6]));

        // Through the helper: the number pushed before the helper call
        // is the subroutine to renumber, and the helper is kept as it is.
        let keep = subset_names(program, [&b"k"[..]]);
        let reached = set(&[0, 1, 2, 3, 5, 7, 8, 9]);
        assert_eq!(reachable_subrs(program, &keep), Some(reached.clone()));
        let dense = renumber(program, &reached, &keep).unwrap();
        assert_eq!(dense.subrs.len(), 8);
        assert_eq!(dense.subrs[7], program.subrs()[9]);
        assert_eq!(
            dense.charstrings[&b"k"[..]],
            CharstringBuilder::new()
                .hsbw(0, 300)
                .num(4)
                .callsubr(7)
                .rmoveto(0, 0)
                .callsubr(6)
                .endchar()
                .bytes()
        );
        let written = write(program, &header, &keep);
        let again = parse_file(&written.bytes).unwrap().program;
        assert_eq!(again.subrs(), dense.subrs);
        assert_eq!(again.glyph(b"k"), program.glyph(b"k"));
        assert_eq!(again.reached_subrs(b"k").unwrap(), set(&[4, 5, 6, 7]));

        // A program with fewer than four subroutines keeps what it has.
        let short = Type1Font::new("Syn")
            .subr(CharstringBuilder::new().r#return().bytes())
            .subr(CharstringBuilder::new().r#return().bytes());
        let crate::Program::Type1(program) = short.program() else {
            unreachable!()
        };
        let keep = subset_names(&program, []);
        assert_eq!(reachable_subrs(&program, &keep), Some(set(&[0, 1])));
        assert_eq!(pruned_subrs(&program, &set(&[])), vec![vec![11], vec![11]]);
        assert_eq!(
            renumber(&program, &set(&[0, 1]), &keep).unwrap().subrs,
            program.subrs()
        );
    }

    #[test]
    fn calls_without_a_literal_operand_fall_back_to_stubs_and_faults_keep_all() {
        use crate::type1::parse_file;
        let encoding: Vec<Option<Vec<u8>>> = vec![None; 256];
        let header = Header {
            font_name: b"Syn",
            font_matrix: [0.001, 0.0, 0.0, 0.001, 0.0, 0.0],
            encoding: &encoding,
        };
        // `16 2 div callsubr` reaches 8 through arithmetic, and a
        // `pop callsubr` fed by an other-subroutine that is not hint
        // replacement is opaque too; a call in dead code after `endchar`
        // may name a subroutine the trace never kept.
        let computed = CharstringBuilder::new()
            .hsbw(0, 300)
            .rmoveto(0, 0)
            .num(16)
            .num(2)
            .div()
            .op(10)
            .endchar()
            .bytes();
        let opaque = CharstringBuilder::new()
            .hsbw(0, 300)
            .rmoveto(0, 0)
            .num(8)
            .num(1)
            .num(13)
            .callothersubr()
            .pop()
            .op(10)
            .endchar()
            .bytes();
        let dead = CharstringBuilder::new()
            .hsbw(0, 300)
            .rmoveto(0, 0)
            .callsubr(8)
            .endchar()
            .callsubr(4)
            .bytes();
        for (name, code) in [("c", computed), ("o", opaque), ("d", dead)] {
            let font = pruning_font().charstring(name, code);
            let parsed = parse_file(&font.pfb()).unwrap();
            let program = &parsed.program;
            let keep = subset_names(program, [name.as_bytes()]);
            let reached = reachable_subrs(program, &keep).unwrap();
            assert_eq!(reached, set(&[0, 1, 2, 3, 7, 8]), "{name}");
            assert_eq!(renumber(program, &reached, &keep), None, "{name}");
            let written = write(program, &header, &keep);
            let again = parse_file(&written.bytes).unwrap().program;
            assert_eq!(again.subrs(), pruned_subrs(program, &reached), "{name}");
            assert_eq!(again.subrs().len(), 10);
            assert_eq!(again.subrs()[4], vec![11]);
            assert_eq!(again.glyph(name.as_bytes()), program.glyph(name.as_bytes()));
        }

        // A kept charstring that cannot be interpreted keeps every
        // subroutine as it is, since its reach is unknown.
        let broken = pruning_font().charstring("m", CharstringBuilder::new().num(1).bytes());
        let parsed = parse_file(&broken.pfb()).unwrap();
        let keep = subset_names(&parsed.program, [&b"m"[..]]);
        assert_eq!(reachable_subrs(&parsed.program, &keep), None);
        let written = write(&parsed.program, &header, &keep);
        let again = parse_file(&written.bytes).unwrap().program;
        assert_eq!(again.subrs(), parsed.program.subrs());
        assert_eq!(again.charstring(b"m"), parsed.program.charstring(b"m"));
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
