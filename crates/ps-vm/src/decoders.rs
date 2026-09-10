// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The decoders behind a decode-filter file and the `eexec` layer. Each
//! is a state machine fed one base byte at a time by the file table's
//! layer entry (see `files.rs`): it appends whatever the byte completes
//! to an output buffer and says whether the data has ended. Holding
//! partial state instead of reading ahead is what lets a layer over a
//! source that may run dry stop at any byte and continue later, and what
//! lets the entry's snapshot and rollback cover a filter as they cover
//! the cipher.
//!
//! The formats are the published ones (PLRM3 §3.13.3): hexadecimal ended
//! by `>`; base-85 in groups of five with `z` for four zero bytes, ended
//! by `~>`; run lengths with 128 as the end; the RFC 1950 container for
//! Flate and the 9- to 12-bit LZW variant, both optionally followed by a
//! row predictor; a sub-file bounded by a count and a string.

use std::collections::VecDeque;

use codec::inflate::Inflater;
use codec::predictor::Predictor;
use ps_fonts::type1::{Decryptor, EEXEC_KEY, hex_value};

use crate::error::VmError;

/// What feeding a byte to a decoder led to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fed {
    /// The byte was taken; more may follow.
    More,
    /// The byte completed the end-of-data marker.
    End,
    /// The data ended before this byte: it, and every byte fed since the
    /// last decoded byte came out, belongs to the base.
    EndBefore,
}

/// A decoding state machine; see the module documentation.
#[derive(Clone, Debug)]
pub(crate) enum Decoder {
    Eexec(Eexec),
    AsciiHex(AsciiHex),
    Ascii85(Ascii85),
    RunLength(RunLength),
    Flate(Box<Flate>),
    Lzw(Box<Lzw>),
    SubFile(SubFile),
    /// Recognised so an image source can be detected, never decoded:
    /// a read through it is `undefined`.
    Dct,
}

impl Decoder {
    pub(crate) fn eexec() -> Self {
        Decoder::Eexec(Eexec {
            cipher: Decryptor::new(EEXEC_KEY),
            form: None,
            first: Vec::new(),
            skip: 4,
            high: None,
        })
    }

    pub(crate) fn ascii_hex() -> Self {
        Decoder::AsciiHex(AsciiHex { high: None })
    }

    pub(crate) fn ascii85() -> Self {
        Decoder::Ascii85(Ascii85 {
            group: [0; 5],
            count: 0,
            tilde: false,
        })
    }

    pub(crate) fn run_length() -> Self {
        Decoder::RunLength(RunLength::Start)
    }

    pub(crate) fn flate(predictor: Option<Predictor>) -> Self {
        Decoder::Flate(Box::new(Flate {
            inflater: Inflater::new(),
            predictor,
            scratch: Vec::new(),
        }))
    }

    pub(crate) fn lzw(early_change: bool, predictor: Option<Predictor>) -> Self {
        Decoder::Lzw(Box::new(Lzw {
            decoder: codec::lzw::Decoder::new(early_change),
            predictor,
            scratch: Vec::new(),
        }))
    }

    /// `count` and `pattern` as `EODCount` and `EODString`: an empty
    /// pattern passes `count` bytes (every byte when the count is 0); a
    /// pattern ends the data at its first occurrence, which is not
    /// passed, when the count is 0, and after its `count`th occurrence,
    /// which is, otherwise.
    pub(crate) fn sub_file(count: usize, pattern: Vec<u8>) -> Self {
        let failure = kmp_failure(&pattern);
        Decoder::SubFile(SubFile {
            remaining: count,
            pattern,
            failure,
            matched: 0,
            held: VecDeque::new(),
        })
    }

    /// Feeds one base byte; the bytes it completes go to `out`.
    /// Malformed data is `ioerror`; a read through the DCT decoder is
    /// `undefined`.
    pub(crate) fn push(&mut self, byte: u8, out: &mut Vec<u8>) -> Result<Fed, VmError> {
        match self {
            Decoder::Eexec(d) => Ok(d.push(byte, out)),
            Decoder::AsciiHex(d) => d.push(byte, out),
            Decoder::Ascii85(d) => d.push(byte, out),
            Decoder::RunLength(d) => Ok(d.push(byte, out)),
            Decoder::Flate(d) => d.push(byte, out),
            Decoder::Lzw(d) => d.push(byte, out),
            Decoder::SubFile(d) => Ok(d.push(byte, out)),
            Decoder::Dct => Err(VmError::Undefined),
        }
    }

