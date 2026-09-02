// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! A reader for Type 1 font files: the PFB segment form and the PFA
//! form with a hexadecimal or binary `eexec` section. It recognises the
//! layout the format prescribes rather than executing the program: the
//! cleartext entries a font dictionary needs, then, in the decrypted
//! private section, `lenIV`, the subroutines, and the charstrings by
//! the `<index> <length> <token> <bytes>` shape (whatever the reading
//! procedures are called), and every other `Private` and `FontInfo`
//! entry as its source text. The result is the same snapshot the
//! interpreter builds from a font a job defines, so one writer embeds
//! both.

use std::ops::Range;

use super::{Type1Dict, Type1Program, decrypt_section};
use crate::encoding::STANDARD_ENCODING;
use crate::program::FontError;

/// Dictionary entries as `(key, value)` byte strings.
type Entries = Vec<(Vec<u8>, Vec<u8>)>;

/// The encoding a font file defines.
#[derive(Clone, Debug, PartialEq)]
pub enum FileEncoding {
    /// `/Encoding StandardEncoding def`.
    Standard,
    /// A 256-entry table built from `dup <code> /<name> put` lines.
    Custom(Vec<Option<Vec<u8>>>),
}

impl FileEncoding {
    /// The glyph name at each code, 256 entries.
    pub fn names(&self) -> Vec<Option<Vec<u8>>> {
        match self {
            FileEncoding::Standard => STANDARD_ENCODING
                .iter()
                .map(|n| n.map(|n| n.as_bytes().to_vec()))
                .collect(),
            FileEncoding::Custom(names) => names.clone(),
        }
    }
}

/// What a font file defines: the cleartext identity and the program.
#[derive(Debug)]
pub struct ParsedFont {
    pub font_name: Vec<u8>,
    pub font_matrix: [f32; 6],
    pub encoding: FileEncoding,
    /// The program with its [`Type1Dict`] attached.
    pub program: Type1Program,
}

/// Reads a font file in PFB or PFA form.
pub fn parse_file(bytes: &[u8]) -> Result<ParsedFont, FontError> {
    let (clear, cipher) = match pfb_segments(bytes) {
        Some(segments) => segments,
        None => split_pfa(bytes)?,
    };
    let clear = Cleartext::parse(&clear)?;
    let plain = decrypt_section(&cipher);
    let private = Private::parse(&plain)?;
    let dict = Type1Dict {
        font_bbox: clear.font_bbox,
        paint_type: clear.paint_type,
        font_info: clear.font_info,
        private: private.entries,
    };
    let program =
        Type1Program::new(private.len_iv, private.subrs, private.charstrings).with_dict(dict);
    Ok(ParsedFont {
        font_name: clear.font_name,
        font_matrix: clear.font_matrix,
        encoding: clear.encoding,
        program,
    })
}

/// The ASCII segments before the first binary one, and the binary
/// segments, of a PFB file; `None` when `bytes` has no segment header.
fn pfb_segments(bytes: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if bytes.len() < 6 || bytes[0] != 0x80 {
        return None;
    }
    let mut clear = Vec::new();
    let mut cipher = Vec::new();
    let mut at = 0;
    while at + 2 <= bytes.len() && bytes[at] == 0x80 {
        let kind = bytes[at + 1];
        if kind == 3 {
            break;
        }
        let header = bytes.get(at + 2..at + 6)?;
        let len = u32::from_le_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let data = bytes.get(at + 6..at + 6 + len)?;
        match kind {
            1 if cipher.is_empty() => clear.extend_from_slice(data),
            // ASCII after the binary section is the zero trailer.
            1 => {}
            2 => cipher.extend_from_slice(data),
            _ => return None,
        }
        at += 6 + len;
    }
    Some((clear, cipher))
}

/// Splits a PFA file at `eexec`: the cleartext through that token, and
/// the section after the single whitespace byte that follows it.
fn split_pfa(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), FontError> {
    let mut lexer = Lexer::new(bytes);
    loop {
        let token = lexer
            .next()?
            .ok_or(FontError::Malformed("font file without eexec"))?;
        if token.kind == Kind::Exec && lexer.text(&token) == b"eexec" {
            let end = token.span.end;
            let section = (end + 1).min(bytes.len());
            return Ok((bytes[..end].to_vec(), bytes[section..].to_vec()));
        }
    }
}

// --- tokens ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// `/name`; the span covers the slash.
    Literal,
    /// A name to be executed, operators included.
    Exec,
    Number,
    String,
    Open,
    Close,
}

