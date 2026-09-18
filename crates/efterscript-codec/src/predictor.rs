// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The `Predictor` parameters of the Flate and LZW filters: rows were
//! filtered before compression to make them compress better, and the
//! [`Predictor`] undoes that after decompression, a row at a time. The
//! PNG predictors (values 10–15) carry a tag byte before every row naming
//! that row's filter — none, sub, up, average, or Paeth (ISO 32000-1
//! §7.4.4.4, Table 10); the TIFF predictor (value 2) is horizontal
//! differencing of each sample against the one before it in the same
//! component.

use std::fmt;

/// Why the parameters could not describe a predictor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A predictor value other than 1, 2, or 10–15.
    Predictor,
    /// `Colors` below 1.
    Colors,
    /// `BitsPerComponent` other than 1, 2, 4, 8, or 16.
    BitsPerComponent,
    /// `Columns` below 1, or a row too long to hold.
    Columns,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::Predictor => "unknown predictor",
            Error::Colors => "invalid Colors",
            Error::BitsPerComponent => "invalid BitsPerComponent",
            Error::Columns => "invalid Columns",
        })
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Tiff,
    Png,
}

/// A row unfilter: see the module documentation.
#[derive(Clone, Debug)]
pub struct Predictor {
    kind: Kind,
    colors: usize,
    bits: usize,
    columns: usize,
    /// Bytes per row, the tag excluded.
    row: usize,
    /// Bytes per pixel, at least one: the distance the sub, average,
    /// and Paeth filters look back.
    bpp: usize,
    previous: Vec<u8>,
    current: Vec<u8>,
    /// The tag of the row being collected; `None` before it arrives.
    tag: Option<u8>,
}

impl Predictor {
    /// A predictor for the parameters, `None` for predictor 1 (no
    /// prediction). `Colors` and `Columns` must be at least 1,
    /// `BitsPerComponent` 1, 2, 4, 8, or 16.
    pub fn new(
        predictor: i64,
        colors: i64,
        bits_per_component: i64,
        columns: i64,
    ) -> Result<Option<Self>, Error> {
        let kind = match predictor {
            1 => return Ok(None),
            2 => Kind::Tiff,
            10..=15 => Kind::Png,
            _ => return Err(Error::Predictor),
        };
        let colors = usize::try_from(colors)
            .ok()
            .filter(|&c| c >= 1)
            .ok_or(Error::Colors)?;
        let bits = match bits_per_component {
            1 | 2 | 4 | 8 | 16 => bits_per_component as usize,
            _ => return Err(Error::BitsPerComponent),
        };
        let columns = usize::try_from(columns)
            .ok()
            .filter(|&c| c >= 1)
            .ok_or(Error::Columns)?;
        let row = colors
            .checked_mul(bits)
            .and_then(|b| b.checked_mul(columns))
            .filter(|&b| b <= 1 << 31)
            .ok_or(Error::Columns)?
            .div_ceil(8);
        let bpp = (colors * bits).div_ceil(8);
        Ok(Some(Predictor {
            kind,
            colors,
            bits,
            columns,
            row,
            bpp,
            previous: vec![0; row],
            current: Vec::with_capacity(row),
            tag: None,
        }))
    }

    /// Feeds one decompressed byte; a completed row is appended to
    /// `out`.
    pub fn push(&mut self, byte: u8, out: &mut Vec<u8>) {
        if self.kind == Kind::Png && self.tag.is_none() {
            self.tag = Some(byte);
            return;
        }
        self.current.push(byte);
        if self.current.len() == self.row {
            self.finish_row(out);
        }
    }

    /// Ends the input: a partial last row is passed through as it is.
    pub fn flush(&mut self, out: &mut Vec<u8>) {
        out.append(&mut self.current);
        self.tag = None;
    }

