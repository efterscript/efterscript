// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The scanner: bytes from a [`Source`] to [`Object`] values.
//!
//! Token syntax is PLRM3 §3.2. The scanner is a byte-driven state machine
//! whose whole state lives in [`Scanner`], so it can suspend at end of input
//! with `NeedMore` and continue when bytes arrive. It reads one byte of
//! lookahead at most and consumes nothing beyond a token except the single
//! whitespace byte that ended it, so an operator can read raw data from the
//! same source right after the token that introduced it.

use std::fmt;

use crate::error::VmError;
use crate::memory::Memory;
use crate::names::{Atom, NameTooLong};
use crate::object::{Object, Space};
use crate::source::{SliceSource, Source, Span};

/// Deepest `{` nesting accepted; deeper is `limitcheck`.
pub const MAX_PROC_DEPTH: usize = 1000;

/// Longest string literal accepted, per PLRM3 Appendix B.
pub const MAX_STRING_LEN: usize = 65535;

/// Answers `//name` lookups; the interpreter supplies its dictionary stack.
pub trait Resolver {
    fn resolve(&mut self, name: Atom, memory: &mut Memory) -> Option<Object>;
}

/// A resolver that defines nothing.
impl Resolver for () {
    fn resolve(&mut self, _: Atom, _: &mut Memory) -> Option<Object> {
        None
    }
}

impl<F: FnMut(Atom, &mut Memory) -> Option<Object>> Resolver for F {
    fn resolve(&mut self, name: Atom, memory: &mut Memory) -> Option<Object> {
        self(name, memory)
    }
}

/// Receives DSC comment lines (`%%…` and `%!…`) with their span.
pub type DscObserver<'a> = Box<dyn FnMut(&[u8], Span) + 'a>;

/// The outcome of one scanning step.
#[derive(Clone, Copy, Debug)]
pub enum Scan {
    Token {
        object: Object,
        span: Span,
    },
    /// Clean end of input.
    End,
    /// Input ended mid-token and the source says more may come.
    NeedMore,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScanErrorKind {
    SyntaxError,
    LimitCheck,
    Undefined,
    /// A binary-token or binary-object-sequence lead byte (128–159), which
    /// this scanner does not yet decode.
    BinaryEncoding,
    /// Raised by the VM while the scanner allocated or read.
    Vm(VmError),
}

impl ScanErrorKind {
    /// The PostScript error name.
    pub const fn name(self) -> &'static str {
        match self {
            ScanErrorKind::SyntaxError | ScanErrorKind::BinaryEncoding => "syntaxerror",
            ScanErrorKind::LimitCheck => "limitcheck",
            ScanErrorKind::Undefined => "undefined",
            ScanErrorKind::Vm(e) => e.name(),
        }
    }
}

impl From<VmError> for ScanErrorKind {
    fn from(e: VmError) -> Self {
        ScanErrorKind::Vm(e)
    }
}

impl From<NameTooLong> for ScanErrorKind {
    fn from(_: NameTooLong) -> Self {
        ScanErrorKind::LimitCheck
    }
}

