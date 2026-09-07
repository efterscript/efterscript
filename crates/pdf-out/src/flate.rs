// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Hand-written Flate compression: the zlib container of RFC 1950 around
//! a DEFLATE stream of RFC 1951. Matching is greedy LZ77 over a 32 KB
//! window through a hash chain on three-byte prefixes; each block covers
//! at most 65 535 input bytes and is written in whichever of the stored,
//! fixed-Huffman, or dynamic-Huffman forms (RFC 1951 §3.2.4–§3.2.7)
//! costs the fewest bits, a dynamic block's codes being length-limited
//! canonical codes built from its own symbol frequencies. No randomness,
//! no threads, no lazy matching, integer arithmetic only: the same input
//! gives the same bytes everywhere.

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

/// Alphabet sizes a dynamic block can declare (RFC 1951 §3.2.7); the
/// fixed literal/length code has two more symbols that are never coded.
const LIT_SYMBOLS: usize = 286;
const FIXED_LIT_SYMBOLS: usize = 288;
const DIST_SYMBOLS: usize = 30;
const CL_SYMBOLS: usize = 19;
/// Smallest counts a dynamic block header can declare.
const MIN_HLIT: usize = 257;
const MIN_HDIST: usize = 1;
const MIN_HCLEN: usize = 4;
/// Longest code word of the two main alphabets, and of the code-length
/// alphabet whose lengths travel in three bits.
const MAX_CODE_BITS: u8 = 15;
const MAX_CL_BITS: u8 = 7;
/// Transmission order of the code-length code's lengths (RFC 1951 §3.2.7).
const CL_ORDER: [usize; CL_SYMBOLS] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

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
#[derive(Clone, Copy, Debug)]
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
    let mut emitter = Emitter::with_capacity(data.len() / 2 + 16);
    emitter.out.bytes(&ZLIB_HEADER);
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
        emitter.block(&data[start..end], &syms, pos == n);
    }
    emitter.out.align();
    emitter.out.bytes(&adler32(data).to_be_bytes());
    emitter.out.out
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

/// A symbol resolved to its code indices and extra bits, so that each
/// form is costed and written from one table lookup per symbol.
#[derive(Clone, Copy)]
struct Coded {
    /// A literal 0..=255 or a length code 257..=285.
    lit: u16,
    extra: u32,
    extra_bits: u32,
    /// A distance code 0..=29 for a match, `NO_DIST` for a literal.
    dist: u16,
    dist_extra: u32,
    dist_extra_bits: u32,
}

const NO_DIST: u16 = u16::MAX;

impl Coded {
    fn of(sym: Sym) -> Coded {
        match sym {
            Sym::Lit(byte) => Coded {
                lit: u16::from(byte),
                extra: 0,
                extra_bits: 0,
                dist: NO_DIST,
                dist_extra: 0,
                dist_extra_bits: 0,
            },
            Sym::Match { len, dist } => {
                let (lit, extra, extra_bits) = length_code(usize::from(len));
                let (dist, dist_extra, dist_extra_bits) = dist_code(usize::from(dist));
                Coded {
                    lit,
                    extra,
                    extra_bits,
                    dist,
                    dist_extra,
                    dist_extra_bits,
                }
            }
        }
    }

    fn is_match(&self) -> bool {
        self.dist != NO_DIST
    }
}

/// A canonical Huffman code: each symbol's length (0 for no code) and the
/// code word assigned from the lengths as RFC 1951 §3.2.2 prescribes, so
/// a conforming inflater rebuilds the same code from the lengths alone.
struct Code {
    lengths: Vec<u8>,
    codes: Vec<u16>,
}

impl Code {
    fn from_lengths(lengths: Vec<u8>) -> Code {
        let mut count = [0u32; 17];
        for &len in &lengths {
            count[usize::from(len)] += 1;
        }
        count[0] = 0;
        let mut next = [0u32; 17];
        let mut code = 0u32;
        for bits in 1..=16 {
            code = (code + count[bits - 1]) << 1;
            next[bits] = code;
        }
        let codes = lengths
            .iter()
            .map(|&len| {
                if len == 0 {
                    return 0;
                }
                let code = next[usize::from(len)];
                next[usize::from(len)] += 1;
                code as u16
            })
            .collect();
        Code { lengths, codes }
    }

