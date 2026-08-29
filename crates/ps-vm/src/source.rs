// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Byte sources the scanner reads from.
//!
//! A [`Source`] is a cursor over bytes with one byte of lookahead. Sources
//! that read VM-owned storage (a string object, a file-table entry) are given
//! the [`Memory`] on every call rather than holding a borrow of it, so the
//! scanner can allocate through the same `Memory` while it reads.

use std::fmt;

use crate::error::VmError;
use crate::memory::Memory;
use crate::object::{Handle, Object, Type};

/// A byte range in a source: `start..end`, in the source's own positions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// The 1-based line containing byte `offset` of `text`. CR, LF, and CR LF
/// each end one line.
pub fn line_of(text: &[u8], offset: usize) -> usize {
    let mut line = 1;
    let mut i = 0;
    let end = offset.min(text.len());
    while i < end {
        match text[i] {
            b'\r' => {
                line += 1;
                if text.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
            }
            b'\n' => line += 1,
            _ => {}
        }
        i += 1;
    }
    line
}

/// A cursor over bytes with one byte of lookahead.
pub trait Source {
    /// The next unread byte without consuming it; `None` at end of input.
    fn peek(&mut self, memory: &mut Memory) -> Result<Option<u8>, VmError>;

    /// Consumes the byte `peek` returned. Consuming at end of input does
    /// nothing.
    fn advance(&mut self, memory: &mut Memory);

    /// Position of the next unread byte.
    fn position(&self, memory: &Memory) -> usize;

    /// Whether end of input may be followed by more bytes later, in which
    /// case the scanner suspends mid-token instead of finishing.
    fn more_may_come(&self) -> bool;
}

/// A source over a byte slice.
#[derive(Debug)]
pub struct SliceSource<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> SliceSource<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        SliceSource { bytes, position: 0 }
    }

    pub fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.position..]
    }
}

impl Source for SliceSource<'_> {
    fn peek(&mut self, _: &mut Memory) -> Result<Option<u8>, VmError> {
        Ok(self.bytes.get(self.position).copied())
    }

    fn advance(&mut self, _: &mut Memory) {
        if self.position < self.bytes.len() {
            self.position += 1;
        }
    }

    fn position(&self, _: &Memory) -> usize {
        self.position
    }

    fn more_may_come(&self) -> bool {
        false
    }
}

/// A source over a string object's bytes, read through `Memory` on each
/// call so a `put` into the string during execution is seen. Positions are
/// relative to the object's own interval.
#[derive(Clone, Copy, Debug)]
pub struct StringSource {
    object: Object,
    position: usize,
}

impl StringSource {
    /// `None` if `object` is not a string.
    pub fn new(object: Object) -> Option<Self> {
        (object.ty() == Type::String).then_some(StringSource {
            object,
            position: 0,
        })
    }

    pub fn object(&self) -> Object {
        self.object
    }

    /// The unread tail of the string as an object, for `token` on a string.
    pub fn remainder(&self) -> Object {
        let length = self.object.length().expect("string is composite");
        let position = u32::try_from(self.position).unwrap_or(length).min(length);
        self.object
            .with_interval(position, length - position)
            .expect("position within the string")
    }
}

impl Source for StringSource {
    fn peek(&mut self, memory: &mut Memory) -> Result<Option<u8>, VmError> {
        let bytes = memory.string(self.object).ok_or(VmError::InvalidAccess)?;
        Ok(bytes.get(self.position).copied())
    }

    fn advance(&mut self, memory: &mut Memory) {
        let length = memory.string(self.object).map_or(0, <[u8]>::len);
        if self.position < length {
            self.position += 1;
        }
    }

    fn position(&self, _: &Memory) -> usize {
        self.position
    }

    fn more_may_come(&self) -> bool {
        false
    }
}