#[derive(Clone, Debug, PartialEq)]
struct Token {
    kind: Kind,
    span: Range<usize>,
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'\0' | b'\x0c')
}

fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

fn parse_number(text: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(text).ok()?;
    if let Ok(v) = text.parse::<i64>() {
        return Some(v as f64);
    }
    if let Some((radix, digits)) = text.split_once('#') {
        let radix: u32 = radix.parse().ok()?;
        return i64::from_str_radix(digits, radix).ok().map(|v| v as f64);
    }
    if text.bytes().any(|b| b.is_ascii_digit())
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+' | b'e' | b'E'))
    {
        return text.parse::<f64>().ok();
    }
    None
}

/// A PostScript tokenizer over the parts of a font file that are text.
/// Comments are skipped; strings and names keep their source spans.
struct Lexer<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Lexer<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Lexer { bytes, at: 0 }
    }

    fn text(&self, token: &Token) -> &'a [u8] {
        &self.bytes[token.span.clone()]
    }

    /// The literal name a token holds, without its slash.
    fn name(&self, token: &Token) -> &'a [u8] {
        let text = self.text(token);
        text.strip_prefix(b"/").unwrap_or(text)
    }

    fn number(&self, token: &Token) -> Option<f64> {
        (token.kind == Kind::Number).then(|| parse_number(self.text(token)))?
    }

    /// Exactly `len` raw bytes after one separating byte, as a charstring
    /// follows its reading procedure.
    fn raw(&mut self, len: usize) -> Result<&'a [u8], FontError> {
        let start = self.at + 1;
        let end = start + len;
        if end > self.bytes.len() {
            return Err(FontError::Truncated("charstring bytes"));
        }
        self.at = end;
        Ok(&self.bytes[start..end])
    }

    fn skip_space_and_comments(&mut self) {
        while self.at < self.bytes.len() {
            let b = self.bytes[self.at];
            if is_space(b) {
                self.at += 1;
            } else if b == b'%' {
                while self.at < self.bytes.len() && !matches!(self.bytes[self.at], b'\n' | b'\r') {
                    self.at += 1;
                }
            } else {
                break;
            }
        }
    }

    fn next(&mut self) -> Result<Option<Token>, FontError> {
        self.skip_space_and_comments();
        let Some(&b) = self.bytes.get(self.at) else {
            return Ok(None);
        };
        let start = self.at;
        let kind = match b {
            b'(' => {
                let mut depth = 0usize;
                loop {
                    let c = *self
                        .bytes
                        .get(self.at)
                        .ok_or(FontError::Truncated("string"))?;
                    self.at += 1;
                    match c {
                        b'\\' => self.at += 1,
                        b'(' => depth += 1,
                        b')' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                Kind::String
            }
            b'<' => {
                if self.bytes.get(self.at + 1) == Some(&b'<') {
                    self.at += 2;
                    Kind::Exec
                } else {
                    while self.bytes.get(self.at).is_some_and(|&c| c != b'>') {
                        self.at += 1;
                    }
                    self.at = (self.at + 1).min(self.bytes.len());
                    Kind::String
                }
            }
            b'>' => {
                self.at += if self.bytes.get(self.at + 1) == Some(&b'>') {
                    2
                } else {
                    1
                };
                Kind::Exec
            }
            b'[' | b'{' => {
                self.at += 1;
                Kind::Open
            }
            b']' | b'}' => {
                self.at += 1;
                Kind::Close
            }
            b')' => {
                self.at += 1;
                Kind::Exec
            }
            b'/' => {
                self.at += 1;
                if self.bytes.get(self.at) == Some(&b'/') {
                    self.at += 1;
                }
                while self
                    .bytes
                    .get(self.at)
                    .is_some_and(|&c| !is_space(c) && !is_delimiter(c))
                {
                    self.at += 1;
                }
                Kind::Literal
            }
            _ => {
                while self
                    .bytes
                    .get(self.at)
                    .is_some_and(|&c| !is_space(c) && !is_delimiter(c))
                {
                    self.at += 1;
                }
                if parse_number(&self.bytes[start..self.at]).is_some() {
                    Kind::Number
                } else {
                    Kind::Exec
                }
            }
        };
        Ok(Some(Token {
            kind,
            span: start..self.at,
        }))
    }

    fn expect(&mut self, what: &'static str) -> Result<Token, FontError> {
        self.next()?.ok_or(FontError::Truncated(what))
    }

    fn expect_number(&mut self, what: &'static str) -> Result<f64, FontError> {
        let token = self.expect(what)?;
        self.number(&token).ok_or(FontError::Malformed(what))
    }

    /// The numbers inside the next `[ … ]` or `{ … }`.
    fn number_array(&mut self, what: &'static str) -> Result<Vec<f32>, FontError> {
        let open = self.expect(what)?;
        if open.kind != Kind::Open {
            return Err(FontError::Malformed(what));
        }
        let mut values = Vec::new();
        loop {
            let token = self.expect(what)?;
            match token.kind {
                Kind::Close => return Ok(values),
                Kind::Number => values.push(self.number(&token).unwrap_or(0.0) as f32),
                _ => return Err(FontError::Malformed(what)),
            }
        }
    }
}

/// Names that end a dictionary entry's value at bracket depth zero.
fn is_terminator(name: &[u8]) -> bool {
    matches!(name, b"def" | b"ND" | b"NP" | b"|-" | b"|")
}

/// Attributes written between a value and its `def`.
fn is_attribute(name: &[u8]) -> bool {
    matches!(name, b"readonly" | b"noaccess" | b"executeonly")
}

/// The source text of an entry's value: the tokens from the current
/// position to the terminator, joined by single spaces (comments
/// dropped, strings verbatim), and whether the value was marked
/// `executeonly`.
fn value_text(lexer: &mut Lexer<'_>, what: &'static str) -> Result<(Vec<u8>, bool), FontError> {
    let mut parts: Vec<&[u8]> = Vec::new();
    let mut depth = 0usize;
    let mut execute_only = false;
    loop {
        let token = lexer.expect(what)?;
        match token.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth = depth.saturating_sub(1),
            Kind::Exec if depth == 0 => {
                let name = lexer.text(&token);
                if is_terminator(name) {
                    break;
                }
                if is_attribute(name) {
                    execute_only |= name == b"executeonly";
                    continue;
                }
            }
            _ => {}
        }
        parts.push(lexer.text(&token));
    }
    Ok((parts.join(&b' '), execute_only))
}

