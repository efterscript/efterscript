// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! File objects and the streams behind them.
//!
//! The VM never opens anything itself: every stream is handed in by the
//! embedder, either directly or through a [`FileCapability`] that answers
//! `file` requests. This module must stay free of `std::fs` and every other
//! host resource.

use std::fmt;

use crate::error::VmError;
use crate::object::Handle;

/// An embedder-provided byte stream.
pub trait Stream {
    /// Reads into `buf`, returning the count; `Ok(0)` is end of data.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VmError>;

    /// Writes from `buf`, returning the count accepted.
    fn write(&mut self, buf: &[u8]) -> Result<usize, VmError>;

    /// Pushes buffered output to its destination.
    fn flush(&mut self) -> Result<(), VmError> {
        Ok(())
    }

    /// Called once when the file is closed.
    fn close(&mut self) -> Result<(), VmError> {
        Ok(())
    }
}

/// Resolves a `file` request into a stream. Absent a capability, every
/// request fails with `undefinedfilename`.
pub trait FileCapability {
    fn open(&mut self, name: &[u8], mode: &[u8]) -> Result<Box<dyn Stream>, VmError>;
}

// The entry owns the one byte of lookahead the scanner may need, so a
// `readstring` or `readline` after a token sees exactly the bytes the scanner
// left unread.
struct Entry {
    stream: Box<dyn Stream>,
    pushback: Option<u8>,
    position: usize,
}

/// Open and closed file entries. Handles are never reused, so a file object
/// that outlives its entry resolves to a closed file.
#[derive(Default)]
pub struct FileTable {
    entries: Vec<Option<Entry>>,
}

impl fmt::Debug for FileTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FileTable")
            .field("entries", &self.entries.len())
            .field("open", &self.open_count())
            .finish()
    }
}

