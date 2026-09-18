// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Inflate: the RFC 1950 container around RFC 1951 stored, fixed-Huffman,
//! and dynamic-Huffman blocks, decoded one input byte at a time. The
//! [`Inflater`] is a state machine that takes a byte, appends whatever it
//! can decode to an output buffer, and reports when the stream has ended
//! at its Adler-32 trailer, so a caller that pulls input from a source
//! that may run dry can stop at any byte and continue later. Code words
//! are read bit by bit through the canonical code's counts (RFC 1951
//! §3.2.2), so a symbol can straddle any number of input bytes.

use std::fmt;

/// The largest back-reference distance, and the window kept for it.
pub const WINDOW: usize = 32_768;
const MAX_BITS: usize = 15;
const ADLER_MOD: u32 = 65_521;

/// Smallest length of each length code 257..=285 (RFC 1951 §3.2.5).
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// Smallest distance of each distance code 0..=29.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
/// The code-length alphabet's transmission order (RFC 1951 §3.2.7).
const CL_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

/// Why a stream could not be inflated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The two-byte header does not describe a deflate stream with a
    /// window of at most 32 KB and no preset dictionary.
    Header,
    /// A block declares the reserved type.
    BlockType,
    /// A stored block's length and its complement disagree.
    StoredLength,
    /// A dynamic block's header or code lengths are inconsistent.
    CodeLengths,
    /// A code word no symbol has, or a symbol outside its alphabet.
    Code,
    /// A back-reference reaching before the start of the output.
    Distance,
    /// The Adler-32 trailer does not match the output.
    Checksum,
    /// The input ended before the stream did.
    Truncated,
    /// Bytes follow the trailer.
    Trailing,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::Header => "not a deflate stream",
            Error::BlockType => "reserved block type",
            Error::StoredLength => "stored block length mismatch",
            Error::CodeLengths => "inconsistent code lengths",
            Error::Code => "invalid code",
            Error::Distance => "distance before the start of the output",
            Error::Checksum => "checksum mismatch",
            Error::Truncated => "truncated stream",
            Error::Trailing => "bytes after the end of the stream",
        })
    }
}

impl std::error::Error for Error {}

/// What a pushed byte led to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The stream continues.
    More,
    /// The byte completed the trailer; nothing more is read.
    Done,
}

/// Bits taken least-significant first, as RFC 1951 §3.1.1 packs them.
#[derive(Clone, Debug, Default)]
struct Bits {
    acc: u64,
    nbits: u32,
}

impl Bits {
    fn push(&mut self, byte: u8) {
        self.acc |= u64::from(byte) << self.nbits;
        self.nbits += 8;
    }

    /// `n` bits (n ≤ 32) if that many are buffered.
    fn take(&mut self, n: u32) -> Option<u32> {
        if self.nbits < n {
            return None;
        }
        let value = (self.acc & ((1u64 << n) - 1)) as u32;
        self.acc >>= n;
        self.nbits -= n;
        Some(value)
    }

    /// Drops the bits left in the current byte.
    fn align(&mut self) {
        let drop = self.nbits % 8;
        self.acc >>= drop;
        self.nbits -= drop;
    }
}

/// A canonical Huffman code: how many codes have each length, and the
/// symbols in code order.
#[derive(Clone, Debug, Default)]
struct Table {
    counts: [u16; MAX_BITS + 1],
    symbols: Vec<u16>,
}