// --- the cleartext -----------------------------------------------------------------------

struct Cleartext {
    font_name: Vec<u8>,
    font_matrix: [f32; 6],
    font_bbox: [f32; 4],
    paint_type: i32,
    encoding: FileEncoding,
    font_info: Entries,
}

impl Cleartext {
    fn parse(bytes: &[u8]) -> Result<Self, FontError> {
        let mut lexer = Lexer::new(bytes);
        let mut font_name = None;
        let mut font_matrix = None;
        let mut font_bbox = [0.0; 4];
        let mut paint_type = 0;
        let mut encoding = FileEncoding::Standard;
        let mut font_info = Vec::new();
        while let Some(token) = lexer.next()? {
            match token.kind {
                Kind::Exec if lexer.text(&token) == b"eexec" => break,
                Kind::Literal => match lexer.name(&token) {
                    b"FontName" => {
                        let name = lexer.expect("FontName")?;
                        font_name = Some(lexer.name(&name).to_vec());
                    }
                    b"FontMatrix" => {
                        let values = lexer.number_array("FontMatrix")?;
                        font_matrix = Some(
                            <[f32; 6]>::try_from(values)
                                .map_err(|_| FontError::Malformed("FontMatrix"))?,
                        );
                    }
                    b"FontBBox" => {
                        let values = lexer.number_array("FontBBox")?;
                        font_bbox = <[f32; 4]>::try_from(values)
                            .map_err(|_| FontError::Malformed("FontBBox"))?;
                    }
                    b"PaintType" => paint_type = lexer.expect_number("PaintType")? as i32,
                    b"Encoding" => encoding = parse_encoding(&mut lexer)?,
                    b"FontInfo" => font_info = parse_entries(&mut lexer, "FontInfo")?,
                    _ => {}
                },
                _ => {}
            }
        }
        Ok(Cleartext {
            font_name: font_name.ok_or(FontError::Malformed("font file without FontName"))?,
            font_matrix: font_matrix.ok_or(FontError::Malformed("font file without FontMatrix"))?,
            font_bbox,
            paint_type,
            encoding,
            font_info,
        })
    }
}

