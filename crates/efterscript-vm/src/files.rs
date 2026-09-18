// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! File objects and the streams behind them.
//!
//! The VM never opens anything itself: every stream is handed in by the
//! embedder, either directly or through a [`FileCapability`] that answers
//! `file` requests. This module must stay free of `std::fs` and every other
//! host resource.
//!
//! A decode layer is an entry that reads another entry — its base —
//! through a `Decoder`: the `eexec` cipher or one of the standard
//! decode filters. The scanner and the read operators use it like any
//! file while the base's position stays exact: the decoder takes base
//! bytes one at a time and holds partial state rather than reading
//! ahead, and closing the cipher layer hands the bytes behind a peeked
//! but untaken byte back to the base.
//!
//! Two further entry kinds serve the `filter` operator's sources: an
//! in-memory stream over a string's bytes, and a procedure source whose
//! bytes come from running a program procedure. The table cannot run the
//! procedure; a read that finds the procedure's buffer empty fails as a
//! read from a growing source does, and records which procedure is
//! wanted so the interpreter runs it and feeds the result back.
//!
//! An in-memory entry may be *positionable* — a reusable stream (PLRM3
//! §3.13.3), whose whole content is at hand — in which case the
//! positioning operators move its read point anywhere from the start to
//! the end, and reaching the end leaves it open. Nothing else in the
//! table is positionable: the entry kinds that read from the embedder or
//! through a decoder have no length to position within.
//!
//! An encode entry is the write-side counterpart of a decode layer: the
//! bytes written to it go through an `Encoder` to another entry, its
//! target, and closing it writes the encoding's final bytes and marker
//! first.

use std::collections::VecDeque;
use std::fmt;

use crate::decoders::{Decoder, Fed};
use crate::encoders::Encoder;
use crate::error::VmError;
use crate::object::{Handle, Object};

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

    /// Whether end of data may be followed by more bytes later. A stream
    /// answering `true` fails a read that finds nothing with
    /// [`VmError::NeedMore`] instead of returning zero, and must
    /// implement [`Stream::unread`].
    fn more_may_come(&self) -> bool {
        false
    }

    /// Hands bytes back so the next read returns them first: the
    /// interpreter undoes an operator's reads when it suspends for more
    /// data. Only streams that may grow are ever asked.
    fn unread(&mut self, _bytes: &[u8]) {}
}

/// Resolves a `file` request into a stream. Absent a capability, every
/// request fails with `undefinedfilename`.
pub trait FileCapability {
    fn open(&mut self, name: &[u8], mode: &[u8]) -> Result<Box<dyn Stream>, VmError>;
}

/// A read-only stream over bytes held in memory: a string source, or a
/// reusable stream, which is positionable.
struct Bytes {
    data: Vec<u8>,
    at: usize,
    positionable: bool,
}

impl Stream for Bytes {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VmError> {
        let n = buf.len().min(self.data.len() - self.at);
        buf[..n].copy_from_slice(&self.data[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }

    fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
        Err(VmError::IoError)
    }
}

/// A source whose bytes a program procedure delivers a string at a time,
/// an empty string ending it. The buffer holds what was delivered and
/// not yet read.
struct Procedure {
    body: Object,
    buffer: VecDeque<u8>,
    exhausted: bool,
}

impl Stream for Procedure {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VmError> {
        if self.buffer.is_empty() && !self.exhausted && !buf.is_empty() {
            return Err(VmError::NeedMore);
        }
        let mut n = 0;
        while n < buf.len() {
            let Some(byte) = self.buffer.pop_front() else {
                break;
            };
            buf[n] = byte;
            n += 1;
        }
        Ok(n)
    }

    fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
        Err(VmError::IoError)
    }

    fn more_may_come(&self) -> bool {
        !self.exhausted
    }

    fn unread(&mut self, bytes: &[u8]) {
        for &byte in bytes.iter().rev() {
            self.buffer.push_front(byte);
        }
    }
}

/// A decoding entry over a base entry. Everything but `base` and
/// `close_source` changes as bytes are read, and all of it is what a
/// snapshot keeps.
#[derive(Clone)]
struct Layer {
    base: Handle,
    decoder: Decoder,
    /// Decoded bytes not yet handed out; the front is next.
    pending: VecDeque<u8>,
    /// The decoder reached its end (or the base ended): nothing more is
    /// read from the base.
    ended: bool,
    /// Base bytes fed since the last decoded byte came out.
    raw: Vec<u8>,
    /// The base bytes behind the byte most recently handed out, kept
    /// while that byte sits unconsumed in `pushback`.
    lookahead: Vec<u8>,
    /// Close the base when this entry closes.
    close_source: bool,
}