impl FileTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// The handle the next `open` will receive; also the count of handles
    /// ever issued.
    pub fn watermark(&self) -> u32 {
        u32::try_from(self.entries.len()).expect("file table exhausted")
    }

    pub fn open_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_some()).count()
    }

    pub fn open(&mut self, stream: Box<dyn Stream>) -> Handle {
        let handle = Handle(self.watermark());
        self.entries.push(Some(Entry {
            stream,
            pushback: None,
            position: 0,
        }));
        handle
    }

    pub fn is_open(&self, handle: Handle) -> bool {
        matches!(self.entries.get(handle.0 as usize), Some(Some(_)))
    }

    fn entry(&mut self, handle: Handle) -> Result<&mut Entry, VmError> {
        match self.entries.get_mut(handle.0 as usize) {
            Some(Some(entry)) => Ok(entry),
            _ => Err(VmError::IoError),
        }
    }

    /// Reads into `buf`, starting with any byte the scanner peeked but did
    /// not consume.
    pub fn read(&mut self, handle: Handle, buf: &mut [u8]) -> Result<usize, VmError> {
        let entry = self.entry(handle)?;
        if buf.is_empty() {
            return Ok(0);
        }
        let mut n = 0;
        if let Some(byte) = entry.pushback.take() {
            buf[0] = byte;
            n = 1;
        }
        n += entry.stream.read(&mut buf[n..])?;
        entry.position += n;
        Ok(n)
    }

    /// The next unread byte without consuming it; `None` at end of data.
    pub fn peek(&mut self, handle: Handle) -> Result<Option<u8>, VmError> {
        let entry = self.entry(handle)?;
        if entry.pushback.is_none() {
            let mut byte = [0u8; 1];
            if entry.stream.read(&mut byte)? == 1 {
                entry.pushback = Some(byte[0]);
            }
        }
        Ok(entry.pushback)
    }

    /// Consumes the next unread byte, if any.
    pub fn consume(&mut self, handle: Handle) -> Result<Option<u8>, VmError> {
        let byte = self.peek(handle)?;
        if byte.is_some() {
            let entry = self.entry(handle)?;
            entry.pushback = None;
            entry.position += 1;
        }
        Ok(byte)
    }

    /// Bytes consumed from the stream so far, peeked bytes excluded.
    pub fn position(&self, handle: Handle) -> Option<usize> {
        match self.entries.get(handle.0 as usize) {
            Some(Some(entry)) => Some(entry.position),
            _ => None,
        }
    }

    pub fn write(&mut self, handle: Handle, buf: &[u8]) -> Result<usize, VmError> {
        self.entry(handle)?.stream.write(buf)
    }

    pub fn flush(&mut self, handle: Handle) -> Result<(), VmError> {
        self.entry(handle)?.stream.flush()
    }

    /// Closes the entry; closing an already closed or unknown file is not
    /// an error.
    pub fn close(&mut self, handle: Handle) -> Result<(), VmError> {
        match self
            .entries
            .get_mut(handle.0 as usize)
            .and_then(Option::take)
        {
            Some(mut entry) => entry.stream.close(),
            None => Ok(()),
        }
    }

    /// Closes every entry opened at or after `watermark`. The first close
    /// error is reported after all entries have been closed.
    pub fn close_from(&mut self, watermark: u32) -> Result<(), VmError> {
        let mut result = Ok(());
        for entry in self.entries.iter_mut().skip(watermark as usize) {
            if let Some(mut entry) = entry.take()
                && let Err(e) = entry.stream.close()
                && result.is_ok()
            {
                result = Err(e);
            }
        }
        result
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    /// A stream over an in-memory buffer that records whether it was closed.
    #[derive(Default)]
    pub struct MemoryStream {
        pub input: Vec<u8>,
        pub position: usize,
        pub output: Vec<u8>,
        pub closed: bool,
    }

    /// Shared handle to a [`MemoryStream`] so a test can inspect it after
    /// handing it to the table.
    #[derive(Clone, Default)]
    pub struct Probe(pub Rc<RefCell<MemoryStream>>);

    impl Probe {
        pub fn with_input(input: &[u8]) -> Self {
            let probe = Probe::default();
            probe.0.borrow_mut().input = input.to_vec();
            probe
        }

        pub fn closed(&self) -> bool {
            self.0.borrow().closed
        }

        pub fn output(&self) -> Vec<u8> {
            self.0.borrow().output.clone()
        }
    }

    impl Stream for Probe {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, VmError> {
            let mut s = self.0.borrow_mut();
            let n = buf.len().min(s.input.len() - s.position);
            buf[..n].copy_from_slice(&s.input[s.position..s.position + n]);
            s.position += n;
            Ok(n)
        }

        fn write(&mut self, buf: &[u8]) -> Result<usize, VmError> {
            self.0.borrow_mut().output.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn close(&mut self) -> Result<(), VmError> {
            self.0.borrow_mut().closed = true;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::Probe;
    use super::*;

    #[test]
    fn handles_are_dense_and_never_reused() {
        let mut t = FileTable::new();
        assert_eq!(t.watermark(), 0);
        let a = t.open(Box::new(Probe::default()));
        let b = t.open(Box::new(Probe::default()));
        assert_eq!((a, b), (Handle(0), Handle(1)));
        t.close(a).unwrap();
        let c = t.open(Box::new(Probe::default()));
        assert_eq!(c, Handle(2));
        assert_eq!(t.watermark(), 3);
        assert_eq!(t.open_count(), 2);
        assert!(!t.is_open(a));
        assert!(t.is_open(b));
        assert!(!t.is_open(Handle(9)));
        assert!(format!("{t:?}").contains("open: 2"));
    }

    #[test]
    fn read_write_close() {
        let mut t = FileTable::new();
        let probe = Probe::with_input(b"hello");
        let h = t.open(Box::new(probe.clone()));
        let mut buf = [0u8; 3];
        assert_eq!(t.read(h, &mut buf), Ok(3));
        assert_eq!(&buf, b"hel");
        assert_eq!(t.read(h, &mut buf), Ok(2));
        assert_eq!(t.read(h, &mut buf), Ok(0));
        assert_eq!(t.write(h, b"out"), Ok(3));
        assert_eq!(probe.output(), b"out");
        assert!(!probe.closed());
        t.close(h).unwrap();
        assert!(probe.closed());
        assert_eq!(t.read(h, &mut buf), Err(VmError::IoError));
        assert_eq!(t.write(h, b"x"), Err(VmError::IoError));
        assert_eq!(t.close(h), Ok(()));
        assert_eq!(t.close(Handle(42)), Ok(()));
    }

    #[test]
    fn peek_and_consume_share_the_read_position() {
        let mut t = FileTable::new();
        let h = t.open(Box::new(Probe::with_input(b"ab\ncd")));
        assert_eq!(t.position(h), Some(0));
        assert_eq!(t.peek(h), Ok(Some(b'a')));
        assert_eq!(t.peek(h), Ok(Some(b'a')));
        assert_eq!(t.position(h), Some(0));
        assert_eq!(t.consume(h), Ok(Some(b'a')));
        assert_eq!(t.position(h), Some(1));
        assert_eq!(t.peek(h), Ok(Some(b'b')));
        let mut buf = [0u8; 4];
        assert_eq!(t.read(h, &mut buf), Ok(4));
        assert_eq!(&buf, b"b\ncd");
        assert_eq!(t.position(h), Some(5));
        assert_eq!(t.peek(h), Ok(None));
        assert_eq!(t.consume(h), Ok(None));
        assert_eq!(t.read(h, &mut []), Ok(0));
        assert_eq!(t.position(Handle(9)), None);
        assert_eq!(t.peek(Handle(9)), Err(VmError::IoError));
    }

    #[test]
    fn close_from_watermark_closes_only_newer_entries() {
        let mut t = FileTable::new();
        let old = Probe::default();
        t.open(Box::new(old.clone()));
        let mark = t.watermark();
        let newer: Vec<Probe> = (0..3).map(|_| Probe::default()).collect();
        for p in &newer {
            t.open(Box::new(p.clone()));
        }
        t.close(Handle(2)).unwrap();
        t.close_from(mark).unwrap();
        assert!(!old.closed());
        assert!(newer.iter().all(Probe::closed));
        assert_eq!(t.open_count(), 1);
        assert_eq!(t.watermark(), 4);
        t.close_from(99).unwrap();
        assert_eq!(t.open_count(), 1);
    }

    #[test]
    fn close_errors_are_reported_after_closing_everything() {
        struct Failing;
        impl Stream for Failing {
            fn read(&mut self, _: &mut [u8]) -> Result<usize, VmError> {
                Ok(0)
            }
            fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
                Ok(0)
            }
            fn close(&mut self) -> Result<(), VmError> {
                Err(VmError::IoError)
            }
        }
        let mut t = FileTable::new();
        t.open(Box::new(Failing));
        let p = Probe::default();
        t.open(Box::new(p.clone()));
        assert_eq!(t.close_from(0), Err(VmError::IoError));
        assert!(p.closed());
        assert_eq!(t.open_count(), 0);
    }

    #[test]
    fn default_close_is_a_no_op() {
        struct Plain;
        impl Stream for Plain {
            fn read(&mut self, _: &mut [u8]) -> Result<usize, VmError> {
                Ok(0)
            }
            fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
                Ok(0)
            }
        }
        let mut t = FileTable::new();
        let h = t.open(Box::new(Plain));
        assert_eq!(t.close(h), Ok(()));
    }
}
