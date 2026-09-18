// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Encoded number strings (PLRM3 §3.14.5): a string holding a single
//! homogeneous number array in the binary token encoding (§3.14.1),
//! which the user-path operators accept in place of an array of
//! numbers. The scanner never sees this form; the operator that wants
//! numbers decodes the string.

use crate::error::VmError;
use crate::ops::Num;

/// The token type byte a homogeneous number array starts with.
const HEADER: u8 = 149;

/// How the numbers after the header are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layout {
    /// A two's-complement integer of `size` bytes with `scale` fraction
    /// bits; scale 0 yields integers.
    Fixed { size: usize, scale: u8 },
    /// An IEEE 754 single.
    Ieee,
    /// A real in the interpreter's native format, which on every host
    /// this interpreter runs on is an IEEE 754 single with the
    /// low-order byte first, whatever the header's byte-order flag says.
    Native,
}

impl Layout {
    fn size(self) -> usize {
        match self {
            Layout::Fixed { size, .. } => size,
            Layout::Ieee | Layout::Native => 4,
        }
    }
}

/// Decodes `bytes` as a homogeneous number array. The representation
/// byte selects 32-bit fixed point with as many fraction bits (0 to
/// 31), 16-bit fixed point with 32 fewer fraction bits (32 to 47), an
/// IEEE single (48), or a native real (49); 128 more than any of those
/// puts the low-order byte first, in the length field too. A string
/// that does not start with the array's type byte is `typecheck`, an
/// unassigned representation `rangecheck`, and a length that is not
/// the header plus exactly the declared numbers `rangecheck`.
pub(crate) fn decode(bytes: &[u8]) -> Result<Vec<Num>, VmError> {
    let [header, representation, len0, len1, data @ ..] = bytes else {
        return Err(VmError::TypeCheck);
    };
    if *header != HEADER {
        return Err(VmError::TypeCheck);
    }
    let low_first = *representation >= 128;
    let layout = match representation & 0x7f {
        scale @ 0..=31 => Layout::Fixed { size: 4, scale },
        code @ 32..=47 => Layout::Fixed {
            size: 2,
            scale: code - 32,
        },
        48 => Layout::Ieee,
        49 => Layout::Native,
        _ => return Err(VmError::RangeCheck),
    };
    let count = if low_first {
        u16::from_le_bytes([*len0, *len1])
    } else {
        u16::from_be_bytes([*len0, *len1])
    };
    let size = layout.size();
    if data.len() != usize::from(count) * size {
        return Err(VmError::RangeCheck);
    }
    Ok(data
        .chunks_exact(size)
        .map(|chunk| number(layout, low_first, chunk))
        .collect())
}