/// An encoding entry writing through to a target entry.
struct Writer {
    target: Handle,
    encoder: Encoder,
    /// Close the target when this entry closes.
    close_target: bool,
}

enum Kind {
    Stream(Box<dyn Stream>),
    Bytes(Bytes),
    Procedure(Procedure),
    Decode(Layer),
    Encode(Writer),
}

impl Kind {
    /// The stream of a plain, in-memory, or procedure entry.
    fn stream(&mut self) -> Option<&mut dyn Stream> {
        match self {
            Kind::Stream(stream) => Some(stream.as_mut()),
            Kind::Bytes(bytes) => Some(bytes),
            Kind::Procedure(procedure) => Some(procedure),
            Kind::Decode(_) | Kind::Encode(_) => None,
        }
    }
}

// The entry owns the lookahead the scanner may need, so a `readstring` or
// `readline` after a token sees exactly the bytes the scanner left unread.
// `pushback` is a stack: the last byte is the next to read.
struct Entry {
    kind: Kind,
    pushback: Vec<u8>,
    position: usize,
}

/// The state of an entry when the current operator first read from it,
/// kept only for entries whose bytes may still be growing, so the reads
/// can be undone if the operator has to wait for more.
struct Saved {
    handle: Handle,
    pushback: Vec<u8>,
    position: usize,
    layer: Option<Layer>,
    /// Bytes taken from the stream since the snapshot.
    taken: Vec<u8>,
}

/// Open and closed file entries. Handles are never reused, so a file object
/// that outlives its entry resolves to a closed file.
#[derive(Default)]
pub struct FileTable {
    entries: Vec<Option<Entry>>,
    saved: Vec<Saved>,
    /// The procedure entry whose empty buffer failed the latest read.
    wanted: Option<Handle>,
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

    /// Opens a read-only entry over `data`, read once from start to end.
    pub fn open_bytes(&mut self, data: Vec<u8>) -> Handle {
        self.push(Kind::Bytes(Bytes {
            data,
            at: 0,
            positionable: false,
        }))
    }

    /// Opens a positionable read-only entry over `data`: a reusable
    /// stream, which stays open at its end and answers the positioning
    /// methods below.
    pub fn open_reusable(&mut self, data: Vec<u8>) -> Handle {
        self.push(Kind::Bytes(Bytes {
            data,
            at: 0,
            positionable: true,
        }))
    }

    /// Whether the entry is open and positionable.
    pub fn is_positionable(&self, handle: Handle) -> bool {
        self.positionable(handle).is_some()
    }

    /// The in-memory bytes of an open positionable entry.
    fn positionable(&self, handle: Handle) -> Option<&Bytes> {
        match self.entries.get(handle.0 as usize) {
            Some(Some(Entry {
                kind: Kind::Bytes(bytes),
                ..
            })) if bytes.positionable => Some(bytes),
            _ => None,
        }
    }

    /// The length of a positionable entry; `ioerror` for any other.
    pub fn length(&self, handle: Handle) -> Result<usize, VmError> {
        self.positionable(handle)
            .map(|bytes| bytes.data.len())
            .ok_or(VmError::IoError)
    }

    /// The read position of a positionable entry, peeked bytes excluded;
    /// `ioerror` for any other, a closed one included.
    pub fn file_position(&self, handle: Handle) -> Result<usize, VmError> {
        self.positionable(handle).ok_or(VmError::IoError)?;
        self.position(handle).ok_or(VmError::IoError)
    }

    /// Moves the read position of a positionable entry to `position`,
    /// dropping any peeked byte; `rangecheck` beyond its length,
    /// `ioerror` for an entry that is not positionable.
    pub fn set_file_position(&mut self, handle: Handle, position: usize) -> Result<(), VmError> {
        let entry = self.entry(handle)?;
        let Kind::Bytes(bytes) = &mut entry.kind else {
            return Err(VmError::IoError);
        };
        if !bytes.positionable {
            return Err(VmError::IoError);
        }
        if position > bytes.data.len() {
            return Err(VmError::RangeCheck);
        }
        bytes.at = position;
        entry.pushback.clear();
        entry.position = position;
        Ok(())
    }