/// A scanning error: what went wrong, where, and the byte that triggered it
/// when one did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanError {
    pub kind: ScanErrorKind,
    pub span: Span,
    pub byte: Option<u8>,
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.kind.name(), self.span)?;
        if let Some(b) = self.byte {
            write!(f, " (byte 0x{b:02x})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ScanError {}

const fn is_whitespace(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0C | 0x00)
}

const fn is_delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

const fn is_regular(b: u8) -> bool {
    !is_whitespace(b) && !is_delimiter(b)
}

const fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

const fn digit_value(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some((b - b'0') as u32),
        b'a'..=b'z' => Some((b - b'a') as u32 + 10),
        b'A'..=b'Z' => Some((b - b'A') as u32 + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Numbers

fn split_sign(s: &[u8]) -> &[u8] {
    match s.first() {
        Some(b'+' | b'-') => &s[1..],
        _ => s,
    }
}

fn ascii(s: &[u8]) -> &str {
    std::str::from_utf8(s).expect("number syntax is ASCII")
}

fn finite(value: f32) -> Result<Object, ScanErrorKind> {
    if value.is_finite() {
        Ok(Object::real(value))
    } else {
        Err(ScanErrorKind::LimitCheck)
    }
}

// `[+-]?digits`; a value outside `i32` becomes a real.
fn parse_decimal(s: &[u8]) -> Result<Option<Object>, ScanErrorKind> {
    let digits = split_sign(s);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Ok(None);
    }
    let text = ascii(s);
    match text.parse::<i32>() {
        Ok(v) => Ok(Some(Object::integer(v))),
        Err(_) => finite(text.parse::<f32>().expect("digits parse as a real")).map(Some),
    }
}

// `base#digits`, base 2–36, as an unsigned 32-bit pattern.
fn parse_radix(s: &[u8]) -> Result<Option<Object>, ScanErrorKind> {
    let Some(hash) = s.iter().position(|&b| b == b'#') else {
        return Ok(None);
    };
    let (base, digits) = (&s[..hash], &s[hash + 1..]);
    if base.is_empty() || digits.is_empty() || !base.iter().all(u8::is_ascii_digit) {
        return Ok(None);
    }
    let Ok(base) = ascii(base).parse::<u32>() else {
        return Ok(None);
    };
    if !(2..=36).contains(&base) {
        return Ok(None);
    }
    let mut value: u32 = 0;
    for &b in digits {
        match digit_value(b) {
            Some(d) if d < base => {
                value = value
                    .checked_mul(base)
                    .and_then(|v| v.checked_add(d))
                    .ok_or(ScanErrorKind::LimitCheck)?;
            }
            _ => return Ok(None),
        }
    }
    Ok(Some(Object::integer(value as i32)))
}

// `[+-]? (digits [. digits*] | . digits) ([eE] [+-]? digits)?` with at
// least a point or an exponent, parsed straight to `f32`.
fn parse_real(s: &[u8]) -> Result<Option<Object>, ScanErrorKind> {
    let body = split_sign(s);
    let (mantissa, exponent) = match body.iter().position(|&b| matches!(b, b'e' | b'E')) {
        Some(e) => (&body[..e], Some(&body[e + 1..])),
        None => (body, None),
    };
    let (int_part, frac_part) = match mantissa.iter().position(|&b| b == b'.') {
        Some(p) => (&mantissa[..p], Some(&mantissa[p + 1..])),
        None => (mantissa, None),
    };
    let digits_only = |d: &[u8]| d.iter().all(u8::is_ascii_digit);
    if !digits_only(int_part) || !frac_part.is_none_or(digits_only) {
        return Ok(None);
    }
    if int_part.is_empty() && frac_part.is_none_or(<[u8]>::is_empty) {
        return Ok(None);
    }
    if frac_part.is_none() && exponent.is_none() {
        return Ok(None);
    }
    if let Some(exponent) = exponent {
        let exponent = split_sign(exponent);
        if exponent.is_empty() || !digits_only(exponent) {
            return Ok(None);
        }
    }
    let Ok(value) = ascii(s).parse::<f32>() else {
        return Ok(None);
    };
    finite(value).map(Some)
}

/// The number a regular-character token denotes, if it is one. `Err` is a
/// number whose value cannot be represented.
pub fn parse_number(s: &[u8]) -> Result<Option<Object>, ScanErrorKind> {
    if let Some(n) = parse_decimal(s)? {
        return Ok(Some(n));
    }
    if let Some(n) = parse_radix(s)? {
        return Ok(Some(n));
    }
    parse_real(s)
}

// ---------------------------------------------------------------------------
// Scanner

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Between,
    Comment { line_start: bool, dsc: Option<bool> },
    Word,
    Slash,
    Name { immediate: bool },
    Less,
    Greater,
    Str { depth: u32 },
    StrEscape { depth: u32 },
    StrOctal { depth: u32, value: u32, digits: u8 },
    StrCr { depth: u32 },
    Hex { high: Option<u8> },
    A85 { group: [u8; 5], n: u8 },
    A85Tilde { group: [u8; 5], n: u8 },
}

#[derive(Debug)]
struct Frame {
    items: Vec<Object>,
    start: usize,
}

/// The scanner state machine plus the procedure-nesting stack.
pub struct Scanner<'a> {
    state: State,
    start: usize,
    buf: Vec<u8>,
    procs: Vec<Frame>,
    at_line_start: bool,
    observer: Option<DscObserver<'a>>,
}