/// `StandardEncoding`, or the `dup <code> /<name> put` lines up to the
/// entry's `def`.
fn parse_encoding(lexer: &mut Lexer<'_>) -> Result<FileEncoding, FontError> {
    let first = lexer.expect("Encoding")?;
    if first.kind == Kind::Exec && lexer.text(&first) == b"StandardEncoding" {
        return Ok(FileEncoding::Standard);
    }
    let mut names: Vec<Option<Vec<u8>>> = vec![None; 256];
    let mut recent: Vec<Token> = vec![first];
    let mut depth = 0usize;
    loop {
        let token = lexer.expect("Encoding")?;
        match token.kind {
            Kind::Open => depth += 1,
            Kind::Close => depth = depth.saturating_sub(1),
            Kind::Exec if depth == 0 && lexer.text(&token) == b"def" => break,
            Kind::Exec if depth == 0 && lexer.text(&token) == b"put" => {
                if let [dup, code, name] = recent.as_slice()
                    && dup.kind == Kind::Exec
                    && lexer.text(dup) == b"dup"
                    && name.kind == Kind::Literal
                    && let Some(code) = lexer.number(code)
                    && (0.0..256.0).contains(&code)
                {
                    let name = lexer.name(name);
                    names[code as usize] = (name != b".notdef").then(|| name.to_vec());
                }
            }
            _ => {}
        }
        recent.push(token);
        if recent.len() > 3 {
            recent.remove(0);
        }
    }
    Ok(FileEncoding::Custom(names))
}

/// The `/<key> <value> def` entries of a `<n> dict dup begin … end`
/// block, as key and source text; `executeonly` procedures are left out.
fn parse_entries(lexer: &mut Lexer<'_>, what: &'static str) -> Result<Entries, FontError> {
    loop {
        let token = lexer.expect(what)?;
        if token.kind == Kind::Exec && lexer.text(&token) == b"begin" {
            break;
        }
    }
    let mut entries = Vec::new();
    loop {
        let token = lexer.expect(what)?;
        match token.kind {
            Kind::Exec if lexer.text(&token) == b"end" => return Ok(entries),
            Kind::Literal => {
                let key = lexer.name(&token).to_vec();
                let (value, execute_only) = value_text(lexer, what)?;
                if !execute_only {
                    entries.push((key, value));
                }
            }
            _ => {}
        }
    }
}

// --- the private section -------------------------------------------------------------------

struct Private {
    len_iv: i32,
    subrs: Vec<Vec<u8>>,
    charstrings: Entries,
    entries: Entries,
}

/// Keys the snapshot carries structurally rather than as text.
fn is_structural(key: &[u8]) -> bool {
    matches!(
        key,
        b"Subrs" | b"lenIV" | b"CharStrings" | b"RD" | b"ND" | b"NP" | b"-|" | b"|-" | b"|"
    )
}

impl Private {
    fn parse(bytes: &[u8]) -> Result<Self, FontError> {
        let mut lexer = Lexer::new(bytes);
        let mut len_iv = 4;
        let mut subrs = Vec::new();
        let mut charstrings = None;
        let mut entries = Vec::new();
        while charstrings.is_none() {
            let Some(token) = lexer.next()? else {
                break;
            };
            if token.kind != Kind::Literal {
                continue;
            }
            match lexer.name(&token) {
                b"Private" => {}
                b"lenIV" => len_iv = lexer.expect_number("lenIV")? as i32,
                b"Subrs" => subrs = parse_subrs(&mut lexer)?,
                b"CharStrings" => charstrings = Some(parse_charstrings(&mut lexer)?),
                key if is_structural(key) => {
                    value_text(&mut lexer, "Private")?;
                }
                key => {
                    let key = key.to_vec();
                    let (value, execute_only) = value_text(&mut lexer, "Private")?;
                    if !execute_only {
                        entries.push((key, value));
                    }
                }
            }
        }
        Ok(Private {
            len_iv,
            subrs,
            charstrings: charstrings
                .ok_or(FontError::Malformed("font file without CharStrings"))?,
            entries,
        })
    }
}

/// `<n> array` then `dup <index> <length> <token> <bytes> …`, ending at
/// the first token that is not `dup`.
fn parse_subrs(lexer: &mut Lexer<'_>) -> Result<Vec<Vec<u8>>, FontError> {
    let count = lexer.expect_number("Subrs")?;
    if count < 0.0 {
        return Err(FontError::Malformed("Subrs"));
    }
    let array = lexer.expect("Subrs")?;
    if array.kind != Kind::Exec || lexer.text(&array) != b"array" {
        return Err(FontError::Malformed("Subrs"));
    }
    let mut subrs: Vec<Vec<u8>> = vec![Vec::new(); count as usize];
    loop {
        let save = lexer.at;
        let token = lexer.expect("Subrs")?;
        if token.kind != Kind::Exec || lexer.text(&token) != b"dup" {
            lexer.at = save;
            return Ok(subrs);
        }
        let index = lexer.expect_number("Subrs")?;
        let len = lexer.expect_number("Subrs")?;
        if index < 0.0 || index >= count || len < 0.0 {
            return Err(FontError::Malformed("Subrs"));
        }
        lexer.expect("Subrs")?;
        let bytes = lexer.raw(len as usize)?.to_vec();
        subrs[index as usize] = bytes;
        // `NP`, `|`, or `noaccess put`.
        loop {
            let token = lexer.expect("Subrs")?;
            if token.kind == Kind::Exec && matches!(lexer.text(&token), b"NP" | b"|" | b"put") {
                break;
            }
        }
    }
}