    /// Bytes left before the end of a positionable entry; `None` when the
    /// count is unknown (any other kind), `ioerror` for a closed entry.
    pub fn bytes_available(&self, handle: Handle) -> Result<Option<usize>, VmError> {
        if !self.is_open(handle) {
            return Err(VmError::IoError);
        }
        Ok(self
            .positionable(handle)
            .map(|bytes| bytes.data.len() - self.position(handle).unwrap_or(0)))
    }

    /// Opens an entry whose bytes `body`, a procedure, delivers when run
    /// (see [`FileTable::take_wanted_procedure`]).
    pub fn open_procedure(&mut self, body: Object) -> Handle {
        self.push(Kind::Procedure(Procedure {
            body,
            buffer: VecDeque::new(),
            exhausted: false,
        }))
    }

    /// Opens an `eexec` layer reading `base` from its current position;
    /// `ioerror` if `base` is not open.
    pub fn open_layer(&mut self, base: Handle) -> Result<Handle, VmError> {
        self.open_decoder(base, Decoder::eexec(), false)
    }

    /// Opens a decode layer reading `base` from its current position;
    /// `ioerror` if `base` is not open. With `close_source`, closing the
    /// layer closes `base` too.
    pub(crate) fn open_decoder(
        &mut self,
        base: Handle,
        decoder: Decoder,
        close_source: bool,
    ) -> Result<Handle, VmError> {
        self.entry(base)?;
        Ok(self.push(Kind::Decode(Layer {
            base,
            decoder,
            pending: VecDeque::new(),
            ended: false,
            raw: Vec::new(),
            lookahead: Vec::new(),
            close_source,
        })))
    }

    /// Opens an encode entry writing through `encoder` to `target`;
    /// `ioerror` if `target` is not open. With `close_target`, closing
    /// the entry closes `target` too after the final bytes are written.
    pub(crate) fn open_encoder(
        &mut self,
        target: Handle,
        encoder: Encoder,
        close_target: bool,
    ) -> Result<Handle, VmError> {
        self.entry(target)?;
        Ok(self.push(Kind::Encode(Writer {
            target,
            encoder,
            close_target,
        })))
    }

    fn layer(&self, handle: Handle) -> Option<&Layer> {
        match self.entries.get(handle.0 as usize) {
            Some(Some(Entry {
                kind: Kind::Decode(layer),
                ..
            })) => Some(layer),
            _ => None,
        }
    }

    /// The base of a layer; `None` for a plain entry or a closed one.
    pub fn layer_base(&self, handle: Handle) -> Option<Handle> {
        self.layer(handle).map(|layer| layer.base)
    }

    /// Whether the entry is a decode layer whose decoder is the DCT
    /// placeholder, so an image reads the encoded bytes from its base.
    pub fn is_dct_layer(&self, handle: Handle) -> bool {
        self.layer(handle).is_some_and(|l| l.decoder.is_dct())
    }

    /// Whether the entry is a filter layer: a decode layer other than
    /// the `eexec` cipher's.
    pub fn is_filter(&self, handle: Handle) -> bool {
        self.layer(handle)
            .is_some_and(|l| !l.decoder.returns_lookahead())
    }

    /// Whether closing the entry should first read it to its end: a
    /// decode layer that has not yet consumed its end-of-data marker,
    /// so the base continues after the marker.
    pub fn ends_at_marker(&self, handle: Handle) -> bool {
        self.layer(handle)
            .is_some_and(|l| !l.ended && l.decoder.has_marker())
    }

    /// The procedure entry whose empty buffer failed the latest read,
    /// with the procedure to run for more; taking it clears the note.
    pub fn take_wanted_procedure(&mut self) -> Option<(Handle, Object)> {
        let handle = self.wanted.take()?;
        match self.entries.get(handle.0 as usize) {
            Some(Some(Entry {
                kind: Kind::Procedure(procedure),
                ..
            })) => Some((handle, procedure.body)),
            _ => None,
        }
    }