impl fmt::Debug for Scanner<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scanner")
            .field("state", &self.state)
            .field("start", &self.start)
            .field("buffered", &self.buf.len())
            .field("depth", &self.procs.len())
            .field("observer", &self.observer.is_some())
            .finish()
    }
}

impl Default for Scanner<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> Scanner<'a> {
    pub fn new() -> Self {
        Scanner {
            state: State::Between,
            start: 0,
            buf: Vec::new(),
            procs: Vec::new(),
            at_line_start: true,
            observer: None,
        }
    }

    pub fn with_dsc_observer(observer: impl FnMut(&[u8], Span) + 'a) -> Self {
        let mut scanner = Self::new();
        scanner.observer = Some(Box::new(observer));
        scanner
    }

    pub fn set_dsc_observer(&mut self, observer: Option<DscObserver<'a>>) {
        self.observer = observer;
    }

    /// Current `{` nesting depth.
    pub fn depth(&self) -> usize {
        self.procs.len()
    }

    /// Whether a token is partially scanned (after `NeedMore`).
    pub fn is_mid_token(&self) -> bool {
        self.state != State::Between || !self.procs.is_empty()
    }

    /// Discards any partial token and open procedures.
    pub fn reset(&mut self) {
        self.state = State::Between;
        self.buf.clear();
        self.procs.clear();
    }

    fn fail(&mut self, kind: ScanErrorKind, span: Span, byte: Option<u8>) -> ScanError {
        self.reset();
        ScanError { kind, span, byte }
    }

    fn take(&mut self, source: &mut dyn Source, memory: &mut Memory, byte: u8) {
        source.advance(memory);
        self.at_line_start = matches!(byte, b'\r' | b'\n');
    }

    // Hands a finished object to the enclosing procedure, or out.
    fn emit(
        &mut self,
        memory: &Memory,
        object: Object,
        span: Span,
    ) -> Result<Option<Scan>, ScanError> {
        self.state = State::Between;
        match self.procs.last_mut() {
            Some(frame) => {
                if memory.current_space() == Space::Global && object.space() == Some(Space::Local) {
                    return Err(self.fail(VmError::InvalidAccess.into(), span, None));
                }
                frame.items.push(object);
                Ok(None)
            }
            None => Ok(Some(Scan::Token { object, span })),
        }
    }

    fn emit_name(
        &mut self,
        memory: &mut Memory,
        text: &[u8],
        executable: bool,
        span: Span,
    ) -> Result<Option<Scan>, ScanError> {
        let name = match memory.intern(text) {
            Ok(name) => name.with_exec(executable),
            Err(e) => return Err(self.fail(e.into(), span, None)),
        };
        self.emit(memory, name, span)
    }

    fn finish_word(&mut self, memory: &mut Memory, end: usize) -> Result<Option<Scan>, ScanError> {
        let span = Span::new(self.start, end);
        let word = std::mem::take(&mut self.buf);
        let result = match parse_number(&word) {
            Ok(Some(number)) => self.emit(memory, number, span),
            Ok(None) => self.emit_name(memory, &word, true, span),
            Err(kind) => Err(self.fail(kind, span, None)),
        };
        self.buf = word;
        self.buf.clear();
        result
    }

    fn finish_name(
        &mut self,
        memory: &mut Memory,
        resolver: &mut dyn Resolver,
        immediate: bool,
        end: usize,
    ) -> Result<Option<Scan>, ScanError> {
        let span = Span::new(self.start, end);
        let text = std::mem::take(&mut self.buf);
        let result = if immediate {
            match memory.names_mut().intern(&text) {
                Ok(atom) => match resolver.resolve(atom, memory) {
                    Some(value) => self.emit(memory, value, span),
                    None => Err(self.fail(ScanErrorKind::Undefined, span, None)),
                },
                Err(e) => Err(self.fail(e.into(), span, None)),
            }
        } else {
            self.emit_name(memory, &text, false, span)
        };
        self.buf = text;
        self.buf.clear();
        result
    }

    fn push_string_byte(&mut self, byte: u8, at: usize) -> Result<(), ScanError> {
        if self.buf.len() >= MAX_STRING_LEN {
            return Err(self.fail(
                ScanErrorKind::LimitCheck,
                Span::new(self.start, at),
                Some(byte),
            ));
        }
        self.buf.push(byte);
        Ok(())
    }

    fn finish_string(
        &mut self,
        memory: &mut Memory,
        end: usize,
    ) -> Result<Option<Scan>, ScanError> {
        let bytes = std::mem::take(&mut self.buf);
        let string = memory.alloc_string(bytes);
        self.emit(memory, string, Span::new(self.start, end))
    }

    fn end_comment(&mut self, dsc: Option<bool>, end: usize) {
        if dsc == Some(true)
            && let Some(observer) = self.observer.as_mut()
        {
            observer(&self.buf, Span::new(self.start, end));
        }
        self.buf.clear();
        self.state = State::Between;
    }

    fn decode_a85(&mut self, group: [u8; 5], count: usize, at: usize) -> Result<(), ScanError> {
        let mut value: u64 = 0;
        for (i, &digit) in group.iter().enumerate() {
            let digit = if i < count { digit } else { 84 };
            value = value * 85 + u64::from(digit);
        }
        let Ok(value) = u32::try_from(value) else {
            return Err(self.fail(ScanErrorKind::SyntaxError, Span::new(self.start, at), None));
        };
        for byte in value.to_be_bytes().into_iter().take(count - 1) {
            self.push_string_byte(byte, at)?;
        }
        Ok(())
    }

    /// Scans the next token. Returns `Scan::End` at a clean end of input and
    /// `Scan::NeedMore` when the input ends mid-token but the source may
    /// still grow; a later call continues where it left off.
    pub fn next(
        &mut self,
        source: &mut dyn Source,
        memory: &mut Memory,
        resolver: &mut dyn Resolver,
    ) -> Result<Scan, ScanError> {
        loop {
            let pos = source.position(memory);
            let byte = match source.peek(memory) {
                Ok(byte) => byte,
                Err(e) => return Err(self.fail(e.into(), Span::new(pos, pos), None)),
            };
            let more = byte.is_none() && source.more_may_come();
            let here = Span::new(self.start, pos);
            let next = Span::new(pos, pos + 1);

            match self.state {
                State::Between => {
                    let Some(b) = byte else {
                        if more {
                            return Ok(Scan::NeedMore);
                        }
                        if let Some(frame) = self.procs.first() {
                            let start = frame.start;
                            return Err(self.fail(
                                ScanErrorKind::SyntaxError,
                                Span::new(start, pos),
                                None,
                            ));
                        }
                        return Ok(Scan::End);
                    };
                    self.start = pos;
                    self.buf.clear();
                    match b {
                        _ if is_whitespace(b) => self.take(source, memory, b),
                        b'%' => {
                            let line_start = self.at_line_start;
                            self.take(source, memory, b);
                            self.buf.push(b);
                            self.state = State::Comment {
                                line_start,
                                dsc: None,
                            };
                        }
                        b'(' => {
                            self.take(source, memory, b);
                            self.state = State::Str { depth: 1 };
                        }
                        b')' => {
                            self.take(source, memory, b);
                            return Err(self.fail(ScanErrorKind::SyntaxError, next, Some(b)));
                        }
                        b'<' => {
                            self.take(source, memory, b);
                            self.state = State::Less;
                        }
                        b'>' => {
                            self.take(source, memory, b);
                            self.state = State::Greater;
                        }
                        b'[' | b']' => {
                            self.take(source, memory, b);
                            if let Some(scan) = self.emit_name(memory, &[b], true, next)? {
                                return Ok(scan);
                            }
                        }
                        b'{' => {
                            self.take(source, memory, b);
                            if self.procs.len() >= MAX_PROC_DEPTH {
                                return Err(self.fail(ScanErrorKind::LimitCheck, next, Some(b)));
                            }
                            self.procs.push(Frame {
                                items: Vec::new(),
                                start: pos,
                            });
                        }
                        b'}' => {
                            self.take(source, memory, b);
                            let Some(frame) = self.procs.pop() else {
                                return Err(self.fail(ScanErrorKind::SyntaxError, next, Some(b)));
                            };
                            let span = Span::new(frame.start, pos + 1);
                            let array = match memory.alloc_array(frame.items) {
                                Ok(array) => array.as_executable(),
                                Err(e) => return Err(self.fail(e.into(), span, None)),
                            };
                            if let Some(scan) = self.emit(memory, array, span)? {
                                return Ok(scan);
                            }
                        }
                        b'/' => {
                            self.take(source, memory, b);
                            self.state = State::Slash;
                        }
                        128..=159 => {
                            self.take(source, memory, b);
                            return Err(self.fail(ScanErrorKind::BinaryEncoding, next, Some(b)));
                        }
                        _ => self.state = State::Word,
                    }
                }

                State::Comment { line_start, dsc } => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    None => self.end_comment(dsc, pos),
                    Some(b @ (b'\r' | b'\n')) => {
                        self.end_comment(dsc, pos);
                        self.take(source, memory, b);
                    }
                    Some(b) => {
                        self.take(source, memory, b);
                        match dsc {
                            None => {
                                let is_dsc = line_start && matches!(b, b'%' | b'!');
                                if is_dsc {
                                    self.buf.push(b);
                                } else {
                                    self.buf.clear();
                                }
                                self.state = State::Comment {
                                    line_start,
                                    dsc: Some(is_dsc),
                                };
                            }
                            Some(true) => self.buf.push(b),
                            Some(false) => {}
                        }
                    }
                },

                State::Word => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    Some(b) if is_regular(b) => {
                        self.take(source, memory, b);
                        self.buf.push(b);
                    }
                    _ => {
                        let scan = self.finish_word(memory, pos)?;
                        if let Some(b) = byte.filter(|&b| is_whitespace(b)) {
                            self.take(source, memory, b);
                        }
                        if let Some(scan) = scan {
                            return Ok(scan);
                        }
                    }
                },

                State::Slash => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    Some(b'/') => {
                        self.take(source, memory, b'/');
                        self.state = State::Name { immediate: true };
                    }
                    _ => self.state = State::Name { immediate: false },
                },

                State::Name { immediate } => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    Some(b) if is_regular(b) => {
                        self.take(source, memory, b);
                        self.buf.push(b);
                    }
                    _ => {
                        let scan = self.finish_name(memory, resolver, immediate, pos)?;
                        if let Some(b) = byte.filter(|&b| is_whitespace(b)) {
                            self.take(source, memory, b);
                        }
                        if let Some(scan) = scan {
                            return Ok(scan);
                        }
                    }
                },

                State::Less => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    None => return Err(self.fail(ScanErrorKind::SyntaxError, here, None)),
                    Some(b'<') => {
                        self.take(source, memory, b'<');
                        let span = Span::new(self.start, pos + 1);
                        if let Some(scan) = self.emit_name(memory, b"<<", true, span)? {
                            return Ok(scan);
                        }
                    }
                    Some(b'~') => {
                        self.take(source, memory, b'~');
                        self.state = State::A85 {
                            group: [0; 5],
                            n: 0,
                        };
                    }
                    Some(_) => self.state = State::Hex { high: None },
                },

                State::Greater => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    Some(b'>') => {
                        self.take(source, memory, b'>');
                        let span = Span::new(self.start, pos + 1);
                        if let Some(scan) = self.emit_name(memory, b">>", true, span)? {
                            return Ok(scan);
                        }
                    }
                    _ => return Err(self.fail(ScanErrorKind::SyntaxError, here, byte)),
                },

                State::Str { depth } => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    None => return Err(self.fail(ScanErrorKind::SyntaxError, here, None)),
                    Some(b'\\') => {
                        self.take(source, memory, b'\\');
                        self.state = State::StrEscape { depth };
                    }
                    Some(b'(') => {
                        self.take(source, memory, b'(');
                        self.push_string_byte(b'(', pos)?;
                        self.state = State::Str { depth: depth + 1 };
                    }
                    Some(b')') => {
                        self.take(source, memory, b')');
                        if depth == 1 {
                            if let Some(scan) = self.finish_string(memory, pos + 1)? {
                                return Ok(scan);
                            }
                        } else {
                            self.push_string_byte(b')', pos)?;
                            self.state = State::Str { depth: depth - 1 };
                        }
                    }
                    Some(b) => {
                        self.take(source, memory, b);
                        self.push_string_byte(b, pos)?;
                    }
                },

                State::StrEscape { depth } => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    None => return Err(self.fail(ScanErrorKind::SyntaxError, here, None)),
                    Some(b) => {
                        self.take(source, memory, b);
                        self.state = State::Str { depth };
                        match b {
                            b'n' => self.push_string_byte(b'\n', pos)?,
                            b'r' => self.push_string_byte(b'\r', pos)?,
                            b't' => self.push_string_byte(b'\t', pos)?,
                            b'b' => self.push_string_byte(0x08, pos)?,
                            b'f' => self.push_string_byte(0x0C, pos)?,
                            b'0'..=b'7' => {
                                self.state = State::StrOctal {
                                    depth,
                                    value: u32::from(b - b'0'),
                                    digits: 1,
                                }
                            }
                            b'\r' => self.state = State::StrCr { depth },
                            b'\n' => {}
                            _ => self.push_string_byte(b, pos)?,
                        }
                    }
                },

                State::StrOctal {
                    depth,
                    value,
                    digits,
                } => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    Some(b @ b'0'..=b'7') if digits < 3 => {
                        self.take(source, memory, b);
                        let value = value * 8 + u32::from(b - b'0');
                        if digits == 2 {
                            self.push_string_byte((value & 0xFF) as u8, pos)?;
                            self.state = State::Str { depth };
                        } else {
                            self.state = State::StrOctal {
                                depth,
                                value,
                                digits: digits + 1,
                            };
                        }
                    }
                    _ => {
                        self.push_string_byte((value & 0xFF) as u8, pos)?;
                        self.state = State::Str { depth };
                    }
                },

                State::StrCr { depth } => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    Some(b'\n') => {
                        self.take(source, memory, b'\n');
                        self.state = State::Str { depth };
                    }
                    _ => self.state = State::Str { depth },
                },

                State::Hex { high } => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    None => return Err(self.fail(ScanErrorKind::SyntaxError, here, None)),
                    Some(b'>') => {
                        self.take(source, memory, b'>');
                        if let Some(h) = high {
                            self.push_string_byte(h << 4, pos)?;
                        }
                        if let Some(scan) = self.finish_string(memory, pos + 1)? {
                            return Ok(scan);
                        }
                    }
                    Some(b) if is_whitespace(b) => self.take(source, memory, b),
                    Some(b) => match hex_value(b) {
                        Some(d) => {
                            self.take(source, memory, b);
                            match high {
                                None => self.state = State::Hex { high: Some(d) },
                                Some(h) => {
                                    self.push_string_byte(h << 4 | d, pos)?;
                                    self.state = State::Hex { high: None };
                                }
                            }
                        }
                        None => {
                            self.take(source, memory, b);
                            return Err(self.fail(ScanErrorKind::SyntaxError, next, Some(b)));
                        }
                    },
                },

                State::A85 { mut group, n } => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    None => return Err(self.fail(ScanErrorKind::SyntaxError, here, None)),
                    Some(b'~') => {
                        self.take(source, memory, b'~');
                        self.state = State::A85Tilde { group, n };
                    }
                    Some(b) if is_whitespace(b) => self.take(source, memory, b),
                    Some(b'z') if n == 0 => {
                        self.take(source, memory, b'z');
                        for _ in 0..4 {
                            self.push_string_byte(0, pos)?;
                        }
                    }
                    Some(b @ b'!'..=b'u') => {
                        self.take(source, memory, b);
                        group[usize::from(n)] = b - b'!';
                        if n == 4 {
                            self.decode_a85(group, 5, pos)?;
                            self.state = State::A85 {
                                group: [0; 5],
                                n: 0,
                            };
                        } else {
                            self.state = State::A85 { group, n: n + 1 };
                        }
                    }
                    Some(b) => {
                        self.take(source, memory, b);
                        return Err(self.fail(ScanErrorKind::SyntaxError, next, Some(b)));
                    }
                },

                State::A85Tilde { group, n } => match byte {
                    None if more => return Ok(Scan::NeedMore),
                    Some(b'>') => {
                        self.take(source, memory, b'>');
                        match n {
                            0 => {}
                            1 => {
                                return Err(self.fail(ScanErrorKind::SyntaxError, here, None));
                            }
                            n => self.decode_a85(group, usize::from(n), pos)?,
                        }
                        if let Some(scan) = self.finish_string(memory, pos + 1)? {
                            return Ok(scan);
                        }
                    }
                    _ => return Err(self.fail(ScanErrorKind::SyntaxError, here, byte)),
                },
            }
        }
    }
}