    fn from_freqs(freqs: &[u32], limit: u8) -> Code {
        Code::from_lengths(code_lengths(freqs, limit))
    }

    /// The fixed literal/length code (RFC 1951 §3.2.6).
    fn fixed_lit() -> Code {
        let lengths = (0..FIXED_LIT_SYMBOLS)
            .map(|sym| match sym {
                0..=143 => 8,
                144..=255 => 9,
                256..=279 => 7,
                _ => 8,
            })
            .collect();
        Code::from_lengths(lengths)
    }

    /// The fixed distance code: five bits each.
    fn fixed_dist() -> Code {
        Code::from_lengths(vec![5; DIST_SYMBOLS])
    }

    fn bits(&self, sym: u16) -> usize {
        usize::from(self.lengths[usize::from(sym)])
    }
}

/// Code lengths for `freqs`, none longer than `limit`, by the
/// package-merge method: the leaves are merged with pairwise packages of
/// the previous level until `limit` levels exist, and a symbol's length
/// is how many of the 2n − 2 cheapest items of the last level contain
/// it. A zero frequency gets no code; a lone symbol gets a one-bit code,
/// the standard's spelling of the one-code case. Among equal weights the
/// lower symbol index takes the shorter code and a leaf goes before a
/// package, so the lengths are deterministic. Needs n ≤ 2^limit, which
/// every alphabet here satisfies.
fn code_lengths(freqs: &[u32], limit: u8) -> Vec<u8> {
    let mut lengths = vec![0u8; freqs.len()];
    let mut leaves: Vec<(u64, usize)> = freqs
        .iter()
        .enumerate()
        .filter(|&(_, &f)| f != 0)
        .map(|(sym, &f)| (u64::from(f), sym))
        .collect();
    leaves.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let n = leaves.len();
    match n {
        0 => return lengths,
        1 => {
            lengths[leaves[0].1] = 1;
            return lengths;
        }
        _ => debug_assert!(n <= 1 << limit),
    }
    // Node arena: the leaves first, packages appended as (weight, parts).
    let mut weight: Vec<u64> = leaves.iter().map(|&(w, _)| w).collect();
    let mut parts: Vec<(u32, u32)> = vec![(NIL, NIL); n];
    let mut list: Vec<u32> = (0..n as u32).collect();
    for _ in 1..limit {
        let mut merged = Vec::with_capacity(n + list.len() / 2);
        let mut leaf = 0usize;
        let (pairs, _) = list.as_chunks::<2>();
        let mut pairs = pairs.iter();
        let mut pair = pairs.next();
        loop {
            let packaged = pair.map(|&[a, b]| weight[a as usize] + weight[b as usize]);
            let take_leaf = leaf < n && packaged.is_none_or(|w| weight[leaf] <= w);
            if take_leaf {
                merged.push(leaf as u32);
                leaf += 1;
            } else if let (Some(w), Some(&[a, b])) = (packaged, pair) {
                merged.push(weight.len() as u32);
                weight.push(w);
                parts.push((a, b));
                pair = pairs.next();
            } else {
                break;
            }
        }
        list = merged;
    }
    let mut stack: Vec<u32> = list[..2 * n - 2].to_vec();
    while let Some(id) = stack.pop() {
        let (a, b) = parts[id as usize];
        if a == NIL {
            lengths[leaves[id as usize].1] += 1;
        } else {
            stack.push(a);
            stack.push(b);
        }
    }
    lengths
}

