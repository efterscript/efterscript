// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Document assembly: id allocation, indirect objects written in any order,
//! and `finish` — which refuses to close a file containing dangling
//! allocations and then writes the cross-reference section and trailer
//! (ISO 32000-1 §7.5).

use std::io::{self, Write};
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

/// Streams a PDF file to `sink` object by object; at most one object body is
/// buffered at a time, so memory stays bounded regardless of document size.
pub struct Document<W: Write> {
    sink: CountingWriter<W>,
    table: ObjectTable,
    /// Reused between objects to avoid reallocating per object.
    body: Vec<u8>,
}

impl<W: Write> Document<W> {
    /// Writes the header immediately; the file on disk is always a prefix of
    /// a valid PDF plus the not-yet-written tail.
    pub fn new(sink: W) -> Result<Self, Error> {
        let mut sink = CountingWriter::new(sink);
        sink.write_all(write::HEADER)?;
        Ok(Document {
            sink,
            table: ObjectTable::new(),
            body: Vec::new(),
        })
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
        Ok(self.sink.finish()?)
    }
}
