// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! LZW as the PostScript language and PDF `LZWDecode` filters define it:
//! codes of 9 to 12 bits packed most-significant bit first, the table
//! starting with the 256 single bytes, code 256 clearing the table and
//! code 257 ending the data, the width growing as the table fills — one
//! code early under the default `EarlyChange` of 1. The [`Decoder`]
//! takes input a byte at a time; [`encode`] produces a stream the
//! decoder reads back, emitting a clear code first and whenever the table
//! is full.

use std::collections::HashMap;
use std::fmt;

const CLEAR: u16 = 256;
const END: u16 = 257;
const FIRST: u16 = 258;
const MIN_WIDTH: u32 = 9;
const MAX_WIDTH: u32 = 12;
const TABLE_SIZE: usize = 1 << MAX_WIDTH;

/// Why a stream could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A code beyond the entries defined so far.
    Code,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid code")
    }
}

impl std::error::Error for Error {}

/// What a pushed byte led to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    More,
    /// The end-of-data code was read; nothing more is read.
    Done,
}

/// A table entry above the single bytes: the string of `prefix` followed
/// by `byte`.
#[derive(Clone, Copy, Debug)]
struct Entry {
    prefix: u16,
    byte: u8,
    first: u8,
    len: u16,
}

/// A streaming decoder: see the module documentation.
#[derive(Clone, Debug)]
pub struct Decoder {
    early: usize,
    acc: u32,
    nbits: u32,
    width: u32,
    entries: Vec<Entry>,
    prev: Option<u16>,
    done: bool,
}

