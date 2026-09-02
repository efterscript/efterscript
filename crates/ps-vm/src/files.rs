// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! File objects and the streams behind them.
//!
//! The VM never opens anything itself: every stream is handed in by the
//! embedder, either directly or through a [`FileCapability`] that answers
//! `file` requests. This module must stay free of `std::fs` and every other
//! host resource.
//!
//! An `eexec` layer is an entry that reads another entry — its base —
//! decrypting as it goes, so the scanner and the read operators use it
//! like any file while the base's position stays exact: closing the layer
//! hands any byte it read ahead back to the base.

use std::fmt;

use ps_fonts::type1::{Decryptor, EEXEC_KEY, hex_value};

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

/// The form of an `eexec` section, decided from its first four bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    Hex,
    Binary,
}

/// The decrypting state of a layer: what it has taken from the base to
/// produce the plain byte it holds in `pushback`, so a close can hand it
/// back.
struct Layer {
    base: Handle,
    cipher: Decryptor,
    /// Decided when the first byte is needed; `None` until then.
    form: Option<Form>,
    /// Leading plain bytes still to discard.
    skip: u8,
    /// The base bytes consumed for the byte in `pushback`, and the
    /// cipher state before them.
    lookahead: Vec<u8>,
    before: Decryptor,
}

enum Kind {
    Stream(Box<dyn Stream>),
    Layer(Layer),
}

// The entry owns the lookahead the scanner may need, so a `readstring` or
// `readline` after a token sees exactly the bytes the scanner left unread.
// `pushback` is a stack: the last byte is the next to read.
struct Entry {
    kind: Kind,
    pushback: Vec<u8>,
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

    fn push(&mut self, kind: Kind) -> Handle {
        let handle = Handle(self.watermark());
        self.entries.push(Some(Entry {
            kind,
            pushback: Vec::new(),
            position: 0,
        }));
        handle
    }

    pub fn open(&mut self, stream: Box<dyn Stream>) -> Handle {
        self.push(Kind::Stream(stream))
    }

    /// Opens an `eexec` layer reading `base` from its current position;
    /// `ioerror` if `base` is not open.
    pub fn open_layer(&mut self, base: Handle) -> Result<Handle, VmError> {
        self.entry(base)?;
        let cipher = Decryptor::new(EEXEC_KEY);
        Ok(self.push(Kind::Layer(Layer {
            base,
            cipher,
            form: None,
            skip: 4,
            lookahead: Vec::new(),
            before: cipher,
        })))
    }

