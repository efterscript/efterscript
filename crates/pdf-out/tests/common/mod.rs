// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Test-only PDF reader: a tokenizer, an xref walker, and structural
//! assertions over the writer's own output. Panics with a message on any
//! violation, so tests fail with the reason. Not product code.

#![allow(dead_code)] // Each integration-test target uses a subset.

pub mod inflate;

use std::collections::BTreeMap;

#[allow(unused_imports)] // Each integration-test target uses a subset.
pub use inflate::inflate;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Real(f64),
    Bool(bool),
    Null,
    Name(Vec<u8>),
    Str(Vec<u8>),
    Array(Vec<Value>),
    Dict(Vec<(Vec<u8>, Value)>),
    Reference(u32, u16),
    Stream {
        dict: Vec<(Vec<u8>, Value)>,
        data: Vec<u8>,
    },
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        let entries = match self {
            Value::Dict(d) | Value::Stream { dict: d, .. } => d,
            _ => return None,
        };
        entries
            .iter()
            .find(|(k, _)| k == key.as_bytes())
            .map(|(_, v)| v)
    }

    pub fn as_int(&self) -> i64 {
        match self {
            Value::Int(v) => *v,
            other => panic!("expected integer, got {other:?}"),
        }
    }

    pub fn as_reference(&self) -> u32 {
        match self {
            Value::Reference(id, 0) => *id,
            other => panic!("expected generation-0 reference, got {other:?}"),
        }
    }

    pub fn as_name(&self) -> &[u8] {
        match self {
            Value::Name(n) => n,
            other => panic!("expected name, got {other:?}"),
        }
    }

    pub fn stream_data(&self) -> &[u8] {
        match self {
            Value::Stream { data, .. } => data,
            other => panic!("expected stream, got {other:?}"),
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

    fn skip_white(&mut self) {
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if is_white(b) {
                self.pos += 1;
            } else if b == b'%' {
                while self.pos < self.bytes.len() && !matches!(self.bytes[self.pos], b'\n' | b'\r')
                {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn next(&mut self) -> Token {
        if let Some(t) = self.peeked.pop() {
            return t;
        }
        self.skip_white();
        let b = *self
            .bytes
            .get(self.pos)
            .unwrap_or_else(|| panic!("unexpected end of file at {}", self.pos));
        match b {
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
            b'<' => self.hex_string(),
            b'(' => self.literal_string(),
            b'/' => self.name(),
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.number(),
            _ => self.keyword(),
        }
    }

    fn push_back(&mut self, t: Token) {
        self.peeked.push(t);
    }

    fn name(&mut self) -> Token {
        self.pos += 1; // consume '/'
        let mut out = Vec::new();
        while let Some(&b) = self.bytes.get(self.pos) {
            if is_white(b) || is_delimiter(b) {
                break;
            }
            self.pos += 1;
            if b == b'#' {
                let hi = self.bytes[self.pos];
                let lo = self.bytes[self.pos + 1];
                self.pos += 2;
                let hex = |c: u8| (c as char).to_digit(16).expect("hex digit in #xx") as u8;
                out.push(hex(hi) << 4 | hex(lo));
            } else {
                out.push(b);
            }
        }
        Token::Name(out)
    }

    fn number(&mut self) -> Token {
        let start = self.pos;
        self.pos += 1;
        while let Some(&b) = self.bytes.get(self.pos) {
            if matches!(b, b'0'..=b'9' | b'.') {
                self.pos += 1;
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap();
        if text.contains('.') {
            Token::Real(text.parse().unwrap_or_else(|_| panic!("bad real {text:?}")))
        } else {
            Token::Int(text.parse().unwrap_or_else(|_| panic!("bad int {text:?}")))
        }
    }

    fn keyword(&mut self) -> Token {
        let start = self.pos;
        while let Some(&b) = self.bytes.get(self.pos) {
            if is_white(b) || is_delimiter(b) {
                break;
            }
            self.pos += 1;
        }
        assert!(self.pos > start, "stray delimiter at {}", start);
        Token::Keyword(String::from_utf8(self.bytes[start..self.pos].to_vec()).unwrap())
    }

    fn literal_string(&mut self) -> Token {
        self.pos += 1; // consume '('
        let mut out = Vec::new();
        let mut depth = 1u32;
        loop {
            let b = self.bytes[self.pos];
            self.pos += 1;
            match b {
                b'\\' => {
                    let e = self.bytes[self.pos];
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
                        return Token::Str(out);
                    }
                    out.push(b);
                }
                other => out.push(other),
            }
        }
    }

    fn hex_string(&mut self) -> Token {
        self.pos += 1; // consume '<'
        let mut digits = Vec::new();
        loop {
            let b = self.bytes[self.pos];
            self.pos += 1;
            if b == b'>' {
                break;
            }
            if is_white(b) {
                continue;
            }
            digits.push(
                (b as char)
                    .to_digit(16)
                    .unwrap_or_else(|| panic!("bad hex digit {b:#x}")) as u8,
            );
        }
        if digits.len() % 2 == 1 {
            digits.push(0);
        }
        Token::Str(digits.chunks(2).map(|p| p[0] << 4 | p[1]).collect())
    }

    fn value(&mut self) -> Value {
        match self.next() {
            Token::Int(v) => {
                // Lookahead for "gen R".
                let second = self.next();
                if let Token::Int(second_num) = second {
                    let third = self.next();
                    if third == Token::Keyword("R".to_string()) {
                        return Value::Reference(
                            u32::try_from(v).expect("reference id fits u32"),
                            u16::try_from(second_num).expect("generation fits u16"),
                        );
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
                    let t = self.next();
                    if t == Token::ArrayClose {
                        return Value::Array(items);
                    }
                    self.push_back(t);
                    items.push(self.value());
                }
            }
            Token::DictOpen => Value::Dict(self.dict_entries()),
            Token::Keyword(k) => match k.as_str() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                "null" => Value::Null,
                other => panic!("unexpected keyword {other:?} where a value was expected"),
            },
            other => panic!("unexpected token {other:?} where a value was expected"),
        }
    }

    fn dict_entries(&mut self) -> Vec<(Vec<u8>, Value)> {
        let mut entries = Vec::new();
        loop {
            match self.next() {
                Token::DictClose => return entries,
                Token::Name(key) => entries.push((key, self.value())),
                other => panic!("expected name key or >>, got {other:?}"),
            }
        }
    }

    fn expect_keyword(&mut self, kw: &str) {
        let t = self.next();
        assert_eq!(t, Token::Keyword(kw.to_string()), "expected {kw:?}");
    }

    /// Consumes exactly one end-of-line sequence after `stream`.
    fn eol_after_stream(&mut self) {
        match self.bytes[self.pos] {
            b'\r' if self.bytes.get(self.pos + 1) == Some(&b'\n') => self.pos += 2,
            b'\n' => self.pos += 1,
            other => panic!("expected end of line after `stream`, got {other:#x}"),
        }
    }
}

pub struct Pdf {
    pub objects: BTreeMap<u32, Value>,
    pub trailer: Vec<(Vec<u8>, Value)>,
    /// In-use xref entries as (id, offset).
    pub entries: Vec<(u32, u64)>,
    pub startxref: u64,
}

impl Pdf {
    pub fn trailer_get(&self, key: &str) -> Option<&Value> {
        self.trailer
            .iter()
            .find(|(k, _)| k == key.as_bytes())
            .map(|(_, v)| v)
    }

    pub fn resolve(&self, r: u32) -> &Value {
        self.objects
            .get(&r)
            .unwrap_or_else(|| panic!("object {r} not in file"))
    }
}

/// Walks the whole file and asserts the structural invariants: header and
/// binary marker, every xref offset at its `N 0 obj`, exact stream lengths,
/// every reference resolvable, trailer `Root` and `Size` correct, `%%EOF`.
pub fn check(bytes: &[u8]) -> Pdf {
    // Header (any 1.x version) and binary marker.
    assert!(bytes.starts_with(b"%PDF-1."), "missing version header");
    assert!(
        bytes[7].is_ascii_digit() && bytes[8] == b'\n',
        "version header must be one digit each side of the dot"
    );
    let marker_end = 9 + bytes[9..]
        .iter()
        .position(|&b| b == b'\n')
        .expect("second line");
    let marker = &bytes[9..marker_end];
    assert_eq!(marker.first(), Some(&b'%'), "second line must be a comment");
    assert!(
        marker[1..].iter().filter(|&&b| b >= 0x80).count() >= 4,
        "binary marker needs four bytes >= 0x80"
    );

    // Tail: startxref <offset> %%EOF.
    let tail_at = bytes
        .windows(9)
        .rposition(|w| w == b"startxref")
        .expect("startxref keyword");
    let mut lex = Lexer::new(bytes, tail_at);
    lex.expect_keyword("startxref");
    let startxref = match lex.next() {
        Token::Int(v) => u64::try_from(v).expect("non-negative startxref"),
        other => panic!("expected offset after startxref, got {other:?}"),
    };
    // Not skip_white: the comment-skipping lexer would swallow the marker.
    assert!(
        bytes[lex.pos..].starts_with(b"\n%%EOF"),
        "file must end with %%EOF"
    );

    // Cross-reference section: fixed 20-byte entries.
    let mut pos = startxref as usize;
    assert!(
        bytes[pos..].starts_with(b"xref\n"),
        "startxref must point at the xref keyword"
    );
    pos += 5;
    let mut entries = Vec::new();
    let mut free_head_seen = false;
    let mut ids_covered = 0u64;
    while !bytes[pos..].starts_with(b"trailer") {
        let line_end = pos + bytes[pos..].iter().position(|&b| b == b'\n').unwrap();
        let header = std::str::from_utf8(&bytes[pos..line_end]).unwrap();
        let (start, count) = header.split_once(' ').expect("subsection header");
        let start: u32 = start.parse().unwrap();
        let count: u32 = count.parse().unwrap();
        pos = line_end + 1;
        for i in 0..count {
            let id = start + i;
            let entry = &bytes[pos..pos + 20];
            pos += 20;
            assert_eq!(&entry[10..11], b" ", "entry format");
            assert_eq!(&entry[16..17], b" ", "entry format");
            assert_eq!(&entry[18..20], b"\r\n", "entry must end in CR LF");
            let offset: u64 = std::str::from_utf8(&entry[..10]).unwrap().parse().unwrap();
            let generation: u32 = std::str::from_utf8(&entry[11..16])
                .unwrap()
                .parse()
                .unwrap();
            match entry[17] {
                b'n' => {
                    assert_eq!(generation, 0, "in-use generations are 0");
                    entries.push((id, offset));
                }
                b'f' => {
                    assert_eq!(id, 0, "only the free-list head is free");
                    assert_eq!(generation, 65535, "free-list head generation");
                    free_head_seen = true;
                }
                other => panic!("bad entry type {other:#x}"),
            }
            ids_covered += 1;
        }
    }
    assert!(free_head_seen, "entry for object 0 missing");

    // Parse each object at its recorded offset.
    let mut objects = BTreeMap::new();
    for &(id, offset) in &entries {
        let mut lex = Lexer::new(bytes, offset as usize);
        // The offset must point at the first byte of "N 0 obj".
        assert!(
            bytes[offset as usize..].starts_with(id.to_string().as_bytes()),
            "xref offset for object {id} does not point at its id"
        );
        assert_eq!(lex.next(), Token::Int(i64::from(id)), "object id");
        assert_eq!(lex.next(), Token::Int(0), "object generation");
        lex.expect_keyword("obj");
        let mut value = lex.value();
        if let Value::Dict(dict) = value {
            let t = lex.next();
            if t == Token::Keyword("stream".to_string()) {
                let dict_value = Value::Dict(dict.clone());
                let length = dict_value.get("Length").expect("stream Length").as_int() as usize;
                lex.eol_after_stream();
                let data = bytes[lex.pos..lex.pos + length].to_vec();
                lex.pos += length;
                lex.expect_keyword("endstream");
                value = Value::Stream { dict, data };
            } else {
                lex.push_back(t);
                value = Value::Dict(dict);
            }
        }
        lex.expect_keyword("endobj");
        objects.insert(id, value);
    }

    // Trailer.
    let mut lex = Lexer::new(bytes, pos);
    lex.expect_keyword("trailer");
    assert_eq!(lex.next(), Token::DictOpen, "trailer dictionary");
    let trailer = lex.dict_entries();

    let pdf = Pdf {
        objects,
        trailer,
        entries,
        startxref,
    };

    // Every reference in every object and in the trailer resolves.
    for value in pdf.objects.values() {
        assert_references_resolve(value, &pdf);
    }
    for (_, value) in &pdf.trailer {
        assert_references_resolve(value, &pdf);
    }

    // Size covers all ids; Root resolves to the catalog.
    let size = pdf.trailer_get("Size").expect("trailer Size").as_int();
    assert_eq!(size as u64, ids_covered, "Size equals entry count");
    let max_id = pdf.objects.keys().max().copied().unwrap_or(0);
    assert_eq!(size, i64::from(max_id) + 1, "Size is highest id plus one");
    let root = pdf
        .trailer_get("Root")
        .expect("trailer Root")
        .as_reference();
    assert_eq!(
        pdf.resolve(root)
            .get("Type")
            .expect("catalog Type")
            .as_name(),
        b"Catalog",
        "Root must point at the catalog"
    );

    pdf
}

fn assert_references_resolve(value: &Value, pdf: &Pdf) {
    match value {
        Value::Reference(id, generation) => {
            assert_eq!(*generation, 0, "generation is always 0");
            assert!(pdf.objects.contains_key(id), "reference to {id} dangles");
        }
        Value::Array(items) => {
            for item in items {
                assert_references_resolve(item, pdf);
            }
        }
        Value::Dict(entries) | Value::Stream { dict: entries, .. } => {
            for (_, v) in entries {
                assert_references_resolve(v, pdf);
            }
        }
        _ => {}
    }
}