/// Run-length coding of a code-length sequence with the code-length
/// alphabet (RFC 1951 §3.2.7), as (symbol, extra-bit value) pairs; the
/// longest run each symbol allows is taken greedily.
fn run_lengths(lengths: &[u8]) -> Vec<(u8, u8)> {
    let mut out = Vec::with_capacity(lengths.len() / 2);
    let mut at = 0;
    while at < lengths.len() {
        let value = lengths[at];
        let mut run = 1;
        while at + run < lengths.len() && lengths[at + run] == value {
            run += 1;
        }
        at += run;
        if value == 0 {
            while run >= 11 {
                let take = run.min(138);
                out.push((18, (take - 11) as u8));
                run -= take;
            }
            if run >= 3 {
                out.push((17, (run - 3) as u8));
                run = 0;
            }
        } else {
            out.push((value, 0));
            run -= 1;
            while run >= 3 {
                let take = run.min(6);
                out.push((16, (take - 3) as u8));
                run -= take;
            }
        }
        out.extend(std::iter::repeat_n((value, 0), run));
    }
    out
}

fn run_extra_bits(sym: u8) -> u32 {
    match sym {
        16 => 2,
        17 => 3,
        18 => 7,
        _ => 0,
    }
}

/// How many bits the coded symbols and the end-of-block code take under
/// a pair of codes.
fn coded_bits(coded: &[Coded], lit: &Code, dist: &Code) -> usize {
    coded
        .iter()
        .map(|c| {
            let mut bits = lit.bits(c.lit) + c.extra_bits as usize;
            if c.is_match() {
                bits += dist.bits(c.dist) + c.dist_extra_bits as usize;
            }
            bits
        })
        .sum::<usize>()
        + lit.bits(END_OF_BLOCK)
}

/// A dynamic block's header and codes, costed before anything is written.
struct Dynamic {
    lit: Code,
    dist: Code,
    cl: Code,
    hlit: usize,
    hdist: usize,
    hclen: usize,
    runs: Vec<(u8, u8)>,
    /// The whole block: three-bit header through end-of-block.
    bits: usize,
}

impl Dynamic {
    fn plan(coded: &[Coded]) -> Dynamic {
        let mut lit_freq = [0u32; LIT_SYMBOLS];
        let mut dist_freq = [0u32; DIST_SYMBOLS];
        lit_freq[usize::from(END_OF_BLOCK)] = 1;
        for c in coded {
            lit_freq[usize::from(c.lit)] += 1;
            if c.is_match() {
                dist_freq[usize::from(c.dist)] += 1;
            }
        }
        let lit = Code::from_freqs(&lit_freq, MAX_CODE_BITS);
        let dist = Code::from_freqs(&dist_freq, MAX_CODE_BITS);
        let hlit = declared(&lit.lengths, MIN_HLIT);
        let hdist = declared(&dist.lengths, MIN_HDIST);
        let mut all = Vec::with_capacity(hlit + hdist);
        all.extend_from_slice(&lit.lengths[..hlit]);
        all.extend_from_slice(&dist.lengths[..hdist]);
        let runs = run_lengths(&all);
        let mut cl_freq = [0u32; CL_SYMBOLS];
        for &(sym, _) in &runs {
            cl_freq[usize::from(sym)] += 1;
        }
        let cl = Code::from_freqs(&cl_freq, MAX_CL_BITS);
        let hclen = CL_ORDER
            .iter()
            .rposition(|&sym| cl.lengths[sym] != 0)
            .map_or(MIN_HCLEN, |i| (i + 1).max(MIN_HCLEN));
        let header = 3 + 5 + 5 + 4 + 3 * hclen;
        let table: usize = runs
            .iter()
            .map(|&(sym, _)| cl.bits(u16::from(sym)) + run_extra_bits(sym) as usize)
            .sum();
        let bits = header + table + coded_bits(coded, &lit, &dist);
        Dynamic {
            lit,
            dist,
            cl,
            hlit,
            hdist,
            hclen,
            runs,
            bits,
        }
    }
}

/// How many leading entries of `lengths` a header must declare: through
/// the last symbol with a code, and never fewer than `min`.
fn declared(lengths: &[u8], min: usize) -> usize {
    lengths
        .iter()
        .rposition(|&len| len != 0)
        .map_or(min, |i| (i + 1).max(min))
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

    fn huff(&mut self, code: &Code, sym: u16) {
        let len = u32::from(code.lengths[usize::from(sym)]);
        debug_assert!(len > 0, "symbol {sym} has no code");
        self.put(
            u32::from(code.codes[usize::from(sym)]).reverse_bits() >> (32 - len),
            len,
        );
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
}

/// Which form a block was written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    Stored,
    Fixed,
    Dynamic,
}

