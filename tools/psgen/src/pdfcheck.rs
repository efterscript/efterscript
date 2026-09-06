// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! A structural check of a distilled document, returning the first
//! violation as text: header and binary marker, `startxref` and `%%EOF`,
//! a cross-reference table whose every in-use offset lands on its
//! `N 0 obj`, exact stream lengths, references that resolve, and a
//! trailer whose `Size` covers the table and whose `Root` is the catalog.
//! The same invariants the writer's own test support asserts, kept here
//! in a form that reports instead of panicking.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Int(i64),
    Real(f64),
    Bool(bool),
    Null,
    Name(Vec<u8>),
    Str(Vec<u8>),
    Array(Vec<Value>),
    Dict(Vec<(Vec<u8>, Value)>),
    Reference(u32, u16),
}

impl Value {
    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(entries) => entries
                .iter()
                .find(|(k, _)| k == key.as_bytes())
                .map(|(_, v)| v),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    DictOpen,
    DictClose,
    ArrayOpen,
    ArrayClose,
    Name(Vec<u8>),
    Str(Vec<u8>),
    Int(i64),
    Real(f64),
    Keyword(String),
}

fn is_white(b: u8) -> bool {
    matches!(b, 0 | 9 | 10 | 12 | 13 | 32)
}

fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

struct Lexer<'a> {
    bytes: &'a [u8],
    pos: usize,
    peeked: Vec<Token>,
}

impl<'a> Lexer<'a> {
    fn new(bytes: &'a [u8], pos: usize) -> Self {
        Lexer {
            bytes,
            pos,
            peeked: Vec::new(),
        }
    }

    fn byte(&self, at: usize) -> Result<u8, String> {
        self.bytes
            .get(at)
            .copied()
            .ok_or_else(|| format!("unexpected end of file at {at}"))
    }