    /// The base ended: whatever a partial unit still yields goes to
    /// `out`.
    pub(crate) fn finish(&mut self, out: &mut Vec<u8>) -> Result<(), VmError> {
        match self {
            Decoder::Eexec(d) => d.finish(out),
            Decoder::AsciiHex(d) => d.finish(out),
            Decoder::Ascii85(d) => d.finish(out)?,
            Decoder::RunLength(_) => {}
            Decoder::Flate(d) => d.finish(out),
            Decoder::Lzw(d) => d.finish(out),
            Decoder::SubFile(d) => d.finish(out),
            Decoder::Dct => {}
        }
        Ok(())
    }

    /// Whether the base bytes behind a decoded byte the reader peeked
    /// but did not take go back to the base when the layer closes: the
    /// cipher's convention, so a section's trailer follows exactly.
    pub(crate) fn returns_lookahead(&self) -> bool {
        matches!(self, Decoder::Eexec(_))
    }

    pub(crate) fn is_dct(&self) -> bool {
        matches!(self, Decoder::Dct)
    }

    /// Whether the data ends at a marker of the decoder's own that a
    /// close consumes, so the base continues after it. The cipher, the
    /// DCT placeholder, and a sub-file bounded only by the base's end
    /// have none.
    pub(crate) fn has_marker(&self) -> bool {
        match self {
            Decoder::Eexec(_) | Decoder::Dct => false,
            Decoder::SubFile(d) => !(d.pattern.is_empty() && d.remaining == 0),
            _ => true,
        }
    }
}

/// The form of an `eexec` section, decided from its first four bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    Hex,
    Binary,
}

#[derive(Clone, Debug)]
pub(crate) struct Eexec {
    cipher: Decryptor,
    form: Option<Form>,
    /// The bytes held until the form is decided.
    first: Vec<u8>,
    /// Leading plain bytes still to discard.
    skip: u8,
    high: Option<u8>,
}

impl Eexec {
    fn plain(&mut self, cipher_byte: u8, out: &mut Vec<u8>) {
        let plain = self.cipher.byte(cipher_byte);
        if self.skip > 0 {
            self.skip -= 1;
        } else {
            out.push(plain);
        }
    }

    fn push(&mut self, byte: u8, out: &mut Vec<u8>) -> Fed {
        let Some(form) = self.form else {
            self.first.push(byte);
            if self.first.len() == 4 {
                let form = if self.first.iter().all(|&b| hex_value(b).is_some()) {
                    Form::Hex
                } else {
                    Form::Binary
                };
                self.decide(form, out);
            }
            return Fed::More;
        };
        match form {
            Form::Binary => {
                self.plain(byte, out);
                Fed::More
            }
            Form::Hex => match hex_value(byte) {
                Some(v) => {
                    match self.high.take() {
                        None => self.high = Some(v),
                        Some(h) => self.plain(h << 4 | v, out),
                    }
                    Fed::More
                }
                None if byte.is_ascii_whitespace() => Fed::More,
                // The section ends at the first byte that is neither.
                None => Fed::EndBefore,
            },
        }
    }

    fn decide(&mut self, form: Form, out: &mut Vec<u8>) {
        self.form = Some(form);
        for byte in std::mem::take(&mut self.first) {
            self.push(byte, out);
        }
    }