/// Writes blocks, choosing the form of each by exact bit count.
struct Emitter {
    out: BitWriter,
    fixed_lit: Code,
    fixed_dist: Code,
    coded: Vec<Coded>,
}

impl Emitter {
    fn with_capacity(capacity: usize) -> Self {
        Emitter {
            out: BitWriter::with_capacity(capacity),
            fixed_lit: Code::fixed_lit(),
            fixed_dist: Code::fixed_dist(),
            coded: Vec::new(),
        }
    }

    /// Writes one block in the cheapest form; ties go to stored, then
    /// fixed, the forms that are simpler to decode.
    fn block(&mut self, input: &[u8], syms: &[Sym], last: bool) -> Form {
        self.coded.clear();
        self.coded.extend(syms.iter().map(|&sym| Coded::of(sym)));
        let fixed_bits = 3 + coded_bits(&self.coded, &self.fixed_lit, &self.fixed_dist);
        let dynamic = Dynamic::plan(&self.coded);
        let pad = usize::try_from((8 - (self.out.nbits + 3) % 8) % 8).expect("under eight");
        let stored_bits = 3 + pad + 32 + 8 * input.len();
        if stored_bits <= fixed_bits && stored_bits <= dynamic.bits {
            self.stored(input, last);
            Form::Stored
        } else if fixed_bits <= dynamic.bits {
            self.fixed(last);
            Form::Fixed
        } else {
            self.dynamic(&dynamic, last);
            Form::Dynamic
        }
    }

    fn stored(&mut self, input: &[u8], last: bool) {
        self.out.put(u32::from(last), 1);
        self.out.put(0, 2);
        self.out.align();
        let len = input.len() as u16;
        self.out.bytes(&len.to_le_bytes());
        self.out.bytes(&(!len).to_le_bytes());
        self.out.bytes(input);
    }

    fn fixed(&mut self, last: bool) {
        self.out.put(u32::from(last), 1);
        self.out.put(1, 2);
        Self::coded(
            &mut self.out,
            &self.coded,
            &self.fixed_lit,
            &self.fixed_dist,
        );
    }

    fn dynamic(&mut self, plan: &Dynamic, last: bool) {
        let out = &mut self.out;
        out.put(u32::from(last), 1);
        out.put(2, 2);
        out.put((plan.hlit - MIN_HLIT) as u32, 5);
        out.put((plan.hdist - MIN_HDIST) as u32, 5);
        out.put((plan.hclen - MIN_HCLEN) as u32, 4);
        for &sym in &CL_ORDER[..plan.hclen] {
            out.put(u32::from(plan.cl.lengths[sym]), 3);
        }
        for &(sym, extra) in &plan.runs {
            out.huff(&plan.cl, u16::from(sym));
            out.put(u32::from(extra), run_extra_bits(sym));
        }
        Self::coded(out, &self.coded, &plan.lit, &plan.dist);
    }

    /// The symbols and the end-of-block code under a pair of codes.
    fn coded(out: &mut BitWriter, coded: &[Coded], lit: &Code, dist: &Code) {
        for c in coded {
            out.huff(lit, c.lit);
            out.put(c.extra, c.extra_bits);
            if c.is_match() {
                out.huff(dist, c.dist);
                out.put(c.dist_extra, c.dist_extra_bits);
            }
        }
        out.huff(lit, END_OF_BLOCK);
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
    use super::inflate::{block_kinds, decode_symbols, inflate};
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
    }

