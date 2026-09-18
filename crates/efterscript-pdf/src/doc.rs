// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Document assembly: id allocation, indirect objects written in any order,
//! and `finish` — which refuses to close a file containing dangling
//! allocations and then writes the cross-reference section and trailer
//! (ISO 32000-1 §7.5).

use std::io::{self, Seek, SeekFrom, Write};
use std::{error, fmt, mem};

use crate::obj::{DictBuilder, Ref, Val};
use crate::stream::{self, Filter};
use crate::write::{self, CountingWriter};
use crate::xref::ObjectTable;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// `finish` was called while these allocated ids had no object written.
    UnwrittenObjects(Vec<u32>),
    ObjectAlreadyWritten(u32),
    /// The `Ref` was not allocated by this document.
    UnallocatedObject(u32),
    /// Comments must be printable ASCII on a single line.
    InvalidComment,
    /// An object would start beyond the 10-digit offset a classic
    /// cross-reference entry can express.
    FileTooLarge,
    /// A name contained a NUL byte, which no PDF name can carry
    /// (per ISO 32000-1 §7.3.5).
    NulInName,
    /// The header's version field holds one digit each side of the dot.
    InvalidVersion,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "i/o error: {e}"),
            Error::UnwrittenObjects(ids) => {
                write!(f, "allocated but never written: object")?;
                for id in ids {
                    write!(f, " {id}")?;
                }
                Ok(())
            }
            Error::ObjectAlreadyWritten(id) => write!(f, "object {id} written twice"),
            Error::UnallocatedObject(id) => write!(f, "object {id} was never allocated"),
            Error::InvalidComment => write!(f, "comment must be single-line printable ASCII"),
            Error::FileTooLarge => write!(f, "object offset exceeds ten decimal digits"),
            Error::NulInName => write!(f, "a name contains a NUL byte"),
            Error::InvalidVersion => write!(f, "version digits must be 0 to 9"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

/// Rewrites bytes at an offset of the sink without moving its end.
type Patch<W> = fn(&mut W, u64, &[u8]) -> io::Result<()>;

/// Rewrites `bytes` at offset `at` of a seekable sink, leaving the
/// position at the end of what was written so far.
fn patch_seekable<W: Write + Seek>(sink: &mut W, at: u64, bytes: &[u8]) -> io::Result<()> {
    let end = sink.stream_position()?;
    sink.seek(SeekFrom::Start(at))?;
    sink.write_all(bytes)?;
    sink.seek(SeekFrom::Start(end))?;
    Ok(())
}

/// Streams a PDF file to `sink` object by object; at most one object body is
/// buffered at a time, so memory stays bounded regardless of document size.
pub struct Document<W: Write> {
    sink: CountingWriter<W>,
    table: ObjectTable,
    /// Reused between objects to avoid reallocating per object.
    body: Vec<u8>,
    /// The version the header names at finish; written as 1.7 up front.
    version: (u8, u8),
    /// How to rewrite the header's version field, when the sink allows it.
    patch: Option<Patch<W>>,
}

impl<W: Write> Document<W> {
    /// Writes the header immediately; the file on disk is always a prefix of
    /// a valid PDF plus the not-yet-written tail. The header names version
    /// 1.7 for good: only [`Self::new_seekable`] can revise it.
    pub fn new(sink: W) -> Result<Self, Error> {
        Self::start(sink, None)
    }

    /// As [`Self::new`], for a sink that can seek back to the header, so a
    /// version set through [`Self::set_version`] before `finish` is the
    /// one the file names.
    pub fn new_seekable(sink: W) -> Result<Self, Error>
    where
        W: Seek,
    {
        Self::start(sink, Some(patch_seekable::<W>))
    }

    fn start(sink: W, patch: Option<Patch<W>>) -> Result<Self, Error> {
        let mut sink = CountingWriter::new(sink);
        sink.write_all(write::HEADER)?;
        Ok(Document {
            sink,
            table: ObjectTable::new(),
            body: Vec::new(),
            version: write::DEFAULT_VERSION,
            patch,
        })
    }

    /// The version the header will name once the document finishes:
    /// `major.minor`, one digit each. Honoured only for a document made
    /// with [`Self::new_seekable`]; see [`Self::version_patchable`].
    pub fn set_version(&mut self, major: u8, minor: u8) -> Result<(), Error> {
        if major > 9 || minor > 9 {
            return Err(Error::InvalidVersion);
        }
        self.version = (major, minor);
        Ok(())
    }

    /// Whether the header can still be revised at finish, which needs a
    /// seekable sink; otherwise the file names 1.7 whatever was set.
    pub fn version_patchable(&self) -> bool {
        self.patch.is_some()
    }

    /// Hands out ids 1, 2, 3, … in call order. Every allocated id must be
    /// written before [`Self::finish`].
    pub fn alloc(&mut self) -> Ref {
        Ref(self.table.alloc())
    }

    /// Writes one indirect object; the closure serializes exactly one value
    /// (an unused sink writes `null`). Objects may be written in any order.
    pub fn write_obj(&mut self, r: Ref, f: impl FnOnce(Val<'_>)) -> Result<(), Error> {
        let mut body = mem::take(&mut self.body);
        body.clear();
        let mut bad_name = false;
        f(Val::new(&mut body, &mut bad_name));
        let result = if bad_name {
            Err(Error::NulInName)
        } else {
            self.emit(r, &body)
        };
        self.body = body;
        result
    }

    /// Writes one stream object. `Length` is the exact byte count of the
    /// encoded data; `extra` adds entries after `Length` and `Filter`.
    pub fn write_stream(
        &mut self,
        r: Ref,
        filter: Filter,
        data: &[u8],
        extra: impl FnOnce(&mut DictBuilder<'_>),
    ) -> Result<(), Error> {
        let mut body = mem::take(&mut self.body);
        body.clear();
        let result = if stream::put_stream(&mut body, filter, data, extra) {
            self.emit(r, &body)
        } else {
            Err(Error::NulInName)
        };
        self.body = body;
        result
    }

    /// Writes a comment line (`% text`) between objects. Text must be
    /// printable ASCII: a comment runs to end of line, so line breaks and
    /// control bytes would change the file's meaning.
    pub fn comment(&mut self, text: &str) -> Result<(), Error> {
        if !text.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
            return Err(Error::InvalidComment);
        }
        self.sink.write_all(b"% ")?;
        self.sink.write_all(text.as_bytes())?;
        self.sink.write_all(b"\n")?;
        Ok(())
    }

    fn emit(&mut self, r: Ref, body: &[u8]) -> Result<(), Error> {
        self.table.record(r.id(), self.sink.offset())?;
        self.sink
            .write_all(format!("{} 0 obj\n", r.id()).as_bytes())?;
        self.sink.write_all(body)?;
        self.sink.write_all(b"\nendobj\n")?;
        Ok(())
    }

    /// Writes cross-reference section, trailer, `startxref`, and `%%EOF`,
    /// then returns the sink. Errors if any allocated id was never written.
    pub fn finish(mut self, root: Ref, info: Option<Ref>) -> Result<W, Error> {
        let unwritten = self.table.unwritten();
        if !unwritten.is_empty() {
            return Err(Error::UnwrittenObjects(unwritten));
        }
        let startxref = self.sink.offset();
        self.sink.write_all(&self.table.section_bytes())?;

        let mut trailer = Vec::new();
        let mut trailer_bad = false;
        Val::new(&mut trailer, &mut trailer_bad).dict(|d| {
            d.key("Size").int(i64::from(self.table.size()));
            d.key("Root").reference(root);
            if let Some(info) = info {
                d.key("Info").reference(info);
            }
        });
        self.sink.write_all(b"trailer\n")?;
        self.sink.write_all(&trailer)?;
        self.sink.write_all(b"\n")?;
        write::write_eof(&mut self.sink, startxref)?;
        if let (Some(patch), true) = (self.patch, self.version != write::DEFAULT_VERSION) {
            let (major, minor) = self.version;
            let field = [b'0' + major, b'.', b'0' + minor];
            patch(self.sink.inner_mut(), write::VERSION_OFFSET, &field)?;
        }
        Ok(self.sink.finish()?)
    }
}
