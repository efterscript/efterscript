// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Where a JPEG stream ends. An image whose data source is a `DCTDecode`
//! filter is passed through undecoded, so the interpreter needs only
//! the extent of the stream: the marker structure of ITU-T T.81 Annex B
//! gives it. Markers are a `0xFF` prefix (fill bytes repeat it) and a
//! code; most are followed by a two-byte length counting itself and
//! the segment; a few stand alone; the start-of-scan segment is followed
//! by entropy-coded data in which `0xFF` is always followed by a stuffed
//! `0x00`, a restart code, or the next marker; the end-of-image marker
//! ends the stream. Nothing inside a segment is interpreted.

/// A marker code standing alone, without a length.
fn is_standalone(code: u8) -> bool {
    matches!(code, 0x01 | 0xD0..=0xD8)
}

const START_OF_SCAN: u8 = 0xDA;
const END_OF_IMAGE: u8 = 0xD9;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Expecting the `0xFF` that begins a marker.
    Prefix,
    /// After the prefix, expecting the marker code.
    Code,
    /// Reading a segment's length; `high` once its first byte is in.
    Length {
        high: Option<u8>,
        scan: bool,
    },
    /// Inside a segment with `remaining` bytes to skip.
    Segment {
        remaining: usize,
        scan: bool,
    },
    /// Inside entropy-coded data.
    Scan,
    /// `0xFF` seen inside entropy-coded data.
    ScanPrefix,
    Done,
}

/// What a byte did to the walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Walk {
    More,
    /// The byte completed the end-of-image marker.
    End,
}

/// A byte where the structure allows none: a marker expected and not
/// found, a stuffed zero outside a scan, or a length under two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Malformed;

/// Walks a JPEG stream's markers one byte at a time; see the module
/// documentation.
#[derive(Clone, Debug)]
pub struct MarkerWalker {
    state: State,
}

impl Default for MarkerWalker {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkerWalker {
    pub fn new() -> Self {
        MarkerWalker {
            state: State::Prefix,
        }
    }

    /// Feeds the next byte of the stream. After the end every byte
    /// answers `End`.
    pub fn push(&mut self, byte: u8) -> Result<Walk, Malformed> {
        self.state = match self.state {
            State::Prefix if byte == 0xFF => State::Code,
            State::Prefix => return Err(Malformed),
            State::Code => self.code(byte)?,
            State::Length { high: None, scan } => State::Length {
                high: Some(byte),
                scan,
            },
            State::Length {
                high: Some(high),
                scan,
            } => {
                let length = usize::from(u16::from_be_bytes([high, byte]));
                match length.checked_sub(2) {
                    None => return Err(Malformed),
                    Some(0) => Self::after_segment(scan),
                    Some(remaining) => State::Segment { remaining, scan },
                }
            }
            State::Segment { remaining, scan } => {
                if remaining > 1 {
                    State::Segment {
                        remaining: remaining - 1,
                        scan,
                    }
                } else {
                    Self::after_segment(scan)
                }
            }
            State::Scan if byte == 0xFF => State::ScanPrefix,
            State::Scan => State::Scan,
            State::ScanPrefix => match byte {
                0x00 | 0xD0..=0xD7 => State::Scan,
                0xFF => State::ScanPrefix,
                _ => self.code(byte)?,
            },
            State::Done => State::Done,
        };
        Ok(if self.state == State::Done {
            Walk::End
        } else {
            Walk::More
        })
    }

    fn after_segment(scan: bool) -> State {
        if scan { State::Scan } else { State::Prefix }
    }

    /// The state after a marker code.
    fn code(&self, byte: u8) -> Result<State, Malformed> {
        Ok(match byte {
            0xFF => State::Code,
            0x00 => return Err(Malformed),
            END_OF_IMAGE => State::Done,
            code if is_standalone(code) => State::Prefix,
            code => State::Length {
                high: None,
                scan: code == START_OF_SCAN,
            },
        })
    }

    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }
}

/// The length of the JPEG stream at the start of `bytes`, end-of-image
/// marker included; `None` if it is malformed or `bytes` ends first.
pub fn stream_len(bytes: &[u8]) -> Option<usize> {
    let mut walker = MarkerWalker::new();
    for (at, &byte) in bytes.iter().enumerate() {
        if walker.push(byte).ok()? == Walk::End {
            return Some(at + 1);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_scans_and_the_end_marker() {
        // SOI, a two-byte-payload segment, SOS with a one-byte header,
        // entropy data with a stuffed FF, a fill byte, a restart, EOI,
        // and bytes after it.
        let stream = [
            0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x04, 0xAA, 0xBB, 0xFF, 0xDA, 0x00, 0x03, 0x01, 0x12,
            0xFF, 0x00, 0xFF, 0xFF, 0xD3, 0x34, 0xFF, 0xD9, 0x56, 0x78,
        ];
        assert_eq!(stream_len(&stream), Some(22));
        let mut walker = MarkerWalker::new();
        for &byte in &stream[..21] {
            assert_eq!(walker.push(byte), Ok(Walk::More));
            assert!(!walker.is_done());
        }
        assert_eq!(walker.push(0xD9), Ok(Walk::End));
        assert!(walker.is_done());
        assert_eq!(walker.push(0x56), Ok(Walk::End));
        // A segment whose payload holds marker-like bytes is skipped by
        // its length; a zero-payload segment goes straight on.
        let stream = [
            0xFF, 0xD8, 0xFF, 0xFE, 0x00, 0x04, 0xFF, 0xD9, 0xFF, 0xFE, 0x00, 0x02, 0xFF, 0xD9,
        ];
        assert_eq!(stream_len(&stream), Some(14));
        // A scan ended by a marker other than EOI (a second scan) goes
        // on to it.
        let stream = [
            0xFF, 0xD8, 0xFF, 0xDA, 0x00, 0x02, 0x11, 0xFF, 0xDA, 0x00, 0x02, 0x22, 0xFF, 0xD9,
        ];
        assert_eq!(stream_len(&stream), Some(14));
        assert_eq!(stream_len(&[0xFF, 0xD9]), Some(2));
        assert_eq!(MarkerWalker::default().state, State::Prefix);
    }

    #[test]
    fn truncation_and_malformed_bytes() {
        assert_eq!(
            stream_len(&[0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x04, 0xAA]),
            None
        );
        assert_eq!(stream_len(&[]), None);
        assert_eq!(stream_len(b"\xFF\xD8x"), None);
        assert_eq!(MarkerWalker::new().push(0x12), Err(Malformed));
        let mut walker = MarkerWalker::new();
        walker.push(0xFF).unwrap();
        assert_eq!(walker.push(0x00), Err(Malformed));
        assert_eq!(stream_len(&[0xFF, 0xDB, 0x00, 0x01]), None);
    }
}