    #[test]
    fn canonical_assignment_reproduces_the_fixed_code() {
        let lit = Code::fixed_lit();
        let word = |sym: usize| (lit.codes[sym], lit.lengths[sym]);
        assert_eq!(word(0), (0x30, 8));
        assert_eq!(word(143), (0xBF, 8));
        assert_eq!(word(144), (0x190, 9));
        assert_eq!(word(255), (0x1FF, 9));
        assert_eq!(word(256), (0, 7));
        assert_eq!(word(279), (0x17, 7));
        assert_eq!(word(280), (0xC0, 8));
        assert_eq!(word(287), (0xC7, 8));
        let dist = Code::fixed_dist();
        assert_eq!((dist.codes[0], dist.lengths[0]), (0, 5));
        assert_eq!((dist.codes[29], dist.lengths[29]), (29, 5));
        // The worked example of RFC 1951 §3.2.2.
        let code = Code::from_lengths(vec![3, 3, 3, 3, 3, 2, 4, 4]);
        assert_eq!(
            code.codes,
            [0b010, 0b011, 0b100, 0b101, 0b110, 0b00, 0b1110, 0b1111]
        );
    }

    /// The Kraft sum scaled so a complete code of `limit` bits is 1 << limit.
    fn kraft(lengths: &[u8], limit: u8) -> u64 {
        lengths
            .iter()
            .filter(|&&len| len != 0)
            .map(|&len| 1u64 << (limit - len))
            .sum()
    }

    #[test]
    fn code_lengths_handle_the_edge_alphabets() {
        assert_eq!(code_lengths(&[0, 0, 0], 15), [0, 0, 0]);
        assert_eq!(code_lengths(&[], 15), Vec::<u8>::new());
        assert_eq!(code_lengths(&[0, 7, 0], 15), [0, 1, 0]);
        assert_eq!(code_lengths(&[1, 1], 15), [1, 1]);
        assert_eq!(code_lengths(&[1, 1, 1, 1], 2), [2, 2, 2, 2]);
        // 1 : 1 : 2 : 4 wants lengths 3, 3, 2, 1 unlimited …
        assert_eq!(code_lengths(&[1, 1, 2, 4], 15), [3, 3, 2, 1]);
        // … and is flattened by a two-bit limit.
        assert_eq!(code_lengths(&[1, 1, 2, 4], 2), [2, 2, 2, 2]);
        // Equal weights: the lower index gets the shorter code.
        assert_eq!(code_lengths(&[5, 5, 5], 15), [1, 2, 2]);
    }

    #[test]
    fn code_lengths_hit_the_limit_on_fibonacci_weights() {
        // An unlimited code over these weights would be 29 bits deep.
        let mut freqs = vec![1u32, 1];
        while freqs.len() < 30 {
            let n = freqs.len();
            freqs.push(freqs[n - 1] + freqs[n - 2]);
        }
        let lengths = code_lengths(&freqs, 15);
        assert_eq!(lengths.iter().max(), Some(&15));
        assert_eq!(kraft(&lengths, 15), 1 << 15);
        let lengths = code_lengths(&freqs[..19], 7);
        assert_eq!(lengths.iter().max(), Some(&7));
        assert_eq!(kraft(&lengths, 7), 1 << 7);
    }