/// A source over a file-table entry. The entry keeps the read position and
/// the lookahead byte, so bytes the scanner leaves unread are exactly what
/// `read` on the file returns next.
#[derive(Clone, Copy, Debug)]
pub struct FileSource {
    handle: Handle,
}

impl FileSource {
    /// `None` if `object` is not a file.
    pub fn new(object: Object) -> Option<Self> {
        (object.ty() == Type::File).then(|| FileSource {
            handle: object.handle().expect("file is composite"),
        })
    }

    pub fn from_handle(handle: Handle) -> Self {
        FileSource { handle }
    }

    pub fn handle(&self) -> Handle {
        self.handle
    }
}

impl Source for FileSource {
    fn peek(&mut self, memory: &mut Memory) -> Result<Option<u8>, VmError> {
        memory.files_mut().peek(self.handle)
    }

    fn advance(&mut self, memory: &mut Memory) {
        let _ = memory.files_mut().consume(self.handle);
    }

    fn position(&self, memory: &Memory) -> usize {
        memory.files().position(self.handle).unwrap_or(0)
    }

    fn more_may_come(&self) -> bool {
        false
    }
}

/// An append-only buffer for input that arrives in pieces. Reports that more
/// may come until [`ChunkSource::finish`] is called; consumed bytes are
/// dropped on each append, so the buffer holds only what is unread.
/// Positions count from the first byte ever appended.
#[derive(Debug, Default)]
pub struct ChunkSource {
    buffer: Vec<u8>,
    position: usize,
    base: usize,
    finished: bool,
}

impl ChunkSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn append(&mut self, bytes: &[u8]) {
        self.compact();
        self.buffer.extend_from_slice(bytes);
    }

    /// Declares that no more bytes will be appended.
    pub fn finish(&mut self) {
        self.finished = true;
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Drops the consumed prefix of the buffer.
    pub fn compact(&mut self) {
        if self.position > 0 {
            self.buffer.drain(..self.position);
            self.base += self.position;
            self.position = 0;
        }
    }

    pub fn unread(&self) -> &[u8] {
        &self.buffer[self.position..]
    }

    /// Bytes currently held, consumed or not.
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }
}

impl Source for ChunkSource {
    fn peek(&mut self, _: &mut Memory) -> Result<Option<u8>, VmError> {
        Ok(self.buffer.get(self.position).copied())
    }

    fn advance(&mut self, _: &mut Memory) {
        if self.position < self.buffer.len() {
            self.position += 1;
        }
    }

    fn position(&self, _: &Memory) -> usize {
        self.base + self.position
    }

    fn more_may_come(&self) -> bool {
        !self.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::testing::Probe;

    fn drain(source: &mut dyn Source, memory: &mut Memory) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some(b) = source.peek(memory).unwrap() {
            out.push(b);
            source.advance(memory);
        }
        out
    }

    #[test]
    fn span_length() {
        assert_eq!(Span::new(2, 5).len(), 3);
        assert!(Span::new(2, 2).is_empty());
        assert_eq!(Span::new(5, 2).len(), 0);
        assert_eq!(Span::new(1, 4).to_string(), "1..4");
    }

    #[test]
    fn lines_count_cr_lf_and_crlf_once() {
        let text = b"a\nb\r\nc\rd";
        assert_eq!(line_of(text, 0), 1);
        assert_eq!(line_of(text, 2), 2);
        assert_eq!(line_of(text, 5), 3);
        assert_eq!(line_of(text, 7), 4);
        assert_eq!(line_of(text, 100), 4);
        assert_eq!(line_of(b"", 0), 1);
    }

    #[test]
    fn slice_source_walks_and_stops() {
        let mut m = Memory::new();
        let mut s = SliceSource::new(b"ab");
        assert_eq!(s.position(&m), 0);
        assert_eq!(s.peek(&mut m), Ok(Some(b'a')));
        s.advance(&mut m);
        assert_eq!(s.remaining(), b"b");
        assert_eq!(drain(&mut s, &mut m), b"b");
        assert_eq!(s.peek(&mut m), Ok(None));
        s.advance(&mut m);
        assert_eq!(s.position(&m), 2);
        assert!(!s.more_may_come());
    }

