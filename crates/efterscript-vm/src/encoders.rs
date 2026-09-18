// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The encoders behind an encode-filter file (PLRM3 §3.13.3). Each takes
//! the bytes a program writes and appends their encoding to an output
//! buffer the file table writes through to the target; `flush` emits
//! what can be emitted without ending the data, and `finish` ends it
//! with the format's marker.
//!
//! Choices the formats leave open: hexadecimal digits are upper case;
//! the text encoders break their output into lines of at most 64
//! hexadecimal digits (32 bytes) or 75 base-85 digits (15 groups), the
//! lengths other interpreters were observed to use; a group of four
//! zero bytes is written as `z`; a run of two or more equal bytes is a
//! repeat run, and with a record length no run crosses a record
//! boundary; Flate and LZW hold the whole input and encode it when the
//! file closes, since `efterscript-codec`'s encoders take a complete buffer.

/// Longest lines the text encoders write, marker aside.
const HEX_LINE: usize = 64;
const BASE85_LINE: usize = 75;

/// An encoding state machine; see the module documentation.
#[derive(Clone, Debug)]
pub(crate) enum Encoder {
    AsciiHex(Text),
    Ascii85(Ascii85),
    RunLength(RunLength),
    Flate(Vec<u8>),
    Lzw { buffer: Vec<u8>, early_change: bool },
    Null,
}

impl Encoder {
    pub(crate) fn ascii_hex() -> Self {
        Encoder::AsciiHex(Text {
            column: 0,
            line: HEX_LINE,
        })
    }

    pub(crate) fn ascii85() -> Self {
        Encoder::Ascii85(Ascii85 {
            text: Text {
                column: 0,
                line: BASE85_LINE,
            },
            group: [0; 4],
            count: 0,
        })
    }

    /// `record_length` bytes per record, 0 for no records: a run ends at
    /// a record boundary.
    pub(crate) fn run_length(record_length: usize) -> Self {
        Encoder::RunLength(RunLength {
            pending: Vec::new(),
            record_length,
            in_record: 0,
        })
    }

    pub(crate) fn flate() -> Self {
        Encoder::Flate(Vec::new())
    }

    pub(crate) fn lzw(early_change: bool) -> Self {
        Encoder::Lzw {
            buffer: Vec::new(),
            early_change,
        }
    }

    /// Encodes `bytes`, appending what they complete to `out`.
    pub(crate) fn push(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        match self {
            Encoder::AsciiHex(text) => {
                for &byte in bytes {
                    text.put(b"0123456789ABCDEF"[usize::from(byte >> 4)], out);
                    text.put(b"0123456789ABCDEF"[usize::from(byte & 15)], out);
                }
            }
            Encoder::Ascii85(e) => e.push(bytes, out),
            Encoder::RunLength(e) => e.push(bytes, out),
            Encoder::Flate(buffer) | Encoder::Lzw { buffer, .. } => buffer.extend_from_slice(bytes),
            Encoder::Null => out.extend_from_slice(bytes),
        }
    }

    /// Appends what can be emitted without ending the data: the runs a
    /// run-length encoder was still extending.
    pub(crate) fn flush(&mut self, out: &mut Vec<u8>) {
        if let Encoder::RunLength(e) = self {
            e.drain(true, out);
        }
    }

    /// Ends the data: the last partial unit and the marker.
    pub(crate) fn finish(&mut self, out: &mut Vec<u8>) {
        match self {
            Encoder::AsciiHex(_) => out.push(b'>'),
            Encoder::Ascii85(e) => {
                e.partial(out);
                out.extend_from_slice(b"~>");
            }
            Encoder::RunLength(e) => {
                e.drain(true, out);
                out.push(128);
            }
            Encoder::Flate(buffer) => out.extend(efterscript_codec::deflate::compress(buffer)),
            Encoder::Lzw {
                buffer,
                early_change,
            } => out.extend(efterscript_codec::lzw::encode(buffer, *early_change)),
            Encoder::Null => {}
        }
    }
}

/// Line breaking for the text encoders.
#[derive(Clone, Debug)]
pub(crate) struct Text {
    column: usize,
    line: usize,
}