/// `<n> dict dup begin` then `/<name> <length> <token> <bytes> …` up to
/// `end`.
fn parse_charstrings(lexer: &mut Lexer<'_>) -> Result<Entries, FontError> {
    loop {
        let token = lexer.expect("CharStrings")?;
        if token.kind == Kind::Exec && lexer.text(&token) == b"begin" {
            break;
        }
    }
    let mut glyphs = Vec::new();
    loop {
        let token = lexer.expect("CharStrings")?;
        match token.kind {
            Kind::Exec if lexer.text(&token) == b"end" => return Ok(glyphs),
            Kind::Literal => {
                let name = lexer.name(&token).to_vec();
                let len = lexer.expect_number("CharStrings")?;
                if len < 0.0 {
                    return Err(FontError::Malformed("CharStrings"));
                }
                lexer.expect("CharStrings")?;
                let bytes = lexer.raw(len as usize)?.to_vec();
                glyphs.push((name, bytes));
                // `ND`, `|-`, or `noaccess def`.
                loop {
                    let token = lexer.expect("CharStrings")?;
                    if token.kind == Kind::Exec
                        && matches!(lexer.text(&token), b"ND" | b"|-" | b"def")
                    {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Type1Font, corpus_type1};
    use crate::type1::write::{Header, subset_names, write};

    fn font() -> Type1Font {
        corpus_type1().standard_subrs().encode(65, "a")
    }

    fn check(parsed: &ParsedFont, font: &Type1Font) {
        let crate::Program::Type1(expected) = font.program() else {
            unreachable!()
        };
        assert_eq!(parsed.font_name, font.name.as_bytes());
        assert_eq!(parsed.font_matrix, [0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
        assert_eq!(parsed.program.len_iv(), 4);
        assert_eq!(parsed.program.subrs(), expected.subrs());
        assert_eq!(parsed.program.charstrings(), expected.charstrings());
        let dict = parsed.program.dict();
        assert_eq!(dict.font_bbox, expected.dict().font_bbox);
        assert_eq!(dict.paint_type, 0);
        assert_eq!(dict.font_info_number("ItalicAngle"), Some(0.0));
        assert_eq!(dict.font_info_bool("isFixedPitch"), Some(false));
        let keys: Vec<&[u8]> = dict.private.iter().map(|(k, _)| k.as_slice()).collect();
        assert_eq!(
            keys,
            [
                &b"password"[..],
                b"MinFeature",
                b"BlueValues",
                b"OtherSubrs"
            ]
        );
        assert_eq!(dict.private_number("password"), Some(5839.0));
        let FileEncoding::Custom(names) = &parsed.encoding else {
            panic!("the builder writes its own encoding");
        };
        assert_eq!(names[65].as_deref(), Some(&b"a"[..]));
        assert_eq!(names[97].as_deref(), Some(&b"a"[..]));
        assert_eq!(names[66], None);
        assert_eq!(parsed.encoding.names().len(), 256);
        // The corpus font's `b` is malformed on purpose; both programs
        // must say so.
        for name in expected.charstrings().keys() {
            assert_eq!(parsed.program.glyph(name), expected.glyph(name), "{name:?}");
        }
    }

    #[test]
    fn pfa_hex_binary_and_pfb_forms_read_the_same_program() {
        let font = font();
        check(&parse_file(font.pfa().as_bytes()).unwrap(), &font);
        check(&parse_file(&font.pfa_binary()).unwrap(), &font);
        check(&parse_file(&font.pfb()).unwrap(), &font);
    }

    #[test]
    fn the_writers_output_reads_back() {
        let font = font();
        let parsed = parse_file(&font.pfb()).unwrap();
        let standard = FileEncoding::Standard.names();
        let header = Header {
            font_name: b"ABCDEF+Syn",
            font_matrix: parsed.font_matrix,
            encoding: &standard,
        };
        let keep = subset_names(&parsed.program, [&b"eacute"[..], b"a"]);
        let written = write(&parsed.program, &header, &keep);
        let again = parse_file(&written.bytes).unwrap();
        assert_eq!(again.font_name, b"ABCDEF+Syn");
        assert_eq!(again.encoding, FileEncoding::Standard);
        let names: Vec<&[u8]> = again
            .program
            .charstrings()
            .keys()
            .map(Vec::as_slice)
            .collect();
        assert_eq!(names, [&b".notdef"[..], b"a", b"acute", b"e", b"eacute"]);
        assert_eq!(again.program.subrs(), parsed.program.subrs());
        assert_eq!(
            again.program.charstring(b"eacute"),
            parsed.program.charstring(b"eacute")
        );
        assert_eq!(again.program.dict().private, parsed.program.dict().private);
        assert_eq!(
            again.program.dict().font_info,
            parsed.program.dict().font_info
        );
    }

    #[test]
    fn the_lexer_splits_the_forms_fonts_use() {
        let text =
            b"dup 223/germandbls put /a(x(y)\\))def %c\n{16 16}ND [1 .5 -2e1] 16#1F <</k 1>> <414>";
        let mut lexer = Lexer::new(text);
        let mut out = Vec::new();
        while let Some(t) = lexer.next().unwrap() {
            out.push((t.kind, String::from_utf8_lossy(lexer.text(&t)).into_owned()));
        }
        let kinds: Vec<Kind> = out.iter().map(|(k, _)| *k).collect();
        let texts: Vec<&str> = out.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(
            texts,
            [
                "dup",
                "223",
                "/germandbls",
                "put",
                "/a",
                "(x(y)\\))",
                "def",
                "{",
                "16",
                "16",
                "}",
                "ND",
                "[",
                "1",
                ".5",
                "-2e1",
                "]",
                "16#1F",
                "<<",
                "/k",
                "1",
                ">>",
                "<414>"
            ]
        );
        assert_eq!(kinds[1], Kind::Number);
        assert_eq!(kinds[2], Kind::Literal);
        assert_eq!(kinds[5], Kind::String);
        assert_eq!(kinds[7], Kind::Open);
        assert_eq!(kinds[17], Kind::Number);
        assert_eq!(kinds[22], Kind::String);
        assert_eq!(parse_number(b"16#1F"), Some(31.0));
        assert_eq!(parse_number(b"e"), None);
        assert_eq!(parse_number(b"-"), None);
    }

    #[test]
    fn entries_keep_their_source_text_and_skip_executeonly_procedures() {
        let text = b"3 dict dup begin\n/RD{string currentfile exch readstring pop}executeonly def\n/BlueValues [ -20 0 % c\n 469 ] ND\n/Notice (a (b) c) readonly def\n/MinFeature{16 16}|-\nend";
        let entries = parse_entries(&mut Lexer::new(text), "test").unwrap();
        let entries: Vec<(String, String)> = entries
            .iter()
            .map(|(k, v)| {
                (
                    String::from_utf8_lossy(k).into_owned(),
                    String::from_utf8_lossy(v).into_owned(),
                )
            })
            .collect();
        assert_eq!(
            entries,
            [
                ("BlueValues".to_string(), "[ -20 0 469 ]".to_string()),
                ("Notice".to_string(), "(a (b) c)".to_string()),
                ("MinFeature".to_string(), "{ 16 16 }".to_string()),
            ]
        );
    }

    #[test]
    fn malformed_files_are_errors_not_panics() {
        assert_eq!(
            parse_file(b"").err(),
            Some(FontError::Malformed("font file without eexec"))
        );
        assert_eq!(
            parse_file(b"%!PS\n/FontName /X def currentfile eexec\n").err(),
            Some(FontError::Malformed("font file without FontMatrix"))
        );
        assert_eq!(
            parse_file(b"\x80\x01\x05\x00\x00\x00abc").err(),
            Some(FontError::Malformed("font file without eexec"))
        );
        let font = font();
        let mut pfb = font.pfb();
        let cut = pfb.len() / 2;
        pfb.truncate(cut);
        assert!(parse_file(&pfb).is_err());
        let mut pfa = font.pfa().into_bytes();
        let at = pfa.windows(5).position(|w| w == b"eexec").unwrap() + 6;
        pfa.truncate(at + 20);
        assert!(parse_file(&pfa).is_err());
    }
}