    /// The base of a layer; `None` for a plain entry or a closed one.
    pub fn layer_base(&self, handle: Handle) -> Option<Handle> {
        match self.entries.get(handle.0 as usize) {
            Some(Some(Entry {
                kind: Kind::Layer(layer),
                ..
            })) => Some(layer.base),
            _ => None,
        }
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

    /// One byte from the base of a layer, for the layer's own use.
    fn base_byte(&mut self, base: Handle) -> Result<Option<u8>, VmError> {
        let mut byte = [0u8; 1];
        Ok((self.read(base, &mut byte)? == 1).then_some(byte[0]))
    }

    /// The next plain byte of the layer at `handle`, decrypted from the
    /// base; `None` at the end of the section.
    fn layer_byte(&mut self, handle: Handle) -> Result<Option<u8>, VmError> {
        let (base, form) = match &self.entry(handle)?.kind {
            Kind::Layer(layer) => (layer.base, layer.form),
            Kind::Stream(_) => unreachable!("layer entries only"),
        };
        let form = match form {
            Some(form) => form,
            None => {
                let mut first = Vec::with_capacity(4);
                while first.len() < 4 {
                    match self.base_byte(base)? {
                        Some(b) => first.push(b),
                        None => break,
                    }
                }
                let form = if first.len() == 4 && first.iter().all(|&b| hex_value(b).is_some()) {
                    Form::Hex
                } else {
                    Form::Binary
                };
                let Kind::Layer(layer) = &mut self.entry(handle)?.kind else {
                    unreachable!("layer entries only");
                };
                layer.form = Some(form);
                // The four bytes read for the decision are the start of
                // the section: hand them back so decryption sees them.
                let base_entry = self.entry(base)?;
                base_entry.position -= first.len();
                base_entry.pushback.extend(first.iter().rev());
                form
            }
        };
        loop {
            let mut raw = Vec::new();
            let cipher_byte = match form {
                Form::Binary => {
                    let Some(b) = self.base_byte(base)? else {
                        return Ok(None);
                    };
                    raw.push(b);
                    b
                }
                Form::Hex => {
                    let mut high = None;
                    loop {
                        let Some(b) = self.base_byte(base)? else {
                            return Ok(None);
                        };
                        match hex_value(b) {
                            Some(v) => {
                                raw.push(b);
                                match high.take() {
                                    None => high = Some(v),
                                    Some(h) => break h << 4 | v,
                                }
                            }
                            None if b.is_ascii_whitespace() => raw.push(b),
                            None => {
                                // The section ends at the first byte that
                                // is neither; it and the whitespace before
                                // it belong to the base.
                                raw.push(b);
                                let base_entry = self.entry(base)?;
                                base_entry.position -= raw.len();
                                base_entry.pushback.extend(raw.iter().rev());
                                return Ok(None);
                            }
                        }
                    }
                }
            };
            let Kind::Layer(layer) = &mut self.entry(handle)?.kind else {
                unreachable!("layer entries only");
            };
            layer.before = layer.cipher;
            let plain = layer.cipher.byte(cipher_byte);
            if layer.skip > 0 {
                layer.skip -= 1;
                continue;
            }
            layer.lookahead = raw;
            return Ok(Some(plain));
        }
    }

    /// Reads into `buf`, starting with any byte the scanner peeked but did
    /// not consume.
    pub fn read(&mut self, handle: Handle, buf: &mut [u8]) -> Result<usize, VmError> {
        if buf.is_empty() {
            return Ok(0);
        }
        let entry = self.entry(handle)?;
        let mut n = 0;
        while n < buf.len() {
            let Some(byte) = entry.pushback.pop() else {
                break;
            };
            buf[n] = byte;
            n += 1;
        }
        entry.position += n;
        if let Kind::Stream(stream) = &mut entry.kind {
            let got = stream.read(&mut buf[n..])?;
            entry.position += got;
            return Ok(n + got);
        }
        while n < buf.len() {
            let Some(byte) = self.layer_byte(handle)? else {
                break;
            };
            buf[n] = byte;
            n += 1;
            let entry = self.entry(handle)?;
            entry.position += 1;
            if let Kind::Layer(layer) = &mut entry.kind {
                layer.lookahead.clear();
            }
        }
        Ok(n)
    }

    /// The next unread byte without consuming it; `None` at end of data.
    pub fn peek(&mut self, handle: Handle) -> Result<Option<u8>, VmError> {
        let entry = self.entry(handle)?;
        if let Some(&byte) = entry.pushback.last() {
            return Ok(Some(byte));
        }
        let byte = match &mut entry.kind {
            Kind::Stream(stream) => {
                let mut byte = [0u8; 1];
                (stream.read(&mut byte)? == 1).then_some(byte[0])
            }
            Kind::Layer(_) => self.layer_byte(handle)?,
        };
        if let Some(byte) = byte {
            self.entry(handle)?.pushback.push(byte);
        }
        Ok(byte)
    }

    /// Consumes the next unread byte, if any.
    pub fn consume(&mut self, handle: Handle) -> Result<Option<u8>, VmError> {
        let byte = self.peek(handle)?;
        if byte.is_some() {
            let entry = self.entry(handle)?;
            entry.pushback.pop();
            entry.position += 1;
            if let Kind::Layer(layer) = &mut entry.kind {
                layer.lookahead.clear();
            }
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
        match &mut self.entry(handle)?.kind {
            Kind::Stream(stream) => stream.write(buf),
            Kind::Layer(_) => Err(VmError::IoError),
        }
    }

    pub fn flush(&mut self, handle: Handle) -> Result<(), VmError> {
        match &mut self.entry(handle)?.kind {
            Kind::Stream(stream) => stream.flush(),
            Kind::Layer(_) => Ok(()),
        }
    }

    /// Closes the entry; closing an already closed or unknown file is not
    /// an error. Closing a layer returns the byte it read ahead, if any,
    /// to its base, so the base continues exactly after the last byte the
    /// layer consumed.
    pub fn close(&mut self, handle: Handle) -> Result<(), VmError> {
        let Some(entry) = self
            .entries
            .get_mut(handle.0 as usize)
            .and_then(Option::take)
        else {
            return Ok(());
        };
        match entry.kind {
            Kind::Stream(mut stream) => stream.close(),
            Kind::Layer(layer) => {
                if !entry.pushback.is_empty()
                    && let Ok(base) = self.entry(layer.base)
                {
                    base.position -= layer.lookahead.len();
                    base.pushback.extend(layer.lookahead.iter().rev());
                }
                Ok(())
            }
        }
    }

    /// Closes every entry opened at or after `watermark`. The first close
    /// error is reported after all entries have been closed.
    pub fn close_from(&mut self, watermark: u32) -> Result<(), VmError> {
        let mut result = Ok(());
        for handle in (watermark..self.watermark()).rev() {
            if let Err(e) = self.close(Handle(handle))
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
    use ps_fonts::type1::encrypt;

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

    // --- eexec layers -------------------------------------------------------------

    fn hex(bytes: &[u8]) -> Vec<u8> {
        bytes
            .iter()
            .enumerate()
            .flat_map(|(k, b)| {
                let mut s = format!("{b:02x}").into_bytes();
                if k % 5 == 4 {
                    s.push(b'\n');
                }
                s
            })
            .collect()
    }

    fn layered(section: &[u8], tail: &[u8]) -> (FileTable, Handle, Handle) {
        let mut input = b"lead ".to_vec();
        input.extend_from_slice(section);
        input.extend_from_slice(tail);
        let mut t = FileTable::new();
        let base = t.open(Box::new(Probe::with_input(&input)));
        let mut buf = [0u8; 5];
        assert_eq!(t.read(base, &mut buf), Ok(5));
        let layer = t.open_layer(base).unwrap();
        (t, base, layer)
    }

    #[test]
    fn a_hex_layer_decrypts_and_leaves_the_base_after_its_last_byte() {
        let cipher = encrypt(EEXEC_KEY, b"/x 42 def", 4);
        let (mut t, base, layer) = layered(&hex(&cipher), b" tail");
        assert_eq!(t.layer_base(layer), Some(base));
        assert_eq!(t.layer_base(base), None);
        assert_eq!(t.peek(layer), Ok(Some(b'/')));
        assert_eq!(t.position(layer), Some(0));
        let mut buf = [0u8; 3];
        assert_eq!(t.read(layer, &mut buf), Ok(3));
        assert_eq!(&buf, b"/x ");
        assert_eq!(t.position(layer), Some(3));
        assert_eq!(t.consume(layer), Ok(Some(b'4')));
        let mut rest = [0u8; 10];
        assert_eq!(t.read(layer, &mut rest), Ok(5));
        assert_eq!(&rest[..5], b"2 def");
        assert_eq!(t.peek(layer), Ok(None));
        assert_eq!(t.position(layer), Some(9));
        assert_eq!(t.write(layer, b"x"), Err(VmError::IoError));
        assert_eq!(t.flush(layer), Ok(()));
        t.close(layer).unwrap();
        let mut tail = [0u8; 8];
        assert_eq!(t.read(base, &mut tail), Ok(5));
        assert_eq!(&tail[..5], b" tail");
    }

    #[test]
    fn a_binary_layer_returns_its_lookahead_on_close() {
        let mut lead = 0u8;
        let cipher = loop {
            let mut plain = vec![lead, 0, 0, 0];
            plain.extend_from_slice(b"ab)cd");
            let cipher = encrypt(EEXEC_KEY, &plain, 0);
            if hex_value(cipher[0]).is_none() {
                break cipher;
            }
            lead += 1;
        };
        let (mut t, base, layer) = layered(&cipher, b"tail");
        let mut buf = [0u8; 2];
        assert_eq!(t.read(layer, &mut buf), Ok(2));
        assert_eq!(&buf, b"ab");
        assert_eq!(t.peek(layer), Ok(Some(b')')));
        let base_position = t.position(base).unwrap();
        t.close(layer).unwrap();
        // The peeked `)` came from one base byte, now back in the base.
        assert_eq!(t.position(base), Some(base_position - 1));
        let mut rest = [0u8; 16];
        let n = t.read(base, &mut rest).unwrap();
        assert_eq!(n, 3 + 4);
        assert_eq!(&rest[3..7], b"tail");
        assert_eq!(t.read(layer, &mut rest), Err(VmError::IoError));
    }

    #[test]
    fn a_hex_layer_returns_digits_and_whitespace_it_read_ahead() {
        let cipher = encrypt(EEXEC_KEY, b"xyz", 4);
        let mut text = hex(&cipher[..6]);
        text.extend_from_slice(b"\n \n");
        text.extend_from_slice(&hex(&cipher[6..]));
        let (mut t, base, layer) = layered(&text, b"!");
        let mut buf = [0u8; 2];
        assert_eq!(t.read(layer, &mut buf), Ok(2));
        assert_eq!(t.peek(layer), Ok(Some(b'z')));
        t.close(layer).unwrap();
        let mut rest = [0u8; 16];
        let n = t.read(base, &mut rest).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&rest[..n]),
            format!("\n \n{:02x}!", cipher[6])
        );
    }

    #[test]
    fn a_hex_layer_ends_at_a_non_hex_byte_which_stays_in_the_base() {
        let cipher = encrypt(EEXEC_KEY, b"ab", 4);
        let (mut t, base, layer) = layered(&hex(&cipher), b"zz");
        let mut buf = [0u8; 8];
        assert_eq!(t.read(layer, &mut buf), Ok(2));
        assert_eq!(&buf[..2], b"ab");
        assert_eq!(t.peek(layer), Ok(None));
        t.close(layer).unwrap();
        let n = t.read(base, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"zz");
    }

    #[test]
    fn a_short_section_is_binary_and_ends_with_the_base() {
        let (mut t, _, layer) = layered(b"ab", b"");
        let mut buf = [0u8; 4];
        assert_eq!(t.read(layer, &mut buf), Ok(0));
        assert_eq!(t.peek(layer), Ok(None));
        let mut t = FileTable::new();
        assert_eq!(t.open_layer(Handle(3)), Err(VmError::IoError));
        let base = t.open(Box::new(Probe::with_input(b"")));
        let layer = t.open_layer(base).unwrap();
        t.close(base).unwrap();
        assert_eq!(t.peek(layer), Err(VmError::IoError));
        assert_eq!(t.close(layer), Ok(()));
    }

    #[test]
    fn close_from_closes_layers_before_their_bases() {
        let mut t = FileTable::new();
        let probe = Probe::with_input(&encrypt(EEXEC_KEY, b"q", 4));
        let base = t.open(Box::new(probe.clone()));
        let layer = t.open_layer(base).unwrap();
        assert_eq!(t.peek(layer), Ok(Some(b'q')));
        t.close_from(0).unwrap();
        assert!(probe.closed());
        assert!(!t.is_open(layer));
    }
}