    /// Fewer than four bytes in all is a binary section.
    fn finish(&mut self, out: &mut Vec<u8>) {
        if self.form.is_none() {
            self.decide(Form::Binary, out);
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AsciiHex {
    high: Option<u8>,
}

impl AsciiHex {
    fn push(&mut self, byte: u8, out: &mut Vec<u8>) -> Result<Fed, VmError> {
        match hex_value(byte) {
            Some(v) => {
                match self.high.take() {
                    None => self.high = Some(v),
                    Some(h) => out.push(h << 4 | v),
                }
                Ok(Fed::More)
            }
            None if byte == b'>' => {
                self.finish(out);
                Ok(Fed::End)
            }
            None if byte.is_ascii_whitespace() => Ok(Fed::More),
            None => Err(VmError::IoError),
        }
    }

    /// An odd final digit is the high half of a byte.
    fn finish(&mut self, out: &mut Vec<u8>) {
        if let Some(h) = self.high.take() {
            out.push(h << 4);
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Ascii85 {
    group: [u8; 5],
    count: usize,
    /// A `~` has been seen; only `>` may follow.
    tilde: bool,
}

impl Ascii85 {
    fn push(&mut self, byte: u8, out: &mut Vec<u8>) -> Result<Fed, VmError> {
        if self.tilde {
            return if byte == b'>' {
                Ok(Fed::End)
            } else {
                Err(VmError::IoError)
            };
        }
        match byte {
            b'!'..=b'u' => {
                self.group[self.count] = byte - b'!';
                self.count += 1;
                if self.count == 5 {
                    self.flush(out)?;
                }
                Ok(Fed::More)
            }
            b'z' if self.count == 0 => {
                out.extend_from_slice(&[0; 4]);
                Ok(Fed::More)
            }
            b'~' => {
                self.finish(out)?;
                self.tilde = true;
                Ok(Fed::More)
            }
            _ if byte.is_ascii_whitespace() => Ok(Fed::More),
            _ => Err(VmError::IoError),
        }
    }

    /// Decodes the group held: `count` digits give `count - 1` bytes,
    /// the missing digits taken as the largest.
    fn flush(&mut self, out: &mut Vec<u8>) -> Result<(), VmError> {
        let count = std::mem::take(&mut self.count);
        let mut value: u64 = 0;
        for (i, &digit) in self.group.iter().enumerate() {
            value = value * 85 + u64::from(if i < count { digit } else { 84 });
        }
        let value = u32::try_from(value).map_err(|_| VmError::IoError)?;
        out.extend_from_slice(&value.to_be_bytes()[..count - 1]);
        Ok(())
    }

    /// A single leftover digit cannot make a byte.
    fn finish(&mut self, out: &mut Vec<u8>) -> Result<(), VmError> {
        match self.count {
            0 => Ok(()),
            1 => Err(VmError::IoError),
            _ => self.flush(out),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum RunLength {
    /// Expecting a length byte.
    Start,
    /// Copying `remaining` bytes as they are.
    Literal(u8),
    /// The next byte repeats `count` times.
    Repeat(u16),
}

impl RunLength {
    fn push(&mut self, byte: u8, out: &mut Vec<u8>) -> Fed {
        match *self {
            RunLength::Start => match byte {
                128 => return Fed::End,
                0..=127 => *self = RunLength::Literal(byte + 1),
                _ => *self = RunLength::Repeat(257 - u16::from(byte)),
            },
            RunLength::Literal(remaining) => {
                out.push(byte);
                *self = if remaining > 1 {
                    RunLength::Literal(remaining - 1)
                } else {
                    RunLength::Start
                };
            }
            RunLength::Repeat(count) => {
                out.extend(std::iter::repeat_n(byte, usize::from(count)));
                *self = RunLength::Start;
            }
        }
        Fed::More
    }
}

/// Routes decompressed bytes through the predictor, when there is one.
fn predicted(predictor: &mut Option<Predictor>, scratch: &mut Vec<u8>, out: &mut Vec<u8>) {
    match predictor {
        Some(p) => {
            for byte in scratch.drain(..) {
                p.push(byte, out);
            }
        }
        None => out.append(scratch),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Flate {
    inflater: Inflater,
    predictor: Option<Predictor>,
    scratch: Vec<u8>,
}

impl Flate {
    fn push(&mut self, byte: u8, out: &mut Vec<u8>) -> Result<Fed, VmError> {
        let status = self
            .inflater
            .push(byte, &mut self.scratch)
            .map_err(|_| VmError::IoError)?;
        predicted(&mut self.predictor, &mut self.scratch, out);
        Ok(match status {
            codec::inflate::Status::More => Fed::More,
            codec::inflate::Status::Done => {
                self.finish(out);
                Fed::End
            }
        })
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        if let Some(p) = &mut self.predictor {
            p.flush(out);
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Lzw {
    decoder: codec::lzw::Decoder,
    predictor: Option<Predictor>,
    scratch: Vec<u8>,
}

impl Lzw {
    fn push(&mut self, byte: u8, out: &mut Vec<u8>) -> Result<Fed, VmError> {
        let status = self
            .decoder
            .push(byte, &mut self.scratch)
            .map_err(|_| VmError::IoError)?;
        predicted(&mut self.predictor, &mut self.scratch, out);
        Ok(match status {
            codec::lzw::Status::More => Fed::More,
            codec::lzw::Status::Done => {
                self.finish(out);
                Fed::End
            }
        })
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        if let Some(p) = &mut self.predictor {
            p.flush(out);
        }
    }
}

/// The failure function of the pattern: for each prefix length, the
/// length of the longest proper prefix that is also its suffix.
fn kmp_failure(pattern: &[u8]) -> Vec<usize> {
    let mut failure = vec![0; pattern.len()];
    let mut k = 0;
    for i in 1..pattern.len() {
        while k > 0 && pattern[i] != pattern[k] {
            k = failure[k - 1];
        }
        if pattern[i] == pattern[k] {
            k += 1;
        }
        failure[i] = k;
    }
    failure
}

#[derive(Clone, Debug)]
pub(crate) struct SubFile {
    /// Occurrences still to pass, or bytes still to pass for an empty
    /// pattern; 0 means "until the first occurrence" or "everything".
    remaining: usize,
    pattern: Vec<u8>,
    failure: Vec<usize>,
    /// How much of the pattern the latest input matches.
    matched: usize,
    /// The bytes that may yet turn out to be an occurrence: exactly the
    /// `matched` latest ones.
    held: VecDeque<u8>,
}

impl SubFile {
    fn push(&mut self, byte: u8, out: &mut Vec<u8>) -> Fed {
        if self.pattern.is_empty() {
            out.push(byte);
            if self.remaining == 0 {
                return Fed::More;
            }
            self.remaining -= 1;
            return if self.remaining == 0 {
                Fed::End
            } else {
                Fed::More
            };
        }
        while self.matched > 0 && self.pattern[self.matched] != byte {
            self.matched = self.failure[self.matched - 1];
        }
        if self.pattern[self.matched] == byte {
            self.matched += 1;
        }
        self.held.push_back(byte);
        while self.held.len() > self.matched {
            out.push(self.held.pop_front().expect("held is longer"));
        }
        if self.matched < self.pattern.len() {
            return Fed::More;
        }
        // An occurrence: with a count it is data and counts down,
        // without one it is the marker and is dropped.
        self.matched = 0;
        if self.remaining == 0 {
            self.held.clear();
            return Fed::End;
        }
        out.extend(self.held.drain(..));
        self.remaining -= 1;
        if self.remaining == 0 {
            Fed::End
        } else {
            Fed::More
        }
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        out.extend(self.held.drain(..));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds every byte, returning the output and where the data ended.
    fn feed(decoder: &mut Decoder, input: &[u8]) -> (Vec<u8>, Option<(usize, Fed)>) {
        let mut out = Vec::new();
        for (at, &byte) in input.iter().enumerate() {
            match decoder.push(byte, &mut out).unwrap() {
                Fed::More => {}
                fed => return (out, Some((at, fed))),
            }
        }
        decoder.finish(&mut out).unwrap();
        (out, None)
    }

    #[test]
    fn hex_decodes_and_ends_at_the_bracket() {
        let mut d = Decoder::ascii_hex();
        let (out, end) = feed(&mut d, b"48 69\n207e>tail");
        assert_eq!(out, b"Hi ~");
        assert_eq!(end, Some((10, Fed::End)));
        let mut d = Decoder::ascii_hex();
        assert_eq!(feed(&mut d, b"4"), (vec![0x40], None));
        let mut d = Decoder::ascii_hex();
        assert_eq!(feed(&mut d, b"7>"), (vec![0x70], Some((1, Fed::End))));
        let mut d = Decoder::ascii_hex();
        assert_eq!(d.push(b'g', &mut Vec::new()), Err(VmError::IoError));
        assert!(d.has_marker());
    }

    #[test]
    fn base85_decodes_groups_partials_and_z() {
        let mut d = Decoder::ascii85();
        let (out, end) = feed(&mut d, b"87cURD_*#4DfTZ)~>x");
        assert_eq!(out, b"Hello, World");
        assert_eq!(end, Some((16, Fed::End)));
        let mut d = Decoder::ascii85();
        assert_eq!(
            feed(&mut d, b"z87cURD_*#4DfTZ)"),
            ([&[0u8; 4][..], b"Hello, World"].concat(), None)
        );
        let mut d = Decoder::ascii85();
        assert_eq!(d.push(b'!', &mut Vec::new()), Ok(Fed::More));
        assert_eq!(d.push(b'z', &mut Vec::new()), Err(VmError::IoError));
        let mut d = Decoder::ascii85();
        assert_eq!(d.push(b'v', &mut Vec::new()), Err(VmError::IoError));
        let mut d = Decoder::ascii85();
        assert_eq!(d.push(b'~', &mut Vec::new()), Ok(Fed::More));
        assert_eq!(d.push(b'x', &mut Vec::new()), Err(VmError::IoError));
        // A lone digit at the end cannot make a byte.
        let mut d = Decoder::ascii85();
        assert_eq!(d.push(b'!', &mut Vec::new()), Ok(Fed::More));
        assert_eq!(d.finish(&mut Vec::new()), Err(VmError::IoError));
        // A group above 2^32 - 1.
        let mut d = Decoder::ascii85();
        let mut out = Vec::new();
        for b in b"uuuu" {
            assert_eq!(d.push(*b, &mut out), Ok(Fed::More));
        }
        assert_eq!(d.push(b'u', &mut out), Err(VmError::IoError));
    }

    #[test]
    fn run_lengths_expand_and_end_at_128() {
        let mut d = Decoder::run_length();
        let (out, end) = feed(&mut d, &[2, b'a', b'b', b'c', 254, b'x', 0, b'!', 128, 9]);
        assert_eq!(out, b"abcxxx!");
        assert_eq!(end, Some((8, Fed::End)));
        let mut d = Decoder::run_length();
        assert_eq!(feed(&mut d, &[1, b'a']), (b"a".to_vec(), None));
    }

    #[test]
    fn sub_file_by_count_and_by_string() {
        let mut d = Decoder::sub_file(0, b"*EOD*".to_vec());
        let (out, end) = feed(&mut d, b"ab*EOD*cd");
        assert_eq!(out, b"ab");
        assert_eq!(end, Some((6, Fed::End)));
        let mut d = Decoder::sub_file(2, b"--".to_vec());
        let (out, end) = feed(&mut d, b"a--b--c--");
        assert_eq!(out, b"a--b--");
        assert_eq!(end, Some((5, Fed::End)));
        let mut d = Decoder::sub_file(3, Vec::new());
        assert_eq!(
            feed(&mut d, b"abcdef"),
            (b"abc".to_vec(), Some((2, Fed::End)))
        );
        let mut d = Decoder::sub_file(0, Vec::new());
        assert_eq!(feed(&mut d, b"abc"), (b"abc".to_vec(), None));
        assert!(!d.has_marker());
        // Near misses are released as the match fails; a partial match
        // at the end is data.
        let mut d = Decoder::sub_file(0, b"aab".to_vec());
        assert_eq!(feed(&mut d, b"aaab"), (b"a".to_vec(), Some((3, Fed::End))));
        let mut d = Decoder::sub_file(0, b"xyz".to_vec());
        assert_eq!(feed(&mut d, b"axy"), (b"axy".to_vec(), None));
        assert_eq!(kmp_failure(b"aabaaab"), [0, 1, 0, 1, 2, 2, 3]);
    }

    #[test]
    fn flate_and_lzw_end_with_their_streams() {
        let z = codec::deflate::compress(b"hello");
        let mut d = Decoder::flate(None);
        let (out, end) = feed(&mut d, &[&z[..], b"rest"].concat());
        assert_eq!(out, b"hello");
        assert_eq!(end, Some((z.len() - 1, Fed::End)));
        let mut d = Decoder::flate(None);
        assert_eq!(d.push(0xFF, &mut Vec::new()), Err(VmError::IoError));
        let l = codec::lzw::encode(b"hello", true);
        let mut d = Decoder::lzw(true, None);
        let (out, end) = feed(&mut d, &[&l[..], b"rest"].concat());
        assert_eq!(out, b"hello");
        assert_eq!(end, Some((l.len() - 1, Fed::End)));
        // A predictor after the inflater.
        let rows = [1u8, 5, 5, 5, 5, 2, 1, 1, 1, 1];
        let z = codec::deflate::compress(&rows);
        let predictor = Predictor::new(12, 1, 8, 4).unwrap();
        let mut d = Decoder::flate(predictor);
        assert_eq!(feed(&mut d, &z).0, [5, 10, 15, 20, 6, 11, 16, 21]);
    }

    #[test]
    fn dct_reads_are_undefined_and_the_cipher_returns_lookahead() {
        let mut d = Decoder::Dct;
        assert!(d.is_dct());
        assert!(!d.has_marker());
        assert_eq!(d.push(0, &mut Vec::new()), Err(VmError::Undefined));
        assert!(Decoder::eexec().returns_lookahead());
        assert!(!Decoder::ascii85().returns_lookahead());
    }

    #[test]
    fn eexec_hex_ends_before_a_foreign_byte() {
        let cipher = ps_fonts::type1::encrypt(EEXEC_KEY, b"ab", 4);
        let mut text: Vec<u8> = cipher
            .iter()
            .flat_map(|b| format!("{b:02x}").into_bytes())
            .collect();
        text.extend_from_slice(b" \n)");
        let mut d = Decoder::eexec();
        let (out, end) = feed(&mut d, &text);
        assert_eq!(out, b"ab");
        assert_eq!(end, Some((text.len() - 1, Fed::EndBefore)));
        let mut d = Decoder::eexec();
        assert_eq!(feed(&mut d, b"ab"), (Vec::new(), None));
    }
}
