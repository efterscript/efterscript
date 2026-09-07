// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Test-only inflater: a straightforward reading of RFC 1950 and
//! RFC 1951 that decodes stored, fixed-Huffman, and dynamic-Huffman
//! blocks and checks the Adler-32, panicking with the reason on any
//! violation. It is included both by the crate's unit tests and by the
//! integration tests, and it verifies the writer's output; it is not
//! product code and it never inflates untrusted input.

#![allow(dead_code)]

const MAX_BITS: usize = 15;

/// Inflates a complete zlib stream, verifying framing and checksum.
pub fn inflate(z: &[u8]) -> Vec<u8> {
    assert!(z.len() >= 2 + 4, "zlib stream too short");
    assert_eq!(z[0] & 0x0F, 8, "compression method must be deflate");
    assert!((z[0] >> 4) <= 7, "window size above 32K");
    assert_eq!((u32::from(z[0]) * 256 + u32::from(z[1])) % 31, 0, "FCHECK");
    assert_eq!(z[1] & 0x20, 0, "no preset dictionary");
    let mut reader = BitReader {
        bytes: z,
        pos: 2,
        acc: 0,
        nbits: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = reader.bits(1) == 1;
        match reader.bits(2) {
            0 => stored(&mut reader, &mut out),
            1 => {
                let (lit, dist) = fixed_tables();
                coded(&mut reader, &mut out, &lit, &dist);
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut reader);
                coded(&mut reader, &mut out, &lit, &dist);
            }
            other => panic!("reserved block type {other}"),
        }
        if last {
            break;
        }
    }
    reader.align();
    let trailer = reader.pos;
    assert_eq!(trailer + 4, z.len(), "trailing bytes after Adler-32");
    let stored = u32::from_be_bytes([z[trailer], z[trailer + 1], z[trailer + 2], z[trailer + 3]]);
    assert_eq!(stored, adler32(&out), "Adler-32 mismatch");
    out
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= MOD;
        b %= MOD;
    }
    b << 16 | a
}

struct BitReader<'a> {
    bytes: &'a [u8],
    pos: usize,
    acc: u32,
    nbits: u32,
}

impl BitReader<'_> {
    fn bits(&mut self, n: u32) -> u32 {
        while self.nbits < n {
            let byte = *self
                .bytes
                .get(self.pos)
                .expect("unexpected end of deflate data");
            self.pos += 1;
            self.acc |= u32::from(byte) << self.nbits;
            self.nbits += 8;
        }
        let value = self.acc & ((1u32 << n) - 1);
        self.acc >>= n;
        self.nbits -= n;
        value
    }

    /// Drops the bits left in the current byte.
    fn align(&mut self) {
        self.acc = 0;
        self.nbits = 0;
    }

    /// One Huffman-coded symbol, read bit by bit through the canonical
    /// code's counts (RFC 1951 §3.2.2).
    fn symbol(&mut self, table: &Table) -> u16 {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: usize = 0;
        for len in 1..=MAX_BITS {
            code |= self.bits(1) as i32;
            let count = i32::from(table.counts[len]);
            if code - count < first {
                return table.symbols[index + (code - first) as usize];
            }
            index += count as usize;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        panic!("invalid Huffman code");
    }
}

/// A canonical Huffman code: how many codes have each length, and the
/// symbols in code order.
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
}

fn stored(reader: &mut BitReader<'_>, out: &mut Vec<u8>) {
    reader.align();
    let z = reader.bytes;
    let pos = reader.pos;
    let len = u16::from_le_bytes([z[pos], z[pos + 1]]);
    let nlen = u16::from_le_bytes([z[pos + 2], z[pos + 3]]);
    assert_eq!(nlen, !len, "NLEN must be the complement of LEN");
    let start = pos + 4;
    out.extend_from_slice(&z[start..start + usize::from(len)]);
    reader.pos = start + usize::from(len);
}

fn fixed_tables() -> (Table, Table) {
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

/// The code-length alphabet's order (RFC 1951 §3.2.7).
const ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

fn dynamic_tables(reader: &mut BitReader<'_>) -> (Table, Table) {
    let hlit = reader.bits(5) as usize + 257;
    let hdist = reader.bits(5) as usize + 1;
    let hclen = reader.bits(4) as usize + 4;
    assert!(hlit <= 286 && hdist <= 30, "too many codes");
    let mut code_lengths = [0u8; 19];
    for &index in ORDER.iter().take(hclen) {
        code_lengths[index] = reader.bits(3) as u8;
    }
    let code_table = Table::from_lengths(&code_lengths);
    let mut lengths = Vec::with_capacity(hlit + hdist);
    while lengths.len() < hlit + hdist {
        let sym = reader.symbol(&code_table);
        match sym {
            0..=15 => lengths.push(sym as u8),
            16 => {
                let last = *lengths.last().expect("no length to repeat");
                let times = 3 + reader.bits(2) as usize;
                lengths.extend(std::iter::repeat_n(last, times));
            }
            17 => {
                let times = 3 + reader.bits(3) as usize;
                lengths.extend(std::iter::repeat_n(0, times));
            }
            _ => {
                let times = 11 + reader.bits(7) as usize;
                lengths.extend(std::iter::repeat_n(0, times));
            }
        }
    }
    assert_eq!(lengths.len(), hlit + hdist, "code lengths overrun");
    assert_ne!(lengths[256], 0, "end-of-block code missing");
    (
        Table::from_lengths(&lengths[..hlit]),
        Table::from_lengths(&lengths[hlit..]),
    )
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn coded(reader: &mut BitReader<'_>, out: &mut Vec<u8>, lit: &Table, dist: &Table) {
    loop {
        let sym = reader.symbol(lit);
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return,
            257..=285 => {
                let index = usize::from(sym - 257);
                let len =
                    usize::from(LENGTH_BASE[index]) + reader.bits(LENGTH_EXTRA[index]) as usize;
                let dsym = usize::from(reader.symbol(dist));
                assert!(dsym < 30, "invalid distance code");
                let distance =
                    usize::from(DIST_BASE[dsym]) + reader.bits(DIST_EXTRA[dsym]) as usize;
                assert!(distance <= out.len(), "distance reaches before the output");
                let from = out.len() - distance;
                for k in 0..len {
                    out.push(out[from + k]);
                }
            }
            _ => panic!("invalid literal/length code {sym}"),
        }
    }
}