impl Table {
    fn from_lengths(lengths: &[u8]) -> Self {
        let mut counts = [0u16; MAX_BITS + 1];
        for &len in lengths {
            counts[usize::from(len)] += 1;
        }
        counts[0] = 0;
        let mut offsets = [0u16; MAX_BITS + 2];
        for len in 1..=MAX_BITS {
            offsets[len + 1] = offsets[len] + counts[len];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (sym, &len) in lengths.iter().enumerate() {
            if len != 0 {
                symbols[usize::from(offsets[usize::from(len)])] = sym as u16;
                offsets[usize::from(len)] += 1;
            }
        }
        Table { counts, symbols }
    }

    fn fixed() -> (Table, Table) {
        let mut lengths = [0u8; 288];
        for (sym, len) in lengths.iter_mut().enumerate() {
            *len = match sym {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            };
        }
        (
            Table::from_lengths(&lengths),
            Table::from_lengths(&[5u8; 30]),
        )
    }
}

/// A symbol being read bit by bit, so it can wait for input mid-word.
#[derive(Clone, Copy, Debug, Default)]
struct Symbol {
    code: i32,
    first: i32,
    index: usize,
    len: usize,
}

impl Symbol {
    /// Continues reading; `None` while bits are missing.
    fn read(&mut self, bits: &mut Bits, table: &Table) -> Result<Option<u16>, Error> {
        while let Some(bit) = bits.take(1) {
            self.code |= bit as i32;
            self.len += 1;
            let count = i32::from(table.counts[self.len]);
            if self.code - count < self.first {
                let at = self.index + (self.code - self.first) as usize;
                let sym = *table.symbols.get(at).ok_or(Error::Code)?;
                *self = Symbol::default();
                return Ok(Some(sym));
            }
            self.index += count as usize;
            self.first += count;
            self.first <<= 1;
            self.code <<= 1;
            if self.len == MAX_BITS {
                return Err(Error::Code);
            }
        }
        Ok(None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Header bytes seen so far.
    Header(u8),
    BlockHeader,
    /// LEN and NLEN of a stored block: the bytes seen so far.
    StoredHeader {
        got: u32,
        n: u8,
    },
    StoredData {
        remaining: u16,
    },
    /// HLIT, HDIST, HCLEN.
    DynamicHeader,
    /// The code-length code's lengths, three bits each.
    CodeLengthCode {
        at: usize,
    },
    /// The two main codes' lengths, through the code-length code.
    CodeLengths,
    /// Extra bits of a repeat symbol (16, 17, 18).
    Repeat {
        sym: u16,
    },
    /// A literal/length symbol.
    Symbol,
    LengthExtra {
        index: usize,
    },
    DistanceSymbol {
        len: usize,
    },
    DistanceExtra {
        len: usize,
        index: usize,
    },
    /// The Adler-32 trailer: the bytes seen so far.
    Trailer {
        got: u32,
        n: u8,
    },
    Done,
}

/// A streaming inflater: see the module documentation.
#[derive(Clone, Debug)]
pub struct Inflater {
    bits: Bits,
    state: State,
    /// Whether the current block is the last.
    last: bool,
    symbol: Symbol,
    lit: Table,
    dist: Table,
    window: Vec<u8>,
    written: usize,
    adler_a: u32,
    adler_b: u32,
    /// The type of every block met, in order: 0 stored, 1 fixed, 2 dynamic.
    kinds: Vec<u8>,
    hlit: usize,
    hdist: usize,
    hclen: usize,
    cl_lengths: [u8; 19],
    cl_table: Table,
    lengths: Vec<u8>,
}

impl Default for Inflater {
    fn default() -> Self {
        Self::new()
    }
}

impl Inflater {
    pub fn new() -> Self {
        Inflater {
            bits: Bits::default(),
            state: State::Header(0),
            last: false,
            symbol: Symbol::default(),
            lit: Table::default(),
            dist: Table::default(),
            window: Vec::new(),
            written: 0,
            adler_a: 1,
            adler_b: 0,
            kinds: Vec::new(),
            hlit: 0,
            hdist: 0,
            hclen: 0,
            cl_lengths: [0; 19],
            cl_table: Table::default(),
            lengths: Vec::new(),
        }
    }

    /// Whether the trailer has been read: the stream is complete.
    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    /// Whether every block has been decoded, so only the trailer, if
    /// any, remains.
    pub fn blocks_done(&self) -> bool {
        matches!(self.state, State::Trailer { .. } | State::Done)
    }

    /// The type of every block met so far, in order: 0 stored, 1 fixed,
    /// 2 dynamic.
    pub fn block_kinds(&self) -> &[u8] {
        &self.kinds
    }

    /// Feeds one input byte, appending the bytes it completes to `out`.
    /// A byte after the trailer is [`Error::Trailing`].
    pub fn push(&mut self, byte: u8, out: &mut Vec<u8>) -> Result<Status, Error> {
        if self.state == State::Done {
            return Err(Error::Trailing);
        }
        if let State::Header(n) = self.state {
            return self.header_byte(n, byte);
        }
        self.bits.push(byte);
        while self.step(out)? {}
        Ok(if self.state == State::Done {
            Status::Done
        } else {
            Status::More
        })
    }

    fn header_byte(&mut self, n: u8, byte: u8) -> Result<Status, Error> {
        if n == 0 {
            if byte & 0x0F != 8 || byte >> 4 > 7 {
                return Err(Error::Header);
            }
            // The first byte is kept in the accumulator until the pair
            // can be checked; it is never read as bits.
            self.bits.acc = u64::from(byte);
            self.state = State::Header(1);
            return Ok(Status::More);
        }
        let cmf = self.bits.acc as u32;
        self.bits.acc = 0;
        if !(cmf * 256 + u32::from(byte)).is_multiple_of(31) || byte & 0x20 != 0 {
            return Err(Error::Header);
        }
        self.state = State::BlockHeader;
        Ok(Status::More)
    }

    fn emit(&mut self, byte: u8, out: &mut Vec<u8>) {
        if self.window.is_empty() {
            self.window = vec![0; WINDOW];
        }
        self.window[self.written % WINDOW] = byte;
        self.written += 1;
        self.adler_a += u32::from(byte);
        if self.adler_a >= ADLER_MOD {
            self.adler_a -= ADLER_MOD;
        }
        self.adler_b += self.adler_a;
        if self.adler_b >= ADLER_MOD {
            self.adler_b -= ADLER_MOD;
        }
        out.push(byte);
    }

    /// Copies `len` bytes from `distance` back, byte by byte so an
    /// overlapping reference repeats as the standard intends.
    fn copy(&mut self, len: usize, distance: usize, out: &mut Vec<u8>) -> Result<(), Error> {
        if distance > self.written || distance == 0 {
            return Err(Error::Distance);
        }
        for _ in 0..len {
            let byte = self.window[(self.written - distance) % WINDOW];
            self.emit(byte, out);
        }
        Ok(())
    }

    /// One unit of work; `Ok(false)` when bits are missing or the stream
    /// is done.
    fn step(&mut self, out: &mut Vec<u8>) -> Result<bool, Error> {
        match self.state {
            State::Header(_) | State::Done => Ok(false),
            State::BlockHeader => {
                let Some(header) = self.bits.take(3) else {
                    return Ok(false);
                };
                self.last = header & 1 == 1;
                let kind = (header >> 1) as u8;
                self.kinds.push(kind);
                self.state = match kind {
                    0 => {
                        self.bits.align();
                        State::StoredHeader { got: 0, n: 0 }
                    }
                    1 => {
                        let (lit, dist) = Table::fixed();
                        self.lit = lit;
                        self.dist = dist;
                        self.symbol = Symbol::default();
                        State::Symbol
                    }
                    2 => State::DynamicHeader,
                    _ => return Err(Error::BlockType),
                };
                Ok(true)
            }
            State::StoredHeader { got, n } => {
                let Some(byte) = self.bits.take(8) else {
                    return Ok(false);
                };
                let got = got | byte << (8 * n);
                if n < 3 {
                    self.state = State::StoredHeader { got, n: n + 1 };
                    return Ok(true);
                }
                let len = (got & 0xFFFF) as u16;
                let nlen = (got >> 16) as u16;
                if nlen != !len {
                    return Err(Error::StoredLength);
                }
                self.state = State::StoredData { remaining: len };
                Ok(true)
            }
            State::StoredData { remaining } => {
                if remaining == 0 {
                    self.state = self.after_block();
                    return Ok(true);
                }
                let Some(byte) = self.bits.take(8) else {
                    return Ok(false);
                };
                self.emit(byte as u8, out);
                self.state = State::StoredData {
                    remaining: remaining - 1,
                };
                Ok(true)
            }
            State::DynamicHeader => {
                let Some(fields) = self.bits.take(14) else {
                    return Ok(false);
                };
                self.hlit = (fields & 0x1F) as usize + 257;
                self.hdist = ((fields >> 5) & 0x1F) as usize + 1;
                self.hclen = (fields >> 10) as usize + 4;
                if self.hlit > 286 || self.hdist > 30 {
                    return Err(Error::CodeLengths);
                }
                self.cl_lengths = [0; 19];
                self.state = State::CodeLengthCode { at: 0 };
                Ok(true)
            }
            State::CodeLengthCode { at } => {
                if at == self.hclen {
                    self.cl_table = Table::from_lengths(&self.cl_lengths);
                    self.lengths.clear();
                    self.symbol = Symbol::default();
                    self.state = State::CodeLengths;
                    return Ok(true);
                }
                let Some(len) = self.bits.take(3) else {
                    return Ok(false);
                };
                self.cl_lengths[CL_ORDER[at]] = len as u8;
                self.state = State::CodeLengthCode { at: at + 1 };
                Ok(true)
            }
            State::CodeLengths => {
                if self.lengths.len() == self.hlit + self.hdist {
                    if self.lengths[256] == 0 {
                        return Err(Error::CodeLengths);
                    }
                    self.lit = Table::from_lengths(&self.lengths[..self.hlit]);
                    self.dist = Table::from_lengths(&self.lengths[self.hlit..]);
                    self.symbol = Symbol::default();
                    self.state = State::Symbol;
                    return Ok(true);
                }
                let Some(sym) = self.symbol.read(&mut self.bits, &self.cl_table)? else {
                    return Ok(false);
                };
                match sym {
                    0..=15 => self.lengths.push(sym as u8),
                    16..=18 => self.state = State::Repeat { sym },
                    _ => return Err(Error::Code),
                }
                Ok(true)
            }
            State::Repeat { sym } => {
                let (extra, base) = match sym {
                    16 => (2, 3),
                    17 => (3, 3),
                    _ => (7, 11),
                };
                let Some(more) = self.bits.take(extra) else {
                    return Ok(false);
                };
                let times = base + more as usize;
                let value = if sym == 16 {
                    *self.lengths.last().ok_or(Error::CodeLengths)?
                } else {
                    0
                };
                if self.lengths.len() + times > self.hlit + self.hdist {
                    return Err(Error::CodeLengths);
                }
                self.lengths.extend(std::iter::repeat_n(value, times));
                self.state = State::CodeLengths;
                Ok(true)
            }
            State::Symbol => {
                let Some(sym) = self.symbol.read(&mut self.bits, &self.lit)? else {
                    return Ok(false);
                };
                match sym {
                    0..=255 => self.emit(sym as u8, out),
                    256 => self.state = self.after_block(),
                    257..=285 => {
                        self.state = State::LengthExtra {
                            index: usize::from(sym - 257),
                        };
                    }
                    _ => return Err(Error::Code),
                }
                Ok(true)
            }
            State::LengthExtra { index } => {
                let Some(extra) = self.bits.take(LENGTH_EXTRA[index]) else {
                    return Ok(false);
                };
                let len = usize::from(LENGTH_BASE[index]) + extra as usize;
                self.symbol = Symbol::default();
                self.state = State::DistanceSymbol { len };
                Ok(true)
            }
            State::DistanceSymbol { len } => {
                let Some(sym) = self.symbol.read(&mut self.bits, &self.dist)? else {
                    return Ok(false);
                };
                let index = usize::from(sym);
                if index >= DIST_BASE.len() {
                    return Err(Error::Code);
                }
                self.state = State::DistanceExtra { len, index };
                Ok(true)
            }
            State::DistanceExtra { len, index } => {
                let Some(extra) = self.bits.take(DIST_EXTRA[index]) else {
                    return Ok(false);
                };
                let distance = usize::from(DIST_BASE[index]) + extra as usize;
                self.copy(len, distance, out)?;
                self.symbol = Symbol::default();
                self.state = State::Symbol;
                Ok(true)
            }
            State::Trailer { got, n } => {
                let Some(byte) = self.bits.take(8) else {
                    return Ok(false);
                };
                let got = got << 8 | byte;
                if n < 3 {
                    self.state = State::Trailer { got, n: n + 1 };
                    return Ok(true);
                }
                if got != self.adler_b << 16 | self.adler_a {
                    return Err(Error::Checksum);
                }
                self.state = State::Done;
                Ok(false)
            }
        }
    }

    /// The state after a block ends: the next block, or the trailer
    /// after the last one.
    fn after_block(&mut self) -> State {
        if self.last {
            self.bits.align();
            State::Trailer { got: 0, n: 0 }
        } else {
            State::BlockHeader
        }
    }
}

/// Inflates a complete stream, verifying framing and checksum; bytes
/// after the trailer are an error, as is a stream that ends early.
pub fn inflate(z: &[u8]) -> Result<Vec<u8>, Error> {
    inflate_with_kinds(z).map(|(out, _)| out)
}

/// The type of every block in `z`, in order: 0 stored, 1 fixed, 2
/// dynamic. The stream is fully inflated and verified on the way.
pub fn block_kinds(z: &[u8]) -> Result<Vec<u8>, Error> {
    inflate_with_kinds(z).map(|(_, kinds)| kinds)
}

fn inflate_with_kinds(z: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let mut inflater = Inflater::new();
    let mut out = Vec::with_capacity(z.len() * 2);
    for &byte in z {
        inflater.push(byte, &mut out)?;
    }
    if !inflater.is_done() {
        return Err(Error::Truncated);
    }
    Ok((out, inflater.kinds))
}

/// Reads `count` symbols of the canonical code for `lengths` from the
/// start of `bytes`, exactly as a coded block would read them.
pub fn decode_symbols(lengths: &[u8], bytes: &[u8], count: usize) -> Result<Vec<u16>, Error> {
    let table = Table::from_lengths(lengths);
    let mut bits = Bits::default();
    let mut symbol = Symbol::default();
    let mut input = bytes.iter();
    let mut symbols = Vec::with_capacity(count);
    while symbols.len() < count {
        match symbol.read(&mut bits, &table)? {
            Some(sym) => symbols.push(sym),
            None => bits.push(*input.next().ok_or(Error::Truncated)?),
        }
    }
    Ok(symbols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deflate::{adler32, compress};

    /// The stream fed one byte at a time, the output collected as it comes.
    fn streamed(z: &[u8]) -> Result<Vec<u8>, Error> {
        let mut inflater = Inflater::new();
        let mut out = Vec::new();
        let mut done = false;
        for &byte in z {
            assert!(!done, "bytes after the trailer");
            done = inflater.push(byte, &mut out)? == Status::Done;
        }
        assert!(done, "stream did not end");
        assert!(inflater.is_done());
        Ok(out)
    }

    #[test]
    fn an_empty_stream_is_a_lone_final_block() {
        let z = [0x78, 0x01, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01];
        assert_eq!(inflate(&z), Ok(Vec::new()));
        assert_eq!(block_kinds(&z), Ok(vec![1]));
        assert_eq!(streamed(&z), Ok(Vec::new()));
    }

    #[test]
    fn a_stored_block_is_copied_through() {
        // A single final stored block holding "abc", then the checksum.
        let mut z = vec![0x78, 0x01, 0x01, 0x03, 0x00, 0xFC, 0xFF];
        z.extend_from_slice(b"abc");
        z.extend_from_slice(&adler32(b"abc").to_be_bytes());
        assert_eq!(inflate(&z), Ok(b"abc".to_vec()));
        assert_eq!(block_kinds(&z), Ok(vec![0]));
        assert_eq!(streamed(&z), Ok(b"abc".to_vec()));
        z[5] = 0xFD;
        assert_eq!(inflate(&z), Err(Error::StoredLength));
    }

    #[test]
    fn the_encoders_output_inflates_whole_and_byte_by_byte() {
        for input in [
            &b""[..],
            b"a",
            b"abcabcabcabcabcabc",
            b"hello hello hello hello",
            &[0u8; 5000],
            &(0..70_000u32).map(|i| (i % 251) as u8).collect::<Vec<u8>>(),
        ] {
            let z = compress(input);
            assert_eq!(inflate(&z).as_deref(), Ok(input));
            assert_eq!(streamed(&z).as_deref(), Ok(input));
        }
    }

    #[test]
    fn a_back_reference_spanning_the_whole_window_resolves() {
        let phrase: Vec<u8> = (0..300u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut data = phrase.clone();
        let mut state = 1u32;
        while data.len() < WINDOW {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            data.push((state >> 24) as u8);
        }
        data.extend_from_slice(&phrase);
        assert_eq!(streamed(&compress(&data)), Ok(data));
    }

    #[test]
    fn framing_errors_are_reported() {
        assert_eq!(inflate(&[0x79, 0x01]), Err(Error::Header));
        assert_eq!(inflate(&[0x78, 0x02]), Err(Error::Header));
        assert_eq!(inflate(&[0x78, 0x21]), Err(Error::Header));
        assert_eq!(inflate(&[0x78, 0x01, 0x07]), Err(Error::BlockType));
        assert_eq!(inflate(&[0x78, 0x01]), Err(Error::Truncated));
        let mut z = compress(b"hello");
        let last = z.len() - 1;
        z[last] ^= 1;
        assert_eq!(inflate(&z), Err(Error::Checksum));
        z[last] ^= 1;
        z.push(0);
        assert_eq!(inflate(&z), Err(Error::Trailing));
    }

    #[test]
    fn a_distance_before_the_output_is_an_error() {
        // Final fixed block: one match of length 3 at distance 1 with
        // nothing written yet.
        let mut z = vec![0x78, 0x01];
        // BFINAL=1, BTYPE=01 → bits 1,1,0; length code 257 (7 bits
        // 0000001), distance code 0 (5 bits 00000).
        let mut acc: u32 = 0;
        let mut n = 0;
        let mut put = |value: u32, bits: u32| {
            acc |= value << n;
            n += bits;
        };
        put(1, 1);
        put(1, 2);
        put(0b1000000, 7);
        put(0, 5);
        z.extend_from_slice(&acc.to_le_bytes()[..2]);
        assert_eq!(inflate(&z), Err(Error::Distance));
    }

    #[test]
    fn symbols_decode_through_the_canonical_code() {
        // The worked example of RFC 1951 §3.2.2: lengths 3 3 3 3 3 2 4 4
        // give codes 010 011 100 101 110 00 1110 1111, packed reversed.
        let lengths = [3u8, 3, 3, 3, 3, 2, 4, 4];
        let mut acc: u32 = 0;
        let mut n = 0;
        for (code, len) in [(0b010u32, 3), (0b00, 2), (0b1111, 4), (0b110, 3)] {
            let reversed = code.reverse_bits() >> (32 - len);
            acc |= reversed << n;
            n += len;
        }
        let bytes = acc.to_le_bytes();
        assert_eq!(decode_symbols(&lengths, &bytes, 4), Ok(vec![0, 5, 7, 4]));
        assert_eq!(
            decode_symbols(&lengths, &bytes[..1], 4),
            Err(Error::Truncated)
        );
    }

    #[test]
    fn a_stream_with_a_missing_end_of_block_code_is_rejected() {
        // Dynamic block header declaring HLIT=257, HDIST=1, HCLEN=4 with
        // every code length zero: no end-of-block code.
        let mut acc: u64 = 0;
        let mut n = 0;
        let mut put = |value: u64, bits: u32| {
            acc |= value << n;
            n += bits;
        };
        put(1, 1);
        put(2, 2);
        put(0, 5);
        put(0, 5);
        put(0, 4);
        // Code-length code: symbol 16 length 0, 17 length 0, 18 length 1,
        // 0 length 1: 18 codes zeros in runs of 138.
        put(0, 3);
        put(0, 3);
        put(1, 3);
        put(1, 3);
        // 258 lengths: symbol 18 (code 1) with 127 extra → 138 zeros,
        // symbol 18 with 109 extra → 120 zeros.
        put(1, 1);
        put(127, 7);
        put(1, 1);
        put(109, 7);
        let mut z = vec![0x78, 0x01];
        z.extend_from_slice(&acc.to_le_bytes()[..6]);
        assert_eq!(inflate(&z), Err(Error::CodeLengths));
    }
}