    #[test]
    fn string_source_reads_the_interval_and_sees_writes() {
        let mut m = Memory::new();
        let whole = m.alloc_string(b"xxabcxx".to_vec());
        let part = whole.with_interval(2, 3).unwrap();
        let mut s = StringSource::new(part).unwrap();
        assert!(s.object().eq(part));
        assert_eq!(s.peek(&mut m), Ok(Some(b'a')));
        s.advance(&mut m);
        m.string_put(part, 1, b'B').unwrap();
        assert_eq!(drain(&mut s, &mut m), b"Bc");
        assert_eq!(s.position(&m), 3);
        assert_eq!(s.remainder().length(), Some(0));
        s.advance(&mut m);
        assert_eq!(s.position(&m), 3);
        let mut fresh = StringSource::new(part).unwrap();
        fresh.advance(&mut m);
        let rest = fresh.remainder();
        assert_eq!(m.string(rest), Some(&b"Bc"[..]));
        assert!(StringSource::new(Object::integer(1)).is_none());
        let stale = Object::string(crate::Space::Local, Handle(50), 1);
        let mut s = StringSource::new(stale).unwrap();
        assert_eq!(s.peek(&mut m), Err(VmError::InvalidAccess));
    }

    #[test]
    fn file_source_shares_the_entry_position() {
        let mut m = Memory::new();
        let f = m.open_stream(Box::new(Probe::with_input(b"abc\ndef")));
        let mut s = FileSource::new(f).unwrap();
        assert_eq!(s.handle(), f.handle().unwrap());
        assert_eq!(s.peek(&mut m), Ok(Some(b'a')));
        assert_eq!(s.position(&m), 0);
        s.advance(&mut m);
        s.advance(&mut m);
        assert_eq!(s.peek(&mut m), Ok(Some(b'c')));
        assert_eq!(s.position(&m), 2);
        let mut buf = [0u8; 5];
        assert_eq!(m.file_read(f, &mut buf), Ok(5));
        assert_eq!(&buf, b"c\ndef");
        assert_eq!(s.peek(&mut m), Ok(None));
        s.advance(&mut m);
        assert_eq!(s.position(&m), 7);
        assert!(!s.more_may_come());
        assert!(FileSource::new(Object::null()).is_none());
        m.close_file(f).unwrap();
        assert_eq!(s.peek(&mut m), Err(VmError::IoError));
        assert_eq!(s.position(&m), 0);
        assert_eq!(FileSource::from_handle(Handle(3)).handle(), Handle(3));
    }

    #[test]
    fn chunk_source_appends_compacts_and_keeps_absolute_positions() {
        let mut m = Memory::new();
        let mut c = ChunkSource::new();
        assert!(c.more_may_come());
        assert_eq!(c.peek(&mut m), Ok(None));
        c.append(b"abc");
        assert_eq!(c.peek(&mut m), Ok(Some(b'a')));
        c.advance(&mut m);
        c.advance(&mut m);
        assert_eq!(c.position(&m), 2);
        assert_eq!(c.unread(), b"c");
        assert_eq!(c.buffered(), 3);
        c.append(b"de");
        assert_eq!(c.buffered(), 3);
        assert_eq!(c.unread(), b"cde");
        assert_eq!(c.position(&m), 2);
        assert_eq!(drain(&mut c, &mut m), b"cde");
        assert_eq!(c.position(&m), 5);
        c.advance(&mut m);
        assert_eq!(c.position(&m), 5);
        c.compact();
        assert_eq!(c.buffered(), 0);
        assert_eq!(c.position(&m), 5);
        assert!(!c.is_finished());
        c.finish();
        assert!(c.is_finished());
        assert!(!c.more_may_come());
    }
}
