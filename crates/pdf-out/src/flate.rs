// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Hand-written Flate compression: the zlib container of RFC 1950 around
//! a DEFLATE stream of RFC 1951. Matching is greedy LZ77 over a 32 KB
//! window through a hash chain on three-byte prefixes; coding uses the
//! fixed Huffman tables (RFC 1951 §3.2.6); each block covers at most
//! 65 535 input bytes and is written stored instead whenever coding
//! would not make it smaller. No randomness, no threads, no lazy
//! matching: the same input gives the same bytes everywhere. Dynamic
//! Huffman tables are a recorded follow-up behind the same API.

/// CMF/FLG pair: 32K-window deflate method, no preset dictionary, and check
/// bits chosen so the big-endian pair is a multiple of 31 as RFC 1950
/// requires (0x7801 = 31 * 991).
const ZLIB_HEADER: [u8; 2] = [0x78, 0x01];

const WINDOW: usize = 32_768;
const WINDOW_MASK: usize = WINDOW - 1;
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 258;
/// A block covers at most this much input, so the stored form (16-bit
/// LEN field) is always available as the fallback.
const MAX_BLOCK: usize = 65_535;
const HASH_BITS: u32 = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
/// Candidates examined per position before the search gives up.
const CHAIN_LIMIT: usize = 64;
/// A match at least this long ends the search early.
const GOOD_ENOUGH: usize = 128;
const NIL: u32 = u32::MAX;
const END_OF_BLOCK: u16 = 256;

/// Smallest length of each length code 257..=285 (RFC 1951 §3.2.5).
const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// Smallest distance of each distance code 0..=29.
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// One LZ77 symbol of a block.
#[derive(Clone, Copy)]
enum Sym {
    Lit(u8),
    /// `len` in 3..=258 back `dist` in 1..=32768.
    Match {
        len: u16,
        dist: u16,
    },
}

/// Compresses `data` into a complete zlib stream.
pub(crate) fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = BitWriter::with_capacity(data.len() / 2 + 16);
    out.bytes(&ZLIB_HEADER);
    let mut matcher = Matcher::new();
    let mut syms = Vec::new();
    let n = data.len();
    let mut pos = 0;
    let mut first = true;
    while first || pos < n {
        first = false;
        let start = pos;
        let end = n.min(pos + MAX_BLOCK);
        syms.clear();
        while pos < end {
            let limit = MAX_MATCH.min(end - pos);
            let found = if limit >= MIN_MATCH {
                matcher.longest(data, pos, limit)
            } else {
                None
            };
            match found {
                Some((len, dist)) => {
                    syms.push(Sym::Match {
                        len: len as u16,
                        dist: dist as u16,
                    });
                    for at in pos..pos + len {
                        matcher.insert(data, at);
                    }
                    pos += len;
                }
                None => {
                    syms.push(Sym::Lit(data[pos]));
                    matcher.insert(data, pos);
                    pos += 1;
                }
            }
        }
        out.block(&data[start..end], &syms, pos == n);
    }
    out.align();
    out.bytes(&adler32(data).to_be_bytes());
    out.out
}

/// The hash chain: `head` holds the newest position of each three-byte
/// hash, `prev` the position before it, indexed modulo the window. A
/// chain is followed only while the distance stays inside the window,
/// which is exactly the span over which a slot has not been reused.
struct Matcher {
    head: Vec<u32>,
    prev: Vec<u32>,
}

impl Matcher {
    fn new() -> Self {
        Matcher {
            head: vec![NIL; HASH_SIZE],
            prev: vec![NIL; WINDOW],
        }
    }