impl Text {
    fn put(&mut self, c: u8, out: &mut Vec<u8>) {
        if self.column == self.line {
            out.push(b'\n');
            self.column = 0;
        }
        out.push(c);
        self.column += 1;
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Ascii85 {
    text: Text,
    group: [u8; 4],
    count: usize,
}

impl Ascii85 {
    fn push(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        for &byte in bytes {
            self.group[self.count] = byte;
            self.count += 1;
            if self.count == 4 {
                if self.group == [0; 4] {
                    self.text.put(b'z', out);
                } else {
                    self.digits(4, out);
                }
                self.count = 0;
            }
        }
    }

    /// The first `n + 1` digits of the group's five, for `n` bytes held.
    fn digits(&mut self, n: usize, out: &mut Vec<u8>) {
        let mut value = u32::from_be_bytes(self.group);
        let mut digits = [0u8; 5];
        for digit in digits.iter_mut().rev() {
            *digit = (value % 85) as u8 + b'!';
            value /= 85;
        }
        for &digit in &digits[..=n] {
            self.text.put(digit, out);
        }
    }

    /// A final group of fewer than four bytes: padded with zeros, one
    /// digit more than its byte count is written.
    fn partial(&mut self, out: &mut Vec<u8>) {
        if self.count > 0 {
            self.group[self.count..].fill(0);
            let n = std::mem::take(&mut self.count);
            self.digits(n, out);
        }
    }
}

/// Bytes not yet emitted: a run being extended, or a literal that may
/// still grow, kept to at most one length byte's worth plus one.
#[derive(Clone, Debug)]
pub(crate) struct RunLength {
    pending: Vec<u8>,
    record_length: usize,
    in_record: usize,
}

impl RunLength {
    /// Takes the bytes record by record, every run decided at a record's
    /// end.
    fn push(&mut self, mut bytes: &[u8], out: &mut Vec<u8>) {
        if self.record_length == 0 {
            self.pending.extend_from_slice(bytes);
            self.drain(false, out);
            return;
        }
        while !bytes.is_empty() {
            let take = (self.record_length - self.in_record).min(bytes.len());
            self.pending.extend_from_slice(&bytes[..take]);
            self.in_record += take;
            bytes = &bytes[take..];
            let ends_record = self.in_record == self.record_length;
            self.drain(ends_record, out);
            if ends_record {
                self.in_record = 0;
            }
        }
    }

    /// Emits every run whose extent is decided: a repeat run once a
    /// different byte follows it or it reaches 128, a literal run once a
    /// repeat begins or it reaches 128; with `all`, the rest as well.
    fn drain(&mut self, all: bool, out: &mut Vec<u8>) {
        loop {
            let Some(&first) = self.pending.first() else {
                return;
            };
            let run = self.pending.iter().take_while(|&&b| b == first).count();
            if run >= 2 {
                if run == self.pending.len() && run < 128 && !all {
                    return;
                }
                let n = run.min(128);
                out.push((257 - n) as u8);
                out.push(first);
                self.pending.drain(..n);
                continue;
            }
            // A literal runs until a byte repeats its predecessor.
            let mut literal = 1;
            let mut decided = false;
            while literal < self.pending.len() {
                if literal == 128 {
                    decided = true;
                    break;
                }
                if self.pending[literal] == self.pending[literal - 1] {
                    literal -= 1;
                    decided = true;
                    break;
                }
                literal += 1;
            }
            if !decided && !all {
                return;
            }
            out.push((literal - 1) as u8);
            out.extend(self.pending.drain(..literal));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoders::{Decoder, Fed};

    /// Writes `input` in one-byte pieces, closes, and returns the target's
    /// bytes.
    fn encode(mut encoder: Encoder, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for byte in input {
            encoder.push(std::slice::from_ref(byte), &mut out);
        }
        encoder.finish(&mut out);
        out
    }

    /// Decodes `data`, asserting the decoder ends exactly at its last byte.
    fn decode(mut decoder: Decoder, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for (at, &byte) in data.iter().enumerate() {
            let fed = decoder.push(byte, &mut out).unwrap();
            assert_eq!(fed == Fed::End, at == data.len() - 1, "at {at}");
        }
        out
    }

    const FOX: &[u8] = b"the quick brown fox";

    #[test]
    fn hexadecimal_is_upper_case_and_wrapped() {
        assert_eq!(encode(Encoder::ascii_hex(), b"Hi ~"), b"4869207E>");
        assert_eq!(encode(Encoder::ascii_hex(), b""), b">");
        let long = encode(Encoder::ascii_hex(), &[0xAB; 40]);
        let text = String::from_utf8(long.clone()).unwrap();
        assert_eq!(text, format!("{}\n{}>", "AB".repeat(32), "AB".repeat(8)));
        assert_eq!(decode(Decoder::ascii_hex(), &long), [0xAB; 40]);
    }

    #[test]
    fn base85_groups_partials_z_and_wrapping() {
        assert_eq!(encode(Encoder::ascii85(), b"hello"), b"BOu!rDZ~>");
        assert_eq!(encode(Encoder::ascii85(), b"hell"), b"BOu!r~>");
        assert_eq!(encode(Encoder::ascii85(), b"h"), b"BE~>");
        assert_eq!(encode(Encoder::ascii85(), b""), b"~>");
        assert_eq!(encode(Encoder::ascii85(), &[0, 0, 0, 0, 1]), b"z!<~>");
        // Three zero bytes at the end are a partial group, not `z`.
        assert_eq!(encode(Encoder::ascii85(), &[0, 0, 0]), b"!!!!~>");
        let long = encode(Encoder::ascii85(), &[b'x'; 100]);
        assert_eq!(long.iter().filter(|&&b| b == b'\n').count(), 1);
        assert_eq!(long.iter().position(|&b| b == b'\n'), Some(75));
        assert_eq!(decode(Decoder::ascii85(), &long), [b'x'; 100]);
        assert_eq!(
            decode(Decoder::ascii85(), &encode(Encoder::ascii85(), FOX)),
            FOX
        );
    }

    #[test]
    fn run_lengths_split_repeats_from_literals() {
        assert_eq!(
            encode(Encoder::run_length(0), b"aaaabcd"),
            [253, b'a', 2, b'b', b'c', b'd', 128]
        );
        assert_eq!(
            encode(Encoder::run_length(0), b"abbc"),
            [0, b'a', 255, b'b', 0, b'c', 128]
        );
        assert_eq!(encode(Encoder::run_length(0), b""), [128]);
        assert_eq!(encode(Encoder::run_length(0), b"x"), [0, b'x', 128]);
        // Runs and literals are capped at 128 bytes each.
        let long = encode(Encoder::run_length(0), &[7; 300]);
        assert_eq!(long, [129, 7, 129, 7, 213, 7, 128]);
        let distinct: Vec<u8> = (0..=255).collect();
        let coded = encode(Encoder::run_length(0), &distinct);
        assert_eq!(coded[0], 127);
        assert_eq!(coded[129], 127);
        assert_eq!(decode(Decoder::run_length(), &coded), distinct);
        assert_eq!(
            decode(Decoder::run_length(), &encode(Encoder::run_length(0), FOX)),
            FOX
        );
        // Records of four: no run crosses a record boundary, whether the
        // bytes arrive one at a time or in one write.
        let records = [
            253, b'a', 253, b'a', 255, b'a', 1, b'b', b'c', 3, b'd', b'e', b'f', b'g', 255, b'g',
            128,
        ];
        assert_eq!(
            encode(Encoder::run_length(4), b"aaaaaaaaaabcdefggg"),
            records
        );
        let mut e = Encoder::run_length(4);
        let mut out = Vec::new();
        e.push(b"aaaaaaaaaabcdefggg", &mut out);
        e.finish(&mut out);
        assert_eq!(out, records);
        // A flush emits the decided runs without the end byte.
        let mut e = Encoder::run_length(0);
        let mut out = Vec::new();
        e.push(b"aab", &mut out);
        assert_eq!(out, [255, b'a']);
        e.flush(&mut out);
        assert_eq!(out, [255, b'a', 0, b'b']);
        e.push(b"c", &mut out);
        e.finish(&mut out);
        assert_eq!(out, [255, b'a', 0, b'b', 0, b'c', 128]);
    }

    #[test]
    fn flate_lzw_and_null_round_trip() {
        let z = encode(Encoder::flate(), FOX);
        assert_eq!(z, efterscript_codec::deflate::compress(FOX));
        assert_eq!(decode(Decoder::flate(None), &z), FOX);
        for early in [true, false] {
            let l = encode(Encoder::lzw(early), FOX);
            assert_eq!(l, efterscript_codec::lzw::encode(FOX, early));
            assert_eq!(decode(Decoder::lzw(early, None), &l), FOX);
        }
        assert_eq!(encode(Encoder::Null, FOX), FOX);
        // Flushing a buffering encoder emits nothing.
        let mut e = Encoder::flate();
        let mut out = Vec::new();
        e.push(FOX, &mut out);
        e.flush(&mut out);
        assert!(out.is_empty());
    }
}
