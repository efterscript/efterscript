// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Byte sink with offset tracking, file header, and end-of-file marker
//! (ISO 32000-1 §7.5.2, §7.5.5).

use std::io::{self, Write};

/// Version header plus the recommended binary-marker comment: four bytes
/// with the high bit set so transfer tools treat the file as binary
/// (per ISO 32000-1 §7.5.2). The four values are `eftr` with the high bit set.
pub(crate) const HEADER: &[u8] = b"%PDF-1.7\n%\xE5\xE6\xF4\xF2\n";

/// Wraps the output stream and counts bytes, so cross-reference offsets are
/// exact by construction.
pub(crate) struct CountingWriter<W: Write> {
    inner: W,
    offset: u64,
}

impl<W: Write> CountingWriter<W> {
    pub(crate) fn new(inner: W) -> Self {
        CountingWriter { inner, offset: 0 }
    }

    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }

    pub(crate) fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.inner.write_all(buf)?;
        self.offset += buf.len() as u64;
        Ok(())
    }

    pub(crate) fn finish(mut self) -> io::Result<W> {
        self.inner.flush()?;
        Ok(self.inner)
    }
}

pub(crate) fn write_eof<W: Write>(w: &mut CountingWriter<W>, startxref: u64) -> io::Result<()> {
    w.write_all(format!("startxref\n{startxref}\n%%EOF\n").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_marker_bytes_are_high_bit() {
        assert!(HEADER.starts_with(b"%PDF-1.7\n%"));
        assert!(HEADER.ends_with(b"\n"));
        let marker = &HEADER[10..14];
        assert_eq!(marker.len(), 4);
        assert!(marker.iter().all(|&b| b >= 0x80));
    }

    #[test]
    fn offsets_track_written_bytes() {
        let mut w = CountingWriter::new(Vec::new());
        w.write_all(b"abc").unwrap();
        assert_eq!(w.offset(), 3);
        w.write_all(b"").unwrap();
        w.write_all(b"defg").unwrap();
        assert_eq!(w.offset(), 7);
        assert_eq!(w.finish().unwrap(), b"abcdefg");
    }
}