fn number(layout: Layout, low_first: bool, chunk: &[u8]) -> Num {
    let raw = |low_first: bool| -> u32 {
        let fold = |value: u32, &byte: &u8| (value << 8) | u32::from(byte);
        if low_first {
            chunk.iter().rev().fold(0, fold)
        } else {
            chunk.iter().fold(0, fold)
        }
    };
    match layout {
        Layout::Fixed { size, scale } => {
            let value = raw(low_first);
            // Sign-extend a 16-bit value; a 32-bit one is already whole.
            let signed = if size == 2 {
                i32::from(value as u16 as i16)
            } else {
                value as i32
            };
            if scale == 0 {
                Num::Int(signed)
            } else {
                Num::Real((f64::from(signed) / f64::from(1u32 << scale)) as f32)
            }
        }
        Layout::Ieee => Num::Real(f32::from_bits(raw(low_first))),
        Layout::Native => Num::Real(f32::from_bits(raw(true))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ints(numbers: &[Num]) -> Vec<i32> {
        numbers
            .iter()
            .map(|n| match n {
                Num::Int(i) => *i,
                Num::Real(r) => panic!("real {r}"),
            })
            .collect()
    }

    fn reals(numbers: &[Num]) -> Vec<f32> {
        numbers
            .iter()
            .map(|n| match n {
                Num::Real(r) => *r,
                Num::Int(i) => panic!("integer {i}"),
            })
            .collect()
    }

    #[test]
    fn thirty_two_bit_integers_both_orders() {
        let high = [
            149, 0, 0, 3, 0, 0, 0, 7, 0xFF, 0xFF, 0xFF, 0xFE, 0x12, 0x34, 0x56, 0x78,
        ];
        assert_eq!(ints(&decode(&high).unwrap()), [7, -2, 0x1234_5678]);
        let low = [
            149, 128, 3, 0, 7, 0, 0, 0, 0xFE, 0xFF, 0xFF, 0xFF, 0x78, 0x56, 0x34, 0x12,
        ];
        assert_eq!(ints(&decode(&low).unwrap()), [7, -2, 0x1234_5678]);
    }

    #[test]
    fn thirty_two_bit_fixed_point_scales() {
        // Scale 16: 2.5 is 0x0002_8000; -1.25 is the two's complement
        // of 0x0001_4000.
        let high = [149, 16, 0, 2, 0, 2, 0x80, 0, 0xFF, 0xFE, 0xC0, 0];
        assert_eq!(reals(&decode(&high).unwrap()), [2.5, -1.25]);
        let low = [149, 144, 2, 0, 0, 0x80, 2, 0, 0, 0xC0, 0xFE, 0xFF];
        assert_eq!(reals(&decode(&low).unwrap()), [2.5, -1.25]);
        // Scale 31: the largest fraction, a half is 0x4000_0000.
        let half = [149, 31, 0, 1, 0x40, 0, 0, 0];
        assert_eq!(reals(&decode(&half).unwrap()), [0.5]);
    }

    #[test]
    fn sixteen_bit_integers_and_fixed_point() {
        let high = [149, 32, 0, 3, 0, 10, 0xFF, 0xFE, 0x7F, 0xFF];
        assert_eq!(ints(&decode(&high).unwrap()), [10, -2, 32767]);
        let low = [149, 160, 3, 0, 10, 0, 0xFE, 0xFF, 0xFF, 0x7F];
        assert_eq!(ints(&decode(&low).unwrap()), [10, -2, 32767]);
        // Scale 4 (representation 36): 10.5 is 0x00A8, -0.5 is 0xFFF8.
        let scaled = [149, 36, 0, 2, 0, 0xA8, 0xFF, 0xF8];
        assert_eq!(reals(&decode(&scaled).unwrap()), [10.5, -0.5]);
        let scaled_low = [149, 164, 2, 0, 0xA8, 0, 0xF8, 0xFF];
        assert_eq!(reals(&decode(&scaled_low).unwrap()), [10.5, -0.5]);
        // Scale 15 (representation 47): 0x4000 is a half.
        let fine = [149, 47, 0, 1, 0x40, 0];
        assert_eq!(reals(&decode(&fine).unwrap()), [0.5]);
    }

    #[test]
    fn ieee_reals_both_orders() {
        let ten = 10.0f32.to_be_bytes();
        let minus = (-1.5f32).to_be_bytes();
        let mut high = vec![149, 48, 0, 2];
        high.extend(ten);
        high.extend(minus);
        assert_eq!(reals(&decode(&high).unwrap()), [10.0, -1.5]);
        let mut low = vec![149, 176, 2, 0];
        low.extend(10.0f32.to_le_bytes());
        low.extend((-1.5f32).to_le_bytes());
        assert_eq!(reals(&decode(&low).unwrap()), [10.0, -1.5]);
    }

    #[test]
    fn native_reals_are_low_order_first_under_either_flag() {
        let mut high_flag = vec![149, 49, 0, 1];
        high_flag.extend(10.0f32.to_le_bytes());
        assert_eq!(reals(&decode(&high_flag).unwrap()), [10.0]);
        let mut low_flag = vec![149, 177, 1, 0];
        low_flag.extend(10.0f32.to_le_bytes());
        assert_eq!(reals(&decode(&low_flag).unwrap()), [10.0]);
    }

    #[test]
    fn empty_arrays_and_malformed_strings() {
        assert_eq!(decode(&[149, 32, 0, 0]).unwrap(), Vec::new());
        assert_eq!(decode(&[149, 160, 0, 0]).unwrap(), Vec::new());
        assert_eq!(decode(&[]), Err(VmError::TypeCheck));
        assert_eq!(decode(&[149, 32, 0]), Err(VmError::TypeCheck));
        assert_eq!(decode(&[148, 32, 0, 0]), Err(VmError::TypeCheck));
        assert_eq!(decode(&[149, 50, 0, 0]), Err(VmError::RangeCheck));
        assert_eq!(decode(&[149, 178, 0, 0]), Err(VmError::RangeCheck));
        assert_eq!(decode(&[149, 255, 0, 0]), Err(VmError::RangeCheck));
        // One number declared, none or two supplied.
        assert_eq!(decode(&[149, 32, 0, 1]), Err(VmError::RangeCheck));
        assert_eq!(
            decode(&[149, 32, 0, 1, 0, 1, 0, 2]),
            Err(VmError::RangeCheck)
        );
        // The length field follows the byte order: 256 numbers high
        // first, not one.
        assert_eq!(decode(&[149, 32, 1, 0, 0, 1]), Err(VmError::RangeCheck));
        assert_eq!(ints(&decode(&[149, 160, 1, 0, 1, 0]).unwrap()), [1]);
    }
}