    fn finish_row(&mut self, out: &mut Vec<u8>) {
        match self.kind {
            Kind::Png => {
                let tag = self.tag.take().unwrap_or(0);
                unfilter_png(tag, self.bpp, &self.previous, &mut self.current);
            }
            Kind::Tiff => undo_tiff(self.colors, self.bits, self.columns, &mut self.current),
        }
        out.extend_from_slice(&self.current);
        std::mem::swap(&mut self.previous, &mut self.current);
        self.current.clear();
    }
}

/// Undoes one PNG row filter in place; an unknown tag leaves the row as
/// it is.
fn unfilter_png(tag: u8, bpp: usize, previous: &[u8], row: &mut [u8]) {
    for i in 0..row.len() {
        let left = if i >= bpp { row[i - bpp] } else { 0 };
        let up = previous[i];
        let up_left = if i >= bpp { previous[i - bpp] } else { 0 };
        let predicted = match tag {
            1 => left,
            2 => up,
            3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
            4 => paeth(left, up, up_left),
            _ => 0,
        };
        row[i] = row[i].wrapping_add(predicted);
    }
}

/// The Paeth predictor: whichever of the neighbours is closest to their
/// linear estimate, ties going left, then up.
fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let (a, b, c) = (i16::from(left), i16::from(up), i16::from(up_left));
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        left
    } else if pb <= pc {
        up
    } else {
        up_left
    }
}