    /// Delivers the next string of a procedure entry; an empty one ends
    /// it. A closed entry takes nothing.
    pub fn feed_procedure(&mut self, handle: Handle, bytes: &[u8]) {
        if let Some(Some(Entry {
            kind: Kind::Procedure(procedure),
            ..
        })) = self.entries.get_mut(handle.0 as usize)
        {
            if bytes.is_empty() {
                procedure.exhausted = true;
            } else {
                procedure.buffer.extend(bytes);
            }
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

    /// Whether the entry's bytes may still be growing: its stream says
    /// so, or it is a layer over one.
    fn may_grow(&self, handle: Handle) -> bool {
        match self.entries.get(handle.0 as usize) {
            Some(Some(Entry {
                kind: Kind::Stream(stream),
                ..
            })) => stream.more_may_come(),
            Some(Some(Entry {
                kind: Kind::Procedure(procedure),
                ..
            })) => procedure.more_may_come(),
            Some(Some(Entry {
                kind: Kind::Decode(layer),
                ..
            })) => self.may_grow(layer.base),
            _ => false,
        }
    }

    /// Records the entry's state before the current operator's first
    /// read from it, if it may grow and is not recorded yet.
    fn guard(&mut self, handle: Handle) {
        if self.saved.iter().any(|s| s.handle == handle) || !self.may_grow(handle) {
            return;
        }
        let Some(Some(entry)) = self.entries.get(handle.0 as usize) else {
            return;
        };
        let layer = match &entry.kind {
            Kind::Stream(_) | Kind::Bytes(_) | Kind::Procedure(_) | Kind::Encode(_) => None,
            Kind::Decode(layer) => Some(layer.clone()),
        };
        self.saved.push(Saved {
            handle,
            pushback: entry.pushback.clone(),
            position: entry.position,
            layer,
            taken: Vec::new(),
        });
    }

    /// Notes bytes taken from the stream behind `handle` since its
    /// snapshot.
    fn note_taken(&mut self, handle: Handle, bytes: &[u8]) {
        if let Some(saved) = self.saved.iter_mut().find(|s| s.handle == handle) {
            saved.taken.extend_from_slice(bytes);
        }
    }

    /// A stream read's outcome, noting a procedure entry that starved.
    fn starving<T>(&mut self, handle: Handle, result: Result<T, VmError>) -> Result<T, VmError> {
        if let Err(VmError::NeedMore) = result
            && let Ok(Entry {
                kind: Kind::Procedure(_),
                ..
            }) = self.entry(handle)
        {
            self.wanted = Some(handle);
        }
        result
    }

    /// Forgets the snapshots: the operator that read completed.
    pub fn commit(&mut self) {
        if !self.saved.is_empty() {
            self.saved.clear();
        }
    }

    /// Restores every recorded entry to its snapshot and hands the bytes
    /// taken since back to their streams: the operator that read is
    /// suspended and will run again from the start.
    pub fn rollback(&mut self) {
        while let Some(saved) = self.saved.pop() {
            let Some(Some(entry)) = self.entries.get_mut(saved.handle.0 as usize) else {
                continue;
            };
            entry.pushback = saved.pushback;
            entry.position = saved.position;
            match (&mut entry.kind, saved.layer) {
                (Kind::Decode(layer), Some(saved)) => *layer = saved,
                (Kind::Decode(_), None) => {}
                (kind, _) => {
                    if let Some(stream) = kind.stream() {
                        stream.unread(&saved.taken);
                    }
                }
            }
        }
    }

    /// Returns bytes a layer took from its base that turned out not to
    /// be its data, so the base reads them next.
    fn give_back(&mut self, base: Handle, raw: &[u8]) -> Result<(), VmError> {
        let entry = self.entry(base)?;
        entry.position -= raw.len();
        entry.pushback.extend(raw.iter().rev());
        Ok(())
    }

    /// One byte from the base of a layer, for the layer's own use.
    fn base_byte(&mut self, base: Handle) -> Result<Option<u8>, VmError> {
        let mut byte = [0u8; 1];
        Ok((self.read(base, &mut byte)? == 1).then_some(byte[0]))
    }

    /// The next decoded byte of the layer at `handle`; `None` at the end
    /// of its data. A base that must wait fails the read as it would any
    /// other; the decoder keeps what it was fed, so the next attempt
    /// continues where this one stopped.
    fn layer_byte(&mut self, handle: Handle) -> Result<Option<u8>, VmError> {
        let base = self.layer_base(handle).ok_or(VmError::IoError)?;
        let mut out = Vec::new();
        loop {
            let Kind::Decode(layer) = &mut self.entry(handle)?.kind else {
                unreachable!("layer entries only");
            };
            if let Some(byte) = layer.pending.pop_front() {
                return Ok(Some(byte));
            }
            if layer.ended {
                return Ok(None);
            }
            let byte = self.base_byte(base)?;
            let Kind::Decode(layer) = &mut self.entry(handle)?.kind else {
                unreachable!("layer entries only");
            };
            let mut foreign = None;
            let fed = match byte {
                None => {
                    layer.decoder.finish(&mut out)?;
                    Fed::End
                }
                Some(byte) => {
                    if layer.decoder.returns_lookahead() {
                        layer.raw.push(byte);
                    }
                    match layer.decoder.push(byte, &mut out) {
                        Ok(fed) => fed,
                        Err(e) => {
                            layer.ended = true;
                            return Err(e);
                        }
                    }
                }
            };
            match fed {
                Fed::More => {}
                Fed::End => layer.ended = true,
                Fed::EndBefore => {
                    layer.ended = true;
                    foreign = Some(std::mem::take(&mut layer.raw));
                }
            }
            if !out.is_empty() {
                layer.lookahead = std::mem::take(&mut layer.raw);
                layer.pending.extend(out.drain(..));
            }
            if let Some(raw) = foreign {
                self.give_back(base, &raw)?;
            }
        }
    }

    /// Reads into `buf`, starting with any byte the scanner peeked but did
    /// not consume.
    pub fn read(&mut self, handle: Handle, buf: &mut [u8]) -> Result<usize, VmError> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.guard(handle);
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
        if let Some(stream) = entry.kind.stream() {
            let result = stream.read(&mut buf[n..]);
            let got = self.starving(handle, result)?;
            self.entry(handle)?.position += got;
            if got > 0 {
                self.note_taken(handle, &buf[n..n + got]);
            }
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
            if let Kind::Decode(layer) = &mut entry.kind {
                layer.lookahead.clear();
            }
        }
        Ok(n)
    }

    /// The next unread byte without consuming it; `None` at end of data.
    pub fn peek(&mut self, handle: Handle) -> Result<Option<u8>, VmError> {
        self.guard(handle);
        let entry = self.entry(handle)?;
        if let Some(&byte) = entry.pushback.last() {
            return Ok(Some(byte));
        }
        let byte = match entry.kind.stream() {
            Some(stream) => {
                let mut byte = [0u8; 1];
                let result = stream.read(&mut byte);
                let got = (self.starving(handle, result)? == 1).then_some(byte[0]);
                if got.is_some() {
                    self.note_taken(handle, &byte);
                }
                got
            }
            None => self.layer_byte(handle)?,
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
            if let Kind::Decode(layer) = &mut entry.kind {
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

    /// Writes into the entry: a stream takes what it accepts; an encode
    /// entry takes everything, its encoding written through to the
    /// target, which must accept it all.
    pub fn write(&mut self, handle: Handle, buf: &[u8]) -> Result<usize, VmError> {
        match &mut self.entry(handle)?.kind {
            Kind::Encode(writer) => {
                let target = writer.target;
                let mut out = Vec::new();
                writer.encoder.push(buf, &mut out);
                self.write_all(target, &out)?;
                Ok(buf.len())
            }
            kind => match kind.stream() {
                Some(stream) => stream.write(buf),
                None => Err(VmError::IoError),
            },
        }
    }

    /// Writes all of `bytes` to the entry, however many writes it takes;
    /// `ioerror` if it stops accepting them.
    fn write_all(&mut self, handle: Handle, mut bytes: &[u8]) -> Result<(), VmError> {
        while !bytes.is_empty() {
            let n = self.write(handle, bytes)?;
            if n == 0 {
                return Err(VmError::IoError);
            }
            bytes = &bytes[n..];
        }
        Ok(())
    }

    /// Pushes buffered output on: an encode entry writes what its
    /// encoder can emit short of ending the data, then flushes the
    /// target.
    pub fn flush(&mut self, handle: Handle) -> Result<(), VmError> {
        match &mut self.entry(handle)?.kind {
            Kind::Encode(writer) => {
                let target = writer.target;
                let mut out = Vec::new();
                writer.encoder.flush(&mut out);
                self.write_all(target, &out)?;
                self.flush(target)
            }
            kind => match kind.stream() {
                Some(stream) => stream.flush(),
                None => Ok(()),
            },
        }
    }

    /// Closes the entry; closing an already closed or unknown file is not
    /// an error. Closing the cipher layer returns the byte it read ahead,
    /// if any, to its base, so the base continues exactly after the last
    /// byte the layer consumed; a layer opened to close its source closes
    /// it. Closing an encode entry writes the encoding's final bytes and
    /// marker to the target, then closes the target if asked to.
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
            Kind::Bytes(_) | Kind::Procedure(_) => Ok(()),
            Kind::Decode(layer) => {
                if layer.decoder.returns_lookahead()
                    && !entry.pushback.is_empty()
                    && let Ok(base) = self.entry(layer.base)
                {
                    base.position -= layer.lookahead.len();
                    base.pushback.extend(layer.lookahead.iter().rev());
                }
                if layer.close_source {
                    self.close(layer.base)?;
                }
                Ok(())
            }
            Kind::Encode(mut writer) => {
                let mut out = Vec::new();
                writer.encoder.finish(&mut out);
                let written = self.write_all(writer.target, &out);
                if writer.close_target {
                    self.close(writer.target)?;
                }
                written
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
    use efterscript_fonts::type1::{EEXEC_KEY, encrypt, hex_value};

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

    // --- filter layers and their sources ------------------------------------------

    #[test]
    fn a_bytes_entry_reads_once_and_refuses_writes() {
        let mut t = FileTable::new();
        let h = t.open_bytes(b"abc".to_vec());
        let mut buf = [0u8; 2];
        assert_eq!(t.read(h, &mut buf), Ok(2));
        assert_eq!(t.read(h, &mut buf), Ok(1));
        assert_eq!(t.read(h, &mut buf), Ok(0));
        assert_eq!(t.write(h, b"x"), Err(VmError::IoError));
        assert!(!t.is_filter(h));
        assert!(!t.ends_at_marker(h));
        // A one-shot entry is not positionable, nor is a stream.
        assert!(!t.is_positionable(h));
        assert_eq!(t.file_position(h), Err(VmError::IoError));
        assert_eq!(t.set_file_position(h, 0), Err(VmError::IoError));
        assert_eq!(t.length(h), Err(VmError::IoError));
        assert_eq!(t.bytes_available(h), Ok(None));
        let s = t.open(Box::new(Probe::with_input(b"xyz")));
        assert_eq!(t.set_file_position(s, 0), Err(VmError::IoError));
        assert_eq!(t.bytes_available(s), Ok(None));
        t.close(s).unwrap();
        assert_eq!(t.bytes_available(s), Err(VmError::IoError));
    }

    #[test]
    fn a_reusable_entry_is_positionable_and_stays_open_at_its_end() {
        let mut t = FileTable::new();
        let h = t.open_reusable(b"hello".to_vec());
        assert!(t.is_positionable(h));
        assert_eq!(t.length(h), Ok(5));
        assert_eq!(t.file_position(h), Ok(0));
        assert_eq!(t.bytes_available(h), Ok(Some(5)));
        let mut buf = [0u8; 8];
        assert_eq!(t.read(h, &mut buf), Ok(5));
        assert_eq!(&buf[..5], b"hello");
        assert_eq!(t.read(h, &mut buf), Ok(0));
        assert!(t.is_open(h));
        assert_eq!(t.file_position(h), Ok(5));
        assert_eq!(t.bytes_available(h), Ok(Some(0)));
        assert_eq!(t.set_file_position(h, 3), Ok(()));
        assert_eq!(t.bytes_available(h), Ok(Some(2)));
        assert_eq!(t.read(h, &mut buf), Ok(2));
        assert_eq!(&buf[..2], b"lo");
        assert_eq!(t.set_file_position(h, 6), Err(VmError::RangeCheck));
        assert_eq!(t.set_file_position(h, 5), Ok(()));
        assert_eq!(t.read(h, &mut buf), Ok(0));
        // A peeked byte is dropped by positioning and excluded from the
        // position.
        assert_eq!(t.set_file_position(h, 0), Ok(()));
        assert_eq!(t.peek(h), Ok(Some(b'h')));
        assert_eq!(t.file_position(h), Ok(0));
        assert_eq!(t.set_file_position(h, 4), Ok(()));
        assert_eq!(t.consume(h), Ok(Some(b'o')));
        assert_eq!(t.file_position(h), Ok(5));
        assert_eq!(t.write(h, b"x"), Err(VmError::IoError));
        assert!(!t.is_filter(h));
        t.close(h).unwrap();
        assert!(!t.is_positionable(h));
        assert_eq!(t.file_position(h), Err(VmError::IoError));
        assert_eq!(t.set_file_position(h, 0), Err(VmError::IoError));
        assert_eq!(t.bytes_available(h), Err(VmError::IoError));
    }

    #[test]
    fn a_procedure_entry_starves_until_fed_and_ends_on_an_empty_string() {
        let mut t = FileTable::new();
        let body = Object::integer(7);
        let h = t.open_procedure(body);
        let mut buf = [0u8; 4];
        assert_eq!(t.read(h, &mut buf), Err(VmError::NeedMore));
        let wanted = t.take_wanted_procedure().expect("the procedure is wanted");
        assert!(wanted.0 == h && wanted.1.eq(body));
        assert!(t.take_wanted_procedure().is_none());
        t.feed_procedure(h, b"abcde");
        assert_eq!(t.read(h, &mut buf), Ok(4));
        assert_eq!(&buf, b"abcd");
        assert_eq!(t.peek(h), Ok(Some(b'e')));
        assert_eq!(t.consume(h), Ok(Some(b'e')));
        assert_eq!(t.peek(h), Err(VmError::NeedMore));
        t.feed_procedure(h, b"");
        assert_eq!(t.peek(h), Ok(None));
        assert_eq!(t.read(h, &mut buf), Ok(0));
        assert_eq!(t.write(h, b"x"), Err(VmError::IoError));
        // A layer over it is starved the same way — a read is all or
        // nothing, so a longer buffer starves even after a byte came —
        // and its reads are undone on rollback so the re-run sees the
        // whole delivery.
        let h = t.open_procedure(body);
        let layer = t.open_decoder(h, Decoder::ascii_hex(), false).unwrap();
        t.feed_procedure(h, b"41");
        assert_eq!(t.read(layer, &mut buf), Err(VmError::NeedMore));
        t.rollback();
        assert_eq!(t.read(layer, &mut buf[..1]), Ok(1));
        assert_eq!(&buf[..1], b"A");
        t.commit();
        assert_eq!(t.read(layer, &mut buf), Err(VmError::NeedMore));
        let wanted = t.take_wanted_procedure().expect("the procedure is wanted");
        assert!(wanted.0 == h && wanted.1.eq(body));
        t.rollback();
        t.feed_procedure(h, b"42>");
        assert_eq!(t.read(layer, &mut buf), Ok(1));
        assert_eq!(&buf[..1], b"B");
        assert_eq!(t.read(layer, &mut buf), Ok(0));
        t.feed_procedure(Handle(99), b"ignored");
    }

    #[test]
    fn a_filter_layer_ends_at_its_marker_and_can_close_its_source() {
        let mut t = FileTable::new();
        let probe = Probe::with_input(b"48 69>tail");
        let base = t.open(Box::new(probe.clone()));
        let layer = t.open_decoder(base, Decoder::ascii_hex(), true).unwrap();
        assert!(t.is_filter(layer));
        assert!(!t.is_filter(base));
        assert!(t.ends_at_marker(layer));
        assert_eq!(t.layer_base(layer), Some(base));
        let mut buf = [0u8; 8];
        assert_eq!(t.read(layer, &mut buf), Ok(2));
        assert_eq!(&buf[..2], b"Hi");
        assert!(!t.ends_at_marker(layer));
        assert_eq!(t.peek(layer), Ok(None));
        assert_eq!(t.position(base), Some(6));
        t.close(layer).unwrap();
        assert!(probe.closed());
        assert!(!t.is_open(base));
        // Without CloseSource the base stays open after the marker.
        let mut t = FileTable::new();
        let base = t.open(Box::new(Probe::with_input(b"BOu!rDZ~>tail")));
        let layer = t.open_decoder(base, Decoder::ascii85(), false).unwrap();
        assert_eq!(t.read(layer, &mut buf), Ok(5));
        t.close(layer).unwrap();
        let n = t.read(base, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"tail");
        // The DCT placeholder is detectable and unreadable.
        let base = t.open(Box::new(Probe::with_input(b"\xFF\xD8")));
        let dct = t.open_decoder(base, Decoder::Dct, false).unwrap();
        assert!(t.is_dct_layer(dct));
        assert!(!t.is_dct_layer(base));
        assert_eq!(t.read(dct, &mut buf), Err(VmError::Undefined));
        assert_eq!(
            t.open_decoder(Handle(77), Decoder::Dct, false),
            Err(VmError::IoError)
        );
    }

    #[test]
    fn an_encode_entry_writes_through_and_ends_the_data_on_close() {
        let mut t = FileTable::new();
        let probe = Probe::default();
        let target = t.open(Box::new(probe.clone()));
        let hex = t.open_encoder(target, Encoder::ascii_hex(), false).unwrap();
        assert!(!t.is_filter(hex));
        assert!(!t.ends_at_marker(hex));
        assert_eq!(t.layer_base(hex), None);
        assert_eq!(t.write(hex, b"Hi"), Ok(2));
        assert_eq!(probe.output(), b"4869");
        assert_eq!(t.flush(hex), Ok(()));
        let mut buf = [0u8; 4];
        assert_eq!(t.read(hex, &mut buf), Err(VmError::IoError));
        assert_eq!(t.peek(hex), Err(VmError::IoError));
        t.close(hex).unwrap();
        assert_eq!(probe.output(), b"4869>");
        assert!(!probe.closed());
        assert!(t.is_open(target));
        // A chain: the outer encoder's bytes go through the inner one,
        // and closing with the target flag closes down the chain.
        let base85 = t.open_encoder(target, Encoder::ascii85(), true).unwrap();
        let run = t
            .open_encoder(base85, Encoder::run_length(0), true)
            .unwrap();
        assert_eq!(t.write(run, b"aaaab"), Ok(5));
        t.close(run).unwrap();
        assert!(probe.closed());
        assert!(!t.is_open(base85));
        assert!(!t.is_open(target));
        let text = String::from_utf8(probe.output()).unwrap();
        assert!(text.starts_with("4869>"));
        assert!(text.ends_with("~>"));
        // The target closed first: writes fail, and so does the close
        // that would write the marker.
        let target = t.open(Box::new(Probe::default()));
        let hex = t.open_encoder(target, Encoder::ascii_hex(), false).unwrap();
        t.close(target).unwrap();
        assert_eq!(t.write(hex, b"x"), Err(VmError::IoError));
        assert_eq!(t.close(hex), Err(VmError::IoError));
        assert_eq!(
            t.open_encoder(Handle(99), Encoder::ascii_hex(), false),
            Err(VmError::IoError)
        );
        // A target that accepts nothing is an error too.
        struct Full;
        impl Stream for Full {
            fn read(&mut self, _: &mut [u8]) -> Result<usize, VmError> {
                Ok(0)
            }
            fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
                Ok(0)
            }
        }
        let full = t.open(Box::new(Full));
        let hex = t.open_encoder(full, Encoder::ascii_hex(), false).unwrap();
        assert_eq!(t.write(hex, b"x"), Err(VmError::IoError));
    }

    #[test]
    fn a_layer_over_a_growing_base_rolls_back_its_decoder() {
        struct Growing(Vec<u8>, usize);
        impl Stream for Growing {
            fn read(&mut self, buf: &mut [u8]) -> Result<usize, VmError> {
                if self.1 == self.0.len() {
                    return Err(VmError::NeedMore);
                }
                let n = buf.len().min(self.0.len() - self.1);
                buf[..n].copy_from_slice(&self.0[self.1..self.1 + n]);
                self.1 += n;
                Ok(n)
            }
            fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
                Ok(0)
            }
            fn more_may_come(&self) -> bool {
                true
            }
            fn unread(&mut self, bytes: &[u8]) {
                self.1 -= bytes.len();
            }
        }
        let mut t = FileTable::new();
        let base = t.open(Box::new(Growing(b"4142 4".to_vec(), 0)));
        let layer = t.open_decoder(base, Decoder::ascii_hex(), false).unwrap();
        let mut buf = [0u8; 3];
        // Two bytes come, the third is half there: the read fails and is
        // undone, so the next attempt starts from the first byte again.
        assert_eq!(t.read(layer, &mut buf), Err(VmError::NeedMore));
        t.rollback();
        assert_eq!(t.position(layer), Some(0));
        assert_eq!(t.position(base), Some(0));
        assert_eq!(t.read(layer, &mut buf), Err(VmError::NeedMore));
        t.rollback();
        t.commit();
        assert_eq!(t.read(layer, &mut buf[..2]), Ok(2));
        assert_eq!(&buf[..2], b"AB");
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