/// Every token in `bytes`, with spans, using a fresh scanner and no DSC
/// observer.
pub fn scan_all(
    bytes: &[u8],
    memory: &mut Memory,
    resolver: &mut dyn Resolver,
) -> Result<Vec<(Object, Span)>, ScanError> {
    let mut source = SliceSource::new(bytes);
    let mut scanner = Scanner::new();
    let mut tokens = Vec::new();
    loop {
        match scanner.next(&mut source, memory, resolver)? {
            Scan::Token { object, span } => tokens.push((object, span)),
            Scan::End | Scan::NeedMore => return Ok(tokens),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(s: &str) -> Option<i32> {
        parse_number(s.as_bytes()).unwrap().and_then(Object::as_i32)
    }

    fn real(s: &str) -> Option<f32> {
        parse_number(s.as_bytes()).unwrap().and_then(Object::as_f32)
    }

    fn is_name(s: &str) -> bool {
        parse_number(s.as_bytes()).unwrap().is_none()
    }

    #[test]
    fn decimal_integers() {
        assert_eq!(int("0"), Some(0));
        assert_eq!(int("+7"), Some(7));
        assert_eq!(int("-7"), Some(-7));
        assert_eq!(int("2147483647"), Some(i32::MAX));
        assert_eq!(int("-2147483648"), Some(i32::MIN));
        assert_eq!(int("007"), Some(7));
        assert_eq!(real("2147483648"), Some(2147483648.0));
        assert_eq!(real("-2147483649"), Some(-2147483649.0));
        assert_eq!(
            parse_number(&[b'9'; 50]).err(),
            Some(ScanErrorKind::LimitCheck)
        );
    }

    #[test]
    fn radix_integers() {
        assert_eq!(int("16#ff"), Some(255));
        assert_eq!(int("16#FF"), Some(255));
        assert_eq!(int("8#17"), Some(15));
        assert_eq!(int("2#101"), Some(5));
        assert_eq!(int("36#zz"), Some(35 * 36 + 35));
        assert_eq!(int("16#ffffffff"), Some(-1));
        assert_eq!(int("16#80000000"), Some(i32::MIN));
        assert_eq!(
            parse_number(b"16#100000000").err(),
            Some(ScanErrorKind::LimitCheck)
        );
        assert!(is_name("16#"));
        assert!(is_name("#1"));
        assert!(is_name("37#1"));
        assert!(is_name("1#0"));
        assert!(is_name("8#9"));
        assert!(is_name("-16#ff"));
        assert!(is_name("16#1.0"));
        assert!(is_name("1e1#1"));
    }

    #[test]
    fn reals() {
        assert_eq!(real("1.5"), Some(1.5));
        assert_eq!(real(".5"), Some(0.5));
        assert_eq!(real("1."), Some(1.0));
        assert_eq!(real("-1e3"), Some(-1000.0));
        assert_eq!(real("1E-3"), Some(0.001));
        assert_eq!(real("+.5e+1"), Some(5.0));
        assert_eq!(real("1e3"), Some(1000.0));
        assert_eq!(real("1.e3"), Some(1000.0));
        assert_eq!(real("0.1"), Some(0.1f32));
        assert_eq!(real("16777217.0"), Some(16777216.0));
        assert_eq!(int("16777217"), Some(16777217));
        assert_eq!(real("1e-50"), Some(0.0));
        assert_eq!(parse_number(b"1e39").err(), Some(ScanErrorKind::LimitCheck));
    }

    #[test]
    fn number_like_names() {
        for s in [
            "123abc", "-", ".", "+", "1e", "e1", "1e+", ".e1", "1.5.2", "1e1e1", "--1", "1-",
            "inf", "nan", "infinity", "1_0", "", "abc",
        ] {
            assert!(is_name(s), "{s:?} should be a name");
        }
    }

    #[test]
    fn character_classes() {
        for b in [b' ', b'\t', b'\r', b'\n', 0x0C, 0x00] {
            assert!(is_whitespace(b));
            assert!(!is_regular(b));
        }
        for b in b"()<>[]{}/%" {
            assert!(is_delimiter(*b));
            assert!(!is_regular(*b));
        }
        for b in [b'a', b'0', b'#', b'!', 0xFF, 0x80, 0x9F, 0xA0] {
            assert!(is_regular(b));
        }
    }

    #[test]
    fn error_names_and_display() {
        assert_eq!(ScanErrorKind::SyntaxError.name(), "syntaxerror");
        assert_eq!(ScanErrorKind::BinaryEncoding.name(), "syntaxerror");
        assert_eq!(ScanErrorKind::LimitCheck.name(), "limitcheck");
        assert_eq!(ScanErrorKind::Undefined.name(), "undefined");
        assert_eq!(
            ScanErrorKind::from(VmError::InvalidAccess).name(),
            "invalidaccess"
        );
        assert_eq!(
            ScanErrorKind::from(NameTooLong { len: 200 }),
            ScanErrorKind::LimitCheck
        );
        let e = ScanError {
            kind: ScanErrorKind::SyntaxError,
            span: Span::new(3, 4),
            byte: Some(b')'),
        };
        assert_eq!(e.to_string(), "syntaxerror at 3..4 (byte 0x29)");
        let e = ScanError {
            kind: ScanErrorKind::Undefined,
            span: Span::new(0, 5),
            byte: None,
        };
        assert_eq!(e.to_string(), "undefined at 0..5");
    }
}