    #[test]
    fn run_length_coding_uses_every_repeat_symbol() {
        assert_eq!(run_lengths(&[]), []);
        assert_eq!(run_lengths(&[4, 4]), [(4, 0), (4, 0)]);
        assert_eq!(run_lengths(&[0, 0]), [(0, 0), (0, 0)]);
        assert_eq!(run_lengths(&[0, 0, 0]), [(17, 0)]);
        assert_eq!(run_lengths(&[0; 10]), [(17, 7)]);
        assert_eq!(run_lengths(&[0; 11]), [(18, 0)]);
        assert_eq!(run_lengths(&[0; 138]), [(18, 127)]);
        assert_eq!(run_lengths(&[0; 139]), [(18, 127), (0, 0)]);
        assert_eq!(run_lengths(&[0; 141]), [(18, 127), (17, 0)]);
        assert_eq!(run_lengths(&[3; 4]), [(3, 0), (16, 0)]);
        assert_eq!(run_lengths(&[3; 7]), [(3, 0), (16, 3)]);
        assert_eq!(run_lengths(&[3; 8]), [(3, 0), (16, 3), (3, 0)]);
        assert_eq!(run_lengths(&[3; 10]), [(3, 0), (16, 3), (16, 0)]);
        assert_eq!(
            run_lengths(&[0, 0, 0, 0, 5, 5, 5, 5, 5, 0, 2]),
            [(17, 1), (5, 0), (16, 1), (0, 0), (2, 0)]
        );
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

    fn repetitive_stream() -> Vec<u8> {
        // A grid of strokes: the operators repeat and so do the numbers.
        let mut data = Vec::new();
        let mut k = 0u32;
        while data.len() < 100_000 {
            let (x, y) = ((k % 10) * 50, (k % 7) * 60);
            data.extend_from_slice(format!("{x} {y} m {} {y} l S\n", x + 40).as_bytes());
            k += 1;
        }
        data
    }

    fn varied_stream() -> Vec<u8> {
        // A staircase of strokes, each five right and six up from the
        // last: the operators repeat, the coordinates never do.
        let mut data = Vec::new();
        let mut k = 0u32;
        while data.len() < 100_000 {
            let (x, y) = (k * 5, k * 6);
            data.extend_from_slice(format!("{x} {y} m {} {y} l S\n", x + 40).as_bytes());
            k += 1;
        }
        data
    }

    #[test]
    fn a_repetitive_stream_shrinks_below_a_quarter_and_round_trips() {
        let data = repetitive_stream();
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
    fn varied_numbers_shrink_below_thirty_percent_with_dynamic_blocks() {
        let data = varied_stream();
        let z = compress(&data);
        assert!(
            z.len() * 100 < data.len() * 30,
            "{} bytes for {} input",
            z.len(),
            data.len()
        );
        let kinds = block_kinds(&z);
        assert_eq!(kinds.len(), data.len().div_ceil(MAX_BLOCK));
        assert!(kinds.iter().all(|&kind| kind == 2), "{kinds:?}");
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
        assert_eq!(block_kinds(&z), [0, 1]);
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
    fn adversarial_inputs_round_trip() {
        // One byte throughout: a three-symbol literal alphabet and a
        // single distance code, over more than one block.
        let same = vec![0x5A; 200_000];
        let z = compress(&same);
        assert!(z.len() < 400, "{}", z.len());
        assert_eq!(inflate(&z), same);
        // Two bytes alternating: every match is at distance two.
        let alternating: Vec<u8> = (0..100_000)
            .map(|i| if i % 2 == 0 { 0 } else { 255 })
            .collect();
        assert_eq!(inflate(&compress(&alternating)), alternating);
        // Every byte value once, repeated: a full literal alphabet.
        let ramp: Vec<u8> = (0..70_000).map(|i| (i % 256) as u8).collect();
        assert_eq!(inflate(&compress(&ramp)), ramp);
        // Skewed literals with no repeated triple, so the block carries
        // literals only and declares an empty distance code.
        let mut skewed = Vec::new();
        for i in 0..4000u32 {
            let b = if i % 3 == 0 {
                b'a'
            } else {
                b'a' + (i.wrapping_mul(2_654_435_761) % 26) as u8
            };
            skewed.push(b);
        }
        assert_eq!(inflate(&compress(&skewed)), skewed);
    }

    #[test]
    fn a_literal_only_block_declares_no_distance_code() {
        let syms: Vec<Sym> = (0..2000u32)
            .map(|i| Sym::Lit(b'a' + (i.wrapping_mul(2_654_435_761) % 26) as u8))
            .collect();
        let coded: Vec<Coded> = syms.iter().map(|&s| Coded::of(s)).collect();
        let plan = Dynamic::plan(&coded);
        assert_eq!(plan.hdist, 1);
        assert!(plan.dist.lengths.iter().all(|&len| len == 0));
        assert!(plan.hlit >= 257 && plan.hlit <= 286);
        let data: Vec<u8> = syms
            .iter()
            .map(|s| match s {
                Sym::Lit(b) => *b,
                _ => unreachable!(),
            })
            .collect();
        let mut emitter = Emitter::with_capacity(0);
        emitter.out.bytes(&ZLIB_HEADER);
        assert_eq!(emitter.block(&data, &syms, true), Form::Dynamic);
        let written = emitter.out.out.len() * 8 + usize::try_from(emitter.out.nbits).unwrap() - 16;
        assert_eq!(written, plan.bits);
        emitter.out.align();
        let mut z = emitter.out.out;
        z.extend_from_slice(&adler32(&data).to_be_bytes());
        assert_eq!(block_kinds(&z), [2]);
        assert_eq!(inflate(&z), data);
    }

    /// The matcher's symbols for `data` as one block.
    fn lz77(data: &[u8]) -> Vec<Sym> {
        let mut matcher = Matcher::new();
        let mut syms = Vec::new();
        let mut pos = 0;
        while pos < data.len() {
            let limit = MAX_MATCH.min(data.len() - pos);
            let found = (limit >= MIN_MATCH)
                .then(|| matcher.longest(data, pos, limit))
                .flatten();
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
        syms
    }

    #[test]
    fn every_form_is_costed_exactly() {
        // Each form's bit count equals what writing it produces.
        let data = varied_stream();
        let syms = lz77(&data[..3000]);
        let bits = |e: &Emitter| e.out.out.len() * 8 + e.out.nbits as usize;
        let mut emitter = Emitter::with_capacity(0);
        emitter.coded.extend(syms.iter().map(|&s| Coded::of(s)));
        let plan = Dynamic::plan(&emitter.coded);
        let fixed_bits = 3 + coded_bits(&emitter.coded, &emitter.fixed_lit, &emitter.fixed_dist);
        emitter.fixed(true);
        assert_eq!(bits(&emitter), fixed_bits);
        let mut emitter = Emitter::with_capacity(0);
        emitter.coded.extend(syms.iter().map(|&s| Coded::of(s)));
        emitter.dynamic(&plan, true);
        assert_eq!(bits(&emitter), plan.bits);
        assert!(plan.bits < fixed_bits);
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

    /// Frequency tables of every shape: flat, sparse, and steep.
    fn freqs() -> impl Strategy<Value = Vec<u32>> {
        prop_oneof![
            proptest::collection::vec(0u32..4, 1..300),
            proptest::collection::vec(0u32..70_000, 1..300),
            proptest::collection::vec(any::<u8>().prop_map(|b| 1u32 << (b % 24)), 1..300),
        ]
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

        #[test]
        fn skewed_bytes_round_trip(
            data in proptest::collection::vec(
                any::<u8>().prop_map(|b| b"aaaaaaaabbbbcceeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee \n"[usize::from(b) % 56]),
                0..30_000,
            )
        ) {
            prop_assert_eq!(inflate(&compress(&data)), data);
        }

        #[test]
        fn code_lengths_are_limited_complete_and_deterministic(
            (freqs, limit) in (freqs(), 7u8..=15),
        ) {
            // An alphabet cannot exceed what the limit can hold.
            let freqs = &freqs[..freqs.len().min(1 << limit)];
            let lengths = code_lengths(freqs, limit);
            prop_assert_eq!(lengths.len(), freqs.len());
            let used = freqs.iter().filter(|&&f| f != 0).count();
            for (len, freq) in lengths.iter().zip(freqs) {
                prop_assert_eq!(*len == 0, *freq == 0);
                prop_assert!(*len <= limit);
            }
            let sum = kraft(&lengths, limit);
            match used {
                0 => prop_assert_eq!(sum, 0),
                1 => prop_assert_eq!(sum, 1 << (limit - 1)),
                _ => prop_assert_eq!(sum, 1 << limit),
            }
            prop_assert_eq!(code_lengths(freqs, limit), lengths);
        }

        #[test]
        fn every_code_decodes_through_the_inflater(freqs in freqs()) {
            let code = Code::from_freqs(&freqs, MAX_CODE_BITS);
            let symbols: Vec<u16> = (0..freqs.len() as u16)
                .filter(|&sym| code.lengths[usize::from(sym)] != 0)
                .collect();
            let mut out = BitWriter::with_capacity(0);
            for &sym in &symbols {
                out.huff(&code, sym);
            }
            out.align();
            prop_assert_eq!(decode_symbols(&code.lengths, &out.out, symbols.len()), symbols);
        }
    }
}
