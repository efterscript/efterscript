// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Hand-written zlib container (RFC 1950 framing, RFC 1951 stored blocks):
//! valid `FlateDecode` input with no actual compression, keeping the crate
//! dependency-free and byte-deterministic. Real DEFLATE is a recorded
//! follow-up behind the same [`crate::Filter`] API.

/// CMF/FLG pair: 32K-window deflate method, no preset dictionary, and check
/// bits chosen so the big-endian pair is a multiple of 31 as RFC 1950
/// requires (0x7801 = 31 * 991).
const ZLIB_HEADER: [u8; 2] = [0x78, 0x01];

/// Stored blocks carry at most this many bytes (16-bit LEN field).
const MAX_STORED: usize = 65_535;

pub(crate) fn compress_stored(data: &[u8]) -> Vec<u8> {
    let blocks = data.len().div_ceil(MAX_STORED).max(1);
    let mut out = Vec::with_capacity(2 + data.len() + 5 * blocks + 4);
    out.extend_from_slice(&ZLIB_HEADER);
    if data.is_empty() {
        // The deflate stream still needs one final (empty) block.
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        let mut chunks = data.chunks(MAX_STORED).peekable();
        while let Some(chunk) = chunks.next() {
            let last = chunks.peek().is_none();
            // BFINAL in bit 0, BTYPE 00 (stored) — a stored block starts
            // byte-aligned, so the header is a whole byte.
            out.push(u8::from(last));
            let len = chunk.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(chunk);
        }
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
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
mod tests {
    use super::*;

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
    fn empty_input_still_emits_a_final_block() {
        let z = compress_stored(b"");
        assert_eq!(z, [0x78, 0x01, 0x01, 0x00, 0x00, 0xFF, 0xFF, 0, 0, 0, 1]);
    }

    #[test]
    fn framing_of_small_block() {
        let z = compress_stored(b"hi");
        assert_eq!(&z[..2], &ZLIB_HEADER);
        // Final stored block, LEN=2, NLEN=!2.
        assert_eq!(&z[2..7], &[0x01, 0x02, 0x00, 0xFD, 0xFF]);
        assert_eq!(&z[7..9], b"hi");
        assert_eq!(&z[9..], &adler32(b"hi").to_be_bytes());
    }

    #[test]
    fn large_input_splits_into_stored_blocks() {
        let data = vec![7u8; MAX_STORED + 1];
        let z = compress_stored(&data);
        // First block: not final, LEN=65535.
        assert_eq!(&z[2..7], &[0x00, 0xFF, 0xFF, 0x00, 0x00]);
        // Second block: final, LEN=1.
        let second = 7 + MAX_STORED;
        assert_eq!(&z[second..second + 5], &[0x01, 0x01, 0x00, 0xFE, 0xFF]);
        assert_eq!(z.len(), 2 + 5 + MAX_STORED + 5 + 1 + 4);
    }
}