    fn hash(data: &[u8], pos: usize) -> usize {
        let key = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], 0]);
        (key.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
    }

    /// Records `pos` as a match candidate; a position without three
    /// bytes after it can start no match and is skipped.
    fn insert(&mut self, data: &[u8], pos: usize) {
        if pos + MIN_MATCH > data.len() {
            return;
        }
        let h = Self::hash(data, pos);
        self.prev[pos & WINDOW_MASK] = self.head[h];
        self.head[h] = pos as u32;
    }

    /// The longest earlier occurrence of the bytes at `pos`, at most
    /// `limit` long (`limit` ≥ 3), as (length, distance); the newest
    /// occurrence wins a tie, so the distance is the smallest.
    fn longest(&self, data: &[u8], pos: usize, limit: usize) -> Option<(usize, usize)> {
        let mut best: Option<(usize, usize)> = None;
        let mut best_len = MIN_MATCH - 1;
        let mut next = self.head[Self::hash(data, pos)];
        let mut budget = CHAIN_LIMIT;
        while next != NIL && budget > 0 {
            let cand = next as usize;
            let dist = pos - cand;
            if dist > WINDOW {
                break;
            }
            if data[cand + best_len] == data[pos + best_len] {
                let len = data[cand..cand + limit]
                    .iter()
                    .zip(&data[pos..pos + limit])
                    .take_while(|(a, b)| a == b)
                    .count();
                if len > best_len {
                    best_len = len;
                    best = Some((len, dist));
                    if len == limit || len >= GOOD_ENOUGH {
                        break;
                    }
                }
            }
            budget -= 1;
            next = self.prev[cand & WINDOW_MASK];
        }
        best
    }
}

/// Bits go out least-significant first; Huffman codes are reversed on
/// the way in, as RFC 1951 §3.1.1 packs them.
struct BitWriter {
    out: Vec<u8>,
    acc: u64,
    nbits: u32,
}

impl BitWriter {
    fn with_capacity(capacity: usize) -> Self {
        BitWriter {
            out: Vec::with_capacity(capacity),
            acc: 0,
            nbits: 0,
        }
    }

    fn put(&mut self, value: u32, nbits: u32) {
        self.acc |= u64::from(value) << self.nbits;
        self.nbits += nbits;
        while self.nbits >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.nbits -= 8;
        }
    }

    fn huff(&mut self, code: u32, len: u32) {
        self.put(code.reverse_bits() >> (32 - len), len);
    }

    /// Pads the pending bits with zeros to the next byte boundary.
    fn align(&mut self) {
        if self.nbits > 0 {
            self.put(0, 8 - self.nbits);
        }
    }

    /// Raw bytes; only meaningful at a byte boundary.
    fn bytes(&mut self, bytes: &[u8]) {
        debug_assert_eq!(self.nbits, 0);
        self.out.extend_from_slice(bytes);
    }

    fn block(&mut self, input: &[u8], syms: &[Sym], last: bool) {
        let coded_bits = 3 + syms.iter().map(|s| fixed_bits(*s)).sum::<usize>() + 7;
        let pad = usize::try_from((8 - (self.nbits + 3) % 8) % 8).expect("under eight");
        let stored_bits = 3 + pad + 32 + 8 * input.len();
        if coded_bits < stored_bits {
            self.fixed(syms, last);
        } else {
            self.stored(input, last);
        }
    }

    fn stored(&mut self, input: &[u8], last: bool) {
        self.put(u32::from(last), 1);
        self.put(0, 2);
        self.align();
        let len = input.len() as u16;
        self.bytes(&len.to_le_bytes());
        self.bytes(&(!len).to_le_bytes());
        self.bytes(input);
    }

    fn fixed(&mut self, syms: &[Sym], last: bool) {
        self.put(u32::from(last), 1);
        self.put(1, 2);
        for sym in syms {
            match *sym {
                Sym::Lit(byte) => self.litlen(u16::from(byte)),
                Sym::Match { len, dist } => {
                    let (code, extra, bits) = length_code(usize::from(len));
                    self.litlen(code);
                    self.put(extra, bits);
                    let (code, extra, bits) = dist_code(usize::from(dist));
                    self.huff(u32::from(code), 5);
                    self.put(extra, bits);
                }
            }
        }
        self.litlen(END_OF_BLOCK);
    }

    /// A literal/length symbol in the fixed code (RFC 1951 §3.2.6).
    fn litlen(&mut self, sym: u16) {
        let (code, len) = fixed_litlen(sym);
        self.huff(code, len);
    }
}

/// The fixed literal/length code word and its length for `sym`.
fn fixed_litlen(sym: u16) -> (u32, u32) {
    let sym = u32::from(sym);
    match sym {
        0..=143 => (0x30 + sym, 8),
        144..=255 => (0x190 + sym - 144, 9),
        256..=279 => (sym - 256, 7),
        _ => (0xC0 + sym - 280, 8),
    }
}