    fn skip_white(&mut self) {
        while let Some(&b) = self.bytes.get(self.pos) {
            if is_white(b) {
                self.pos += 1;
            } else if b == b'%' {
                while self
                    .bytes
                    .get(self.pos)
                    .is_some_and(|&b| !matches!(b, b'\n' | b'\r'))
                {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn next(&mut self) -> Result<Token, String> {
        if let Some(t) = self.peeked.pop() {
            return Ok(t);
        }
        self.skip_white();
        let b = self.byte(self.pos)?;
        let token = match b {
            b'[' => {
                self.pos += 1;
                Token::ArrayOpen
            }
            b']' => {
                self.pos += 1;
                Token::ArrayClose
            }
            b'<' if self.bytes.get(self.pos + 1) == Some(&b'<') => {
                self.pos += 2;
                Token::DictOpen
            }
            b'>' if self.bytes.get(self.pos + 1) == Some(&b'>') => {
                self.pos += 2;
                Token::DictClose
            }
            b'<' => self.hex_string()?,
            b'(' => self.literal_string()?,
            b'/' => self.name()?,
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.number()?,
            _ => self.keyword()?,
        };
        Ok(token)
    }

    fn push_back(&mut self, t: Token) {
        self.peeked.push(t);
    }

    fn name(&mut self) -> Result<Token, String> {
        self.pos += 1;
        let mut out = Vec::new();
        while let Some(&b) = self.bytes.get(self.pos) {
            if is_white(b) || is_delimiter(b) {
                break;
            }
            self.pos += 1;
            if b == b'#' {
                let hex = |c: u8| {
                    (c as char)
                        .to_digit(16)
                        .map(|d| d as u8)
                        .ok_or_else(|| "bad hex digit in name".to_string())
                };
                let hi = hex(self.byte(self.pos)?)?;
                let lo = hex(self.byte(self.pos + 1)?)?;
                self.pos += 2;
                out.push(hi << 4 | lo);
            } else {
                out.push(b);
            }
        }
        Ok(Token::Name(out))
    }

    fn number(&mut self) -> Result<Token, String> {
        let start = self.pos;
        self.pos += 1;
        while let Some(&b) = self.bytes.get(self.pos) {
            if matches!(b, b'0'..=b'9' | b'.') {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = String::from_utf8_lossy(&self.bytes[start..self.pos]);
        if text.contains('.') {
            text.parse()
                .map(Token::Real)
                .map_err(|_| format!("bad real {text:?} at {start}"))
        } else {
            text.parse()
                .map(Token::Int)
                .map_err(|_| format!("bad integer {text:?} at {start}"))
        }
    }

    fn keyword(&mut self) -> Result<Token, String> {
        let start = self.pos;
        while let Some(&b) = self.bytes.get(self.pos) {
            if is_white(b) || is_delimiter(b) {
                break;
            }
            self.pos += 1;
        }
        if self.pos == start {
            return Err(format!("stray delimiter at {start}"));
        }
        Ok(Token::Keyword(
            String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned(),
        ))
    }

    fn literal_string(&mut self) -> Result<Token, String> {
        self.pos += 1;
        let mut out = Vec::new();
        let mut depth = 1u32;
        loop {
            let b = self.byte(self.pos)?;
            self.pos += 1;
            match b {
                b'\\' => {
                    let e = self.byte(self.pos)?;
                    self.pos += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'0'..=b'7' => {
                            let mut v = u32::from(e - b'0');
                            for _ in 0..2 {
                                match self.bytes.get(self.pos) {
                                    Some(&d @ b'0'..=b'7') => {
                                        v = v * 8 + u32::from(d - b'0');
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(v as u8);
                        }
                        other => out.push(other),
                    }
                }
                b'(' => {
                    depth += 1;
                    out.push(b);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(Token::Str(out));
                    }
                    out.push(b);
                }
                other => out.push(other),
            }
        }
    }

    fn hex_string(&mut self) -> Result<Token, String> {
        self.pos += 1;
        let mut digits = Vec::new();
        loop {
            let b = self.byte(self.pos)?;
            self.pos += 1;
            if b == b'>' {
                break;
            }
            if is_white(b) {
                continue;
            }
            let d = (b as char)
                .to_digit(16)
                .ok_or_else(|| format!("bad hex digit {b:#x}"))? as u8;
            digits.push(d);
        }
        if digits.len() % 2 == 1 {
            digits.push(0);
        }
        Ok(Token::Str(
            digits.chunks(2).map(|p| p[0] << 4 | p[1]).collect(),
        ))
    }

    fn value(&mut self) -> Result<Value, String> {
        let value = match self.next()? {
            Token::Int(v) => {
                let second = self.next()?;
                if let Token::Int(generation) = second {
                    let third = self.next()?;
                    if third == Token::Keyword("R".to_string()) {
                        let id = u32::try_from(v).map_err(|_| "reference id out of range")?;
                        let generation = u16::try_from(generation)
                            .map_err(|_| "reference generation out of range")?;
                        return Ok(Value::Reference(id, generation));
                    }
                    self.push_back(third);
                }
                self.push_back(second);
                Value::Int(v)
            }
            Token::Real(v) => Value::Real(v),
            Token::Name(n) => Value::Name(n),
            Token::Str(s) => Value::Str(s),
            Token::ArrayOpen => {
                let mut items = Vec::new();
                loop {
                    let t = self.next()?;
                    if t == Token::ArrayClose {
                        break;
                    }
                    self.push_back(t);
                    items.push(self.value()?);
                }
                Value::Array(items)
            }
            Token::DictOpen => Value::Dict(self.dict_entries()?),
            Token::Keyword(k) => match k.as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "null" => Value::Null,
                other => return Err(format!("unexpected keyword {other:?} in a value")),
            },
            other => return Err(format!("unexpected token {other:?} in a value")),
        };
        Ok(value)
    }

    fn dict_entries(&mut self) -> Result<Vec<(Vec<u8>, Value)>, String> {
        let mut entries = Vec::new();
        loop {
            match self.next()? {
                Token::DictClose => return Ok(entries),
                Token::Name(key) => entries.push((key, self.value()?)),
                other => return Err(format!("expected a name key or >>, got {other:?}")),
            }
        }
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<(), String> {
        let t = self.next()?;
        if t == Token::Keyword(keyword.to_string()) {
            Ok(())
        } else {
            Err(format!("expected {keyword:?}, got {t:?}"))
        }
    }
}

fn find_last(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes.windows(needle.len()).rposition(|w| w == needle)
}

fn references(value: &Value, out: &mut Vec<(u32, u16)>) {
    match value {
        Value::Reference(id, generation) => out.push((*id, *generation)),
        Value::Array(items) => items.iter().for_each(|v| references(v, out)),
        Value::Dict(entries) => entries.iter().for_each(|(_, v)| references(v, out)),
        _ => {}
    }
}

/// Checks the structure of `bytes`; the first violation found.
pub fn check(bytes: &[u8]) -> Result<(), String> {
    if !bytes.starts_with(b"%PDF-1.7\n") {
        return Err("missing version header".to_string());
    }
    let marker_end = 9 + bytes[9..]
        .iter()
        .position(|&b| b == b'\n')
        .ok_or("no second line")?;
    let marker = &bytes[9..marker_end];
    if marker.first() != Some(&b'%') {
        return Err("second line must be a comment".to_string());
    }
    if marker[1..].iter().filter(|&&b| b >= 0x80).count() < 4 {
        return Err("binary marker needs four bytes at or above 0x80".to_string());
    }

    let tail_at = find_last(bytes, b"startxref").ok_or("no startxref")?;
    let mut lex = Lexer::new(bytes, tail_at);
    lex.expect_keyword("startxref")?;
    let startxref = match lex.next()? {
        Token::Int(v) if v >= 0 => v as usize,
        other => return Err(format!("expected an offset after startxref, got {other:?}")),
    };
    if !bytes[lex.pos..].starts_with(b"\n%%EOF") {
        return Err("file must end with %%EOF".to_string());
    }

    if !bytes
        .get(startxref..)
        .is_some_and(|rest| rest.starts_with(b"xref\n"))
    {
        return Err("startxref does not point at the xref keyword".to_string());
    }
    let mut pos = startxref + 5;
    let mut entries = Vec::new();
    let mut free_head = false;
    let mut covered = 0u64;
    while !bytes
        .get(pos..)
        .is_some_and(|rest| rest.starts_with(b"trailer"))
    {
        let line_end = pos
            + bytes
                .get(pos..)
                .and_then(|rest| rest.iter().position(|&b| b == b'\n'))
                .ok_or("xref subsection header unterminated")?;
        let header = String::from_utf8_lossy(&bytes[pos..line_end]);
        let (start, count) = header
            .split_once(' ')
            .ok_or_else(|| format!("bad xref subsection header {header:?}"))?;
        let start: u32 = start.trim().parse().map_err(|_| "bad subsection start")?;
        let count: u32 = count.trim().parse().map_err(|_| "bad subsection count")?;
        pos = line_end + 1;
        for i in 0..count {
            let id = start + i;
            let entry = bytes
                .get(pos..pos + 20)
                .ok_or_else(|| format!("xref entry for {id} truncated"))?;
            pos += 20;
            if entry[10] != b' ' || entry[16] != b' ' || &entry[18..20] != b"\r\n" {
                return Err(format!("xref entry for {id} malformed"));
            }
            let offset: u64 = String::from_utf8_lossy(&entry[..10])
                .parse()
                .map_err(|_| format!("xref entry for {id}: bad offset"))?;
            let generation: u32 = String::from_utf8_lossy(&entry[11..16])
                .parse()
                .map_err(|_| format!("xref entry for {id}: bad generation"))?;
            match entry[17] {
                b'n' => {
                    if generation != 0 {
                        return Err(format!("object {id}: in-use generation is not 0"));
                    }
                    entries.push((id, offset));
                }
                b'f' => {
                    if id != 0 || generation != 65535 {
                        return Err(format!("free entry {id} is not the free-list head"));
                    }
                    free_head = true;
                }
                other => return Err(format!("bad entry type {other:#x} for {id}")),
            }
            covered += 1;
        }
    }
    if !free_head {
        return Err("entry for object 0 missing".to_string());
    }

    let mut objects = BTreeMap::new();
    for &(id, offset) in &entries {
        let offset = offset as usize;
        let head = format!("{id} 0 obj");
        if !bytes
            .get(offset..)
            .is_some_and(|rest| rest.starts_with(head.as_bytes()))
        {
            return Err(format!("xref offset for object {id} does not land on it"));
        }
        let mut lex = Lexer::new(bytes, offset + head.len());
        let mut value = lex.value()?;
        if let Value::Dict(dict) = &value {
            let t = lex.next()?;
            if t == Token::Keyword("stream".to_string()) {
                let length = match Value::Dict(dict.clone()).get("Length") {
                    Some(Value::Int(n)) if *n >= 0 => *n as usize,
                    _ => return Err(format!("object {id}: stream without a direct Length")),
                };
                match bytes.get(lex.pos) {
                    Some(b'\n') => lex.pos += 1,
                    Some(b'\r') if bytes.get(lex.pos + 1) == Some(&b'\n') => lex.pos += 2,
                    _ => return Err(format!("object {id}: no end of line after stream")),
                }
                if bytes.len() < lex.pos + length {
                    return Err(format!("object {id}: stream data truncated"));
                }
                lex.pos += length;
                lex.expect_keyword("endstream")
                    .map_err(|e| format!("object {id}: {e}"))?;
                value = Value::Dict(dict.clone());
            } else {
                lex.push_back(t);
            }
        }
        lex.expect_keyword("endobj")
            .map_err(|e| format!("object {id}: {e}"))?;
        objects.insert(id, value);
    }

    let mut lex = Lexer::new(bytes, pos);
    lex.expect_keyword("trailer")?;
    if lex.next()? != Token::DictOpen {
        return Err("trailer dictionary missing".to_string());
    }
    let trailer = Value::Dict(lex.dict_entries()?);

    let mut refs = Vec::new();
    for value in objects.values() {
        references(value, &mut refs);
    }
    references(&trailer, &mut refs);
    for (id, generation) in refs {
        if generation != 0 {
            return Err(format!("reference to {id} has generation {generation}"));
        }
        if !objects.contains_key(&id) {
            return Err(format!("reference to {id} dangles"));
        }
    }

    let size = match trailer.get("Size") {
        Some(Value::Int(n)) => *n,
        _ => return Err("trailer Size missing".to_string()),
    };
    if size as u64 != covered {
        return Err(format!(
            "Size {size} does not equal the {covered} xref entries"
        ));
    }
    let max_id = objects.keys().max().copied().unwrap_or(0);
    if size != i64::from(max_id) + 1 {
        return Err(format!(
            "Size {size} is not the highest id {max_id} plus one"
        ));
    }
    let root = match trailer.get("Root") {
        Some(Value::Reference(id, 0)) => *id,
        _ => return Err("trailer Root missing".to_string()),
    };
    match objects.get(&root).and_then(|catalog| catalog.get("Type")) {
        Some(Value::Name(name)) if name == b"Catalog" => Ok(()),
        _ => Err("Root does not point at the catalog".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner;

    #[test]
    fn the_writers_documents_pass() {
        let pdf = runner::distill(
            "0 0 10 10 rectfill /Helvetica findfont 12 scalefont setfont 10 10 moveto (Hi) show showpage 1 0 0 setrgbcolor 0 0 moveto 5 5 lineto stroke showpage",
            100_000,
        )
        .unwrap();
        assert_eq!(check(&pdf), Ok(()));
        let empty = runner::distill("1 2 add", 100_000).unwrap();
        assert_eq!(check(&empty), Ok(()));
    }

    /// The first occurrence of `from` replaced by `to`, on bytes, so the
    /// binary marker survives.
    fn replaced(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
        let at = bytes
            .windows(from.len())
            .position(|w| w == from)
            .expect("the needle is present");
        let mut out = bytes[..at].to_vec();
        out.extend_from_slice(to);
        out.extend_from_slice(&bytes[at + from.len()..]);
        out
    }

    #[test]
    fn corruptions_are_named() {
        let pdf = runner::distill("0 0 10 10 rectfill showpage", 100_000).unwrap();
        assert!(check(b"%PDF-1.4\n").unwrap_err().contains("version header"));
        let no_eof = &pdf[..pdf.len() - 3];
        assert!(check(no_eof).unwrap_err().contains("%%EOF"));
        let shifted = replaced(&pdf, b"1 0 obj", b"9 0 obj");
        assert!(check(&shifted).unwrap_err().contains("does not land"));
        let bad_size = replaced(&pdf, b"/Size 6", b"/Size 7");
        assert!(check(&bad_size).unwrap_err().contains("Size"));
        let dangling = replaced(&pdf, b"/Root 4 0 R", b"/Root 9 0 R");
        assert!(check(&dangling).unwrap_err().contains("dangles"));
        let not_catalog = replaced(&pdf, b"/Root 4 0 R", b"/Root 5 0 R");
        assert!(check(&not_catalog).unwrap_err().contains("catalog"));
        let short_stream = replaced(&pdf, b"/Length 32", b"/Length 20");
        assert!(check(&short_stream).unwrap_err().contains("endstream"));
        let bad_entry = replaced(&pdf, b" n\r\n", b" x\r\n");
        assert!(check(&bad_entry).unwrap_err().contains("entry type"));
        let no_xref = replaced(&pdf, b"xref\n0 6", b"xref\n0 5");
        assert!(check(&no_xref).is_err());
    }

    #[test]
    fn the_lexer_reads_every_token_kind() {
        let mut lex = Lexer::new(
            b"<< /A [1 2.5 (s\\)t) <41 4> /N#20x true null] /R 3 0 R >>",
            0,
        );
        let value = lex.value().unwrap();
        assert_eq!(
            value.get("A"),
            Some(&Value::Array(vec![
                Value::Int(1),
                Value::Real(2.5),
                Value::Str(b"s)t".to_vec()),
                Value::Str(vec![0x41, 0x40]),
                Value::Name(b"N x".to_vec()),
                Value::Bool(true),
                Value::Null,
            ]))
        );
        assert_eq!(value.get("R"), Some(&Value::Reference(3, 0)));
        assert!(Lexer::new(b"]", 0).value().is_err());
        assert!(Lexer::new(b"", 0).next().is_err());
    }
}