/// Undoes horizontal differencing in place: each sample is a delta from
/// the sample `colors` positions before it.
fn undo_tiff(colors: usize, bits: usize, columns: usize, row: &mut [u8]) {
    match bits {
        8 => {
            for i in colors..row.len() {
                row[i] = row[i].wrapping_add(row[i - colors]);
            }
        }
        16 => {
            let samples = row.len() / 2;
            for i in colors..samples {
                let prior = u16::from_be_bytes([row[2 * (i - colors)], row[2 * (i - colors) + 1]]);
                let delta = u16::from_be_bytes([row[2 * i], row[2 * i + 1]]);
                row[2 * i..2 * i + 2].copy_from_slice(&prior.wrapping_add(delta).to_be_bytes());
            }
        }
        _ => {
            let mask = (1u8 << bits) - 1;
            let per_byte = 8 / bits;
            let count = columns * colors;
            let get = |row: &[u8], k: usize| {
                let shift = 8 - bits * (k % per_byte + 1);
                (row[k / per_byte] >> shift) & mask
            };
            let mut previous = vec![0u8; colors];
            for k in 0..count {
                let value = (get(row, k) + previous[k % colors]) & mask;
                let shift = 8 - bits * (k % per_byte + 1);
                row[k / per_byte] &= !(mask << shift);
                row[k / per_byte] |= value << shift;
                previous[k % colors] = value;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(predictor: Predictor, input: &[u8]) -> Vec<u8> {
        let mut p = predictor;
        let mut out = Vec::new();
        for &byte in input {
            p.push(byte, &mut out);
        }
        p.flush(&mut out);
        out
    }

    #[test]
    fn parameters_are_checked() {
        assert!(Predictor::new(1, 1, 8, 4).unwrap().is_none());
        assert!(Predictor::new(2, 1, 8, 4).unwrap().is_some());
        assert!(Predictor::new(15, 3, 8, 4).unwrap().is_some());
        assert_eq!(Predictor::new(3, 1, 8, 4).err(), Some(Error::Predictor));
        assert_eq!(Predictor::new(12, 0, 8, 4).err(), Some(Error::Colors));
        assert_eq!(
            Predictor::new(12, 1, 3, 4).err(),
            Some(Error::BitsPerComponent)
        );
        assert_eq!(Predictor::new(12, 1, 8, 0).err(), Some(Error::Columns));
        assert_eq!(
            Predictor::new(12, 1, 8, 1 << 40).err(),
            Some(Error::Columns)
        );
    }

    #[test]
    fn png_rows_unfilter_by_their_tag() {
        // Four one-byte pixels per row, tags: none, sub, up, average, Paeth.
        let p = Predictor::new(12, 1, 8, 4).unwrap().unwrap();
        let input = [
            0, 10, 20, 30, 40, // none: as is
            1, 5, 5, 5, 5, // sub: 5 10 15 20
            2, 1, 1, 1, 1, // up: 6 11 16 21
            3, 0, 0, 0, 0, // average of left and up: 3 7 11 16
            4, 0, 0, 0, 0, // Paeth: 3 7 11 16
        ];
        assert_eq!(
            run(p, &input),
            [
                10, 20, 30, 40, 5, 10, 15, 20, 6, 11, 16, 21, 3, 7, 11, 16, 3, 7, 11, 16
            ]
        );
    }

    #[test]
    fn png_filters_look_back_a_whole_pixel() {
        // Two RGB pixels per row: sub adds the byte three back.
        let p = Predictor::new(11, 3, 8, 2).unwrap().unwrap();
        let input = [1, 10, 20, 30, 1, 2, 3];
        assert_eq!(run(p, &input), [10, 20, 30, 11, 22, 33]);
        // Sixteen-bit gray: two bytes per pixel.
        let p = Predictor::new(11, 1, 16, 2).unwrap().unwrap();
        let input = [1, 0x01, 0x02, 0x00, 0x03];
        assert_eq!(run(p, &input), [0x01, 0x02, 0x01, 0x05]);
        // A partial last row passes through with the tag dropped.
        let p = Predictor::new(11, 1, 8, 4).unwrap().unwrap();
        assert_eq!(run(p, &[0, 1, 2, 3, 4, 2, 9, 9]), [1, 2, 3, 4, 9, 9]);
    }

    #[test]
    fn paeth_picks_the_nearest_neighbour() {
        // p = left + up - up_left; the neighbour nearest p wins, ties
        // going left, then up.
        assert_eq!(paeth(10, 20, 15), 15);
        assert_eq!(paeth(20, 10, 15), 15);
        assert_eq!(paeth(10, 10, 100), 10);
        assert_eq!(paeth(3, 3, 0), 3);
        assert_eq!(paeth(0, 10, 0), 10);
        assert_eq!(paeth(100, 100, 100), 100);
        assert_eq!(paeth(50, 60, 55), 55);
        assert_eq!(paeth(5, 200, 100), 100);
    }

    #[test]
    fn tiff_differencing_adds_the_previous_sample_of_the_component() {
        let p = Predictor::new(2, 1, 8, 4).unwrap().unwrap();
        assert_eq!(
            run(p, &[10, 1, 1, 1, 200, 100, 0, 0]),
            [10, 11, 12, 13, 200, 44, 44, 44]
        );
        let p = Predictor::new(2, 3, 8, 2).unwrap().unwrap();
        assert_eq!(run(p, &[1, 2, 3, 1, 1, 1]), [1, 2, 3, 2, 3, 4]);
        let p = Predictor::new(2, 1, 16, 2).unwrap().unwrap();
        assert_eq!(run(p, &[0x01, 0x00, 0x00, 0xFF]), [0x01, 0x00, 0x01, 0xFF]);
    }

    #[test]
    fn tiff_differencing_handles_packed_samples() {
        // Four-bit gray, four columns: 1 +1 +1 +1 → 1 2 3 4.
        let p = Predictor::new(2, 1, 4, 4).unwrap().unwrap();
        assert_eq!(run(p, &[0x11, 0x11]), [0x12, 0x34]);
        // One-bit, eight columns: 1 then seven +1s wrap around.
        let p = Predictor::new(2, 1, 1, 8).unwrap().unwrap();
        assert_eq!(run(p, &[0xFF]), [0b1010_1010]);
        // Two-bit, two colours, two columns: deltas apply per colour.
        let p = Predictor::new(2, 2, 2, 2).unwrap().unwrap();
        assert_eq!(run(p, &[0b01_10_01_01]), [0b01_10_10_11]);
    }
}