/// Length code, extra-bit value, and extra-bit count for a match length.
fn length_code(len: usize) -> (u16, u32, u32) {
    let index = LENGTH_BASE
        .iter()
        .rposition(|&b| usize::from(b) <= len)
        .expect("len >= 3");
    let extra = (len - usize::from(LENGTH_BASE[index])) as u32;
    (257 + index as u16, extra, u32::from(LENGTH_EXTRA[index]))
}

/// Distance code, extra-bit value, and extra-bit count for a distance.
fn dist_code(dist: usize) -> (u16, u32, u32) {
    let index = DIST_BASE
        .iter()
        .rposition(|&b| usize::from(b) <= dist)
        .expect("dist >= 1");
    let extra = (dist - usize::from(DIST_BASE[index])) as u32;
    (index as u16, extra, u32::from(DIST_EXTRA[index]))
}

/// How many bits the fixed code spends on one symbol.
fn fixed_bits(sym: Sym) -> usize {
    match sym {
        Sym::Lit(byte) => fixed_litlen(u16::from(byte)).1 as usize,
        Sym::Match { len, dist } => {
            let (code, _, len_extra) = length_code(usize::from(len));
            let (_, _, dist_extra) = dist_code(usize::from(dist));
            fixed_litlen(code).1 as usize + len_extra as usize + 5 + dist_extra as usize
        }
    }
}

/// Adler-32 (RFC 1950 §8): two running sums modulo 65521. The inner chunk
/// size is the largest count of 0xFF bytes the u32 sums can absorb between
/// reductions without overflowing.
pub(crate) fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    const CHUNK: usize = 5552;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for chunk in data.chunks(CHUNK) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    (b << 16) | a
}

#[cfg(test)]
#[path = "../tests/common/inflate.rs"]
mod inflate;

#[cfg(test)]
mod tests {
    use super::inflate::inflate;
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn header_is_multiple_of_31() {
        let pair = u32::from(ZLIB_HEADER[0]) * 256 + u32::from(ZLIB_HEADER[1]);
        assert_eq!(pair % 31, 0);
    }