impl Decoder {
    /// `early_change` is the `EarlyChange` parameter: the width grows one
    /// code before the table needs it when true (the default).
    pub fn new(early_change: bool) -> Self {
        Decoder {
            early: usize::from(early_change),
            acc: 0,
            nbits: 0,
            width: MIN_WIDTH,
            entries: Vec::new(),
            prev: None,
            done: false,
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    fn next_code(&self) -> usize {
        usize::from(FIRST) + self.entries.len()
    }

    fn entry(&self, code: u16) -> Option<&Entry> {
        self.entries
            .get(usize::from(code).checked_sub(usize::from(FIRST))?)
    }

    fn first_byte(&self, code: u16) -> u8 {
        match self.entry(code) {
            Some(entry) => entry.first,
            None => code as u8,
        }
    }

    /// Appends the string of `code` to `out`.
    fn expand(&self, code: u16, out: &mut Vec<u8>) {
        let Some(entry) = self.entry(code) else {
            out.push(code as u8);
            return;
        };
        let start = out.len();
        out.resize(start + usize::from(entry.len), 0);
        let mut at = out.len();
        let mut code = code;
        while let Some(entry) = self.entry(code) {
            at -= 1;
            out[at] = entry.byte;
            code = entry.prefix;
        }
        at -= 1;
        out[at] = code as u8;
    }

    fn add(&mut self, prefix: u16, byte: u8) {
        if self.next_code() >= TABLE_SIZE {
            return;
        }
        let (first, len) = match self.entry(prefix) {
            Some(entry) => (entry.first, entry.len + 1),
            None => (prefix as u8, 2),
        };
        self.entries.push(Entry {
            prefix,
            byte,
            first,
            len,
        });
        if self.next_code() + self.early >= 1 << self.width && self.width < MAX_WIDTH {
            self.width += 1;
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.width = MIN_WIDTH;
        self.prev = None;
    }

    fn code(&mut self, code: u16, out: &mut Vec<u8>) -> Result<Status, Error> {
        match code {
            CLEAR => {
                self.clear();
                return Ok(Status::More);
            }
            END => {
                self.done = true;
                return Ok(Status::Done);
            }
            _ => {}
        }
        let Some(prev) = self.prev else {
            if code >= FIRST {
                return Err(Error::Code);
            }
            out.push(code as u8);
            self.prev = Some(code);
            return Ok(Status::More);
        };
        let next = self.next_code();
        if usize::from(code) < next {
            self.expand(code, out);
            let first = self.first_byte(code);
            self.add(prev, first);
        } else if usize::from(code) == next {
            // The string being defined: the previous one plus its own
            // first byte.
            let first = self.first_byte(prev);
            self.expand(prev, out);
            out.push(first);
            self.add(prev, first);
        } else {
            return Err(Error::Code);
        }
        self.prev = Some(code);
        Ok(Status::More)
    }

    /// Feeds one input byte, appending the bytes it completes to `out`.
    /// Bytes after the end-of-data code are ignored.
    pub fn push(&mut self, byte: u8, out: &mut Vec<u8>) -> Result<Status, Error> {
        if self.done {
            return Ok(Status::Done);
        }
        self.acc = self.acc << 8 | u32::from(byte);
        self.nbits += 8;
        while self.nbits >= self.width {
            let code = (self.acc >> (self.nbits - self.width)) & ((1 << self.width) - 1);
            self.nbits -= self.width;
            self.acc &= (1 << self.nbits) - 1;
            if self.code(code as u16, out)? == Status::Done {
                return Ok(Status::Done);
            }
        }
        Ok(Status::More)
    }
}

/// Bits packed most-significant first.
struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    nbits: u32,
}

impl BitWriter {
    fn put(&mut self, code: u16, width: u32) {
        self.acc = self.acc << width | u32::from(code);
        self.nbits += width;
        while self.nbits >= 8 {
            self.out.push((self.acc >> (self.nbits - 8)) as u8);
            self.nbits -= 8;
            self.acc &= (1 << self.nbits) - 1;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.out.push((self.acc << (8 - self.nbits)) as u8);
        }
        self.out
    }
}

/// Encodes `data` as a stream [`Decoder::new`]`(early_change)` reads
/// back: a clear code, the codes, a clear code whenever the table fills,
/// and the end-of-data code.
pub fn encode(data: &[u8], early_change: bool) -> Vec<u8> {
    let early = usize::from(early_change);
    let mut writer = BitWriter {
        out: Vec::with_capacity(data.len() / 2 + 8),
        acc: 0,
        nbits: 0,
    };
    let mut table: HashMap<(u16, u8), u16> = HashMap::new();
    let mut next = usize::from(FIRST);
    let mut width = MIN_WIDTH;
    writer.put(CLEAR, width);
    let mut current: Option<u16> = None;
    // After a code is written the decoder defines one entry and may
    // widen; the encoder mirrors that before defining its own entry,
    // which is one ahead of the decoder's.
    let widen = |next: usize, width: &mut u32| {
        if next + early >= 1 << *width && *width < MAX_WIDTH {
            *width += 1;
        }
    };
    for &byte in data {
        let Some(prefix) = current else {
            current = Some(u16::from(byte));
            continue;
        };
        if let Some(&code) = table.get(&(prefix, byte)) {
            current = Some(code);
            continue;
        }
        writer.put(prefix, width);
        widen(next, &mut width);
        if next >= TABLE_SIZE {
            writer.put(CLEAR, width);
            table.clear();
            next = usize::from(FIRST);
            width = MIN_WIDTH;
        } else {
            table.insert((prefix, byte), next as u16);
            next += 1;
        }
        current = Some(u16::from(byte));
    }
    if let Some(code) = current {
        writer.put(code, width);
        widen(next, &mut width);
    }
    writer.put(END, width);
    writer.finish()
}

/// Decodes a complete stream.
pub fn decode(data: &[u8], early_change: bool) -> Result<Vec<u8>, Error> {
    let mut decoder = Decoder::new(early_change);
    let mut out = Vec::with_capacity(data.len() * 2);
    for &byte in data {
        if decoder.push(byte, &mut out)? == Status::Done {
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// Packs `codes` at the given widths, most-significant bit first, as
    /// a check on the encoder independent of its own bit writer.
    fn packed(codes: &[(u16, u32)]) -> Vec<u8> {
        let mut bits = Vec::new();
        for &(code, width) in codes {
            for k in (0..width).rev() {
                bits.push((code >> k) & 1 == 1);
            }
        }
        while bits.len() % 8 != 0 {
            bits.push(false);
        }
        bits.chunks(8)
            .map(|byte| byte.iter().fold(0u8, |acc, &bit| acc << 1 | u8::from(bit)))
            .collect()
    }

    #[test]
    fn the_worked_example_of_the_pdf_specification() {
        // ISO 32000-1 §7.4.4.2: `-----A---B` codes as 256 45 258 258 65
        // 259 66 257, nine bits each.
        let input = [45u8, 45, 45, 45, 45, 65, 45, 45, 45, 66];
        let expected = [0x80, 0x0B, 0x60, 0x50, 0x22, 0x0C, 0x0C, 0x85, 0x01];
        assert_eq!(encode(&input, true), expected);
        assert_eq!(encode(&input, false), expected);
        assert_eq!(decode(&expected, true), Ok(input.to_vec()));
        assert_eq!(decode(&expected, false), Ok(input.to_vec()));
    }

    /// Every byte value once: 256 literal codes, so the width grows
    /// during the run — after 254 literals under early change, after 255
    /// without.
    #[test]
    fn the_width_grows_one_code_early_under_early_change() {
        let input: Vec<u8> = (0..=255).collect();
        for (early, nine_bit) in [(true, 254usize), (false, 255)] {
            let mut codes = vec![(CLEAR, 9)];
            for (k, &byte) in input.iter().enumerate() {
                codes.push((u16::from(byte), if k < nine_bit { 9 } else { 10 }));
            }
            codes.push((END, 10));
            let z = encode(&input, early);
            assert_eq!(z, packed(&codes), "early change {early}");
            assert_eq!(decode(&z, early), Ok(input.clone()));
        }
        assert_ne!(encode(&input, true), encode(&input, false));
        // Read with the wrong setting, the stream falls apart.
        assert_ne!(decode(&encode(&input, true), false), Ok(input.clone()));
    }

    #[test]
    fn a_clear_code_in_the_middle_resets_the_table() {
        // "abab" defines 258 = ab, 259 = ba; then a clear, then codes
        // that reuse 258 for a fresh "cd".
        let codes = [
            (CLEAR, 9),
            (b'a' as u16, 9),
            (b'b' as u16, 9),
            (258, 9),
            (CLEAR, 9),
            (b'c' as u16, 9),
            (b'd' as u16, 9),
            (258, 9),
            (END, 9),
        ];
        assert_eq!(decode(&packed(&codes), true), Ok(b"ababcdcd".to_vec()));
    }

    #[test]
    fn the_string_being_defined_decodes_as_itself() {
        // "aaa": codes 97 then 258, the latter defined by that very code.
        let codes = [(CLEAR, 9), (97, 9), (258, 9), (END, 9)];
        assert_eq!(decode(&packed(&codes), true), Ok(b"aaa".to_vec()));
        assert_eq!(encode(b"aaa", true), packed(&codes));
    }

    #[test]
    fn invalid_codes_are_errors_and_the_end_code_stops_reading() {
        let codes = [(CLEAR, 9), (300, 9), (END, 9)];
        assert_eq!(decode(&packed(&codes), true), Err(Error::Code));
        let codes = [(CLEAR, 9), (97, 9), (400, 9), (END, 9)];
        assert_eq!(decode(&packed(&codes), true), Err(Error::Code));
        let mut z = packed(&[(CLEAR, 9), (97, 9), (END, 9)]);
        z.extend_from_slice(&[0xFF; 4]);
        let mut decoder = Decoder::new(true);
        let mut out = Vec::new();
        let mut statuses = Vec::new();
        for &byte in &z {
            statuses.push(decoder.push(byte, &mut out).unwrap());
        }
        assert_eq!(out, b"a");
        assert!(decoder.is_done());
        assert_eq!(statuses[3], Status::Done);
        assert!(statuses[4..].iter().all(|&s| s == Status::Done));
        assert_eq!(decode(b"", true), Ok(Vec::new()));
    }

    #[test]
    fn a_full_table_is_cleared_and_the_stream_continues() {
        // Nothing repeats over four bytes, so entries pile up past 4096.
        let mut data = Vec::new();
        let mut state = 7u32;
        while data.len() < 30_000 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            data.push((state >> 24) as u8);
        }
        for early in [true, false] {
            let z = encode(&data, early);
            assert_eq!(decode(&z, early), Ok(data.clone()), "early change {early}");
        }
        let long = vec![b'x'; 100_000];
        let z = encode(&long, true);
        assert!(z.len() < 2000, "{}", z.len());
        assert_eq!(decode(&z, true), Ok(long));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn random_bytes_round_trip(
            data in proptest::collection::vec(any::<u8>(), 0..6000),
            early in any::<bool>(),
        ) {
            prop_assert_eq!(decode(&encode(&data, early), early), Ok(data));
        }

        #[test]
        fn repetitive_bytes_round_trip(
            (unit, times) in (proptest::collection::vec(any::<u8>(), 1..16), 1usize..1500),
            early in any::<bool>(),
        ) {
            let data = unit.repeat(times);
            prop_assert_eq!(decode(&encode(&data, early), early), Ok(data));
        }
    }
}