    #[test]
    fn adler_known_values() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"\x00"), 0x0001_0001);
        // 'a' = 97: a = 98, b = 98.
        assert_eq!(adler32(b"a"), 0x0062_0062);
        // Exercise the reduction path.
        let big = vec![0xFF; 100_000];
        let (mut a, mut b) = (1u64, 0u64);
        for _ in 0..big.len() {
            a = (a + 0xFF) % 65_521;
            b = (b + a) % 65_521;
        }
        assert_eq!(adler32(&big), ((b as u32) << 16) | a as u32);
    }

    #[test]
    fn code_tables_cover_their_ranges() {
        assert_eq!(length_code(3), (257, 0, 0));
        assert_eq!(length_code(10), (264, 0, 0));
        assert_eq!(length_code(11), (265, 0, 1));
        assert_eq!(length_code(12), (265, 1, 1));
        assert_eq!(length_code(257), (284, 30, 5));
        assert_eq!(length_code(258), (285, 0, 0));
        assert_eq!(dist_code(1), (0, 0, 0));
        assert_eq!(dist_code(4), (3, 0, 0));
        assert_eq!(dist_code(5), (4, 0, 1));
        assert_eq!(dist_code(6), (4, 1, 1));
        assert_eq!(dist_code(32_768), (29, 8191, 13));
        assert_eq!(fixed_litlen(0), (0x30, 8));
        assert_eq!(fixed_litlen(143), (0xBF, 8));
        assert_eq!(fixed_litlen(144), (0x190, 9));
        assert_eq!(fixed_litlen(255), (0x1FF, 9));
        assert_eq!(fixed_litlen(256), (0, 7));
        assert_eq!(fixed_litlen(279), (0x17, 7));
        assert_eq!(fixed_litlen(280), (0xC0, 8));
        assert_eq!(fixed_litlen(287), (0xC7, 8));
    }

    #[test]
    fn empty_input_is_one_final_block() {
        let z = compress(b"");
        // Final fixed block holding only the end-of-block code.
        assert_eq!(z, [0x78, 0x01, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]);
        assert_eq!(inflate(&z), b"");
    }

    #[test]
    fn small_inputs_round_trip() {
        for input in [
            &b"a"[..],
            b"ab",
            b"abc",
            b"aaaa",
            b"aaaaaaaaaaaaaaaaaaaaaaaa",
            b"abcabcabcabcabcabc",
            b"hello hello hello hello",
            b"\x00\x00\x00\x00\x00\x00\x00\x00",
            b"\xFF\xFE\xFF\xFE\xFF\xFE",
        ] {
            assert_eq!(inflate(&compress(input)), input, "{input:?}");
        }
    }

    #[test]
    fn a_repetitive_stream_shrinks_below_a_quarter_and_round_trips() {
        // A grid of strokes: the operators repeat and so do the numbers.
        let mut data = Vec::new();
        let mut k = 0u32;
        while data.len() < 100_000 {
            let (x, y) = ((k % 10) * 50, (k % 7) * 60);
            data.extend_from_slice(format!("{x} {y} m {} {y} l S\n", x + 40).as_bytes());
            k += 1;
        }
        let z = compress(&data);
        assert!(
            z.len() * 4 < data.len(),
            "{} bytes for {} input",
            z.len(),
            data.len()
        );
        assert_eq!(inflate(&z), data);
    }

    #[test]
    fn incompressible_data_is_stored_and_barely_grows() {
        // A linear congruential generator: pseudo-random, fixed seed.
        let mut state = 0x1234_5678u32;
        let data: Vec<u8> = (0..65_536)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state >> 24) as u8
            })
            .collect();
        let z = compress(&data);
        assert!(z.len() <= data.len() + data.len() / 100);
        // The first block is stored in full: header byte, LEN, NLEN.
        assert_eq!(&z[2..7], &[0x00, 0xFF, 0xFF, 0x00, 0x00]);
        assert_eq!(&z[7..7 + MAX_BLOCK], &data[..MAX_BLOCK]);
        // The one byte left over is cheaper coded (18 bits) than stored
        // (48 bits), so the final block is a fixed one.
        assert_eq!(z[7 + MAX_BLOCK] & 0x07, 0b011);
        assert_eq!(inflate(&z), data);
    }

    #[test]
    fn matches_reach_the_maximum_length_and_the_full_window() {
        let long = vec![b'x'; 10_000];
        let z = compress(&long);
        assert!(z.len() < 100);
        assert_eq!(inflate(&z), long);

        // A 300-byte phrase, then filler to push it 32 768 bytes back,
        // then the phrase again: the second copy is one match at the
        // window's far edge.
        let phrase: Vec<u8> = (0..300u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut data = phrase.clone();
        let mut state = 1u32;
        while data.len() < WINDOW {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            data.push((state >> 24) as u8);
        }
        data.extend_from_slice(&phrase);
        assert_eq!(inflate(&compress(&data)), data);
    }

    #[test]
    fn blocks_split_at_the_input_limit_and_stay_consistent() {
        for len in [MAX_BLOCK - 1, MAX_BLOCK, MAX_BLOCK + 1, 2 * MAX_BLOCK + 7] {
            let data: Vec<u8> = (0..len).map(|i| (i % 13) as u8).collect();
            let z = compress(&data);
            assert_eq!(inflate(&z), data, "length {len}");
        }
    }

    #[test]
    fn output_is_deterministic() {
        let data: Vec<u8> = (0..50_000u32).map(|i| (i * i % 97) as u8).collect();
        assert_eq!(compress(&data), compress(&data));
    }

    fn repetitive() -> impl Strategy<Value = Vec<u8>> {
        (proptest::collection::vec(any::<u8>(), 1..64), 1usize..2000)
            .prop_map(|(unit, times)| unit.repeat(times))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn random_bytes_round_trip(data in proptest::collection::vec(any::<u8>(), 0..20_000)) {
            prop_assert_eq!(inflate(&compress(&data)), data);
        }

        #[test]
        fn repetitive_bytes_round_trip(data in repetitive()) {
            let z = compress(&data);
            prop_assert!(z.len() <= data.len() + 16);
            prop_assert_eq!(inflate(&z), data);
        }

        #[test]
        fn mixed_bytes_round_trip(
            parts in proptest::collection::vec(
                prop_oneof![
                    proptest::collection::vec(any::<u8>(), 0..500),
                    repetitive(),
                ],
                1..8,
            )
        ) {
            let data: Vec<u8> = parts.concat();
            prop_assert_eq!(inflate(&compress(&data)), data);
        }
    }
}
